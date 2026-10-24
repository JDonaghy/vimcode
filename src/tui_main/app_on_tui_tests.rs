#[cfg(test)]
mod tests {
    //! App-on-TUI gap inventory, as executable tests.
    //!
    //! `crate::app::App` (the cross-backend-shared shell — also what `gtk` and
    //! the `tui` control arm of `crate::harness`'s `backend_conformance!`
    //! macro wrap) already runs on `quadraui::tui::TuiBackend` via
    //! [`crate::tui_main::testing::conformance_harness`]. This module drives
    //! it through a representative slice of the same scenarios
    //! `src/tui_main/shell_app.rs`'s own `#[cfg(test)] mod tests` already
    //! covers for the *shipped* TUI shell — reusing that module's test names
    //! where the assertion body transfers unmodified (so `grep`-ing a name
    //! finds both halves of the comparison), and picking a new, descriptive
    //! name where `App`'s own construction/fields differ enough that a
    //! faithful port needed a different shape.
    //!
    //! # No production code here (exceptions: #1762, #1763, #1764, #1750, #1991)
    //!
    //! Every test below drives already-shipped code through the existing
    //! [`crate::tui_main::testing::conformance_harness`] / [`crate::harness`]
    //! seams. Nothing in `src/app.rs`, `src/render.rs`, or `src/tui_main/`
    //! (outside this file and the one `mod` declaration in `mod.rs`) changes
    //! — except the following exceptions, which pin fixes landing in
    //! `src/app.rs` and/or `src/render.rs` that only reproduce through the
    //! real `App::handle_dispatch` pipeline this module's harness already
    //! exercises, so these were the natural home for them:
    //!
    //! - `activity_bar_adjacent_clicks_are_not_dropped_by_double_click_fold_1762`
    //!   pins a fix in `App::handle_dispatch` that rescues folded double-clicks.
    //! - `alt_g_dropdown_does_not_survive_a_vim_dw_1763` and
    //!   `escape_closes_the_dropdown_but_leaves_the_toggleable_bar_row_visible_1763`
    //!   cover a genuine new `src/app.rs` change (the `MenuEvent::Ignored` arm).
    //! - `colon_opens_the_command_line_after_an_alt_chord_swallows_escape_1764`
    //!   covers new `src/app.rs` and `src/render.rs` code for Alt-chord handling.
    //! - All tests under `editor_hover_dwell_1750` cover new `src/app.rs` code
    //!   that arms the dwell timer in the `MouseMoved` arm of `App::handle_dispatch`.
    //! - `opening_a_terminal_on_tui_sizes_the_pty_within_one_column_of_the_painted_width`
    //!   pins the terminal panel scrollbar gutter fix in `src/app/dialogs.rs`
    //!   (`App::terminal_panel_cols`), `src/app/mouse_tabs.rs`
    //!   (`App::compose_bottom_band_rungs`), `src/render/frame_editor_band.rs`
    //!   (`BottomPanelUnits`) and `src/render/hit_test_geometry.rs`
    //!   (`UnitProfile::terminal_scrollbar_gutter`).
    //! - `pressing_the_painted_terminal_scrollbar_track_on_tui_scrolls_the_scrollback`
    //!   pins the other half of the same change: the reserved gutter carries a
    //!   hit-testable `quadraui::SurfaceScrollbar`, registered by
    //!   `src/render/frame_editor_band.rs` from the width
    //!   `App::compose_bottom_band_rungs` passes.
    //!
    //! Everything else in this file only drives already-shipped code.
    //!
    //! # Reading a failure here
    //!
    //! A test that panics *unwrapped* is a regression — it must not happen on
    //! a clean `cargo test`. A test wrapped in
    //! `crate::harness::known_bug_gate` and listed in
    //! `crate::harness::KNOWN_BUGS` is a **known, categorised** gap between
    //! `App` and the shipped TUI: the comment directly above the gate names
    //! the root-cause category (unit / caps / feature / quadraui / product)
    //! and the epic child expected to close it, per #1425's own acceptance
    //! bar. Deleting a `KNOWN_BUGS` entry without also fixing the gap is
    //! itself caught — `known_bug_gate` fails the build the moment a gated
    //! body starts passing and the entry is left behind (see that function's
    //! own doc in `src/harness.rs`).
    //!
    //! # Why this whole module lives inside one `#[cfg(test)] mod tests`
    //!
    //! `mod.rs`'s own `mod app_on_tui_tests;` declaration *is*
    //! `#[cfg(test)]`-gated, same as every item this file defines (this one
    //! `mod tests` block wraps literally everything below, including its own
    //! module doc you're reading right now). Gating that bodiless,
    //! semicolon-form `mod` declaration used to trip a `scripts/prod_lines.py`
    //! bug: its brace-balance skip loop only knew how to skip a *braced*
    //! item, so on a `#[cfg(test)] mod x;` line with no body it never found
    //! an opening `{` to close on and instead ran off the end of the
    //! *`mod.rs` file*, silently miscounting everything below it as skipped.
    //! `prod_lines.py` now also recognises a bodiless item — one that ends in
    //! `;` before any `{` is seen — as a single skippable line, so the `mod`
    //! declaration above is gated for real and this file's ~830 lines are
    //! correctly excluded from `src/tui_main`'s production count (delta: 0).
    //!
    //! # Why `(80, 24)`
    //!
    //! Every scenario here uses the same cell size `shell_app.rs`'s own driver
    //! tests use — a realistic terminal, not the oversized `(800, 480)`
    //! `crate::harness`'s `backend_conformance!` scenarios use for its `tui`
    //! arm (chosen there specifically to give plenty of headroom). Running at
    //! a realistic size is what surfaces the **unit** category gap #1425's own
    //! diagnosis names: `App::render_content` reserves
    //! `render::tab_row_height_px`/`breadcrumb_row_height_px` (35/22 px) as if
    //! they were cell-grid *rows* rather than pixels, so on an 80x24 terminal
    //! the tab bar alone claims more rows than exist and the editor content
    //! band collapses. That is exactly the gap this module exists to inventory
    //! test-by-test rather than leave as a single paragraph in an issue body.
    //!
    //! # Test porting status: #1432 tranches 1–3
    //!
    //! This module currently ports: `panels.rs`'s `sc_panel_tests` (all 7 — see
    //! `sidebar_panels`'s `sc_panel_*` tests below) and two of `mouse.rs`'s
    //! right-click scenarios (see `explorer_context_menu`'s `right_click_*`
    //! tests below), plus all behavioural tests under `activity_bar` and
    //! `key_dispatch` that fit within the scopes of those tranches.
    //!
    //! Unported, remains as work for future tranches: the remaining ~9
    //! `mouse.rs` behavioural tests, `panels.rs`'s 5-test
    //! `activity_bar_keyboard_ring_tests`, `render_impl.rs`'s 39 tests,
    //! `mod.rs`'s 4, and the ~392 in `shell_app.rs`.

    use quadraui::testing::ConformanceDriver;

    use crate::core::window::SplitDirection;
    use crate::harness::known_bug_gate;

    /// Build an [`crate::core::Engine`] fixture and drive it through
    /// [`crate::tui_main::testing::conformance_harness`] at this module's
    /// standard `(80, 24)` cell size. Thin wrapper purely so every test below
    /// reads `harness(engine_fixture())` instead of repeating the two-line
    /// `conformance_harness(..., 80, 24)` call.
    fn harness(
        engine: crate::core::Engine,
    ) -> crate::harness::ConformanceHarness<
        quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
    > {
        crate::tui_main::testing::conformance_harness(engine, 80, 24)
    }

    /// The plainest possible fixture: one scratch buffer, no sidebar/terminal/
    /// dialog state. Nerd fonts off — same reasoning every `crate::harness`
    /// fixture already gives: icon glyphs are irrelevant to what these tests
    /// assert on and the ASCII fallbacks are what a nerd-font-less CI runner
    /// actually paints.
    fn plain_engine() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    /// Click the Explorer activity-bar icon, twice, to collapse the
    /// sidebar's screen-space reservation — the real production
    /// toggle-closed gesture. Several tests below want the full terminal width
    /// for the editor/tab bar/bottom band rather than competing with the
    /// sidebar for the same narrow 80-column budget.
    ///
    /// Two clicks, not one: `App::shell_config`'s `cell`-profile
    /// hamburger `PanelDefinition` now occupies index 0 in the *runner's*
    /// fresh `AppShell` (`quadraui::AppShell::new` always activates index
    /// 0), so a fixture built from [`plain_engine`] no longer starts with
    /// Explorer as the runner's already-active panel the way it did before
    /// hamburger existed — the first click on Explorer's icon *activates*
    /// it (a fresh `PanelChanged`, different panel than the hamburger
    /// default) rather than collapsing it. The second click, now that
    /// Explorer genuinely is the active + visible panel, hits
    /// `AppShell::handle_activity_click`'s toggle-closed branch as
    /// originally intended. Re-locates the icon between clicks (cheap,
    /// and the row it paints on is unaffected either way) rather than
    /// assuming its position is stable, matching every other zone lookup
    /// in this file.
    ///
    /// Mutating `engine.app_shell` directly (`hide_sidebar()`) does **not**
    /// achieve this, and is deliberately not used here: `App`'s own
    /// runner-side `AppShell` (the actual layout/column reservation) starts
    /// from `ShellConfig`'s own default and is never synced from the engine's
    /// shadow copy at startup — only a real click through
    /// `AppShell::handle_activity_click`'s toggle-closed branch flips it.
    fn collapse_sidebar(driver: &mut quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>) {
        // Two real, independent clicks: without disabling folding, two
        // `driver.click()` calls with no simulated time between them fold
        // into a single `UiEvent::DoubleClick`. Before #1762's `App::handle_dispatch`
        // rung, that fold would only ever have activated Explorer once and
        // never reached the toggle-closed branch; the rung now rescues it
        // by replaying the fold as a second plain `MouseDown`. Left disabled
        // anyway, since this helper's job is a deterministic two-click sequence,
        // not a fold-rescue scenario (that's
        // `activity_bar_adjacent_clicks_are_not_dropped_by_double_click_fold_1762`'s
        // job, and the GTK-side
        // `activity_bar_double_click_on_active_icon_reopens_sidebar_via_gtk_driver`'s).
        // Needs the concrete `TuiDriver` type (not the generic
        // `ConformanceDriver`/`DriverInput` bound this helper used before
        // #1427), since `set_double_click_folding` is TUI-only — fine
        // here, this whole module is TUI-only by construction (see its own
        // doc).
        driver.set_double_click_folding(false);
        let explorer_bounds = |driver: &mut quadraui::tui::testing::TuiDriver<_>| {
            driver
                .inventory()
                .zones()
                .iter()
                .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_EXPLORER)
                .map(|z| z.bounds)
                .expect("the Explorer activity-bar icon must register a chrome zone")
        };
        for _ in 0..2 {
            let zone = explorer_bounds(driver);
            driver.click(zone.x + zone.width / 2.0, zone.y + zone.height / 2.0);
        }
    }

    /// [`harness`], with the sidebar immediately collapsed via
    /// [`collapse_sidebar`].
    fn harness_no_sidebar(
        engine: crate::core::Engine,
    ) -> crate::harness::ConformanceHarness<
        quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
    > {
        let mut h = harness(engine);
        collapse_sidebar(&mut h.driver);
        h
    }

    /// Dispatch a plain hover `MouseMoved` — no button held. `TuiDriver::mouse_move`
    /// itself always sends the left button *held* (it exists to drive drag-selection
    /// scenarios, per its own doc), so a genuine no-op hover move has to be built
    /// and sent directly through `driver.dispatch` instead.
    fn hover_move(
        driver: &mut quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        x: f32,
        y: f32,
    ) -> quadraui::Reaction {
        driver.dispatch(quadraui::UiEvent::MouseMoved {
            position: quadraui::Point::new(x, y),
            buttons: quadraui::ButtonMask::default(),
        })
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Editor hover dwell: `MouseMoved` must arm `Engine::editor_hover_
    // mouse_move`, on every backend, with no intervening input event.
    //
    // This covers a genuine new `src/app.rs` change (the `MouseMoved` arm in
    // `App::handle_dispatch`) that restores the dwell timer: #731 deleted the
    // GTK-only polling block that used to call `Engine::editor_hover_mouse_move`,
    // and #1434 then deleted TUI's own `mouse.rs` copy of the same wiring. So by
    // #1750, no backend ever armed the dwell timer and the hover popup — LSP
    // hover and every extension's own `vimcode.editor.set_hover` hover —
    // stopped appearing anywhere. Fixed once in the shared `MouseMoved` arm both
    // backends already reach, so one test per content source covers both backends.
    // ─────────────────────────────────────────────────────────────────────────
    mod editor_hover_dwell_1750 {
        use super::*;

        /// Enforces (rather than merely documenting) the "this test must
        /// never launch a real language server" invariant the two `.rs`-
        /// fixture tests below depend on. `Engine::lsp_manager` starts as
        /// `None` and is only ever populated by `ensure_lsp_manager()`, which
        /// every spawn path — `lsp_did_open` (reached from `new_tab` /
        /// `split_window`) and `lsp_request_hover_at` (reached from
        /// `poll_editor_hover`) alike — has to go through before it can
        /// `resolve_and_start_server` the registry's `rust-analyzer` off
        /// `PATH`/`~/.cargo/bin`. So `lsp_manager.is_none()` at the end of
        /// the test is a sound, cheap proxy for "no server process was
        /// started, nothing leaked, and nothing indexed this workspace".
        fn assert_no_lsp_server_was_spawned(engine: &crate::core::Engine) {
            assert!(
                engine.lsp_manager.is_none(),
                "this test must never start a real language server: \
                 `Engine::lsp_manager` is `Some`, which means some path \
                 reached `ensure_lsp_manager()` with `settings.lsp_enabled` \
                 still true and may have spawned (and leaked) a real \
                 `rust-analyzer` indexing this whole workspace. Check that \
                 `lsp_enabled = false` is set BOTH before `new_tab`/\
                 `split_window` and again after the first `driver.tick()` \
                 (which `check_settings_reload` clobbers) — see this \
                 module's doc comments (#1750 review)"
            );
        }

        /// LSP-sourced hover content, end to end through the shared
        /// `App::handle_dispatch`: a plain `MouseMoved` dwelling over a word,
        /// with no key press or click anywhere in the test, must reach
        /// `Engine::editor_hover_mouse_move` (arming the dwell timer), and
        /// `TuiDriver::tick()` — the real idle-tick path,
        /// `render::run_shared_tick_chores` -> `Engine::poll_idle` ->
        /// `poll_editor_hover` -> `show_editor_hover_at_inner` — must then
        /// paint the popup once the dwell delay elapses.
        ///
        /// Content source is an `Engine::lsp_diagnostics` entry (real LSP
        /// content — `textDocument/publishDiagnostics` — consumed by
        /// `show_editor_hover_at_inner`'s section 1, the same function a
        /// genuine `textDocument/hover` response's section 4 sits beside),
        /// not a live round-trip to a real `rust-analyzer` session: this
        /// repo deliberately avoids depending on a live LSP server in tests
        /// (see `tests/extensions.rs`'s own `ruby-lsp`-on-`PATH` discussion
        /// for the flakiness this project has already hit doing that), and
        /// `lsp_enabled = false` below keeps `show_editor_hover_at_inner`'s
        /// `request_lsp` branch a no-op so this test cannot accidentally
        /// spawn a real server process.
        ///
        /// That flag has to be cleared **twice**, before `new_tab` *and*
        /// again after the first `driver.tick()`, because the two writes are
        /// load-bearing for different server-spawn sites and neither
        /// subsumes the other:
        ///
        /// 1. **Before `Engine::new_tab`** — `new_tab` is itself a spawn
        ///    site: it calls `self.lsp_did_open(buffer_id)`
        ///    (`src/core/engine/windows.rs`), which returns early only at
        ///    `if !self.settings.lsp_enabled` (`src/core/engine/lsp_ops.rs`)
        ///    and otherwise falls through to `ensure_lsp_manager()` (rooted
        ///    at `std::env::current_dir()`, i.e. the whole vimcode
        ///    workspace) and `notify_did_open` ->
        ///    `ensure_server_for_language("rust")` -> the built-in registry's
        ///    `rust-analyzer`, resolved off `PATH`/`~/.cargo/bin`. A
        ///    `plain_engine()` starts from `Settings::default()`, whose
        ///    `default_lsp_enabled()` is `true`, so without this write the
        ///    `.rs` fixture below launches a real `rust-analyzer` indexing
        ///    this repo on any machine that has one installed. (Every
        ///    *other* `new_tab(Some(..))` in this file sidesteps the issue
        ///    by using a `.txt` fixture, so `language_id_from_path` returns
        ///    `None` and `lsp_did_open` bails before the manager. These
        ///    tests need a real `.rs` path for the diagnostics key.)
        /// 2. **After the first `driver.tick()`** — that tick runs
        ///    `Engine::check_settings_reload`, which replaces the whole
        ///    `settings` struct wholesale (`self.settings = new_settings`),
        ///    so write 1 is clobbered back to whatever
        ///    `default_lsp_enabled()`/the dev's on-disk `settings.json` says
        ///    (`true`). Re-setting it there is what keeps the *later*
        ///    `poll_editor_hover` -> `show_editor_hover_at_inner(request_lsp
        ///    = true)` -> `lsp_request_hover_at` path inert — same reason
        ///    `hover_delay` has to be set after the tick too.
        ///
        /// This version uses `Engine::lsp_diagnostics` to drive the dwell
        /// timer, not a pre-seeded `Engine::lsp_hover_text` field which would
        /// paint immediately regardless of dwell state — a false green that
        /// could not have failed against unfixed code. Diagnostics have no
        /// such independent paint path, making this version RED-verified:
        /// reverting this PR's `src/app.rs` `MouseMoved` arm leaves the dwell
        /// timer unarmed and the final assertion below fails.
        #[test]
        fn lsp_diagnostic_hover_dwell_paints_popup_via_mouse_move_1750() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1750_hover_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&dir).expect("create fixture dir");
            let path = dir.join("fixture.rs");
            std::fs::write(&path, "let needle = 1;\n").expect("write fixture file");

            let mut engine = plain_engine();
            // Write 1 of 2 — see this test's doc comment: `new_tab` calls
            // `lsp_did_open`, which would spawn a real `rust-analyzer` for
            // this `.rs` fixture otherwise. (`plain_engine()`'s
            // `Settings::default()` has `lsp_enabled == true`.)
            engine.settings.lsp_enabled = false;
            engine.new_tab(Some(&path));
            let diag_key = engine
                .active_buffer_diagnostics_key()
                .expect("the just-opened fixture file must have a diagnostics key");
            engine.lsp_diagnostics.insert(
                diag_key,
                vec![crate::core::lsp::Diagnostic {
                    range: crate::core::lsp::LspRange {
                        start: crate::core::lsp::LspPosition {
                            line: 0,
                            character: 0,
                        },
                        end: crate::core::lsp::LspPosition {
                            line: 0,
                            character: 15,
                        },
                    },
                    severity: crate::core::lsp::DiagnosticSeverity::Error,
                    message: "ZQXW_1750_LSP_DIAG_MARKER".to_string(),
                    source: Some("rust-analyzer".to_string()),
                    code: None,
                }],
            );
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // See the sibling plugin-hover test's identical comment: a fresh
            // `Engine`'s first idle tick reloads settings from disk — that
            // reload replaces the whole `settings` struct, so BOTH
            // `hover_delay` and `lsp_enabled` have to be (re-)set after this
            // first tick, or they're silently clobbered back to their
            // on-disk/default values.
            driver.tick();
            {
                let mut engine = h.engine.borrow_mut();
                engine.settings.hover_delay = 1;
                // Write 2 of 2 (see the doc comment): the tick above just
                // clobbered write 1 back to `true`. Keeps
                // `lsp_request_hover_at` (fired alongside the diagnostic
                // section, since that function doesn't know a diagnostic
                // already answered the hover) a no-op.
                engine.settings.lsp_enabled = false;
            }
            driver.render();

            let (x, y) = driver
                .find("needle")
                .expect("the word to hover over must be painted");

            hover_move(driver, x, y);
            assert!(
                !driver.screen_has("ZQXW_1750_LSP_DIAG_MARKER"),
                "precondition: the popup must not appear before the dwell \
                 delay elapses and a tick polls it"
            );

            // No key press, no click — only time passing and the idle tick
            // the real TUI runner drives between input batches.
            std::thread::sleep(std::time::Duration::from_millis(15));
            driver.tick();

            assert!(
                driver.screen_has("ZQXW_1750_LSP_DIAG_MARKER"),
                "dwelling over a word with an LSP-sourced diagnostic must, \
                 with no intervening key press or click, paint the hover \
                 popup once the dwell timer elapses (#1750); screen:\n{}",
                driver.screen()
            );

            // ── Dismiss (#1750 review round 0, finding #2) ────────────
            // Moving off the word must *repaint* without the popup, driven
            // by nothing but the `MouseMoved` itself. This is the fail-first
            // test for the `had_hover != engine.editor_hover.is_some()` ->
            // `self.draw_needed.set(true)` line the hover arm in
            // `App::handle_dispatch` grew: `TuiDriver::dispatch` re-renders
            // the cell grid *only* when the app returns
            // `EventOutcome::Redraw`, and `handle_dispatch` only returns
            // `Reaction::Redraw` when `draw_needed` is set. So without that
            // line the engine clears `editor_hover` but no frame is drawn
            // and the marker is still in `driver.screen()` — exactly the
            // "popup lingers until some unrelated event forces a frame"
            // symptom the review described. (`x - 1.0` is the space at
            // column 3, one cell left of `needle`'s `n` — whitespace, so
            // `editor_hover_mouse_move`'s own off-word branch clears
            // `editor_hover` directly rather than going through the match
            // arm's `dismiss_editor_hover()`; both dismiss paths funnel
            // through the same `draw_needed` comparison.)
            assert!(x >= 1.0, "the fixture's word must not start at column 0");
            hover_move(driver, x - 1.0, y);
            assert!(
                !driver.screen_has("ZQXW_1750_LSP_DIAG_MARKER"),
                "moving the mouse off the hovered word must repaint without \
                 the hover popup, with no other event forcing a frame \
                 (#1750 review); screen:\n{}",
                driver.screen()
            );

            assert_no_lsp_server_was_spawned(&h.engine.borrow());

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// Plugin-sourced hover (`vimcode.editor.set_hover`, surfaced to the
        /// engine as `Engine::editor_hover_content`) — #1750's triage identified
        /// every extension hover (git-insights' blame hover included) shares the
        /// same dwell-trigger mechanism, so it broke identically. Plugin/annotation
        /// hover content only counts once the mouse is past the end of the line's
        /// real text (`Engine::editor_hover_mouse_move`'s `on_annotation` gate) —
        /// the ghost-text region a real inline annotation
        /// (`Engine::line_annotations`, e.g. inline blame) paints into — so
        /// `line_annotations` is set purely to give the dwell point a painted
        /// anchor to find on screen; the content asserted on below is the
        /// plugin's own `editor_hover_content`, not the annotation text
        /// (`show_editor_hover_at_inner` prefers that section over the
        /// annotation one — see that function's own section ordering).
        #[test]
        fn plugin_set_hover_dwell_paints_popup_via_mouse_move_1750() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "let x = 1;\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // A freshly constructed `Engine` has no recorded settings-file
            // mtime, so the very first idle tick's `settings_file_changed`
            // check (`App::handle_poll_tick`) unconditionally reloads
            // `engine.settings` from whatever real `settings.json` this
            // machine has — clobbering any field this test sets beforehand.
            // Flush that one-time reload before configuring the fixture, so
            // `hover_delay` below survives it (confirmed: without this,
            // `driver.tick()` further down logs a "Settings reloaded"
            // status message and resets `hover_delay` back to its on-disk
            // value, so the dwell timer's elapsed check never fires in the
            // 15ms this test sleeps for).
            driver.tick();

            {
                let mut engine = h.engine.borrow_mut();
                engine.settings.hover_delay = 1;
                engine
                    .line_annotations
                    .insert(0, "ZQXW_1750_BLAME_ANNOTATION".to_string());
                engine
                    .editor_hover_content
                    .insert(0, "ZQXW_1750_PLUGIN_HOVER_MARKER".to_string());
            }
            driver.render();

            let (ax, ay) = driver
                .find("ZQXW_1750_BLAME_ANNOTATION")
                .expect("the inline annotation must be painted after the line");

            hover_move(driver, ax, ay);
            assert!(
                !driver.screen_has("ZQXW_1750_PLUGIN_HOVER_MARKER"),
                "precondition: the popup must not appear before the dwell \
                 delay elapses and a tick polls it"
            );

            std::thread::sleep(std::time::Duration::from_millis(15));
            driver.tick();

            assert!(
                driver.screen_has("ZQXW_1750_PLUGIN_HOVER_MARKER"),
                "dwelling over a plugin-annotated region must, with no \
                 intervening key press or click, paint the plugin's own \
                 hover content once the dwell timer elapses (#1750); \
                 screen:\n{}",
                driver.screen()
            );
        }

        /// Review finding on the first #1750 fix: `ClickTarget::BufferPos`'s
        /// `(line, col)` come from the *hovered* window's own scroll offset
        /// (`render::window_zone_hit_test`), but `Engine::editor_hover_
        /// mouse_move` reads `self.buffer()` — the *active* window's buffer
        /// — unconditionally. Resolving a `BufferPos` that discards its
        /// window id (binding it to `_`) therefore arms the dwell timer with
        /// a `(line, col)` pair that means one thing in the window physically
        /// under the pointer and something else entirely in whichever
        /// window happens to be focused. With two splits open on two
        /// different files, dwelling over the *inactive* pane must not
        /// touch the dwell state at all — it must be treated exactly like
        /// hovering outside the editor (dismiss any visible popup, arm
        /// nothing new).
        ///
        /// RED-verified: this test is built so the bug is observable
        /// without waiting out any delay or touching diagnostics/LSP at
        /// all. `fixture_a.rs`'s line 0 has a word (`needle`) at column 4;
        /// `fixture_b.rs`'s line 0 has a *different* word (`zzzzz`) at that
        /// same column 4 — chosen so the buggy, window-id-discarding match
        /// arm would still find a word character at `(0, 4)` when it wrongly
        /// looks it up in the *active* window's buffer and would arm
        /// `editor_hover_dwell` regardless of which window the pointer is
        /// actually over. Fixed, the match arm's `wid == engine.active_window_id()`
        /// guard rejects `fixture_a.rs`'s window outright and the dwell timer
        /// is never armed.
        #[test]
        fn hovering_an_inactive_split_does_not_arm_dwell_against_the_wrong_buffer_1750() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1750_split_hover_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&dir).expect("create fixture dir");
            let path_a = dir.join("fixture_a.rs");
            let path_b = dir.join("fixture_b.rs");
            std::fs::write(&path_a, "let needle = 1;\n").expect("write fixture_a");
            std::fs::write(&path_b, "let zzzzz = 9;\n").expect("write fixture_b");

            let mut engine = plain_engine();
            // Write 1 of 2 — see `lsp_diagnostic_hover_dwell_paints_popup_
            // via_mouse_move_1750`'s doc comment: both `new_tab` and
            // `split_window` open a file, i.e. both run `lsp_did_open`, which
            // would spawn a real `rust-analyzer` for these `.rs` fixtures
            // unless `lsp_enabled` is already false *here*, before them.
            engine.settings.lsp_enabled = false;
            engine.new_tab(Some(&path_a));
            let window_a = engine.active_window_id();
            engine.split_window(SplitDirection::Horizontal, Some(&path_b));
            let window_b = engine.active_window_id();
            assert_ne!(
                window_a, window_b,
                "precondition: splitting with a different file must focus a \
                 genuinely different window"
            );

            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;
            // The first idle tick replaces the whole `settings` struct, so both
            driver.tick();
            {
                let mut engine = h.engine.borrow_mut();
                engine.settings.hover_delay = 1;
                engine.settings.lsp_enabled = false;
            }
            driver.render();

            assert_eq!(
                h.engine.borrow().active_window_id(),
                window_b,
                "precondition: window B (fixture_b.rs) must still be the \
                 active/focused window after the split"
            );
            assert!(
                h.engine.borrow().editor_hover_dwell.is_none(),
                "precondition: nothing has dwelt yet"
            );

            let (x, y) = driver
                .find("needle")
                .expect("fixture_a.rs's word, painted in the unfocused pane");

            hover_move(driver, x, y);

            assert!(
                h.engine.borrow().editor_hover_dwell.is_none(),
                "dwelling over the INACTIVE split (fixture_a.rs, window A) \
                 must not arm the dwell timer at all — not even against \
                 the active window's (fixture_b.rs) own buffer content at \
                 that same (line, col) — because the hovered pixel belongs \
                 to a window that isn't focused (#1750 review)"
            );

            // Confirm it stays that way even after the dwell delay elapses
            // and a tick polls it: no popup from either file's content.
            std::thread::sleep(std::time::Duration::from_millis(15));
            driver.tick();
            assert!(
                h.engine.borrow().editor_hover.is_none(),
                "no hover popup must appear from dwelling over an inactive \
                 split; screen:\n{}",
                driver.screen()
            );

            assert_no_lsp_server_was_spawned(&h.engine.borrow());

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // The hover popup's `command:definition` link must navigate
    // and dismiss the popup.
    //
    // This covers already-shipped code (`render::route_editor_hover_popup_click`
    // and `apply_editor_hover_popup_route`'s `Command` arm, both believed fixed
    // by c05914e / #755) — not a new `src/app.rs` change. It could not be
    // confirmed before #1750's fix because mouse hover could never open the
    // popup in the first place; now that it can, this closes that gap.
    // ─────────────────────────────────────────────────────────────────────────
    /// Builds the popup directly via `Engine::show_editor_hover` (not mouse
    /// dwell — this test is about the *click* route, an independent code
    /// path from the dwell-arm fix above) with a literal `[Definition]
    /// (command:definition)` link, places the cursor away from the link's
    /// anchor position, clicks the painted link text, and asserts both
    /// halves of the route's documented contract: the cursor jumps to the
    /// anchor position `execute_hover_goto` records before dispatching the
    /// command, and the popup itself is dismissed (so it doesn't cover the
    /// definition just jumped to — the exact #272/#491 symptom). `lsp_
    /// enabled = false` keeps the `command:definition` dispatch's own `Engine
    /// ::lsp_request_definition()` call a no-op, so this test cannot
    /// accidentally spawn a real LSP server process.
    #[test]
    fn hover_popup_definition_link_click_navigates_and_dismisses_the_popup_272_491() {
        let mut engine = plain_engine();
        engine.buffer_mut().insert(0, "fn foo() {}\ncall_site();\n");
        engine.settings.lsp_enabled = false;
        // Cursor starts on line 2 (1-indexed), away from the link's anchor
        // (line 0, col 0) — so a jump is actually observable.
        engine.view_mut().cursor.line = 1;
        engine.show_editor_hover(
            0,
            0,
            "Go to [Definition](command:definition)",
            crate::core::engine::EditorHoverSource::Lsp,
            true,  // take_focus — matches a real keyboard/dwell-opened popup
            false, // add_goto_links — the markdown above already has one
        );
        let mut h = harness_no_sidebar(engine);
        let driver = &mut h.driver;

        assert!(
            driver.screen_has("Ln 2, Col 1"),
            "precondition: cursor starts on line 2, away from the link's \
             anchor; screen:\n{}",
            driver.screen()
        );

        let (x, y) = driver
            .find("Definition")
            .expect("the hover popup's link text must be painted");
        driver.click(x, y);

        assert!(
            driver.screen_has("Ln 1, Col 1"),
            "clicking the `command:definition` link must move the cursor to \
             the hover popup's anchor position (#272); screen:\n{}",
            driver.screen()
        );
        assert!(
            !driver.screen_has("Definition"),
            "the popup must be dismissed after navigating, so it doesn't \
             cover the definition just jumped to (#491); screen:\n{}",
            driver.screen()
        );
    }

    /// TUI twin of `gtk::testing::editor_popups::editor_hover_popup_
    /// registers_every_bare_url_as_its_own_click_region_on_gtk`. That GTK
    /// test needs a pixel-colour scan to locate each link independently
    /// of its own registered hit rect, because GTK's proportional chrome
    /// font can drift from a char-count estimate; `driver.find`/`click`
    /// here are *not* the same kind of tautology, because TUI's hit
    /// region (`editor_hover_popup_paint`'s `unit_w = 1.0` arm) is exact
    /// per-character-cell arithmetic — the same grid `TestBackend` itself
    /// paints onto — so clicking the position `find` resolves on the real
    /// screen buffer is independent of whatever `editor_hover_link_rects`
    /// recorded. Kept intentionally simpler than the GTK test since there
    /// is no proportional-font drift for this arm to reproduce; what's left
    /// to pin is the shared per-line bare-URL *scanner* not stopping after
    /// the first match, on this backend too.
    #[test]
    fn editor_hover_popup_registers_every_bare_url_as_its_own_click_region_via_shell_app() {
        // Short URLs, deliberately: the 80-column terminal this module
        // standardises on (see its own module doc) leaves the popup
        // content area well under 80 cells once the sidebar/gutter
        // reserve their own columns, and quadraui's `RichTextPopup`
        // doesn't re-wrap a bare-URL token that doesn't fit — it's
        // simply clipped off-screen. The GTK twin's longer
        // `example.com/first504`-style URLs fit its 1400px fixture
        // without that constraint.
        let markdown = "Bold504 then http://ex.co/first504 and http://ex.co/second504 end504";
        let first_url = "http://ex.co/first504";
        let second_url = "http://ex.co/second504";

        fn fresh_popup(
            markdown: &str,
        ) -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let mut engine = plain_engine();
            engine.show_editor_hover(
                1,
                4,
                markdown,
                crate::core::engine::EditorHoverSource::Lsp,
                false,
                false,
            );
            harness_no_sidebar(engine)
        }

        // Sanity: both URLs paint at distinct positions on the same row —
        // if the scanner stopped after the first match, only one would paint.
        {
            let h = fresh_popup(markdown);
            let (x1, y1) = h
                .driver
                .find(first_url)
                .expect("the first bare URL must paint");
            let (x2, y2) = h
                .driver
                .find(second_url)
                .expect("the second bare URL must paint");
            assert_eq!(
                y1, y2,
                "this fixture only exercises the per-line scanner bug if \
                 both URLs land on the same popup row; got y1={y1} y2={y2}"
            );
            assert_ne!(
                (x1, y1),
                (x2, y2),
                "the first and second bare URL must paint at distinct \
                 positions, not alias the same spot"
            );
        }

        // Each case gets its own fresh harness/popup — a `Link` click
        // dismisses the popup, so clicking the second link in the same
        // popup instance the first click already closed would trivially
        // prove nothing (same reasoning as the GTK twin).
        let click_and_collect =
            |url_to_click: &str| -> Vec<crate::core::engine::PendingPlatformAction> {
                let mut h = fresh_popup(markdown);
                let (x, y) = h
                    .driver
                    .find(url_to_click)
                    .unwrap_or_else(|| panic!("{url_to_click} must be painted on screen"));
                h.driver.click(x, y);
                let actions = h.engine.borrow().pending_platform_actions.clone();
                actions
            };

        let actions_from_first = click_and_collect(first_url);
        assert!(
            actions_from_first.iter().any(|a| matches!(
                a,
                crate::core::engine::PendingPlatformAction::OpenUrl(u) if u == first_url
            )),
            "clicking the first bare URL must queue OpenUrl({first_url}); \
             got {actions_from_first:?}"
        );

        let actions_from_second = click_and_collect(second_url);
        assert!(
            actions_from_second.iter().any(|a| matches!(
                a,
                crate::core::engine::PendingPlatformAction::OpenUrl(u) if u == second_url
            )),
            "clicking the second bare URL must queue OpenUrl({second_url}) \
             — this is the #504 regression: only the first bare URL was \
             clickable, the rest painted in link color but did nothing; \
             got {actions_from_second:?}"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Key dispatch
    // ─────────────────────────────────────────────────────────────────────────
    mod key_dispatch {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name (#603 baseline): a
        /// plain `KeyPressed` sequence with no modal state open must reach
        /// `Engine::handle_key` and mutate the buffer, establishing the general
        /// fallback is wired at all on `App` too.
        #[test]
        fn key_press_inserts_text_via_shell_app_general_fallback() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            // The editor content band was collapsed by #1425 (unit category gap),
            // then fixed by #1426 (`render::UnitProfile`).
            driver.type_char('i'); // Normal -> Insert
            for c in "ZQXW_TYPED".chars() {
                driver.type_char(c);
            }
            let screen = driver.screen();
            assert!(
                screen.contains("ZQXW_TYPED"),
                "typed text should reach the buffer via Engine::handle_key; screen:\n{screen}"
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: in Normal
        /// mode `build_command_line` renders `engine.message` verbatim onto
        /// the `:`-command row.
        #[test]
        fn render_content_paints_command_line_via_shell_app() {
            let mut engine = plain_engine();
            engine.message = "ZQXW_605_CMDLINE_MARKER".to_string();
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::render_content_paints_command_line_via_shell_app",
                || {
                    let screen = driver.screen();
                    assert!(
                        screen.contains("ZQXW_605_CMDLINE_MARKER"),
                        "engine.message should paint on the command line; screen:\n{screen}"
                    );
                },
            );
        }

        /// `dd` on a two-line buffer must delete the current line — the
        /// Normal-mode operator-pending dispatch path, driven end to end
        /// through the real `App`+`TuiBackend` key pipeline.
        #[test]
        fn dd_deletes_the_current_line() {
            let mut engine = plain_engine();
            engine
                .buffer_mut()
                .insert(0, "ZQXW_LINE_ONE\nZQXW_LINE_TWO\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // The editor content band was collapsed by #1425 (unit category gap),
            // then fixed by #1426 (`render::UnitProfile`).
            assert!(
                driver.screen_has("ZQXW_LINE_ONE"),
                "precondition: the first line must be painted"
            );
            driver.type_char('d');
            driver.type_char('d');
            assert!(
                !driver.screen_has("ZQXW_LINE_ONE") && driver.screen_has("ZQXW_LINE_TWO"),
                "'dd' must delete the current (first) line and leave the \
                 second one painted; screen:\n{}",
                driver.screen()
            );
        }

        /// #1796 reports `dd` leaving the cursor at the previous
        /// column instead of column 0 of the line that moves up. The symptom
        /// was the *painted* status bar, not engine state. `test_nvim_dd_preserves_
        /// column_not_reset_to_zero_1796` in `src/core/engine/tests.rs` pins the
        /// engine-state half; this test pins the rendered half.
        ///
        /// Repro: type `aaa`, Enter, `bbb`, Enter, `ccc`, Escape (cursor on
        /// "ccc", col 2), `k` up onto "bbb"'s last column (col 2), then `dd`.
        /// Oracle-verified against real `nvim --headless` 0.12.5: the resulting
        /// buffer is `aaa`/`ccc` with the cursor on column 3 (1-indexed) of
        /// line 2 — the status bar must paint `Ln 2, Col 3`, not `Ln 2, Col 1`.
        ///
        /// 200-column harness, not this module's usual 80: at 80 columns the
        /// status bar's priority-drop sheds the "Ln N, Col N" segment before
        /// the rightmost ones, so this would pass vacuously (never painted).
        #[test]
        fn dd_preserves_painted_status_bar_column_1796() {
            let engine = plain_engine();
            let mut h = crate::tui_main::testing::conformance_harness(engine, 200, 24);
            collapse_sidebar(&mut h.driver);
            let driver = &mut h.driver;

            driver.type_char('i');
            for c in "aaa".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            for c in "bbb".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            for c in "ccc".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Escape);
            driver.type_char('k');
            driver.type_char('d');
            driver.type_char('d');

            let screen = driver.screen();
            assert!(
                screen.contains("Ln 2, Col 3"),
                "dd must preserve the painted column (clamped), not reset \
                 it to 1 -- see #1796; screen:\n{screen}"
            );
        }

        /// Pressing `o` on a buffer whose
        /// last line becomes the buffer's new last line must still paint
        /// *every* line of the result, each in its own row, in order — the
        /// original bug report's single-line repro ('foo' + `o` + 'bar' +
        /// Escape). The buffer/file content was always correct (`:w` wrote
        /// `foo\nbar\n`); only the TUI *paint* dropped the first line and
        /// shifted the new last line up one row, leaving a blank row below
        /// it. Drives the real `App` + `TuiBackend` key pipeline (not a
        /// bare `Engine::handle_key` + content-string assertion — the bug
        /// is specifically in what gets painted, not in the buffer).
        ///
        /// RED-verified in-process: the earlier claim here ("Confirmed NOT
        /// RED — `TuiDriver` dispatches and renders each key synchronously
        /// with no gap") was wrong. `TuiDriver::tick()` is public and routes
        /// straight to `App::tick` -> `render::run_shared_tick_chores` —
        /// the exact function this bug lives in — so interleaving one
        /// `driver.tick()` between the initial render and the `o` keystroke
        /// reproduces the pre-fix sequence without any pty: the tick caches
        /// `rw.lines.len()` (1, for this one-line buffer) as the window's
        /// viewport height, and the `o` keystroke's `ensure_cursor_visible`
        /// then scrolls line 0 out of view to keep the cursor's new line
        /// "visible" against that stale 1-row belief. Confirmed red against
        /// the pre-fix `rw.lines.len().max(1)` in `run_shared_tick_chores`
        /// (`src/render.rs`), green against the
        /// `rw.visible_line_capacity.max(1)` fix.
        #[test]
        fn opening_a_line_below_the_last_line_paints_every_line_in_order_1779() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQXW_FOO\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            assert!(
                driver.screen_has("ZQXW_FOO"),
                "precondition: the single existing line must be painted"
            );
            // Caches the window's viewport height from this frame's paint
            // (`run_shared_tick_chores`) — the pre-fix version of that
            // function pinned it to the buffer's current 1-line length
            // instead of the window's real row capacity, reproducing the
            // bug the moment the next keystroke grows the buffer.
            driver.tick();

            driver.type_char('o');
            for c in "ZQXW_BAR".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Escape);

            let screen = driver.screen();
            let foo_pos = driver.find("ZQXW_FOO");
            let bar_pos = driver.find("ZQXW_BAR");
            assert!(
                foo_pos.is_some() && bar_pos.is_some(),
                "both the original first line and the line opened below it \
                 must still be painted somewhere on screen; screen:\n{screen}"
            );
            let (_, foo_y) = foo_pos.unwrap();
            let (_, bar_y) = bar_pos.unwrap();
            assert!(
                bar_y > foo_y,
                "the line opened with 'o' must paint strictly below the \
                 original first line, not replace it; foo_y={foo_y} \
                 bar_y={bar_y} screen:\n{screen}"
            );
            assert_eq!(
                bar_y,
                foo_y + 1.0,
                "the new line must paint immediately below the first line \
                 with no blank row between them; foo_y={foo_y} bar_y={bar_y} \
                 screen:\n{screen}"
            );
        }

        /// Same root cause as #1779
        /// (`run_shared_tick_chores` pinning `view.viewport_lines` to the
        /// *previous frame's painted line count* instead of the window's
        /// real row capacity), reached through a different repro: typing
        /// several lines of new text in **Insert** mode on a trivially-short
        /// buffer that fits the whole viewport. Each `driver.tick()` stands in
        /// for the real runner's idle tick between input batches, interleaved
        /// before every keystroke that could grow the buffer.
        ///
        /// RED-verified by reverting `run_shared_tick_chores`'s `rw.visible_line_capacity.
        /// max(1)` back to `rw.lines.len().max(1)`: with the single-line
        /// starting buffer, the first tick caches a 1-row viewport, and the
        /// Enter that grows the buffer to 2 lines scrolls line 0 out of view.
        /// GREEN again with that line restored (fixed by #1779): `RenderedWindow::
        /// visible_line_capacity` is the window's row capacity independent of
        /// buffer content.
        #[test]
        fn multiline_insert_does_not_hide_earlier_lines_1783() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;
            driver.tick();
            driver.type_char('i');
            for c in "ZQXW_AAA".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.tick();
            for c in "ZQXW_BBB".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.tick();
            for c in "ZQXW_CCC".chars() {
                driver.type_char(c);
            }
            driver.tick();
            driver.press_named(quadraui::NamedKey::Escape);

            let screen = driver.screen();
            assert!(
                driver.screen_has("ZQXW_AAA"),
                "the first inserted line must not have scrolled out of view; screen:\n{screen}"
            );
            assert!(
                driver.screen_has("ZQXW_BBB"),
                "the second inserted line must not have scrolled out of view; screen:\n{screen}"
            );
            assert!(
                driver.screen_has("ZQXW_CCC"),
                "the third (cursor) line must be painted; screen:\n{screen}"
            );
        }

        /// #1783 (bugbash:mac-native), second repro from the same issue:
        /// deleting a line with `dd` and pasting it back below with `p`
        /// must not hide earlier lines — same `run_shared_tick_chores`
        /// mechanism as [`multiline_insert_does_not_hide_earlier_lines_1783`]
        /// above, via the buffer shrinking (by `dd`) and then growing again
        /// (by `p`) instead of via Insert-mode typing. Buffer starts as `aaa/bbb/
        /// ccc`; `j` moves to `bbb`, `dd` deletes it (cursor lands on `ccc`),
        /// `p` pastes it back below `ccc`, producing `aaa/ccc/bbb`.
        ///
        /// RED-verified the same way: reverting `run_shared_tick_chores`'s
        /// `rw.visible_line_capacity.max(1)` to `rw.lines.len().max(1)`
        /// makes this fail with `aaa` missing from the screen. GREEN again
        /// with the fix restored (fixed by #1779).
        #[test]
        fn dd_then_p_does_not_hide_earlier_lines_1783() {
            let mut engine = plain_engine();
            engine
                .buffer_mut()
                .insert(0, "ZQXW_AAA\nZQXW_BBB\nZQXW_CCC\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;
            driver.tick();
            driver.type_char('j'); // move onto ZQXW_BBB
            driver.tick();
            driver.type_char('d');
            driver.type_char('d'); // delete ZQXW_BBB; cursor lands on ZQXW_CCC
            driver.tick();
            driver.type_char('p'); // paste ZQXW_BBB back below ZQXW_CCC
            driver.tick();

            let screen = driver.screen();
            assert!(
                driver.screen_has("ZQXW_AAA"),
                "the first (untouched) line must not have scrolled out of view; screen:\n{screen}"
            );
            assert!(
                driver.screen_has("ZQXW_CCC"),
                "the line the cursor stayed on across the delete must still be painted; screen:\n{screen}"
            );
            assert!(
                driver.screen_has("ZQXW_BBB"),
                "the pasted-back line must be painted; screen:\n{screen}"
            );
        }

        /// A real-pty run of `tests/smoke-spec/tui.yaml`'s `vim-dw-deletes-word`
        /// journey caught the menu bar's "Go" dropdown appearing mid-sequence
        /// over a plain `dw` and never going away. `TuiDriver::type_char`'s
        /// synthetic `KeyPressed` never carries `alt: true` (same terminal-decode
        /// limitation `TuiDriver` structurally cannot reach), so that collapse
        /// is not reproduced here. What *is* reachable and fixable: once the
        /// dropdown is open, an unrecognised `KeyPressed` fell through to
        /// `MenuEvent::Ignored`, and pre-fix `App::handle_dispatch` left the
        /// dropdown open while the keystroke kept flowing to Vim underneath,
        /// so the buffer was edited while the dropdown painted on top of it.
        /// RED-verified by reverting the new `MenuEvent::Ignored` arm.
        #[test]
        fn alt_g_dropdown_does_not_survive_a_vim_dw_1763() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "foo bar baz\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char('g'),
                modifiers: quadraui::Modifiers {
                    alt: true,
                    ..quadraui::Modifiers::default()
                },
                repeat: false,
            });
            assert!(
                driver.screen_has("Go to File"),
                "precondition: Alt+g must open the \"Go\" dropdown; screen:\n{}",
                driver.screen()
            );
            // `0 w d w`: move to "bar", then `dw` deletes it — the exact
            // `vim-dw-deletes-word` keystrokes from `tui.yaml`.
            driver.type_char('0');
            driver.type_char('w');
            driver.type_char('d');
            driver.type_char('w');
            assert!(
                !driver.screen_has("Go to File"),
                "the stale \"Go\" dropdown must not survive a key it doesn't \
                 recognise; screen:\n{}",
                driver.screen()
            );
            assert!(
                driver.screen_has("foo baz"),
                "the `dw` motion must still reach the editor underneath (not \
                 get swallowed by the dropdown); screen:\n{}",
                driver.screen()
            );
        }

        /// A real-pty run of `tests/smoke-spec/tui.yaml`'s `vim-dd-deletes-line`
        /// setup caught literal text being inserted instead of opening the ex
        /// command-line — #1763's own fix comment identified the root cause:
        /// "a real macOS pty apparently collapsing a fast Escape-then-letter into
        /// an Alt+g chord ... the mis-decoded keystroke is still swallowed by
        /// whichever handle_alt_char/menu-open arm actually consumes it upstream".
        ///
        /// Standing in for that collapse: starts in **Insert** mode (mirroring the
        /// preceding `i`/typing step) and dispatches `Alt+g` directly in place of
        /// an Escape-then-`'g'` the pty fused into one chord. Before this fix,
        /// that chord opened the "Go" dropdown *and* swallowed the Escape it
        /// stood in for — the engine never left Insert mode, so every following
        /// keystroke inserted literally instead of running as Vim
        /// motions/ex-commands.
        ///
        /// **Verified RED against unfixed `develop`:** before this fix, the
        /// final buffer read `g0wdw:%dfoo bar baz` (every keystroke inserted
        /// as literal Insert-mode text) and the status bar stayed on `INSERT`.
        #[test]
        fn colon_opens_the_command_line_after_an_alt_chord_swallows_escape_1764() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "foo bar baz\n");
            engine.handle_key("i", Some('i'), false);
            assert_eq!(
                engine.mode,
                crate::core::Mode::Insert,
                "precondition: the journey enters this sequence from Insert mode"
            );
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // Stand-in for a real pty collapsing Escape and the first 'g' of
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char('g'),
                modifiers: quadraui::Modifiers {
                    alt: true,
                    ..quadraui::Modifiers::default()
                },
                repeat: false,
            });
            assert!(
                !driver.screen_has("Go to File"),
                "the Alt+g chord must not open the \"Go\" dropdown while \
                 the engine is mid-text-entry; screen:\n{}",
                driver.screen()
            );
            // The menu *row* itself must
            // stay hidden, not just the dropdown's items — `route_menu_bar_
            // reveal`'s own Alt+<letter> shim (which flips
            // `engine.menu_bar_visible`, a separate half of this action from
            // the dropdown-open gate just above) must be gated the same way,
            // or the fused chord leaves a permanently-revealed, empty bar
            // row consuming a terminal row with nothing in it. "File" is the
            // menu row's own always-first label (see `hamburger_relocated_
            // click_after_reveal_hides_menu_bar_via_app_on_tui`'s identical
            // precondition check), so its absence here pins the row itself,
            // not just this one dropdown's contents.
            assert!(
                !driver.screen_contains("File"),
                "the Alt+g chord must not reveal the menu-bar row at all \
                 while the engine is mid-text-entry — a revealed-but-empty \
                 row is the same stray-artifact family as #1763's stuck \
                 dropdown; screen:\n{}",
                driver.screen()
            );
            assert!(
                driver.screen_has("NORMAL"),
                "the Alt+g chord must be treated as the Escape it stood in \
                 for, returning to Normal mode, not left stuck in Insert; \
                 screen:\n{}",
                driver.screen()
            );

            // The second 'g' of 'gg', then the exact `vim-dw-deletes-word`
            // keystrokes from `tui.yaml`.
            driver.type_char('g');
            driver.type_char('0');
            driver.type_char('w');
            driver.type_char('d');
            driver.type_char('w');
            assert!(
                driver.screen_has("foo baz"),
                "'dw' must run as a Vim motion (not insert literal text) \
                 now that the engine recovered to Normal mode; screen:\n{}",
                driver.screen()
            );

            // The exact `rebuild5-clear-*` ex-command sequence from
            // `tui.yaml`: `:%d<Enter>` must clear the whole buffer, not get
            // typed as literal characters.
            driver.type_char(':');
            driver.type_char('%');
            driver.type_char('d');
            assert!(
                driver.screen_has("COMMAND"),
                "':' must open the ex command-line, not insert a literal \
                 ':'; screen:\n{}",
                driver.screen()
            );
            driver.press_named(quadraui::NamedKey::Enter);
            assert!(
                !driver.screen_has("foo baz"),
                "':%d<Enter>' must clear the buffer, not leave 'foo baz' \
                 (or a literal 'g0wdw:%d') behind; screen:\n{}",
                driver.screen()
            );
            assert!(
                driver.screen_has("NORMAL"),
                "the buffer-clearing ex command must leave the engine back \
                 in Normal mode; screen:\n{}",
                driver.screen()
            );
        }

        /// Backs up the `MenuEvent::Ignored` arm's own comment claim: a plain
        /// Escape close leaves the toggleable menu bar's row itself on screen —
        /// only the dropdown's items go away.
        #[test]
        fn escape_closes_the_dropdown_but_leaves_the_toggleable_bar_row_visible_1763() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char('g'),
                modifiers: quadraui::Modifiers {
                    alt: true,
                    ..quadraui::Modifiers::default()
                },
                repeat: false,
            });
            assert!(
                driver.screen_has("Go to File"),
                "precondition: Alt+g must open the \"Go\" dropdown; screen:\n{}",
                driver.screen()
            );

            driver.press_named(quadraui::NamedKey::Escape);

            assert!(
                !driver.screen_has("Go to File"),
                "Escape must close the open dropdown; screen:\n{}",
                driver.screen()
            );
            assert!(
                driver.screen_has("Go"),
                "Escape closing the dropdown must leave the toggleable bar \
                 row itself (the \"Go\" label) on screen; screen:\n{}",
                driver.screen()
            );
        }

        /// `u` after `dd` must restore the deleted line — the undo stack, same
        /// key pipeline as [`dd_deletes_the_current_line`].
        #[test]
        fn undo_restores_after_dd() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQXW_UNDO_LINE\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // The editor content band was collapsed by #1425 (unit category gap),
            // then fixed by #1426 (`render::UnitProfile`).
            driver.type_char('d');
            driver.type_char('d');
            driver.type_char('u');
            assert!(
                driver.screen_has("ZQXW_UNDO_LINE"),
                "'u' after 'dd' must restore the deleted line; screen:\n{}",
                driver.screen()
            );
        }

        /// `i` then `Escape` must return to Normal mode — a typed key after
        /// `Escape` must not insert further text.
        #[test]
        fn escape_returns_to_normal_mode_after_insert() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            // The editor content band was collapsed by #1425 (unit category gap),
            // then fixed by #1426 (`render::UnitProfile`).
            driver.type_char('i');
            for c in "ZQXWESC".chars() {
                driver.type_char(c);
            }
            assert!(
                driver.screen_has("ZQXWESC"),
                "precondition: the typed marker text must be painted \
                 before Escape can be meaningfully tested; screen:\n{}",
                driver.screen()
            );
            driver.press_named(quadraui::NamedKey::Escape);
            // In Normal mode, 'x' deletes the character under the cursor
            // rather than inserting — if Escape didn't work, this 'x' would
            // instead insert a literal 'x' into the buffer.
            driver.type_char('x');
            assert!(
                !driver.screen_has("ZQXWESCx"),
                "'x' after Escape must delete under the cursor (Normal \
                 mode), not insert a literal 'x' (would mean Escape never \
                 left Insert mode); screen:\n{}",
                driver.screen()
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Activity bar
    //
    // Every test below is a mechanical port of its `shell_app.rs` namesake
    // (the pre-#1434 TUI shell's `new_for_test`/`new(None)` → [`plain_engine`]/
    // [`harness`], `app.engine.*` → the local `Engine` before it's handed to
    // [`harness`]). See this module's own "No production code here" doc.
    // ─────────────────────────────────────────────────────────────────────────
    mod activity_bar {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name: Ctrl-L
        /// while the activity bar holds the keyboard must **not** activate
        /// the selected item (`render::activity_bar_key_action` guards
        /// `Activate` on `!ctrl`, shared by both backends) — a bare `l`
        /// immediately afterwards must still activate, pairing the negative
        /// with a positive so a fixture that simply cannot activate could
        /// not pass this test by accident.
        #[test]
        fn activity_bar_ctrl_l_does_not_activate_via_shell_app() {
            use crate::core::engine::sidebar::TOOLBAR_IDX_SETTINGS;

            let mut engine = plain_engine();
            engine.activity_bar_focus_in_at(TOOLBAR_IDX_SETTINGS);
            let mut h = harness(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::activity_bar_ctrl_l_does_not_activate_via_shell_app",
                || {
                    let before = driver.screen();
                    assert!(
                        !before.contains("Settings"),
                        "precondition: the Settings panel must not already be \
                         open; screen:\n{before}"
                    );

                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Char('l'),
                        modifiers: quadraui::Modifiers {
                            ctrl: true,
                            ..quadraui::Modifiers::default()
                        },
                        repeat: false,
                    });

                    let after_ctrl = driver.screen();
                    assert!(
                        !after_ctrl.contains("Settings"),
                        "Ctrl-L in the activity bar must not activate the \
                         selected item; screen:\n{after_ctrl}"
                    );

                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Char('l'),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });

                    let after_plain = driver.screen();
                    // Either casing: unlike the click path (title-case
                    // `"Settings"`, from `AppShell`'s own `PanelDefinition`
                    // tooltip — see `render.rs:19074`), the keyboard-
                    // activation path paints the settings panel's own
                    // all-caps `"SETTINGS"` header. Not a functional gap —
                    // both headers name the same panel — so this checks
                    // either, matching `sidebar_panels::extensions_icon_
                    // click_paints_header`'s identical `||` for the same
                    // reason.
                    assert!(
                        after_plain.contains("Settings") || after_plain.contains("SETTINGS"),
                        "a bare `l` on the Settings slot must still activate \
                         it; screen:\n{after_plain}"
                    );
                },
            );
        }

        /// Ctrl-W h from a keyboard-focused sidebar panel (not the
        /// activity bar itself) moves focus onto the activity-bar
        /// toolbar and leaves every subsequent key reaching it: `j`/`k`
        /// walk the toolbar cursor and Enter activates whatever icon it
        /// lands on, twice in a row. `build_activity_bar` always paints
        /// `is_keyboard_focused: false`, so quadraui's own
        /// `preprocess_event` layer never reroutes a raw `KeyPressed`
        /// into a `UiEvent::ActivityBar(..)` consumed entirely by
        /// quadraui's `ShellAdapter`; every key instead reaches
        /// `route_sidebar_chord_key`/`handle_activity_bar_key` in
        /// `src/app/input.rs`, which is what actually drives the
        /// toolbar's focused-cursor movement and activation.
        #[test]
        fn ctrl_w_h_from_sidebar_panel_reaches_activity_bar_toolbar() {
            use crate::core::engine::sidebar::PANEL_EXPLORER;

            let mut engine = plain_engine();
            engine.focus_sidebar_panel(PANEL_EXPLORER);
            let mut h = harness(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::ctrl_w_h_from_sidebar_panel_reaches_activity_bar_toolbar",
                || {
                    let before = driver.screen();
                    assert!(
                        !before.contains("SOURCE CONTROL"),
                        "precondition: Source Control must not already be \
                         the active panel; screen:\n{before}"
                    );

                    // Ctrl-W, h: Explorer panel -> activity-bar toolbar,
                    // parked at Explorer's own toolbar index.
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Char('w'),
                        modifiers: quadraui::Modifiers {
                            ctrl: true,
                            ..quadraui::Modifiers::default()
                        },
                        repeat: false,
                    });
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Char('h'),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });
                    // j, j: Explorer -> Search -> Source Control (`PANEL_GIT`).
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Char('j'),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Char('j'),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Named(quadraui::NamedKey::Enter),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });

                    let after = driver.screen();
                    assert!(
                        after.contains("SOURCE CONTROL"),
                        "j, j, Enter after Ctrl-W h must land on and \
                         activate Source Control; screen:\n{after}"
                    );

                    // Enter already left keyboard focus on the Source
                    // Control panel itself (`sc_has_focus`), not the
                    // toolbar, so a second Ctrl-W h from here re-enters the
                    // toolbar directly -- the cursor must still be
                    // reachable a second time, parked now at Source
                    // Control's own index, not reset to Explorer's.
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Char('w'),
                        modifiers: quadraui::Modifiers {
                            ctrl: true,
                            ..quadraui::Modifiers::default()
                        },
                        repeat: false,
                    });
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Char('h'),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });
                    // j: Source Control -> Run and Debug (`PANEL_DEBUG`).
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Char('j'),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Named(quadraui::NamedKey::Enter),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });

                    let after2 = driver.screen();
                    assert!(
                        after2.contains("RUN AND DEBUG"),
                        "a second Ctrl-W h / j / Enter round must still \
                         reach and activate Run and Debug; screen:\n{after2}"
                    );
                },
            );
        }

        /// Ctrl-W l (or Right) from the activity-bar toolbar, when a sidebar
        /// panel is already visible, must move focus to that panel — the
        /// mirror of `ctrl_w_h_from_sidebar_panel_reaches_activity_bar_toolbar`.
        #[test]
        fn ctrl_w_right_from_toolbar_focuses_panel() {
            use crate::core::engine::sidebar::PANEL_EXPLORER;

            let mut engine = plain_engine();
            engine.focus_sidebar_panel(PANEL_EXPLORER);
            let mut h = harness(engine);
            let driver = &mut h.driver;

            assert!(
                h.engine.borrow().explorer_has_focus,
                "precondition: Explorer must have focus"
            );

            // Ctrl-W, h: Explorer panel -> activity-bar toolbar.
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char('w'),
                modifiers: quadraui::Modifiers {
                    ctrl: true,
                    ..quadraui::Modifiers::default()
                },
                repeat: false,
            });
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char('h'),
                modifiers: quadraui::Modifiers::default(),
                repeat: false,
            });

            assert!(
                h.engine.borrow().activity_bar_focused,
                "after Ctrl-W h, toolbar must have focus"
            );

            // Ctrl-W, Right: toolbar -> Explorer panel.
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char('w'),
                modifiers: quadraui::Modifiers {
                    ctrl: true,
                    ..quadraui::Modifiers::default()
                },
                repeat: false,
            });
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Named(quadraui::NamedKey::Right),
                modifiers: quadraui::Modifiers::default(),
                repeat: false,
            });

            let engine = h.engine.borrow();
            assert!(
                !engine.activity_bar_focused,
                "toolbar must lose focus after Ctrl-W Right"
            );
            assert!(
                engine.explorer_has_focus,
                "Explorer must gain focus after Ctrl-W Right from toolbar"
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: clicking
        /// each activity-bar icon in turn must switch the sidebar's own
        /// *content*, not just its chrome — every icon located via
        /// `driver.find`, never a stored coordinate (this file's own
        /// `collapse_sidebar` doc explains why a stale coordinate can
        /// silently exercise a different code path). The hamburger is
        /// clicked last, after every real panel, for the same reason the
        /// mirrored test does: revealing the menu bar shifts the whole
        /// activity bar down by one row, and there is nothing left to
        /// click afterwards.
        #[test]
        fn driver_click_on_every_activity_bar_icon_opens_its_panel_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1053_activity_bar_all_targets_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let marker_file = dir.join("zqxw1053.txt");
            std::fs::write(&marker_file, "marker").unwrap();

            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.explorer_reveal_path(&marker_file);
            let mut h = harness(engine);
            let driver = &mut h.driver;
            // Without this, the six back-to-back `driver.click()`
            // calls (no simulated time between them) let quadraui's
            // `TuiBackend::translate_injected` fold any pair landing within
            // its double-click time/radius window into a `DoubleClick`.
            // #1762's `App::handle_dispatch` rung now rescues that case by
            // replaying it as a plain `MouseDown`, so folding is no longer
            // load-bearing — left disabled regardless for determinism. Same root
            // cause as this module's own `collapse_sidebar` doc.
            driver.set_double_click_folding(false);

            // Unlike the mirrored `shell_app.rs` test, `App`'s shadow
            // `engine.app_shell` defaults its *own* active panel to
            // Explorer regardless of the runner chrome's separate
            // hamburger-active default (this module's own
            // `collapse_sidebar` doc) — so the marker is already
            // painted here, not hidden. Confirmed by
            // `key_dispatch`/`sidebar_panels`' own fixtures, which
            // never depend on this precondition either way.
            assert!(
                driver.screen_has("zqxw1053.txt"),
                "precondition: Explorer is the shadow engine's default \
                         active panel, so its content paints even before any \
                         click; screen:\n{}",
                driver.screen()
            );
            assert!(
                !driver.screen_has("File"),
                "precondition: menu bar starts hidden; screen:\n{}",
                driver.screen()
            );

            // Located by chrome zone id, not `driver.find`'s glyph search:
            // several fallback icons are single ASCII characters that also
            let mut click_icon_and_expect = |panel_id: &str, marker: &str, label: &str| {
                let bounds = driver
                    .inventory()
                    .zones()
                    .iter()
                    .find(|z| z.id.as_str() == panel_id)
                    .map(|z| z.bounds)
                    .unwrap_or_else(|| {
                        panic!(
                            "{label} icon must register a chrome zone; \
                                     screen:\n{}",
                            driver.screen()
                        )
                    });
                driver.click(
                    bounds.x + bounds.width / 2.0,
                    bounds.y + bounds.height / 2.0,
                );
                let screen = driver.screen();
                assert!(
                    screen.contains(marker),
                    "clicking the {label} icon must open its panel via \
                             the real App path (marker {marker:?} missing); \
                             screen:\n{screen}"
                );
            };

            use crate::core::engine::sidebar::{
                PANEL_EXTENSIONS, PANEL_GIT, PANEL_SEARCH, PANEL_SETTINGS,
            };
            click_icon_and_expect(PANEL_SEARCH, "Replace…", "Search");
            click_icon_and_expect(PANEL_GIT, "SOURCE CONTROL", "Source Control");
            // This was gated as "#1430: clicking Extensions right after
            click_icon_and_expect(PANEL_EXTENSIONS, "EXTENSIONS", "Extensions");
            click_icon_and_expect(PANEL_SETTINGS, "SETTINGS", "Settings");
            click_icon_and_expect(
                crate::core::engine::sidebar::PANEL_EXPLORER,
                "zqxw1053.txt",
                "Explorer",
            );

            let (hx, hy) = driver
                .find(crate::icons::HAMBURGER.s())
                .expect("hamburger icon must paint on the activity bar");
            driver.click(hx, hy);
            assert!(
                driver.screen_has("File"),
                "clicking the hamburger must reveal the menu bar via \
                         the real App path; screen:\n{}",
                driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// The active panel's icon must paint a left-edge accent line
        /// from vimcode's own theme (`theme.activity_active_accent`), not
        /// quadraui's unset `None` default (`ActivityBar::active_accent`'s
        /// own doc: "`None` paints zero accent-line pixels"). `App::handle`
        /// (`src/app/shell.rs`) sets the runner's own `AppShell` accent on
        /// every real dispatch, so clicking the Explorer icon below both
        /// activates Explorer and is itself the real dispatch that leaves
        /// the accent painted.
        #[test]
        fn activity_bar_paints_active_accent_from_theme_via_shell_app() {
            let mut engine = plain_engine();
            engine.settings.colorscheme = "vscode-light".to_string();
            let mut h = harness(engine);
            let driver = &mut h.driver;

            let bounds = driver
                .inventory()
                .zones()
                .iter()
                .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_EXPLORER)
                .map(|z| z.bounds)
                .unwrap_or_else(|| {
                    panic!(
                        "Explorer icon must register a chrome zone; screen:\n{}",
                        driver.screen()
                    )
                });
            driver.click(
                bounds.x + bounds.width / 2.0,
                bounds.y + bounds.height / 2.0,
            );

            // The accent column is the activity bar's own left edge —
            // quadraui's `compose/app_shell.rs` registers each item's zone
            // starting at that same `x` (`layout.activity_bar_bounds.x`), so
            // `bounds.x`/`bounds.y` already address it directly.
            let accent_x = bounds.x as u16;
            let accent_y = bounds.y as u16;
            let (glyph, style) = driver
                .styled_row(accent_y)
                .into_iter()
                .nth(accent_x as usize)
                .expect("the active panel's accent column must paint a styled cell");

            let theme = crate::render::Theme::light();
            let expected_fg = quadraui::tui::ratatui_color(theme.activity_active_accent);

            // Independent of the colour assertion below: quadraui's TUI
            // rasteriser (`quadraui::tui::activity_bar::draw_activity_bar_with_style`)
            // only ever writes the `'▎'` glyph into this column when
            // `ActivityBar::active_accent` is `Some`; its unset `None`
            // default leaves the column's background-fill space character.
            assert_eq!(
                glyph,
                '▎',
                "the active panel's accent column must paint quadraui's \
                 accent glyph, not the plain background fill left by its \
                 unset `active_accent: None` default; screen:\n{}",
                driver.screen()
            );
            assert_eq!(
                style.fg,
                expected_fg,
                "the active panel's accent column must paint with \
                 theme.activity_active_accent, not quadraui's unset `None` \
                 default; screen:\n{}",
                driver.screen()
            );
        }

        /// Clicking exercises the mouse path only; the issue's actual trigger is
        /// keyboard navigation (`Ctrl-W Left` to give the activity bar keyboard
        /// focus, then `j`/`l` to move the cursor and activate the item under
        /// it) — `src/app/shell.rs`'s `App::handle` reaches this through
        /// `on_shell_event_ctx`, the same early-return path quadraui's
        /// `ShellAdapter::handle` takes for a mouse click. This drives that
        /// sequence directly (activity-bar keyboard focus is itself the `Ctrl-W
        /// Left` destination — see `Engine::activity_bar_focus_in_at`) and
        /// asserts the accent follows the newly activated panel rather than
        /// staying on the one it left.
        #[test]
        fn activity_bar_keyboard_activation_moves_active_accent_via_shell_app() {
            use crate::core::engine::sidebar::{PANEL_EXPLORER, PANEL_SEARCH};

            let mut engine = plain_engine();
            engine.settings.colorscheme = "vscode-light".to_string();
            let explorer_idx = engine
                .activity_bar_idx_for_item_id(PANEL_EXPLORER)
                .expect("Explorer must have a keyboard (toolbar) index");
            engine.activity_bar_focus_in_at(explorer_idx);
            let mut h = harness(engine);
            let driver = &mut h.driver;

            assert!(
                !driver.screen_has("SEARCH"),
                "precondition: the Search panel must not already be open; screen:\n{}",
                driver.screen()
            );

            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char('j'),
                modifiers: quadraui::Modifiers::default(),
                repeat: false,
            });
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char('l'),
                modifiers: quadraui::Modifiers::default(),
                repeat: false,
            });

            assert!(
                driver.screen_has("SEARCH"),
                "`j` then `l` must move the keyboard cursor to Search and \
                 activate it; screen:\n{}",
                driver.screen()
            );

            let zone_bounds = |id: &str| {
                driver
                    .inventory()
                    .zones()
                    .iter()
                    .find(|z| z.id.as_str() == id)
                    .map(|z| z.bounds)
                    .unwrap_or_else(|| {
                        panic!(
                            "{id} icon must register a chrome zone; screen:\n{}",
                            driver.screen()
                        )
                    })
            };
            let explorer_bounds = zone_bounds(PANEL_EXPLORER);
            let search_bounds = zone_bounds(PANEL_SEARCH);

            let theme = crate::render::Theme::light();
            let expected_fg = quadraui::tui::ratatui_color(theme.activity_active_accent);

            let (explorer_glyph, _) = driver
                .styled_row(explorer_bounds.y as u16)
                .into_iter()
                .nth(explorer_bounds.x as usize)
                .expect("Explorer's accent column must paint a styled cell");
            assert_ne!(
                explorer_glyph,
                '▎',
                "Explorer is no longer the active panel after keyboard-\
                 activating Search, so its accent column must not paint the \
                 accent glyph; screen:\n{}",
                driver.screen()
            );

            let (search_glyph, search_style) = driver
                .styled_row(search_bounds.y as u16)
                .into_iter()
                .nth(search_bounds.x as usize)
                .expect("Search's accent column must paint a styled cell");
            assert_eq!(
                search_glyph,
                '▎',
                "the keyboard-activated Search panel's accent column must paint \
                 quadraui's accent glyph; screen:\n{}",
                driver.screen()
            );
            assert_eq!(
                search_style.fg,
                expected_fg,
                "the keyboard-activated Search panel's accent column must paint \
                 with theme.activity_active_accent; screen:\n{}",
                driver.screen()
            );
        }

        /// `tests/smoke-spec/tui.yaml`'s header comment claimed the real-terminal
        /// activity-bar row order is Explorer/Search/Debug/Source-Control/Extensions/
        /// AI/Board. The *actual* order — `sidebar::FIXED_ACTIVITY_PANEL_IDS`,
        /// per `render::build_activity_bar`'s own `debug_assert_eq!` — is
        /// Explorer/Search/Source-Control/Debug/Extensions/AI/Board (#1698 put Git
        /// before Debug, to match VS Code's own ordering).
        ///
        /// This pins the exact row→panel mapping at the Tier-1 level, using
        /// the *same addressing scheme* the Tier-2 pty spec's `click` steps
        /// use — plain cell-unit `(col, row)` coordinates, row 0 reserved
        /// for the hamburger toggle, rows 1..=7 the fixed panels in order —
        /// rather than this file's usual `driver.inventory()`/`driver.find`
        /// zone lookup, precisely so a future row/order mismatch fails here,
        /// in-process. `quadraui::tui::testing::TuiDriver::click`'s own doc
        /// confirms `(x, y)` are "cell units for TUI", so `(1.5, row + 0.5)`
        /// lands in the middle of the icon cell at that row exactly like a
        /// real SGR click would.
        ///
        /// RED-verified by hand: swapping this list's row-3/row-4 entries
        /// — encoding the spec's own wrong claim — makes the run report the
        /// wrong panel is painted, because row 3 really paints `SOURCE CONTROL`.
        #[test]
        fn activity_bar_row_click_order_matches_fixed_activity_panel_ids() {
            let mut h = harness(plain_engine());
            let driver = &mut h.driver;
            // Six back-to-back clicks with no simulated time between them
            // would otherwise fold pairwise into `DoubleClick`s. #1762's
            // `App::handle_dispatch` rung now rescues most of those folds,
            // but this test's own point is the row→panel mapping, not fold
            // behaviour, so folding stays disabled for determinism.
            driver.set_double_click_folding(false);

            // (row, expected sidebar-header marker, label) — row 0 is the
            // hamburger; rows 1..=7 are `FIXED_ACTIVITY_PANEL_IDS` in
            // order, per `sidebar.rs`'s own "Index mapping: 0 = hamburger,
            // 1..=7 = FIXED_ACTIVITY_PANEL_IDS" doc.
            let expectations: [(f32, &str, &str); 7] = [
                (1.0, "EXPLORER", "Explorer"),
                (2.0, "SEARCH", "Search"),
                (3.0, "SOURCE CONTROL", "Source Control (Git)"),
                (4.0, "RUN AND DEBUG", "Run and Debug"),
                (5.0, "EXTENSIONS", "Extensions"),
                (6.0, "AI", "AI Assistant"),
                (7.0, "BOARD", "Board"),
            ];

            for (row, marker, label) in expectations {
                driver.click(1.5, row + 0.5);
                let screen = driver.screen();
                assert!(
                    screen.contains(marker),
                    "row {row} (col 1) — the exact (row, col) addressing \
                     tests/smoke-spec/tui.yaml's own `click` steps use — \
                     must open {label} (marker {marker:?} missing); \
                     screen:\n{screen}"
                );
            }
        }

        /// Adjacent activity-bar rows are exactly `1.0` cell apart, inside
        /// quadraui's `DOUBLE_CLICK_RADIUS = 1.5` TUI cells, so two genuinely
        /// distinct real clicks on *adjacent* icons landing within
        /// `DOUBLE_CLICK_MS = 400` of each other fold into one synthesized
        /// `UiEvent::DoubleClick`. `quadraui::AppShell::handle` only matches a
        /// plain `MouseDown` for activity-bar hit-testing, so that `DoubleClick`
        /// resolves as `AppShellEvent::Ignored` and the second click is silently
        /// dropped. The sidebar then stays on whatever panel was already active
        /// until the *next* real click. This test fixes that.
        ///
        /// This test does NOT prove this is the mechanism behind #1762's
        /// reported run: the issue's own repro pairs every click with an
        /// `expect_within` and `wait_idle`, comfortably over the 400ms fold
        /// window. Separately, the issue's "activity-bar-explorer-reselect-1636"
        /// step clicks non-adjacent rows (a 4.0-cell gap, far outside the
        /// 1.5-cell radius), so no timing makes that step droppable by this
        /// mechanism. This fix stands on its own as a genuine adjacency-fold
        /// bug fix, not as a demonstrated resolution of #1762's exact reported
        /// run.
        ///
        /// This replays `tests/smoke-spec/tui.yaml`'s activity-bar section
        /// (same six real clicks, same rows, in order), but with **no simulated
        /// time between clicks** — the worst case for the 400ms fold window,
        /// deliberately more aggressive than the spec's own paced timing.
        #[test]
        fn activity_bar_adjacent_clicks_are_not_dropped_by_double_click_fold_1762() {
            let mut h = harness(plain_engine());
            let driver = &mut h.driver;
            // Double-click folding left at its production DEFAULT (on) --
            // this scenario exists specifically to prove a real click
            // sequence survives it, unlike every other test in this module
            // that calls `set_double_click_folding(false)` to sidestep it.

            driver.click(1.5, 1.5); // row 1: Explorer
            assert!(
                driver.screen().contains("EXPLORER"),
                "precondition: clicking Explorer must open it; screen:\n{}",
                driver.screen()
            );

            driver.click(1.5, 4.5); // row 4: tui.yaml's stale "-1636" step (actually Debug)
            assert!(
                driver.screen().contains("RUN AND DEBUG"),
                "precondition: row 4 opens Debug (FIXED_ACTIVITY_PANEL_IDS, \
                 #1698/#1740); screen:\n{}",
                driver.screen()
            );

            driver.click(1.5, 3.5); // row 3: tui.yaml's corrected "-1740" Source Control step
            assert!(
                driver.screen().contains("SOURCE CONTROL"),
                "clicking Source Control right after Debug (adjacent rows, \
                 zero delay) must switch to it, not get folded into a \
                 dropped DoubleClick; screen:\n{}",
                driver.screen()
            );

            driver.click(1.5, 4.5); // row 4: tui.yaml's corrected "-1740" Debug step
            assert!(
                driver.screen().contains("RUN AND DEBUG"),
                "clicking Debug right after Source Control (adjacent rows, \
                 zero delay) must switch to it, not get folded into a \
                 dropped DoubleClick; screen:\n{}",
                driver.screen()
            );

            driver.click(1.5, 5.5); // row 5: Extensions
            assert!(
                driver.screen().contains("EXTENSIONS"),
                "clicking Extensions right after Debug (adjacent rows, zero \
                 delay) must switch to it -- this is the bugbash's own \
                 'activity-bar-extensions-switches-panel-1636' failure, \
                 which found \"RUN AND DEBUG\" instead; screen:\n{}",
                driver.screen()
            );

            driver.click(1.5, 1.5); // row 1: Explorer again
            assert!(
                driver.screen().contains("EXPLORER"),
                "re-clicking Explorer must switch back to it -- this is the \
                 bugbash's own 'activity-bar-explorer-reselect-1636' \
                 failure, which found \"RUN AND DEBUG\" instead; \
                 screen:\n{}",
                driver.screen()
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: a
        /// hamburger click with the sidebar closed beforehand must not
        /// panic through the real dispatch pipeline, and must reveal the
        /// menu bar.
        ///
        /// Uses [`harness_no_sidebar`], not the bare [`harness`] the
        /// mirrored `shell_app.rs` test uses: `App`'s fresh runner
        /// `AppShell` boots with the hamburger (index 0) already active
        /// (this module's own `collapse_sidebar` doc), so a hamburger
        /// click against an *un*collapsed fixture lands on
        /// `handle_activity_click`'s "already active" branch and never
        /// reaches the reveal this test means to exercise —
        /// `harness_no_sidebar`'s two real Explorer clicks move the active
        /// panel off the hamburger first, so this click genuinely is the
        /// hamburger's first activation, sidebar collapsed, same as the
        /// mirrored test's own precondition.
        #[test]
        fn driver_hamburger_click_sidebar_closed_does_not_panic() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::driver_hamburger_click_sidebar_closed_does_not_panic",
                || {
                    let (hx, hy) = driver
                        .find(crate::icons::HAMBURGER.s())
                        .expect("hamburger icon must paint on the activity bar");
                    driver.click(hx, hy);
                    let screen = driver.screen();
                    assert!(
                        screen.contains("File"),
                        "hamburger click should have opened the menu bar; \
                         screen:\n{screen}"
                    );
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: the
        /// hamburger clicked twice in a row — the second click lands on the
        /// now-active hamburger item and toggles it back off — must not
        /// panic. See [`driver_hamburger_click_sidebar_closed_does_not_panic`]'s
        /// own doc for why this uses [`harness_no_sidebar`].
        #[test]
        fn driver_hamburger_click_twice_does_not_panic() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::driver_hamburger_click_twice_does_not_panic",
                || {
                    let (hx, hy) = driver
                        .find(crate::icons::HAMBURGER.s())
                        .expect("hamburger icon must paint on the activity bar");
                    driver.click(hx, hy);
                    assert!(
                        driver.screen_has("File"),
                        "first hamburger click should open the menu bar; \
                         screen:\n{}",
                        driver.screen()
                    );
                    driver.click(hx, hy);
                    let _ = driver.screen();
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: hamburger
        /// click with the sidebar already open on a real panel (Explorer)
        /// beforehand must not panic.
        #[test]
        fn driver_hamburger_click_with_sidebar_open_does_not_panic() {
            let mut h = harness(plain_engine());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::driver_hamburger_click_with_sidebar_open_does_not_panic",
                || {
                    let (ex, ey) = driver
                        .find(crate::icons::EXPLORER.s())
                        .expect("explorer icon must paint on the activity bar");
                    driver.click(ex, ey); // opens the sidebar
                    let (hx, hy) = driver
                        .find(crate::icons::HAMBURGER.s())
                        .expect("hamburger icon must paint on the activity bar");
                    driver.click(hx, hy);
                    let _ = driver.screen();
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: the first
        /// real key event after a hamburger click must not panic. See
        /// [`driver_hamburger_click_sidebar_closed_does_not_panic`]'s own
        /// doc for why this uses [`harness_no_sidebar`].
        #[test]
        fn driver_hamburger_click_then_key_does_not_panic() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::driver_hamburger_click_then_key_does_not_panic",
                || {
                    let (hx, hy) = driver
                        .find(crate::icons::HAMBURGER.s())
                        .expect("hamburger icon must paint on the activity bar");
                    driver.click(hx, hy);
                    assert!(
                        driver.screen_has("File"),
                        "hamburger click must open the menu bar so the \
                         following key press reaches the menu-system \
                         dispatch; screen:\n{}",
                        driver.screen()
                    );
                    let _ = driver.press(quadraui::Key::Char('j'));
                    let _ = driver.screen();
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: combines
        /// the previous two — sidebar open on a real panel, then a
        /// hamburger click, then a key — must not panic.
        #[test]
        fn driver_hamburger_click_then_key_with_sidebar_open_does_not_panic() {
            let mut h = harness(plain_engine());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::driver_hamburger_click_then_key_with_sidebar_open_does_not_panic",
                || {
                    let (ex, ey) = driver
                        .find(crate::icons::EXPLORER.s())
                        .expect("explorer icon must paint on the activity bar");
                    driver.click(ex, ey);
                    let (hx, hy) = driver
                        .find(crate::icons::HAMBURGER.s())
                        .expect("hamburger icon must paint on the activity bar");
                    driver.click(hx, hy);
                    let _ = driver.press(quadraui::Key::Char('j'));
                    let _ = driver.screen();
                },
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Menu bar: Selection menu
    // ─────────────────────────────────────────────────────────────────────────
    mod selection_menu {
        use super::*;

        /// VS Code's menu bar is `File · Edit · Selection · View · Go ·
        /// Run · Terminal · Help` — vimcode's dropped `Selection` entirely.
        /// `MENU_STRUCTURE` (`render.rs`) is the one shared static both
        /// backends paint their top-level row from. This is the TUI
        /// twin of `gtk::testing::selection_menu::
        /// menu_bar_has_selection_between_edit_and_view_in_order`, driving the
        /// real `App` through [`quadraui::tui::testing::TuiDriver`] rather
        /// than GTK's Cairo paint path, off the same `MENU_STRUCTURE` data.
        #[test]
        fn menu_bar_has_selection_between_edit_and_view_in_order() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            // The menu bar starts hidden on TUI (same as `activity_bar`'s
            // hamburger-reveal tests above) — reveal it the same way, via a
            // real click through `App`'s dispatch path, before any top-level
            // label can paint.
            let (hx, hy) = driver
                .find(crate::icons::HAMBURGER.s())
                .expect("hamburger icon must paint on the activity bar");
            driver.click(hx, hy);
            assert!(
                driver.screen_has("File"),
                "hamburger click must reveal the menu bar before the \
                 top-level labels below can be located; screen:\n{}",
                driver.screen()
            );

            let labels = [
                "File",
                "Edit",
                "Selection",
                "View",
                "Go",
                "Run",
                "Terminal",
                "Help",
            ];
            let mut xs = Vec::with_capacity(labels.len());
            for label in labels {
                let bounds = driver
                    .find_bounds(label)
                    .unwrap_or_else(|| panic!("top-level menu label {label:?} must paint"));
                xs.push((label, bounds.x));
            }
            for i in 1..xs.len() {
                let (prev_label, prev_x) = xs[i - 1];
                let (label, x) = xs[i];
                assert!(
                    x > prev_x,
                    "menu bar labels must paint left-to-right in VS Code's \
                     order (File, Edit, Selection, View, Go, Run, Terminal, \
                     Help); {label:?} at x={x} did not paint after \
                     {prev_label:?} at x={prev_x}; screen:\n{}",
                    driver.screen()
                );
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Sidebar panels
    // ─────────────────────────────────────────────────────────────────────────
    mod sidebar_panels {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name: the explorer
        /// sidebar's own painted content (a real scratch directory tree, not
        /// just its header) must reach the screen via `App::render_content`.
        #[test]
        fn render_content_paints_explorer_sidebar_content_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vc1425expl_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("mk1425.txt"), b"").unwrap();

            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            // The root row itself is collapsed by default, so its child would
            // never paint without this — the ~20-column sidebar width at this
            // module's `(80, 24)` size leaves no room to also show the (long,
            // thread-id-suffixed) root name, hence the short `vc1425expl_`
            // prefix above rather than this file's usual `vimcode_test_1425_*`
            // convention.
            engine.explorer_expanded.insert(dir);
            engine.explorer_rebuild_rows();
            engine.session.explorer_visible = true;
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            let h = harness(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::render_content_paints_explorer_sidebar_content_via_shell_app",
                || {
                    assert!(
                        driver.screen_has("mk1425.txt"),
                        "the explorer sidebar must paint the scratch directory's \
                     own file; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// The Explorer's selected row, when the tree does **not**
        /// have keyboard focus, must paint with `theme.sidebar_sel_bg_
        /// inactive` — not quadraui's own dark `Theme::default()` navy,
        /// which is what leaked through before `render::
        /// to_quadraui_theme_chrome` mapped `inactive_selected_bg` at all.
        /// Reveals a real file so the row selection is genuine (not
        /// hand-set on the primitive), under `vscode-light` specifically —
        /// the colourscheme the issue reported — with the tree explicitly
        /// unfocused, the same "file matching the active editor tab" case
        /// `sidebar_sel_bg_inactive`'s own doc names.
        #[test]
        fn explorer_unfocused_selected_row_paints_sidebar_sel_bg_inactive_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vc1574explinact_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let marker = dir.join("zz1574.txt");
            std::fs::write(&marker, "marker").unwrap();

            let mut engine = plain_engine();
            engine.settings.colorscheme = "vscode-light".to_string();
            engine.cwd = dir.clone();
            engine.explorer_reveal_path(&marker);
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            engine.session.explorer_visible = true;
            engine.explorer_has_focus = false;
            let h = harness(engine);
            let driver = &h.driver;

            let (x, y) = driver
                .find("zz1574.txt")
                .expect("the revealed file must paint in the explorer sidebar");
            let style = driver
                .style_at(x as u16, y as u16)
                .expect("the revealed row must paint a styled cell");

            let expected_bg =
                quadraui::tui::ratatui_color(crate::render::Theme::light().sidebar_sel_bg_inactive);
            assert_eq!(
                style.bg,
                expected_bg,
                "an unfocused Explorer's selected row must paint with \
                 `sidebar_sel_bg_inactive`, not quadraui's own dark default; \
                 screen:\n{}",
                driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// The sidebar header row (the `" EXPLORER "` strip above the
        /// tree, painted by quadraui's `AppShell::render`) must come from
        /// vimcode's own theme, not quadraui's hard-coded literal.
        /// `to_quadraui_theme_chrome` (`src/render.rs`) has mapped
        /// `header_bg`/`header_fg` from `theme.status_bg`/`theme.status_fg`,
        /// but until quadraui#1180 `AppShell::render` never read those fields
        /// — it painted the literal directly. Confirmed RED against the pre-
        /// quadraui#1180 pin: the painted header is quadraui's literal, not
        /// the theme's value.
        #[test]
        fn sidebar_header_paints_vimcode_theme_not_quadraui_dark_literal_via_shell_app() {
            let mut engine = plain_engine();
            engine.settings.colorscheme = "vscode-light".to_string();
            let mut h = harness(engine);
            let driver = &mut h.driver;

            // The shadow `engine.app_shell` already defaults its active
            // panel to Explorer (so the tree content paints from the very
            // first frame — see this module's `driver_click_on_every_
            // activity_bar_icon_opens_its_panel_via_shell_app` for the same
            // precondition), but the *runner's own*, separate `AppShell`
            // (what `quadraui::AppShell::render` actually paints the header
            // chrome from) starts on the hamburger panel until a real click
            // lands — `App::take_requested_panel`'s own doc explains the
            // shadow/runner split. Click the Explorer activity-bar icon
            // (located by chrome zone id, not glyph — the Nerd-Fonts-off
            // fallback icons are ambiguous single ASCII chars) to drive a
            // real dispatch and pick up the runner's header.
            let bounds = driver
                .inventory()
                .zones()
                .iter()
                .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_EXPLORER)
                .map(|z| z.bounds)
                .unwrap_or_else(|| {
                    panic!(
                        "Explorer icon must register a chrome zone; screen:\n{}",
                        driver.screen()
                    )
                });
            driver.click(
                bounds.x + bounds.width / 2.0,
                bounds.y + bounds.height / 2.0,
            );

            let (x, y) = driver
                .find("EXPLORER")
                .expect("the sidebar header must paint its panel title");
            let style = driver
                .style_at(x as u16, y as u16)
                .expect("the header title must paint a styled cell");

            let theme = crate::render::Theme::light();
            let expected_bg = quadraui::tui::ratatui_color(theme.status_bg);
            let expected_fg = quadraui::tui::ratatui_color(theme.status_fg);
            // quadraui's old hard-coded header literal, `#252526` /
            // `Color::rgb(220, 220, 220)` — must NOT show up here.
            let old_literal_bg = quadraui::tui::ratatui_color(quadraui::Color::rgb(37, 37, 38));

            assert_eq!(
                style.bg,
                expected_bg,
                "sidebar header bg must come from theme.status_bg (vimcode's \
                 theme, via `to_quadraui_theme_chrome`'s header_bg mapping), \
                 not quadraui's hard-coded dark literal; screen:\n{}",
                driver.screen()
            );
            assert_ne!(
                style.bg,
                old_literal_bg,
                "sidebar header bg must not be quadraui's old #252526 literal \
                 under a light theme; screen:\n{}",
                driver.screen()
            );
            assert_eq!(
                style.fg,
                expected_fg,
                "sidebar header fg must come from theme.status_fg, not \
                 quadraui's hard-coded dark literal; screen:\n{}",
                driver.screen()
            );
        }

        /// The search sidebar's `"  NN: "` line-number prefix must
        /// paint with `theme.line_number_fg` — before this issue,
        /// `populate_search_sidebar_system` hard-coded `Color::rgb(100, 100,
        /// 100)` for that span regardless of colourscheme, which read as an
        /// odd, unthemed grey specifically under a light colourscheme like
        /// `vscode-light` (its own `line_number_fg` is a teal, `#237893`,
        /// nothing like the mid-grey literal). Uses a line number
        /// (`9999`) picked to be a `find`-unique needle — unlikely to
        /// collide with any other on-screen text — so the located cell is
        /// unambiguously the prefix span, not the match text next to it.
        #[test]
        fn search_result_line_number_prefix_paints_theme_line_number_fg_via_shell_app() {
            let mut engine = plain_engine();
            engine.settings.colorscheme = "vscode-light".to_string();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_SEARCH,
            ));
            engine.project_search_query = "zz1574".to_string();
            engine.project_search_results = vec![crate::core::project_search::ProjectMatch {
                file: std::path::PathBuf::from("zz1574_file.rs"),
                line: 9998,
                col: 0,
                line_text: "zz1574 needle line".to_string(),
            }];
            let h = harness(engine);
            let driver = &h.driver;

            let (x, y) = driver
                .find("9999:")
                .expect("the search result's line-number prefix must paint");
            let style = driver
                .style_at(x as u16, y as u16)
                .expect("the line-number prefix must paint a styled cell");

            let expected_fg =
                quadraui::tui::ratatui_color(crate::render::Theme::light().line_number_fg);
            assert_eq!(
                style.fg,
                expected_fg,
                "the search sidebar's line-number prefix must paint with \
                 `theme.line_number_fg`, not a hard-coded grey; screen:\n{}",
                driver.screen()
            );
        }

        /// Dotfiles show in the explorer by default (VS Code-style)
        /// but the small `explorer_exclude` list (`.git`, `.svn`, `.hg`,
        /// `.DS_Store`, `Thumbs.db`) stays hidden regardless — driven end to
        /// end through the real tree build (`Engine::explorer_rebuild_rows`
        /// → `build_explorer_rows`/`collect_explorer_rows`), not just
        /// asserted against the `Settings` struct. Confirmed red against
        /// unfixed `develop`: with `show_hidden_files` defaulting to
        /// `false` there, `.dotmk1545` never reaches
        /// `collect_explorer_rows`'s output at all, so the first assertion
        /// below fails.
        #[test]
        fn render_content_shows_dotfiles_but_hides_git_by_default_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vc1545dot_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(".dotmk1545"), b"").unwrap();
            std::fs::create_dir_all(dir.join(".git")).unwrap();
            std::fs::write(dir.join(".git").join("HEAD"), b"").unwrap();

            // Default settings only — no explicit `show_hidden_files` or
            // `explorer_exclude` override, so this exercises the real
            // shipped defaults end to end.
            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.explorer_expanded.insert(dir);
            engine.explorer_rebuild_rows();
            engine.session.explorer_visible = true;
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            let h = harness(engine);
            let driver = &h.driver;

            assert!(
                driver.screen_has(".dotmk1545"),
                "dotfiles must show in the explorer by default (#1545); screen:\n{}",
                driver.screen()
            );
            assert!(
                !driver.screen_has(".git"),
                "'.git' must stay hidden via the default explorer_exclude list \
                 even though dotfiles now show by default (#1545); screen:\n{}",
                driver.screen()
            );
        }

        // No TUI driver test covers the `PANEL_SETTINGS` arm's
        // `background: Some(theme.tab_bar_bg)` fix — confirmed empirically
        // while writing this issue's fix that it can't: on this module's TUI
        // harness, `FormController` already paints an opaque per-row
        // background across the full sidebar width regardless of what
        // `SidebarPanelBody.background` is set to (verified by probing a
        // blank cell past an ordinary row's text with `background: None` vs
        // `Some(theme.tab_bar_bg)` — identical `rgb(236, 236, 236)` either
        // way). The bug is GTK-specific: `settings_panel_scrollbar_gutter_
        // paints_theme_tab_bar_bg` in `src/gtk/testing.rs` covers it instead,
        // where the pixel-level probe genuinely goes RED with `background:
        // None` restored (a mid-grey `rgb(120, 121, 124)` scrollbar-gutter
        // leak) and GREEN with the fix.

        /// Clicking the Search activity-bar icon must switch the sidebar body
        /// to the search panel (the "Replace…" row is unique to it) — located
        /// via its own chrome zone, not a hardcoded coordinate, same technique
        /// `crate::harness::activity_bar_click_focuses_search_panel` uses.
        #[test]
        fn search_icon_click_switches_sidebar_content() {
            let mut h = harness(plain_engine());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::search_icon_click_switches_sidebar_content",
                || {
                    let search_zone = driver
                        .inventory()
                        .zones()
                        .iter()
                        .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_SEARCH)
                        .map(|z| z.bounds)
                        .expect("the Search activity-bar icon must register a chrome zone");
                    assert!(
                        !driver.screen_has("Replace…"),
                        "precondition: startup sidebar does not already show Search"
                    );
                    driver.click(
                        search_zone.x + search_zone.width / 2.0,
                        search_zone.y + search_zone.height / 2.0,
                    );
                    assert!(
                        driver.screen_has("Replace…"),
                        "clicking the Search icon must switch the sidebar body to \
                 the search panel; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// The Settings bottom-item icon must paint the settings form body
        /// ("Appearance" is one of its section headers).
        #[test]
        fn settings_icon_click_paints_appearance_section() {
            let mut h = harness(plain_engine());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::settings_icon_click_paints_appearance_section",
                || {
                    driver.click_text(crate::icons::SETTINGS.s());
                    assert!(
                        driver.screen_has("Appearance"),
                        "clicking the Settings icon must paint the settings form's \
                 Appearance section; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// With `status_bar_under_main_column`, the activity bar runs
        /// the full window height (VS Code layout) while the status bar
        /// and command line span only the main column. The Settings gear
        /// and status bar share a y-row but occupy different x-columns,
        /// so they never overlap.
        #[test]
        fn settings_icon_paints_above_the_status_and_command_rows() {
            let mut h = harness(plain_engine());
            let driver = &mut h.driver;
            driver.render();

            const ROWS: usize = 24;
            let screen = driver.screen();
            let rows: Vec<&str> = screen.lines().collect();
            assert_eq!(rows.len(), ROWS, "harness geometry; screen:\n{screen}");

            let gear = driver
                .inventory()
                .zones()
                .iter()
                .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_SETTINGS)
                .map(|z| z.bounds)
                .unwrap_or_else(|| {
                    panic!(
                        "the Settings activity-bar item must register its \
                         chrome zone; screen:\n{screen}"
                    )
                });

            // The gear must be in the activity bar column (leftmost
            // columns), not overlapping the main-column status bar.
            assert!(
                gear.x < 5.0,
                "the Settings gear must be in the activity bar column, \
                 but its x is {}; screen:\n{screen}",
                gear.x
            );

            // The status bar paints on the second-to-last row, indented
            // past the activity bar — confirming column separation.
            assert!(
                rows[ROWS - 2].contains("NORMAL"),
                "fixture: the status row must paint on row {}; screen:\n{screen}",
                ROWS - 2
            );

            // The command line paints on the last row, past the
            // activity-bar column (which still shows the settings gear).
            driver.type_char(':');
            let screen = driver.screen();
            let rows: Vec<&str> = screen.lines().collect();
            assert!(
                rows[ROWS - 1].contains(':'),
                "fixture: the command line must paint on the last row; \
                 screen:\n{screen}"
            );
        }

        /// The bottom-left strip `shell_config`'s reservation vacates from
        /// `activity_bar_bounds` (two rows tall, as wide as the activity bar)
        /// must paint a themed chrome colour, not whatever the frame clear
        /// left there.
        ///
        /// `status_bar_1690_spans_full_window_width_and_orders_segments_
        /// vs_code_style` (`src/gtk/testing.rs`) already pins that the *status*
        /// row gets a full-width backdrop from `App::render_content`'s
        /// `paint_status_backdrop` call. Nothing plays that role for the
        /// *command-line* row below it: `FrameOp::CommandLine` never reaches
        /// the activity-bar/sidebar column. Before `render::bottom_chrome_
        /// reservation_fill_rect`'s fill, that column at the very last row
        /// showed `theme.background` — the bug.
        ///
        /// RED-verified by temporarily removing the fill call: the sampled
        /// style at `(0, ROWS - 1)` comes back as `theme.background`, not
        /// `theme.tab_bar_bg`.
        #[test]
        fn bottom_left_reclaimed_strip_paints_tab_bar_bg_not_background_via_shell_app() {
            let mut h = harness(plain_engine());
            let driver = &mut h.driver;
            driver.render();

            const ROWS: u16 = 24;
            let theme = crate::render::Theme::from_name(&h.engine.borrow().settings.colorscheme);
            let expected_bg = quadraui::tui::ratatui_color(theme.tab_bar_bg);
            let background = quadraui::tui::ratatui_color(theme.background);

            let style = driver.style_at(0, ROWS - 1).unwrap_or_else(|| {
                panic!(
                    "the command-line row's leading column must paint a \
                     styled cell; screen:\n{}",
                    driver.screen()
                )
            });
            assert_ne!(
                style.bg,
                background,
                "the bottom-left reclaimed strip (0, {}) must not show \
                 the bare frame-clear background colour; screen:\n{}",
                ROWS - 1,
                driver.screen()
            );
            assert_eq!(
                style.bg,
                expected_bg,
                "the bottom-left reclaimed strip (0, {}) must paint \
                 theme.tab_bar_bg, matching the activity bar's own fill \
                 colour just above it; screen:\n{}",
                ROWS - 1,
                driver.screen()
            );
        }

        /// A user who has never touched `:colorscheme` must see vimcode's
        /// own **`dark`** palette on screen — not One Dark, and not VS
        /// Code's own Dark Modern chrome colour.
        ///
        /// The oracle is deliberately spelled as raw RGB literals rather than
        /// derived from [`crate::render::Theme::from_name`], so the test
        /// cannot just read back the very default it exists to pin. `dark`
        /// keeps VS Code Dark Modern's `editor.background` (`0x1f1f1f`)
        /// but repaints the unified status-bar/sidebar/tab-strip chrome in
        /// vimcode's own teal-tinted `0x171b1b`, never VS Code's `0x181818`.
        /// One Dark's `0x282c34` and `0x21252b` are asserted *absent* from
        /// the whole painted grid for the same reason: this test is red
        /// whenever the default resolves to that palette instead.
        ///
        /// **Verified RED against unfixed `develop`:** with
        /// `Theme::dark()`'s `tab_bar_bg`/`status_bg` temporarily reverted
        /// to VS Code Dark Modern's `#181818`, the `VSCODE_DARK_MODERN_CHROME_BG`
        /// assertion below failed at cell (0, 0) with `left: Rgb(24, 24, 24)
        /// / right: Rgb(24, 24, 24)`; restoring `#171b1b` makes it pass.
        #[test]
        fn fresh_engine_paints_vimcode_dark_chrome_via_shell_app() {
            use quadraui::tui::testing::Color;

            let mut h = harness(plain_engine());
            let driver = &mut h.driver;
            driver.render();

            const COLS: u16 = 80;
            const ROWS: u16 = 24;
            const VIMCODE_DARK_EDITOR_BG: Color = Color::Rgb(0x1f, 0x1f, 0x1f);
            const VIMCODE_DARK_CHROME_BG: Color = Color::Rgb(0x17, 0x1b, 0x1b);
            const VSCODE_DARK_MODERN_CHROME_BG: Color = Color::Rgb(0x18, 0x18, 0x18);
            const ONEDARK_EDITOR_BG: Color = Color::Rgb(0x28, 0x2c, 0x34);
            const ONEDARK_CHROME_BG: Color = Color::Rgb(0x21, 0x25, 0x2b);

            let mut saw_editor_bg = false;
            let mut saw_chrome_bg = false;
            for y in 0..ROWS {
                for x in 0..COLS {
                    let Some(style) = driver.style_at(x, y) else {
                        continue;
                    };
                    saw_editor_bg |= style.bg == VIMCODE_DARK_EDITOR_BG;
                    saw_chrome_bg |= style.bg == VIMCODE_DARK_CHROME_BG;
                    assert_ne!(
                        style.bg,
                        ONEDARK_EDITOR_BG,
                        "cell ({x}, {y}) paints One Dark's editor background; \
                         the default colorscheme must be vimcode's own 'dark'; \
                         screen:\n{}",
                        driver.screen()
                    );
                    assert_ne!(
                        style.bg,
                        ONEDARK_CHROME_BG,
                        "cell ({x}, {y}) paints One Dark's chrome background; \
                         the default colorscheme must be vimcode's own 'dark'; \
                         screen:\n{}",
                        driver.screen()
                    );
                    assert_ne!(
                        style.bg,
                        VSCODE_DARK_MODERN_CHROME_BG,
                        "cell ({x}, {y}) paints VS Code Dark Modern's own \
                         chrome background #181818; vimcode's 'dark' theme \
                         must repaint it with its own #171b1b; screen:\n{}",
                        driver.screen()
                    );
                }
            }

            assert!(
                saw_editor_bg,
                "no cell painted 'dark's editor.background #1f1f1f; \
                 screen:\n{}",
                driver.screen()
            );
            assert!(
                saw_chrome_bg,
                "no cell painted 'dark's unified chrome background #171b1b \
                 (status bar / sidebar / tab strip); screen:\n{}",
                driver.screen()
            );
        }

        /// The Extensions activity-bar icon must paint a header naming the
        /// panel — same assertion `crate::harness`'s
        /// `issue_1256_sidebar_chrome::extensions_header_is_painted` uses for
        /// `gtk`/`tui_prod`.
        #[test]
        fn extensions_icon_click_paints_header() {
            let mut h = harness(plain_engine());
            let driver = &mut h.driver;

            known_bug_gate("app_on_tui::extensions_icon_click_paints_header", || {
                driver.click_text(crate::icons::EXTENSIONS.s());
                assert!(
                    driver.screen_has("EXTENSIONS") || driver.screen_has("Extensions"),
                    "clicking the Extensions icon must paint a header naming \
                 it; screen:\n{}",
                    driver.screen()
                );
            });
        }

        /// The Source Control panel must paint its own header when shown —
        /// the `App`+`TuiBackend` twin of `crate::harness`'s
        /// `sc_hint_row_shows_only_while_focused` precondition (that scenario
        /// only ever ran at `(800, 480)`; this is the same panel at a
        /// realistic terminal size).
        #[test]
        fn source_control_panel_paints_header() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1425_sc_panel_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let _ = std::process::Command::new("git")
                .args(["init"])
                .current_dir(&dir)
                .output();

            let mut engine = plain_engine();
            engine.cwd = dir;
            engine.git_branch = Some("main".to_string());
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_GIT,
            ));
            let h = harness(engine);
            let driver = &h.driver;

            known_bug_gate("app_on_tui::source_control_panel_paints_header", || {
                assert!(
                    driver.screen_has("SOURCE CONTROL"),
                    "the SC panel must paint its own header; screen:\n{}",
                    driver.screen()
                );
            });
        }

        /// Build an engine with a real (empty) git repo as `cwd` and the
        /// Source Control panel already shown and focused. `tag` keeps
        /// concurrently-running tests' tmp dirs from colliding.
        fn sc_engine(tag: &str) -> crate::core::Engine {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1432_sc_{tag}_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let _ = std::process::Command::new("git")
                .args(["init"])
                .current_dir(&dir)
                .output();

            let mut engine = plain_engine();
            engine.cwd = dir;
            engine.git_branch = Some("main".to_string());
            engine.sc_has_focus = true;
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_GIT,
            ));
            engine
        }

        /// Ports `panels.rs::sc_panel_tests::empty_commit_message_shows_
        /// placeholder` onto `App`: with no commit message typed yet, the
        /// commit `TextInput` must paint its placeholder rather than an
        /// empty box.
        #[test]
        fn sc_panel_empty_commit_message_shows_placeholder() {
            let h = harness(sc_engine("placeholder"));
            let driver = &h.driver;
            // Missing the trailing ")": this module's sidebar column budget
            // (~20 cols, vs. `panels.rs`'s own isolated-panel harness's 40)
            // truncates the closing paren off the placeholder text — same
            // truncation-tolerance reasoning `explorer_context_menu`'s
            // `context_menu_new_file_starts_inline_edit` applies to its own
            // "file name" (vs. the full "New file name...") assertion.
            assert!(
                driver.screen_has("Message (press c"),
                "expected the commit-input placeholder; screen:\n{}",
                driver.screen()
            );
        }

        /// Ports `panels.rs::sc_panel_tests::active_commit_input_renders_
        /// typed_message_not_placeholder` onto `App`.
        #[test]
        fn sc_panel_active_commit_input_renders_typed_message_not_placeholder() {
            let mut engine = sc_engine("typed");
            engine.sc_commit_message = "Fix the thing".to_string();
            engine.sc_commit_cursor = engine.sc_commit_message.len();
            engine.sc_commit_input_active = true;
            let h = harness(engine);
            let driver = &h.driver;
            assert!(
                driver.screen_has("Fix the thing"),
                "expected the typed commit message; screen:\n{}",
                driver.screen()
            );
            assert!(
                !driver.screen_has("Message (press c)"),
                "placeholder should not show while actively editing; screen:\n{}",
                driver.screen()
            );
        }

        /// Ports `panels.rs::sc_panel_tests::multiline_commit_message_
        /// renders_every_line` onto `App`.
        #[test]
        fn sc_panel_multiline_commit_message_renders_every_line() {
            let mut engine = sc_engine("multiline");
            engine.sc_commit_message = "Summary line\n\nBody line one\nBody line two".to_string();
            engine.sc_commit_cursor = 0;
            engine.sc_commit_input_active = true;
            let h = harness(engine);
            let driver = &h.driver;
            let screen = driver.screen();
            assert!(driver.screen_has("Summary line"), "{screen}");
            assert!(driver.screen_has("Body line one"), "{screen}");
            assert!(driver.screen_has("Body line two"), "{screen}");
        }

        /// Ports `panels.rs::sc_panel_tests::branch_picker_list_mode_
        /// renders_branches_and_marks_current` onto `App`.
        #[test]
        fn sc_panel_branch_picker_list_mode_renders_branches_and_marks_current() {
            let mut engine = sc_engine("branch_list");
            engine.sc_branch_picker_open = true;
            engine.sc_branch_picker_branches = vec![
                crate::core::git::BranchEntry {
                    name: "main".to_string(),
                    is_current: true,
                    upstream: None,
                    ahead_behind: None,
                },
                crate::core::git::BranchEntry {
                    name: "feature/foo".to_string(),
                    is_current: false,
                    upstream: None,
                    ahead_behind: None,
                },
            ];
            let h = harness(engine);
            let driver = &h.driver;
            let screen = driver.screen();
            assert!(driver.screen_has("Switch Branch"), "{screen}");
            assert!(driver.screen_has("main"), "{screen}");
            assert!(driver.screen_has("feature/foo"), "{screen}");
            assert!(
                driver.screen_has("\u{25cf} main"),
                "the current branch must be marked with the current-branch \
                 glyph; screen:\n{screen}"
            );
            assert!(
                !driver.screen_has("\u{25cf} feature/foo"),
                "a non-current branch must not carry the current-branch \
                 glyph; screen:\n{screen}"
            );
        }

        /// Ports `panels.rs::sc_panel_tests::branch_picker_create_mode_
        /// renders_typed_name` onto `App`.
        #[test]
        fn sc_panel_branch_picker_create_mode_renders_typed_name() {
            let mut engine = sc_engine("branch_create");
            engine.sc_branch_create_mode = true;
            engine.sc_branch_create_input = "wip-feature".to_string();
            let h = harness(engine);
            let driver = &h.driver;
            let screen = driver.screen();
            assert!(driver.screen_has("New Branch"), "{screen}");
            assert!(driver.screen_has("wip-feature"), "{screen}");
        }

        /// Ports `panels.rs::sc_panel_tests::help_dialog_renders_
        /// keybindings_table` onto `App`.
        #[test]
        fn sc_panel_help_dialog_renders_keybindings_table() {
            let mut engine = sc_engine("help");
            engine.sc_help_open = true;
            let h = harness(engine);
            let driver = &h.driver;
            let screen = driver.screen();
            assert!(driver.screen_has("Keybindings"), "{screen}");
            // "Naviga[te]": the same ~20-column sidebar budget that eats the
            // placeholder's closing paren above truncates the "Action"
            // column's text too — "Close" survives intact only because its
            // own row happens to fit, so it's left unabbreviated below.
            assert!(driver.screen_has("Naviga"), "{screen}");
            assert!(driver.screen_has("Close"), "{screen}");
        }

        /// A plugin extension panel's own `?`-triggered keybindings
        /// help popup must render with a full 4-sided border and the
        /// title ("Keybindings") embedded in the top border — the same
        /// shape [`sc_panel_help_dialog_renders_keybindings_table`] (just
        /// above) pins for the Source Control panel's own help dialog.
        /// The popup uses `render::ext_panel_help_tooltip_layout` +
        /// `Backend::draw_tooltip_with_chrome` with quadraui's
        /// `TooltipChrome` (`TooltipBorder::Full` + a title) — see
        /// that function's doc for why this is the real fix. Drives the
        /// real `?` keypress through a registered `git-insights` ext panel
        /// activated via the real `render::apply_activity_panel_switch`
        /// path (the same one a real activity-bar click takes), not a
        /// hand-set `ext_panel_help_open = true` — so this test can only
        /// pass if the whole chain (keypress → `Engine::ext_panel_help_bindings`
        /// lookup → paint) is wired up.
        #[test]
        fn ext_panel_help_popup_renders_full_border_and_title() {
            let mut engine = plain_engine();
            engine.ext_panels.insert(
                "git-insights".to_string(),
                crate::core::plugin::PanelRegistration {
                    name: "git-insights".to_string(),
                    title: "Git Insights".to_string(),
                    icon: '\u{f113}',
                    fallback_icon: Some('X'),
                    sections: vec!["Log".to_string()],
                },
            );
            engine.ext_panel_help_bindings.insert(
                "git-insights".to_string(),
                vec![
                    ("j/k".to_string(), "Navigate commits".to_string()),
                    ("q/Esc".to_string(), "Close panel".to_string()),
                ],
            );
            crate::render::apply_activity_panel_switch(&mut engine, "ext:git-insights");

            let mut h = harness(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::ext_panel_help_popup_renders_full_border_and_title",
                || {
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Char('?'),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });
                    let screen = driver.screen();
                    assert!(
                        screen.contains("Keybindings"),
                        "help popup must render its title; screen:\n{screen}"
                    );
                    // "Naviga[te commits]": the narrow sidebar column budget
                    // truncates the Action column, same as
                    // `sc_panel_help_dialog_renders_keybindings_table` observes
                    // for the Source Control dialog's own "Naviga[te]" text.
                    assert!(
                        screen.contains("Naviga"),
                        "help popup must render the panel's own registered \
                     bindings, not the Source Control ones; screen:\n{screen}"
                    );

                    // The regression downgraded this popup to side-bars
                    // only ('│' on the first/last column, no top/bottom rule and
                    // no border-embedded title) — assert the actual box-drawing
                    // glyphs a full 4-sided `TooltipBorder::Full` chrome paints
                    // (top-left/top-right square corners joined by a '─' rule),
                    // not just that *some* text rendered. A tooltip-only
                    // regression would fail these two lines while still passing
                    // the two above.
                    assert!(
                        screen.contains('┌') && screen.contains('┐'),
                        "help popup must paint square top corners (full \
                     border, not side-bars-only); screen:\n{screen}"
                    );
                    let has_top_rule = screen
                        .lines()
                        .any(|line| line.contains('┌') && line.contains('─') && line.contains('┐'));
                    assert!(
                        has_top_rule,
                        "help popup's top border must be a continuous rule \
                     between its corners, not a bare side-bar; screen:\n{screen}"
                    );
                },
            );
        }

        /// Ports `panels.rs::sc_panel_tests::renders_without_panicking_at_
        /// minimum_size` onto `App`: a regression guard that the migrated
        /// `TextInput`/`Palette`/`Dialog` primitives degrade gracefully
        /// instead of panicking when the whole shell (not just the SC panel
        /// in isolation, as the original harness rendered) is squeezed to a
        /// pathologically tiny terminal. Uses a bespoke tiny-sized harness
        /// rather than this module's usual `harness()` (which fixes
        /// `(80, 24)`), same pattern `conformance_harness`'s own callers use
        /// when a scenario needs a non-standard cell size.
        #[test]
        fn sc_panel_renders_without_panicking_at_minimum_size() {
            let mut engine = sc_engine("tiny_commit");
            engine.sc_commit_message = "line one\nline two".to_string();
            engine.sc_commit_input_active = true;
            let h = crate::tui_main::testing::conformance_harness(engine, 10, 3);
            let _ = h.driver.screen();

            let mut engine2 = sc_engine("tiny_help");
            engine2.sc_help_open = true;
            let h2 = crate::tui_main::testing::conformance_harness(engine2, 10, 3);
            let _ = h2.driver.screen();
        }

        /// The CHANGES section must list a
        /// modified tracked file with an 'M' badge and an untracked file
        /// with a 'U' badge once the panel has been refreshed. The report
        /// claimed the CHANGES list stayed completely empty after a
        /// manual refresh in a real repo with exactly this shape (one
        /// modified tracked file, one untracked file). This is a
        /// `src/render.rs`/`src/core/engine/source_control.rs`
        /// shared-engine path — not GTK- or TUI-specific — so a passing
        /// `TuiDriver` scenario here is the Tier-1 shared conformance
        /// scenario the issue's acceptance bar asks for; win-native shares
        /// this exact rendering path (no `src/win/` code touches
        /// `sc_*`/`source_control`, confirmed by grep).
        ///
        /// Two review fixes vs. the first version of this test:
        ///
        /// 1. **Drives `Engine::startup_without_session_restore` instead of
        ///    hand-setting `engine.cwd`.** The bug's actual root cause is
        ///    `startup_inner`'s file-opening arm never touching `cwd` at
        ///    all, so a fixture that sets `engine.cwd = <the file's own
        ///    repo>` directly bypasses the broken code path entirely and
        ///    cannot fail against unfixed `develop`. Here `engine.cwd`
        ///    starts out pointing at an unrelated, repo-less `launch_cwd`
        ///    — standing in for "wherever the process's cwd happened to be
        ///    at launch" — and only `startup_without_session_restore`
        ///    (which calls the fixed `adopt_cwd_for_startup_file`) moves it
        ///    to the file's real repo.
        /// 2. **Clicks the sync/refresh toolbar icon through the driver**
        ///    instead of calling `engine.sc_refresh()` directly, matching
        ///    the report's own repro step ("click the refresh icon"). The
        ///    production GUI path for a *manual* refresh is exactly this
        ///    click → `route_sc_sidebar_click` → `sc_button_hit` →
        ///    `sc_activate_button(3)` → `sc_sync()` → `sc_refresh()`
        ///    dispatch, not a synchronous engine-method call a test
        ///    invented.
        ///
        /// RED-verified: reverting `adopt_cwd_for_startup_file` to a no-op
        /// (the pre-fix `startup_inner` body, which never touched `cwd` at
        /// all) leaves `cwd` at `launch_cwd` — no repo there — so
        /// `sc_refresh()` (reached via the same toolbar click) returns no
        /// files and every `screen_has` assertion below goes red.
        #[test]
        fn sc_panel_changes_section_shows_status_badges_for_modified_and_untracked_files_1797() {
            use crate::core::engine::SC_BUTTON_IDS;

            // The git workspace the file actually lives in: one committed
            // file modified afterward (porcelain " M"), plus one untracked
            // file (porcelain "??") -- the issue's own exact reproduction
            // shape ("git status --short showing 'M main.rs' / '??
            // extra.rs'").
            let workspace = std::env::temp_dir().join(format!(
                "vimcode_test_1797_sc_changes_badges_ws_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&workspace);
            std::fs::create_dir_all(&workspace).unwrap();

            let run_git = |args: &[&str]| {
                let status = std::process::Command::new("git")
                    .args(args)
                    .current_dir(&workspace)
                    .output()
                    .unwrap()
                    .status;
                assert!(status.success(), "git {args:?} failed");
            };
            run_git(&["init"]);
            run_git(&["config", "user.email", "t@t.com"]);
            run_git(&["config", "user.name", "T"]);

            let tracked = workspace.join("main.rs");
            std::fs::write(&tracked, "fn main() {}\n").unwrap();
            run_git(&["add", "."]);
            run_git(&["commit", "-m", "init"]);
            std::fs::write(&tracked, "fn main() { /* changed */ }\n").unwrap();

            let untracked = workspace.join("extra.rs");
            std::fs::write(&untracked, "// new file\n").unwrap();

            // A completely unrelated, repo-less directory standing in for
            // "wherever the process's cwd happened to be at launch" --
            // deliberately *not* `workspace` above, so only the fixed
            // `adopt_cwd_for_startup_file` can make `cwd` land on the
            // file's real repo.
            let launch_cwd = std::env::temp_dir().join(format!(
                "vimcode_test_1797_sc_changes_badges_launch_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&launch_cwd);
            std::fs::create_dir_all(&launch_cwd).unwrap();

            let mut engine = plain_engine();
            engine.cwd = launch_cwd.clone();
            engine.settings.swap_file = false;
            engine.startup_without_session_restore(Some(&tracked));
            engine.git_branch = Some("main".to_string());
            // Deliberately *not* focused yet -- see the Alt+Right loop
            // below, which needs `sidebar_has_focus()` to be false so the
            // chord actually reaches `render::route_alt_key` instead of
            // being swallowed as a plain SC-panel navigation key first.
            // The sync click further down focuses the panel for real, the
            // same way a production click does
            // (`route_sc_sidebar_click`'s `engine.sc_set_focus(true)`).
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_GIT,
            ));

            let mut h = harness(engine);
            let driver = &mut h.driver;
            // Default sidebar width (~20 columns, the same budget
            // `sc_panel_help_dialog_renders_keybindings_table`'s own
            // comment above documents truncating text elsewhere) only
            // leaves room for the Commit button -- Push/Pull/Sync get no
            // painted bounds at all, so the sync click below would have
            // nothing to find. Widen it the real way: `Engine::app_shell`
            // is only a shadow copy the runner's own `AppShell` is never
            // synced from at startup (see `collapse_sidebar`'s own doc),
            // so mutating it directly has no effect here -- the actual
            // production gesture is repeated plain `Alt+Right`
            // (`render::route_alt_key`'s unconditional
            // `AltBase::Right => ResizeSidebar(1)` arm in non-VSCode
            // mode), same as a user holding the real resize chord.
            for _ in 0..20 {
                driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }

            // First paint: materialises `Engine::sc_panel_layout`'s cached
            // toolbar layout, the same cache `Engine::sc_button_hit` reads
            // for a real click -- needed to locate the sync button's
            // painted bounds below instead of guessing coordinates.
            driver.render();
            assert!(
                driver.screen_has("SOURCE CONTROL"),
                "precondition: the SC panel must be showing before the \
                 refresh click is driven; screen:\n{}",
                driver.screen()
            );

            let (sync_x, sync_y) = {
                let engine = h.engine.borrow();
                let layout = engine.sc_panel_layout.borrow();
                let toolbar = layout
                    .as_ref()
                    .and_then(|l| l.toolbar_layout.as_ref())
                    .expect("the SC panel toolbar must have painted a layout");
                let item = toolbar
                    .visible_items
                    .iter()
                    .find(|v| v.action_id.as_ref().map(|id| id.as_str()) == Some(SC_BUTTON_IDS[3]))
                    .expect("the sync/refresh toolbar button must be painted");
                (
                    item.bounds.x + item.bounds.width / 2.0,
                    item.bounds.y + item.bounds.height / 2.0,
                )
            };

            // The report's actual repro gesture: click the sync/refresh
            // icon (`sc:sync`, VS Code's circular-arrows "Sync Changes"
            // icon -- this codebase's own icon constant is
            // `icons::GIT_SYNC`) through the real dispatch pipeline,
            // rather than calling `Engine::sc_refresh()` directly.
            // `sc_activate_button(3)` -> `sc_sync()` tries a pull+push
            // (which fail harmlessly here -- no remote configured) and
            // then *unconditionally* calls `sc_refresh()`, so this
            // exercises the identical production path
            // `route_sc_sidebar_click` wires a real mouse click through.
            driver.click(sync_x, sync_y);

            let screen = driver.screen();
            assert!(
                driver.screen_has("CHANGES"),
                "precondition: the CHANGES section header must be \
                 painted; screen:\n{screen}"
            );
            assert!(
                driver.screen_has("main.rs"),
                "the modified tracked file's name must be painted in the \
                 CHANGES list; screen:\n{screen}"
            );
            assert!(
                driver.screen_has("extra.rs"),
                "the untracked file's name must be painted in the CHANGES \
                 list; screen:\n{screen}"
            );
            assert!(
                driver.screen_has("M main.rs"),
                "the modified tracked file must carry an 'M' status badge; \
                 screen:\n{screen}"
            );
            assert!(
                driver.screen_has("U extra.rs"),
                "the untracked file must carry a 'U' status badge (VS \
                 Code's untracked glyph -- see `StatusKind::label`'s own \
                 doc for why this is 'U', not git's raw '?'); \
                 screen:\n{screen}"
            );

            let _ = std::fs::remove_dir_all(&workspace);
            let _ = std::fs::remove_dir_all(&launch_cwd);
        }

        /// `render::route_sidebar_hover` returns whether
        /// `engine.sc_button_hovered` — the one thing this function paints
        /// synchronously, everything else it touches (`panel_hover_mouse_move`/
        /// `dismiss_panel_hover`) only arms a dwell/dismiss timer the tick
        /// loop's own `poll_panel_hover` resolves and reports separately —
        /// actually changed, mirroring the before/after comparison
        /// `route_gutter_hover`'s own caller already used. This prevents
        /// unnecessary repaints when the pointer moves within the sidebar
        /// without changing which hover target is active.
        ///
        /// Drives the actual bug: a Source Control sidebar open (not the
        /// no-sidebar fixture [`super::mouse_moved_to_the_same_plain_editor_cell_does_not_repaint`]
        /// uses, which structurally never reaches `route_sidebar_hover` at
        /// all since `ctx.layout.sidebar_content_bounds` is `None`),
        /// hovering the same row twice.
        #[test]
        fn mouse_moved_to_the_same_sidebar_row_does_not_repaint() {
            let mut h = harness(sc_engine("hover_1722"));
            let driver = &mut h.driver;
            driver.render();

            let (x, y) = driver
                .find("SOURCE CONTROL")
                .expect("the SC panel header must be painted");

            // First move establishes whatever hover state a move to this
            // sidebar row implies, then settle before taking the comparison
            // snapshot.
            hover_move(driver, x, y);
            let screen0 = driver.screen();

            // Second move to the exact same cell: nothing about the pointer
            // target changed, so no hover state can have changed either.
            let reaction = hover_move(driver, x, y);
            assert_eq!(
                reaction,
                quadraui::Reaction::Continue,
                "a MouseMoved to the same sidebar row as the previous one, \
                 with the sidebar open and touching no hover target, must \
                 not force a repaint (#1722 review)"
            );
            assert_eq!(
                driver.screen(),
                screen0,
                "rendered text must not change from a no-op sidebar hover move"
            );
        }

        /// A second click on the already-active Search icon must toggle the
        /// sidebar closed — mirrors `shell_app.rs`'s
        /// `driver_click_on_search_icon_switches_and_toggles_sidebar`, located
        /// by chrome zone rather than that test's hardcoded `(1.0, 2.0)`.
        #[test]
        fn search_icon_second_click_toggles_sidebar_closed() {
            let mut h = harness(plain_engine());
            let driver = &mut h.driver;
            // Two real clicks close together must land as two independent
            // clicks, not fold into one double-click — same reason
            // `shell_app.rs`'s mirrored
            // `driver_click_on_search_icon_switches_and_toggles_sidebar` calls
            // this before its own two-click sequence.
            driver.set_double_click_folding(false);

            known_bug_gate(
                "app_on_tui::search_icon_second_click_toggles_sidebar_closed",
                || {
                    let search_zone = driver
                        .inventory()
                        .zones()
                        .iter()
                        .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_SEARCH)
                        .map(|z| z.bounds)
                        .expect("the Search activity-bar icon must register a chrome zone");
                    let (cx, cy) = (
                        search_zone.x + search_zone.width / 2.0,
                        search_zone.y + search_zone.height / 2.0,
                    );
                    driver.click(cx, cy);
                    assert!(
                        driver.screen_has("Replace…"),
                        "precondition: first click opens Search"
                    );
                    driver.click(cx, cy);
                    assert!(
                        !driver.screen_has("Replace…"),
                        "a second click on the active Search icon must close the \
                 sidebar; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// An explorer sidebar showing one real file, revealed and focused,
        /// with a temp `cwd` the caller must clean up — the `App`-on-TUI
        /// twin of `shell_app.rs`'s `app_with_focused_explorer`. Returns the
        /// engine plus the temp dir so the test can remove it.
        fn engine_with_focused_explorer(tag: &str) -> (crate::core::Engine, std::path::PathBuf) {
            let dir = std::env::temp_dir().join(format!(
                "vc1430focus_{tag}_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let marker = dir.join("zqxw757.txt");
            std::fs::write(&marker, "marker").unwrap();

            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.explorer_reveal_path(&marker);
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            engine.session.explorer_visible = true;
            engine.explorer_has_focus = true;
            (engine, dir)
        }

        /// Mirrors `shell_app.rs`'s test of the same name: a plugin
        /// panel's own focus must outrank a stale `explorer_has_focus`
        /// left set alongside it — reached via the *real*
        /// keyboard-activation path (`Engine::activity_bar_activate`'s
        /// ext-panel branch), not by hand-set fields.
        #[test]
        fn focused_plugin_panel_outranks_a_stale_explorer_flag_via_shell_app() {
            use crate::core::engine::sidebar::TOOLBAR_IDX_EXT_BASE;

            let (mut engine, dir) = engine_with_focused_explorer("ext_panel_stale");
            engine.explorer_tree.borrow_mut().start_editing(
                vec![0u16],
                "ZQXWEXT757".to_string(),
                "ZQXWEXT757".len(),
                None,
                None,
            );
            engine.ext_panels.clear();
            engine.ext_panels.insert(
                "git-insights".to_string(),
                crate::core::plugin::PanelRegistration {
                    name: "git-insights".to_string(),
                    title: "Git Insights".to_string(),
                    icon: '\u{f113}',
                    fallback_icon: Some('Ж'),
                    sections: Vec::new(),
                },
            );
            // Park the activity bar's keyboard cursor on the (only) plugin
            // panel.
            engine.activity_bar_focus_in_at(TOOLBAR_IDX_EXT_BASE);

            let mut h = harness(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::focused_plugin_panel_outranks_a_stale_explorer_flag_via_shell_app",
                || {
                    let before = driver.screen();
                    assert!(
                        before.contains("ZQXWEXT757"),
                        "precondition: the explorer's inline edit must paint \
                         before the activity bar is activated; screen:\n{before}"
                    );

                    // Activate the plugin panel — the real
                    // `Engine::activity_bar_activate` ext-panel branch, which
                    // leaves `explorer_has_focus` stale-true.
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Char('l'),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });
                    let mid = driver.screen();
                    assert!(
                        !mid.contains("ZQXWEXT757"),
                        "precondition: the plugin panel must now own the \
                         sidebar body; screen:\n{mid}"
                    );

                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Named(quadraui::NamedKey::Backspace),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });

                    // Reveal: click the explorer's own activity-bar icon to
                    // switch the visible panel back — the same production
                    // panel-changed path a real click takes.
                    let (ex, ey) = driver
                        .find(crate::icons::EXPLORER.s())
                        .expect("explorer icon must paint on the activity bar");
                    driver.click(ex, ey);

                    let after = driver.screen();
                    assert!(
                        after.contains("ZQXWEXT757"),
                        "a focused plugin panel must outrank a stale explorer \
                         focus flag — Backspace must not have reached the \
                         explorer's inline edit; screen:\n{after}"
                    );
                },
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// Mirrors `shell_app.rs`'s test of the same name: a real focus
        /// flag must outrank the merely *visible* explorer panel — the
        /// explorer is left as the default visible panel while
        /// `settings_has_focus` is set directly, the state a plugin or
        /// future codepath that sets the flag without also calling
        /// `app_shell.show_panel` would produce.
        #[test]
        fn focused_settings_panel_outranks_the_default_visible_explorer_via_shell_app() {
            let (mut engine, dir) =
                engine_with_focused_explorer("settings_flag_vs_visible_explorer");
            engine.explorer_tree.borrow_mut().start_editing(
                vec![0u16],
                "ZQXWFLAG757".to_string(),
                "ZQXWFLAG757".len(),
                None,
                None,
            );
            // A real, current focus flag — but the visible panel (app_shell's
            // untouched default) is still the explorer.
            engine.settings_has_focus = true;

            let mut h = harness(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::focused_settings_panel_outranks_the_default_visible_explorer_via_shell_app",
                || {
                    let before = driver.screen();
                    assert!(
                        before.contains("ZQXWFLAG757"),
                        "precondition: the explorer's inline edit must paint \
                         (it is still the visible panel); screen:\n{before}"
                    );

                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Named(quadraui::NamedKey::Backspace),
                        modifiers: quadraui::Modifiers::default(),
                        repeat: false,
                    });

                    let after = driver.screen();
                    assert!(
                        after.contains("ZQXWFLAG757"),
                        "a real settings_has_focus must outrank the \
                         merely-visible explorer panel — Backspace must not \
                         reach the explorer's inline edit; screen:\n{after}"
                    );
                },
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// A terminal pane left focused, then a real
        /// `Engine::focus_sidebar_panel` call (the method every
        /// keyboard-driven focus transfer — Ctrl-W chord, activity-bar
        /// `Activate` — routes through) into Settings must hand the very
        /// next keypress to the settings form, not the stale terminal.
        ///
        /// `focus_sidebar_panel` clears `terminal_has_focus` itself, right
        /// after calling the shared `Engine::clear_sidebar_focus` and before
        /// setting `settings_has_focus`; without it,
        /// `render::route_focus_key`'s terminal gate (checked before every
        /// sidebar arm) keeps outranking Settings even though the sidebar
        /// is now the real focus owner. The clear lives at this specific
        /// focus-*grant* call site rather than inside `clear_sidebar_focus`
        /// itself, since that function is also called from several focus-
        /// *release* paths (hiding the sidebar, a mouse click landing on the
        /// bottom panel) where a focused terminal must stay focused.
        ///
        /// The needle is Enter on the "Cursor Line" row rather than the
        /// issue's literal colour-scheme picker: both are `settings_flat_list`
        /// rows gated by the exact same focus check, but the colour-scheme
        /// row's value text ("dark" / "vscode-dark") doesn't fit the default
        /// sidebar width, so quadraui's `Form` TUI renderer drops it
        /// entirely rather than truncating it — an unrelated, upstream
        /// rendering-geometry limit this test must not depend on. "Cursor
        /// Line" is a `Bool` setting with a short label, so its `[ ]`/`[x]`
        /// toggle glyph always paints; unswallowed, Enter flips
        /// `settings.cursorline` from its default `true` to `false`.
        #[test]
        fn settings_enter_activates_after_focus_transfer_leaves_terminal_stale() {
            let mut engine = plain_engine();
            engine.terminal_new_tab(80, 10);
            engine.terminal_has_focus = true;

            // The real production focus-transfer call — not a raw flag
            // poke — so this exercises `clear_sidebar_focus` itself.
            engine.focus_sidebar_panel(crate::core::engine::sidebar::PANEL_SETTINGS);
            // Row 0 of `settings_flat_list` is the "Appearance" category
            // header; row 9 is "Cursor Line" (`SETTING_DEFS[8]`).
            engine.settings_selected = 9;

            let mut h = harness(engine);
            let driver = &mut h.driver;

            let cursor_line_row = |screen: &str| -> String {
                screen
                    .lines()
                    .find(|l| l.contains("Cursor Line"))
                    .unwrap_or_default()
                    .to_string()
            };

            let before = cursor_line_row(&driver.screen());
            assert!(
                before.contains("[x]"),
                "precondition: Cursor Line must default to its checked \
                 glyph before Enter is pressed; row:\n{before}"
            );

            driver.press_named(quadraui::NamedKey::Enter);

            let after = cursor_line_row(&driver.screen());
            assert!(
                after.contains("[ ]"),
                "Enter on the Cursor Line row must flip its toggle and \
                 repaint the unchecked glyph — if the terminal still \
                 swallowed the key, the glyph stays checked; row:\n{after}"
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: Alt+Right
        /// must widen the painted sidebar by one column, pushing the editor
        /// one column right; Alt+Left must narrow it straight back.
        /// Asserted through the editor text's painted column, never through
        /// sidebar-width state (CLAUDE.md rule 1: rendered output, not
        /// state). Runs at `(120, 24)`, not this module's usual `(80, 24)`
        /// — the sidebar plus a widened-by-one column needs the extra room.
        #[test]
        fn alt_right_widens_the_painted_sidebar_via_shell_app() {
            let mut engine = plain_engine();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            engine.session.explorer_visible = true;
            engine.buffer_mut().insert(0, "ZQXW759W");
            let mut h = crate::tui_main::testing::conformance_harness(engine, 120, 24);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::alt_right_widens_the_painted_sidebar_via_shell_app",
                || {
                    let alt_press =
                        |driver: &mut quadraui::tui::testing::TuiDriver<_>, key, shift| {
                            driver.dispatch(quadraui::UiEvent::KeyPressed {
                                key,
                                modifiers: quadraui::Modifiers {
                                    alt: true,
                                    shift,
                                    ..Default::default()
                                },
                                repeat: false,
                            });
                        };

                    let before = driver
                        .find_bounds("ZQXW759W")
                        .expect("the editor marker must paint before the resize");
                    alt_press(
                        driver,
                        quadraui::Key::Named(quadraui::NamedKey::Right),
                        false,
                    );
                    let after = driver
                        .find_bounds("ZQXW759W")
                        .expect("the editor marker must still paint after the resize");
                    assert_eq!(
                        after.x,
                        before.x + 1.0,
                        "Alt+Right must widen the painted sidebar by one \
                         column, pushing the editor one column right; \
                         screen:\n{}",
                        driver.screen()
                    );

                    alt_press(
                        driver,
                        quadraui::Key::Named(quadraui::NamedKey::Left),
                        false,
                    );
                    let back = driver
                        .find_bounds("ZQXW759W")
                        .expect("the editor marker must still paint after Alt+Left");
                    assert_eq!(
                        back.x,
                        before.x,
                        "Alt+Left must narrow it straight back; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }
    }

    // Driver-tier black-box coverage for VS Code-mode keybindings
    /// Driver-tier black-box coverage for the five VS Code-mode chords
    /// `route_alt_key`/`handle_vscode_key` decode. Each test dispatches
    /// a real `UiEvent::KeyPressed` through the production
    /// `App::handle_dispatch` → `route_alt_key`/`Engine::handle_key`
    /// path (not a direct `route_alt_key`/`handle_key` call) and asserts
    /// on `driver.screen()` — TUI's driver has a real character grid,
    /// so these can assert on the duplicated/typed editor buffer text
    /// directly, not just a status-bar proxy for it.
    mod vscode_mode_alt_rung_1744 {
        use super::*;

        fn vscode_engine(buffer: &str) -> crate::core::Engine {
            let mut engine = plain_engine();
            engine.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
            engine.mode = crate::core::Mode::Insert;
            engine.buffer_mut().insert(0, buffer);
            engine.view_mut().cursor = crate::core::Cursor { line: 0, col: 0 };
            engine
        }

        fn press(
            driver: &mut quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            key: quadraui::Key,
            modifiers: quadraui::Modifiers,
        ) {
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key,
                modifiers,
                repeat: false,
            });
        }

        /// Ctrl+Alt+Down is VS Code's real `insertCursorBelow` — typing
        /// after it must insert on *two* lines, not just move the line
        /// down. Painted editor text is the strongest possible proof here:
        /// a real second cursor, not a state flag.
        #[test]
        fn ctrl_alt_down_adds_a_cursor_below_in_vscode_mode_via_shell_app() {
            let engine = vscode_engine("aaa\nbbb\n");
            let mut h = harness(engine);
            let driver = &mut h.driver;

            press(
                driver,
                quadraui::Key::Named(quadraui::NamedKey::Down),
                quadraui::Modifiers {
                    ctrl: true,
                    alt: true,
                    ..Default::default()
                },
            );
            press(
                driver,
                quadraui::Key::Char('X'),
                quadraui::Modifiers::default(),
            );

            let screen = driver.screen();
            assert!(
                screen.contains("Xaaa") && screen.contains("Xbbb"),
                "Ctrl+Alt+Down (insertCursorBelow) must add a second \
                 cursor, so typing 'X' inserts on both lines; screen:\n{screen}"
            );
        }

        /// Shift+Alt+Down is VS Code's real `copyLinesDownAction` — it must
        /// duplicate the line (the marker appears twice), not add a cursor
        /// (which would leave it appearing once, with typed text doubling
        /// up instead — a different, distinguishable failure mode this test
        /// doesn't need to also check).
        #[test]
        fn shift_alt_down_duplicates_the_line_in_vscode_mode_via_shell_app() {
            let engine = vscode_engine("ZQXW1744A\nZQXW1744B\n");
            let mut h = harness(engine);
            let driver = &mut h.driver;

            press(
                driver,
                quadraui::Key::Named(quadraui::NamedKey::Down),
                quadraui::Modifiers {
                    alt: true,
                    shift: true,
                    ..Default::default()
                },
            );

            let screen = driver.screen();
            assert_eq!(
                screen.matches("ZQXW1744A").count(),
                2,
                "Shift+Alt+Down (copyLinesDownAction) must duplicate the \
                 line, not add a cursor; screen:\n{screen}"
            );
            assert!(
                screen.contains("ZQXW1744B"),
                "the second line must be untouched; screen:\n{screen}"
            );
        }

        /// Plain Alt+Left is VS Code's `navigateBack` — it must return the
        /// cursor to the last jump-list entry, not resize the sidebar (the
        /// mode-independent meaning this chord used to have unconditionally).
        #[test]
        fn alt_left_navigates_the_jump_list_in_vscode_mode_via_shell_app() {
            let mut engine = vscode_engine(&"line\n".repeat(10));
            engine.push_jump_location();
            engine.view_mut().cursor = crate::core::Cursor { line: 9, col: 0 };
            // 200, not this module's usual 80: at 80 columns the status
            // bar's own priority-drop sheds the "Ln N, Col N" segment
            // before the rightmost encoding/line-ending/toggle segments —
            // same mechanism `status_bar_segment_click_opens_go_to_line_
            // picker`'s GTK sibling documents needing *more* width for, not
            // less. `harness_no_sidebar` also frees up columns, since
            // `plain_engine`'s sidebar is visible by default.
            let mut h = crate::tui_main::testing::conformance_harness(engine, 200, 24);
            collapse_sidebar(&mut h.driver);
            let driver = &mut h.driver;

            assert!(
                driver.screen().contains("Ln 10, Col 1"),
                "precondition: painted cursor must start on line 10; \
                 screen:\n{}",
                driver.screen()
            );

            press(
                driver,
                quadraui::Key::Named(quadraui::NamedKey::Left),
                quadraui::Modifiers {
                    alt: true,
                    ..Default::default()
                },
            );

            assert!(
                driver.screen().contains("Ln 1, Col 1"),
                "Alt+Left (navigateBack) must return the painted cursor to \
                 the jump-list entry at line 1, not resize the sidebar; \
                 screen:\n{}",
                driver.screen()
            );
        }

        /// VSCode mode's own alternate home for keyboard sidebar resize is
        /// Ctrl+**Shift**+Alt+Right — not plain Ctrl+Alt+Right, which is
        /// already the shipped `panel_keys.nav_forward` global accelerator
        /// and would never reach `route_alt_key` at all in the live app
        /// (see that function's own doc). Mirrors
        /// `alt_right_widens_the_painted_sidebar_via_shell_app` above, in
        /// VSCode mode with the alternate chord. Runs at `(120, 24)` for the
        /// same reason that test does.
        ///
        /// **Verified RED against unfixed `develop`:** before this fix
        /// iteration, this rung's alternate-resize arms matched on `ctrl`
        /// alone (plain Ctrl+Alt+Right) — a chord the live accelerator tier
        /// claims first and this rung never actually sees — so dispatching
        /// Ctrl+Shift+Alt+Right found no matching arm and fell through to
        /// `vscode_alt_key_name`'s plain-`Alt_Right` lookup (navigate-
        /// forward) instead of widening the sidebar.
        #[test]
        fn ctrl_shift_alt_right_resizes_the_sidebar_in_vscode_mode_via_shell_app() {
            let mut engine = vscode_engine("");
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            engine.session.explorer_visible = true;
            engine.buffer_mut().insert(0, "ZQXW1744R");
            let mut h = crate::tui_main::testing::conformance_harness(engine, 120, 24);
            let driver = &mut h.driver;

            let before = driver
                .find_bounds("ZQXW1744R")
                .expect("the editor marker must paint before the resize");

            press(
                driver,
                quadraui::Key::Named(quadraui::NamedKey::Right),
                quadraui::Modifiers {
                    ctrl: true,
                    shift: true,
                    alt: true,
                    ..Default::default()
                },
            );

            let after = driver
                .find_bounds("ZQXW1744R")
                .expect("the editor marker must still paint after the resize");
            assert_eq!(
                after.x,
                before.x + 1.0,
                "Ctrl+Shift+Alt+Right must widen the painted sidebar by one \
                 column in VSCode mode; screen:\n{}",
                driver.screen()
            );
        }

        /// Ctrl+Shift+\ is VS Code's real `editor.action.jumpToBracket` —
        /// it must move the cursor onto the matching bracket. Covers both
        /// input shapes `App::handle_dispatch`'s `Key::Char` arm resolves to
        /// `"Shift_backslash"`/`"|"`: the literal already-shifted glyph
        /// `'|'`, and the base key `'\\'` plus an explicit Shift bit (the
        /// kitty/CSI-u shape — see `render::engine_key_from_ui`'s
        /// module doc and `App::handle_dispatch`'s own
        /// `Key::Char('\\') if modifiers.ctrl && modifiers.shift` arm).
        #[test]
        fn ctrl_shift_backslash_jumps_to_the_matching_bracket_in_vscode_mode_via_shell_app() {
            for (key, label) in [
                (quadraui::Key::Char('|'), "literal '|' glyph"),
                (
                    quadraui::Key::Char('\\'),
                    "base '\\' + explicit Shift bit (kitty/CSI-u)",
                ),
            ] {
                let engine = vscode_engine("(abc)\n");
                // 200-column, sidebar-collapsed harness: see the comment on
                // `alt_left_navigates_the_jump_list_in_vscode_mode_via_shell_app`
                // above for why this module's usual 80-column default drops
                // the "Ln N, Col N" segment entirely.
                let mut h = crate::tui_main::testing::conformance_harness(engine, 200, 24);
                collapse_sidebar(&mut h.driver);
                let driver = &mut h.driver;

                assert!(
                    driver.screen().contains("Ln 1, Col 1"),
                    "[{label}] precondition: painted cursor must start on \
                     the opening '('; screen:\n{}",
                    driver.screen()
                );

                press(
                    driver,
                    key,
                    quadraui::Modifiers {
                        ctrl: true,
                        shift: true,
                        ..Default::default()
                    },
                );

                assert!(
                    driver.screen().contains("Ln 1, Col 5"),
                    "[{label}] Ctrl+Shift+\\ must move the painted cursor \
                     onto the matching ')' (col 5); screen:\n{}",
                    driver.screen()
                );
            }
        }

        /// Smoke-spec `modeswitch-alt-m-from-normal`: switching from
        /// Vim Normal mode to VSCode mode via Alt-M must paint the status
        /// bar hint exactly as README/the catalogue document it —
        /// `EDIT  F1:palette  Alt-M:vim` — not `EDIT  F1:cmd  Alt-M:vim`.
        /// `F1` does open the Command Palette either way, so this is purely
        /// a label-text mismatch, not a functional gap; the painted text is
        /// still the contract here, since it's what a user actually reads.
        ///
        /// **Verified RED against unfixed `develop`:** before this fix,
        /// `Engine::mode_str`'s VSCode-mode arm returned the literal string
        /// `"EDIT  F1:cmd  Alt-M:vim"`, so `screen.contains("F1:palette")`
        /// failed (the screen had `F1:cmd` instead).
        #[test]
        fn alt_m_from_normal_shows_the_documented_f1_palette_hint_via_shell_app() {
            let engine = plain_engine();
            let mut h = harness(engine);
            let driver = &mut h.driver;

            assert!(
                driver.screen_has("NORMAL"),
                "precondition: a fresh engine starts in Vim Normal mode; \
                 screen:\n{}",
                driver.screen()
            );

            press(
                driver,
                quadraui::Key::Char('m'),
                quadraui::Modifiers {
                    alt: true,
                    ..Default::default()
                },
            );

            let screen = driver.screen();
            assert!(
                screen.contains("EDIT  F1:palette  Alt-M:vim"),
                "Alt-M from Vim Normal mode must switch to VSCode mode and \
                 paint the documented status bar hint \
                 'EDIT  F1:palette  Alt-M:vim'; screen:\n{screen}"
            );
        }
    }

    // Mode-toggle regression test
    /// A mode-toggle regression test does not belong inside
    /// `vscode_mode_alt_rung_1744` — that module is scoped to the five
    /// brand-new VS Code-mode chords, not mode-toggle chrome — so it
    /// gets its own sibling module instead.
    mod modeswitch_1780 {
        use super::*;

        fn press(
            driver: &mut quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            key: quadraui::Key,
            modifiers: quadraui::Modifiers,
        ) {
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key,
                modifiers,
                repeat: false,
            });
        }

        /// Bugbash catalogue scenario `modeswitch-alt-m-back-to-vim`:
        /// a second Alt-M (VSCode → Vim) must remove the menu-bar row the
        /// first Alt-M (Vim → VSCode) added, not just flip the status bar
        /// back to `NORMAL`. `Engine::toggle_editor_mode` unconditionally
        /// sets `menu_bar_visible = true` on the Vim → VSCode arm but, before
        /// this fix, never cleared it back on the VSCode → Vim arm, so the
        /// row — and the one-row downward shift of the activity bar/sidebar/
        /// editor content it causes — was permanent after the first toggle.
        /// "File" is the menu row's own always-first label (same signal
        /// `colon_opens_the_command_line_after_an_alt_chord_swallows_escape_1764`
        /// and `hamburger_relocated_click_after_reveal_hides_menu_bar_via_app_on_tui`
        /// both key off of). The issue's own
        /// evidence is a *geometry* shift ("all other rows have shifted
        /// down by one"), not just a label disappearing, so this also pins
        /// the editor buffer's own painted row back to its pre-toggle `y`
        /// — a half-fix that stopped painting "File" but left the row
        /// reserved would still move the buffer content down and fail
        /// this. Tracked via a unique marker inserted into the buffer
        /// rather than a sidebar label: VSCode mode collapses the
        /// default-active hamburger panel's body entirely (no header, no
        /// tree), and switching to Explorer to get a stable sidebar label
        /// would steal keyboard focus from the editor and swallow the
        /// Alt-M below before it ever reaches `route_alt_key`.
        ///
        /// **Verified RED against unfixed `develop`:** before this fix,
        /// `toggle_editor_mode`'s Vim-mode arm never touched
        /// `menu_bar_visible`, so after the second Alt-M below the screen
        /// still painted "File" and the marker stayed shifted down by one
        /// row — both final assertions failed.
        #[test]
        fn alt_m_round_trip_hides_the_menu_bar_row_again_via_shell_app_1780() {
            let mut engine = plain_engine();
            // Unique marker — see this fn's doc comment for why the editor
            // buffer, not a sidebar label, is what this test tracks.
            engine.buffer_mut().insert(0, "ZQXW1780MARKER\n");
            let mut h = harness(engine);
            let driver = &mut h.driver;

            assert!(
                !driver.screen_contains("File"),
                "precondition: a fresh Vim-mode engine paints no menu-bar \
                 row; screen:\n{}",
                driver.screen()
            );
            let marker_y_before = driver
                .find_bounds("ZQXW1780MARKER")
                .expect("precondition: the editor buffer must paint before any Alt-M")
                .y;

            let alt_m = quadraui::Modifiers {
                alt: true,
                ..Default::default()
            };

            // First Alt-M: Vim -> VSCode. The menu-bar row must appear.
            press(driver, quadraui::Key::Char('m'), alt_m);
            assert!(
                driver.screen_contains("EDIT"),
                "first Alt-M must switch to VSCode mode; screen:\n{}",
                driver.screen()
            );
            assert!(
                driver.screen_contains("File"),
                "first Alt-M (Vim -> VSCode) must reveal the menu-bar row; \
                 screen:\n{}",
                driver.screen()
            );
            let marker_y_with_menu_bar = driver
                .find_bounds("ZQXW1780MARKER")
                .expect("the editor buffer must still paint with the menu bar shown")
                .y;
            assert!(
                marker_y_with_menu_bar > marker_y_before,
                "revealing the menu-bar row must push the editor content \
                 down; before {marker_y_before}, with menu bar \
                 {marker_y_with_menu_bar}"
            );

            // Second Alt-M: VSCode -> Vim. The status bar flips back...
            press(driver, quadraui::Key::Char('m'), alt_m);
            let screen = driver.screen();
            assert!(
                screen.contains("NORMAL"),
                "second Alt-M must switch back to Vim Normal mode; \
                 screen:\n{screen}"
            );
            // ...and the menu-bar row the first toggle added must be gone,
            // not left behind permanently shifting everything below it down
            // by one row.
            assert!(
                !screen.contains("File"),
                "second Alt-M (VSCode -> Vim) must remove the menu-bar row \
                 the first toggle added; screen:\n{screen}"
            );
            let marker_y_after = driver
                .find_bounds("ZQXW1780MARKER")
                .expect("the editor buffer must still paint after the round trip")
                .y;
            assert_eq!(
                marker_y_after, marker_y_before,
                "the editor content (and everything below the menu row) \
                 must return to its pre-toggle row, not stay shifted down \
                 by the menu-bar row's height; screen:\n{screen}"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // #1787 fix-iteration driver-tier coverage
    // ─────────────────────────────────────────────────────────────────────────
    /// Bugbash catalogue scenario `modeswitch-alt-m-from-insert`
    /// (variant: undo first): in VSCode mode, typing a character opens an
    /// auto-trigger completion popup (`Engine::trigger_completion`, called
    /// unconditionally after every plain-char insert —
    /// `src/core/engine/vscode.rs`). Ctrl+Z then undoes that insert
    /// but `Engine::undo` never calls `dismiss_completion()`, so the
    /// popup survives with its *pre-undo* candidate text still cached.
    /// Alt-M doesn't call `dismiss_completion` either, so the stale
    /// popup rides along into Vim mode and paints over whatever chrome
    /// happens to occupy that screen region.
    ///
    /// This test drives the issue's exact repro (type → Ctrl+Z → Alt-M)
    /// with the Explorer sidebar painted and asserts the *painted*
    /// screen no longer shows the stale pre-undo candidate text.
    #[test]
    fn stale_pre_undo_completion_popup_is_dismissed_by_undo_1787() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_1787_stale_pre_undo_completion_popup_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker_file = dir.join("zqxw1787.txt");
        std::fs::write(&marker_file, "marker").unwrap();

        let mut engine = plain_engine();
        engine.cwd = dir.clone();
        engine.explorer_reveal_path(&marker_file);
        engine.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
        engine.mode = crate::core::Mode::Insert;
        engine.buffer_mut().insert(0, "smoke");
        engine.view_mut().cursor = crate::core::Cursor { line: 0, col: 1 };
        let mut h = harness(engine);
        let driver = &mut h.driver;

        assert!(
            driver.screen_has("zqxw1787.txt"),
            "precondition: Explorer must be the painted sidebar before any \
             of this test's keys; screen:\n{}",
            driver.screen()
        );

        let press = |driver: &mut quadraui::tui::testing::TuiDriver<_>,
                     key: quadraui::Key,
                     modifiers: quadraui::Modifiers| {
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key,
                modifiers,
                repeat: false,
            });
        };

        // Type 'X' mid-word: "smoke" -> "sXmoke". VSCode mode auto-triggers
        // completion after every plain-char insert.
        press(
            driver,
            quadraui::Key::Char('X'),
            quadraui::Modifiers::default(),
        );
        assert!(
            driver.screen_has("sXmoke"),
            "precondition: typing 'X' must open the auto-trigger completion \
             popup showing the whole word under the cursor as a candidate; \
             screen:\n{}",
            driver.screen()
        );

        // Ctrl+Z: undo the insert. The buffer reverts to "smoke"...
        press(
            driver,
            quadraui::Key::Char('z'),
            quadraui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
        );

        // ...then Alt-M: switch VSCode -> Vim mode.
        press(
            driver,
            quadraui::Key::Char('m'),
            quadraui::Modifiers {
                alt: true,
                ..Default::default()
            },
        );

        let screen = driver.screen();
        assert!(
            screen.contains("NORMAL"),
            "Alt-M must switch back to Vim Normal mode; screen:\n{screen}"
        );
        assert!(
            !screen.contains("sXmoke"),
            "the completion popup's stale pre-undo candidate text must not \
             survive an undo + Alt-M mode switch, overlapping the Explorer \
             sidebar until the next cursor-movement keypress; screen:\n{screen}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // Multi-cursor regression test for Ctrl+D and Ctrl+A
    /// Extends the `vscode-ctrl-d-adds-next-occurrence` smoke-spec
    /// journey one step further: typing *after* a Ctrl+D multi-cursor
    /// edit, then Ctrl+A (select-all), then typing again.
    ///
    /// The engine-level regression test for the same repro drives
    /// `Engine::handle_key` directly, which cannot prove the real TUI
    /// key decode actually *produces* that shape for Ctrl+D/Ctrl+A.
    /// This module drives the production `App::handle_dispatch` ->
    /// `render::engine_key_from_ui` -> `Engine::handle_key` pipeline
    /// through `TuiDriver`'s real dispatch and asserts on *painted*
    /// content, not engine buffer state.
    mod vscode_select_all_after_multicursor_edit_1785 {
        use super::*;

        fn vscode_engine(buffer: &str) -> crate::core::Engine {
            let mut engine = plain_engine();
            engine.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
            engine.mode = crate::core::Mode::Insert;
            engine.buffer_mut().insert(0, buffer);
            engine.view_mut().cursor = crate::core::Cursor { line: 0, col: 0 };
            engine
        }

        /// Exact bug-bash repro: buffer "foo bar foo", cursor at start,
        /// Ctrl+D Ctrl+D (selects both "foo"s), type "X" (-> "X bar X", two
        /// cursors survive the edit), Ctrl+A (select all), type "Y" ->
        /// expected: the whole painted buffer becomes "Y", same as the
        /// single-cursor case.
        ///
        /// **Verified RED against unfixed `develop`:** with the
        /// `extra_cursors.clear()` fix in `Engine::vscode_select_all`
        /// (`src/core/engine/vscode.rs`) reverted, the final
        /// `driver.type_char('Y')` call below panics the entire test binary
        /// inside ropey (`rope.rs:952`, `Option::unwrap` on `None`) via
        /// `vscode_mc_delete_selections`'s `ec.col + 1 - sel_len` underflow —
        /// the same backtrace the bug report captured
        /// (`/tmp/bugbash-captures/multicursor-crash.log`). Confirmed by
        /// temporarily removing that one line and re-running this test with
        /// `cargo test`: the whole binary aborted (no normal pass/fail
        /// report) instead of this test merely failing an assertion;
        /// restored immediately after.
        #[test]
        fn ctrl_a_after_ctrl_d_multicursor_edit_then_typing_replaces_the_whole_buffer_via_shell_app(
        ) {
            let engine = vscode_engine("foo bar foo");
            let mut h = harness(engine);
            let driver = &mut h.driver;

            assert!(
                driver.screen_has("foo bar foo"),
                "precondition: the buffer must paint before any key; \
                 screen:\n{}",
                driver.screen()
            );

            // Ctrl+D twice: select the first "foo", then add a second
            // cursor on the next "foo" occurrence.
            driver.ctrl_char('d');
            driver.ctrl_char('d');

            // Type 'X': both selections get replaced; two cursors survive
            // the edit (the bug's precondition).
            driver.type_char('X');
            assert!(
                driver.screen_has("X bar X"),
                "Ctrl+D Ctrl+D then typing 'X' must replace both \"foo\" \
                 occurrences, proving the multi-cursor edit landed on both; \
                 screen:\n{}",
                driver.screen()
            );

            // Ctrl+A: select all. This used to leave the stale extra
            // cursor from the Ctrl+D edit in `extra_cursors`.
            driver.ctrl_char('a');

            // Typing now must not panic (pre-fix: the whole process died
            // here, see this fn's doc) and must replace the *entire*
            // painted buffer, same as the single-cursor case.
            driver.type_char('Y');

            let screen = driver.screen();
            assert!(
                screen.contains('Y'),
                "typing after Ctrl+A must paint the typed character; \
                 screen:\n{screen}"
            );
            assert!(
                !screen.contains("X bar X") && !screen.contains("bar"),
                "typing after Ctrl+A must replace the *entire* buffer, not \
                 leave any of the pre-edit text painted; screen:\n{screen}"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Popups
    // ─────────────────────────────────────────────────────────────────────────
    mod popups {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name: opening the command
        /// palette via its accelerator must intercept subsequent keys into the
        /// picker query, not the editor buffer.
        #[test]
        fn command_palette_open_intercepts_keys_via_shell_app() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::command_palette_open_intercepts_keys_via_shell_app",
                || {
                    driver.dispatch(quadraui::UiEvent::Accelerator(
                        quadraui::AcceleratorId::new(crate::render::ACC_COMMAND_PALETTE),
                        quadraui::Modifiers::default(),
                    ));
                    driver.type_char('i');
                    for c in "ZQXW_TYPED".chars() {
                        driver.type_char(c);
                    }
                    let open_screen = driver.screen();
                    assert!(
                        open_screen.contains("iZQXW_TYPED"),
                        "keys typed while the command palette is open should feed \
                 the picker query; screen:\n{open_screen}"
                    );

                    driver.press_named(quadraui::NamedKey::Escape);
                    let screen = driver.screen();
                    assert!(
                        !screen.contains("ZQXW_TYPED"),
                        "keys typed while the command palette is open must not \
                 reach the editor buffer; screen:\n{screen}"
                    );
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: opening the
        /// unified picker must paint its header, and dismissing it must leave
        /// no trace on the grid.
        #[test]
        fn picker_dismiss_leaves_no_popup_glyphs_on_the_grid_via_shell_app() {
            let mut engine = plain_engine();
            engine.open_picker(crate::core::engine::PickerSource::Keybindings);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::picker_dismiss_leaves_no_popup_glyphs_on_the_grid_via_shell_app",
                || {
                    assert!(
                        driver.screen_has("Key Bindings"),
                        "opening the Keybindings picker must paint its header; \
                     screen:\n{}",
                        driver.screen()
                    );
                    driver.press_named(quadraui::NamedKey::Escape);
                    assert!(
                        !driver.screen_has("Key Bindings"),
                        "Escape must dismiss the picker and leave no trace of \
                     it on the grid; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// Opening the unified picker must darken the chrome behind it — a
        /// corner cell outside the popup's own rect (top-left, where the
        /// collapsed-sidebar tab bar paints) must switch to the dark scrim
        /// background once the picker opens, and switch back once it's
        /// dismissed. Without `paint_picker_rung`'s `draw_solid_fill` call
        /// this cell's background never changes, so this test is red against
        /// unfixed `develop`.
        #[test]
        fn picker_open_paints_a_backdrop_scrim_behind_it_via_shell_app() {
            let engine = plain_engine();
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            let before = driver
                .style_at(0, 0)
                .expect("top-left cell must be in bounds")
                .bg;

            driver.dispatch(quadraui::UiEvent::Accelerator(
                quadraui::AcceleratorId::new(crate::render::ACC_COMMAND_PALETTE),
                quadraui::Modifiers::default(),
            ));

            let scrim = crate::render::MODAL_SCRIM_COLOR;
            let during = driver
                .style_at(0, 0)
                .expect("top-left cell must be in bounds")
                .bg;
            assert_eq!(
                during,
                ratatui::style::Color::Rgb(scrim.r, scrim.g, scrim.b),
                "opening the command palette must dim the top-left corner cell \
                 behind it to the scrim colour; screen:\n{}",
                driver.screen()
            );
            assert_ne!(
                before, during,
                "the scrim must actually change the corner cell's background, \
                 not merely repaint the same colour"
            );

            driver.press_named(quadraui::NamedKey::Escape);
            let after = driver
                .style_at(0, 0)
                .expect("top-left cell must be in bounds")
                .bg;
            assert_eq!(
                after,
                before,
                "dismissing the picker must remove the scrim and restore the \
                 corner cell's original background; screen:\n{}",
                driver.screen()
            );
        }

        /// The scrim behind the unified picker must stop above the
        /// status-bar/command-line band, not blank it: `FRAME_Z_ORDER`
        /// composes `StatusBar`/`CommandLine` before `UnifiedPicker`, so a
        /// scrim fill that reaches into their rows would overwrite their
        /// text with blank scrim-coloured cells on TUI (`draw_solid_fill`
        /// fills every cell in its rect with a blank glyph, it does not
        /// alpha-blend). Opening the command palette must leave the
        /// "NORMAL" mode indicator on the status bar readable both before
        /// and while the picker is open.
        #[test]
        fn picker_open_leaves_status_bar_text_visible_via_shell_app() {
            let engine = plain_engine();
            let mut h = harness(engine);
            let driver = &mut h.driver;

            assert!(
                driver.screen_has("NORMAL"),
                "precondition: a fresh engine starts in Vim Normal mode; \
                 screen:\n{}",
                driver.screen()
            );

            driver.dispatch(quadraui::UiEvent::Accelerator(
                quadraui::AcceleratorId::new(crate::render::ACC_COMMAND_PALETTE),
                quadraui::Modifiers::default(),
            ));

            assert!(
                driver.screen_has("NORMAL"),
                "opening the command palette must dim the editor/sidebar \
                 behind it, not blank the status bar's mode indicator; \
                 screen:\n{}",
                driver.screen()
            );
        }

        /// The tallest bottom-chrome frame: a global status bar *and* a live
        /// wildmenu row, so `status_bar_height_px` reserves three rows where
        /// the shell's static two-row reservation covers only two. Opening the
        /// picker over that frame must leave both rows readable — a scrim
        /// bounded by the shell's static reservation instead of this frame's
        /// `EditorLayout::editor_bottom` would blank the wildmenu's row with
        /// scrim-coloured cells.
        ///
        /// `open_picker` does not clear `wildmenu_items`, so the two really
        /// can be live in the same frame.
        #[test]
        fn picker_over_an_open_wildmenu_leaves_both_chrome_rows_visible_via_shell_app() {
            let mut engine = plain_engine();
            // Explicit, not ambient: a global status bar exists only when
            // per-window status lines are off, and the default is on.
            engine.settings.window_status_line = false;
            engine.wildmenu_items = vec!["ZQXWwildA".to_string(), "ZQXWwildB".to_string()];
            engine.wildmenu_selected = Some(0);
            let mut h = harness(engine);
            let driver = &mut h.driver;

            assert!(
                driver.screen_has("ZQXWwildA") && driver.screen_has("NORMAL"),
                "precondition: the wildmenu row and the status bar's mode \
                 indicator both paint before the picker opens; screen:\n{}",
                driver.screen()
            );

            driver.dispatch(quadraui::UiEvent::Accelerator(
                quadraui::AcceleratorId::new(crate::render::ACC_COMMAND_PALETTE),
                quadraui::Modifiers::default(),
            ));

            assert!(
                driver.screen_has("ZQXWwildA"),
                "the scrim must stop at this frame's editor-band bottom, which \
                 is one row higher when the wildmenu is up; screen:\n{}",
                driver.screen()
            );
            assert!(
                driver.screen_has("NORMAL"),
                "the status bar below the wildmenu must stay readable too; \
                 screen:\n{}",
                driver.screen()
            );
        }

        /// Open the command palette through `render::ACC_COMMAND_PALETTE`
        /// and narrow it to the Save command, so the painted list's shortcut
        /// column is a single row.
        fn open_palette_filtered_to_save(
            driver: &mut quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        ) {
            driver.dispatch(quadraui::UiEvent::Accelerator(
                quadraui::AcceleratorId::new(crate::render::ACC_COMMAND_PALETTE),
                quadraui::Modifiers::default(),
            ));
            for c in "File: Save".chars() {
                driver.type_char(c);
            }
        }

        /// On a macOS backend the palette must paint Apple's own chord
        /// spelling (`⌘S`) in its shortcut column, never the `Ctrl+S` text
        /// the other platforms use.
        ///
        /// `platform_name` is set *after* the harness is built: `ShellApp::
        /// setup` writes the live backend's own name (`"tui"` here) into the
        /// engine while the driver is constructed, so a fixture-time
        /// assignment would be overwritten before the first frame.
        #[test]
        fn command_palette_paints_cmd_glyph_on_macos_via_shell_app() {
            let mut h = harness_no_sidebar(plain_engine());
            h.engine.borrow_mut().platform_name = "macos";
            let driver = &mut h.driver;

            open_palette_filtered_to_save(driver);

            assert!(
                driver.screen_has("⌘S"),
                "the macOS palette must paint the Cmd glyph for Save; \
                 screen:\n{}",
                driver.screen()
            );
            assert!(
                !driver.screen_has("Ctrl+S"),
                "the macOS palette must not paint the Ctrl spelling of Save's \
                 shortcut; screen:\n{}",
                driver.screen()
            );
        }

        /// The twin of the test above on every other backend: the painted
        /// shortcut column keeps the `Ctrl+S` text and grows no Cmd glyph.
        #[test]
        fn command_palette_paints_ctrl_text_off_macos_via_shell_app() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            open_palette_filtered_to_save(driver);

            assert!(
                driver.screen_has("Ctrl+S"),
                "a non-macOS palette must paint Save's shortcut as Ctrl+S; \
                 screen:\n{}",
                driver.screen()
            );
            assert!(
                !driver.screen_has("⌘"),
                "a non-macOS palette must paint no Cmd glyph; screen:\n{}",
                driver.screen()
            );
        }

        /// A dialog (e.g. quit-with-unsaved-changes) must intercept keys: a
        /// key that would otherwise be a Normal-mode buffer edit must not
        /// reach the buffer while the dialog is open, and `Escape` must
        /// dismiss it.
        #[test]
        fn dialog_intercepts_all_keys() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQXW_DIALOG_LINE\n");
            engine.dialog = Some(crate::core::engine::Dialog {
                title: "Confirm".to_string(),
                body: vec!["ZQXW_1425_DIALOG_MARKER".to_string()],
                buttons: vec![],
                selected: 0,
                tag: "app_on_tui_test".to_string(),
                input: None,
            });
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // `App::render_content` filters native-dialog options on
            // `backend.backend_caps().native_dialogs` before queuing a
            // native present, so TUI (no native alert facility) falls back
            // to the in-canvas `Dialog` rung instead of never painting it.
            assert!(
                driver.screen_has("ZQXW_1425_DIALOG_MARKER"),
                "precondition: the dialog must be painted"
            );
            // 'dd' would delete the line under Normal-mode dispatch; with a
            // dialog open it must be swallowed instead.
            driver.type_char('d');
            driver.type_char('d');
            assert!(
                driver.screen_has("ZQXW_DIALOG_LINE"),
                "keys must not reach the buffer while a dialog is open; \
                 screen:\n{}",
                driver.screen()
            );
            driver.press_named(quadraui::NamedKey::Escape);
            assert!(
                !driver.screen_has("ZQXW_1425_DIALOG_MARKER"),
                "Escape must dismiss the dialog; screen:\n{}",
                driver.screen()
            );
        }

        /// An open quickfix list with a real item must paint that item's text
        /// in the bottom panel.
        #[test]
        fn quickfix_with_item_paints_row() {
            let mut engine = plain_engine();
            // Short filename and marker — at this module's `(80, 24)` size the
            // quickfix row (`file:line: text`) has far less width than the
            // `crate::harness` scenarios' usual `(800, 480)`, so a long marker
            // truncates before it ever reaches the screen buffer.
            engine
                .quickfix
                .items
                .push(crate::core::project_search::ProjectMatch {
                    file: std::path::PathBuf::from("a.rs"),
                    line: 0,
                    col: 0,
                    line_text: "QFMARK".to_string(),
                });
            engine.quickfix.open = true;
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate("app_on_tui::quickfix_with_item_paints_row", || {
                assert!(
                    driver.screen_has("QFMARK"),
                    "an open quickfix list with a real item must paint that \
                 item; screen:\n{}",
                    driver.screen()
                );
            });
        }

        // Completion/hover popup anchoring

        /// Mirrors `shell_app.rs`'s test of the same name: the
        /// completion popup must anchor at the cursor's real *display*
        /// column, not its raw character column — on a tab-indented line the
        /// two disagree. The completion-popup width floor is a bare GTK pixel
        /// constant, but this function runs unmodified for TUI too, where
        /// `cw` (raw units per character) is `1.0` — so the same "100"
        /// demanded a 100-*cell*-wide popup, wider than the 80-column
        /// terminal, which made `quadraui::Completions::layout`'s
        /// right-edge-overflow branch always fire and clamp `x` straight
        /// back to the window's left edge. The fix scales the floor by `cw`
        /// and matches `tui_main::render_impl`'s own completion-width
        /// formula exactly.
        #[test]
        fn completion_popup_anchors_at_the_real_cursor_column_on_tab_indented_line_via_shell_app() {
            let mut engine = plain_engine();
            engine.settings.expand_tab = false;
            engine.buffer_mut().insert(0, "ZQXWFOOBAR\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            driver.type_char('G');
            driver.type_char('o');
            driver.type_char('\t');
            driver.type_char('\t');
            for c in "ZQXWFOO".chars() {
                driver.type_char(c);
            }

            let screen = driver.screen();
            assert_eq!(
                screen.matches("ZQXWFOOBAR").count(),
                2,
                "precondition: the popup must be showing the \
                 \"ZQXWFOOBAR\" candidate (once in the dictionary line, \
                 once in the popup); screen:\n{screen}"
            );

            let (cursor_x, _) = driver
                .terminal_cursor_position()
                .expect("insert-mode cursor must be visible after typing");
            let popup_bounds = driver
                // The mirrored `shell_app.rs` test brackets with
                // *both* the popup's own left and right borders
                // (`"│ ZQXWFOOBAR │"`) specifically because the candidate
                // text also appears verbatim as plain buffer content (the
                // dictionary line above) with its own leading `"│ "` —
                // this fixture's persistent activity-bar/divider chrome
                // puts a `"│"` immediately left of *every* row's content,
                // and that row's 1-cell gutter reservation happens to
                // reproduce the leading `"│ "` prefix too, so a
                // left-border-only needle silently matched the dictionary
                // line first (the topmost row `find_bounds` scans) instead
                // of the popup — this port had dropped the trailing
                // `" │"` when copying the needle over, silently comparing
                // the dictionary line's own x against the cursor instead
                // of the popup's.
                .find_bounds("│ ZQXWFOOBAR │")
                .expect("completion popup must be visible on screen");

            assert!(
                (popup_bounds.x - cursor_x as f32).abs() <= 1.0,
                "completion popup (x={}) must anchor at the real \
                 cursor's display column (x={cursor_x}), not the raw \
                 character column; screen:\n{screen}",
                popup_bounds.x,
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: the
        /// LSP hover popup (`ScreenLayout::hover`) must anchor at the same
        /// tab-expanded display column as the completion popup above.
        #[test]
        fn editor_hover_popup_anchors_at_the_visual_column_on_tab_indented_line_via_shell_app() {
            let mut engine = plain_engine();
            engine.settings.expand_tab = false;
            engine.buffer_mut().insert(0, "\t\tZZQ\n");
            engine.view_mut().cursor.col = 5;
            engine.lsp_hover_text = Some("QXZZYHVR".to_string());
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::editor_hover_popup_anchors_at_the_visual_column_on_tab_indented_line_via_shell_app",
                || {
                    let screen = driver.screen();
                    let zzq_bounds = driver
                        .find_bounds("ZZQ")
                        .expect("the tab-indented buffer line must be visible");
                    let hover_bounds = driver
                        .find_bounds("QXZZYHVR")
                        .expect("hover popup must be visible on screen");

                    let expected_x = zzq_bounds.x + zzq_bounds.width + 2.0;
                    assert!(
                        (hover_bounds.x - expected_x).abs() <= 1.0,
                        "hover popup (x={}) must anchor at the tab-expanded \
                         display column (x≈{expected_x}, right after \"ZZQ\"); \
                         screen:\n{screen}",
                        hover_bounds.x,
                    );
                },
            );
        }

        // Tab switcher popup clicks

        /// Create file tabs, zero-padded so no name is a substring of
        /// another, with the tab switcher open.
        fn engine_with_two_file_tabs_and_switcher_open() -> crate::core::Engine {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1431_tab_switcher_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let file_a = dir.join("a1431.txt");
            let file_b = dir.join("b1431.txt");
            let content: String = (0..40).map(|i| format!("AAA1431 line {i}\n")).collect();
            std::fs::write(&file_a, &content).unwrap();
            std::fs::write(&file_b, &content).unwrap();

            let mut engine = plain_engine();
            engine
                .open_file_with_mode(&file_a, crate::core::engine::OpenMode::Permanent)
                .unwrap();
            engine.new_tab(Some(&file_b));
            engine.open_tab_switcher();
            assert!(
                engine.tab_switcher_open,
                "fixture must actually open the tab switcher"
            );
            engine
        }

        /// Mirrors `shell_app.rs`'s test of the same name: a click
        /// that lands inside the painted tab-switcher popup must dismiss it
        /// **and be consumed**, so the editor underneath never sees it.
        #[test]
        fn driver_click_inside_tab_switcher_popup_dismisses_and_is_consumed() {
            let mut h = harness_no_sidebar(engine_with_two_file_tabs_and_switcher_open());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::driver_click_inside_tab_switcher_popup_dismisses_and_is_consumed",
                || {
                    let title = driver
                        .find_bounds("Open Tabs")
                        .expect("the tab-switcher popup must paint its title");
                    assert!(
                        driver.screen_contains("Ln 1, Col 1"),
                        "precondition: the cursor starts on line 1; screen:\n{}",
                        driver.screen()
                    );

                    driver.dispatch(quadraui::UiEvent::WindowFocused(true));
                    driver.render();

                    driver.click(title.x + 1.0, title.y + 2.0);

                    let screen = driver.screen();
                    assert!(
                        !screen.contains("Open Tabs"),
                        "a click inside the tab-switcher popup must dismiss \
                         it; screen:\n{screen}"
                    );
                    assert!(
                        screen.contains("Ln 1, Col 1"),
                        "the click must be consumed by the popup, not leak \
                         through to the editor and move the cursor; \
                         screen:\n{screen}"
                    );
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: the complementary
        /// half — a click *outside* the popup still dismisses it, but
        /// propagates to the editor underneath.
        #[test]
        fn driver_click_outside_tab_switcher_popup_dismisses_and_propagates() {
            let mut h = harness_no_sidebar(engine_with_two_file_tabs_and_switcher_open());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::driver_click_outside_tab_switcher_popup_dismisses_and_propagates",
                || {
                    let title = driver
                        .find_bounds("Open Tabs")
                        .expect("the tab-switcher popup must paint its title");
                    assert!(
                        driver.screen_contains("Ln 1, Col 1"),
                        "precondition: the cursor starts on line 1"
                    );

                    driver.dispatch(quadraui::UiEvent::WindowFocused(true));
                    driver.render();

                    // Editor body left of the centred popup.
                    let body = driver
                        .find_bounds("AAA1431 line 0")
                        .expect("the editor body must paint under the popup");
                    assert!(
                        body.x + 1.0 < title.x,
                        "fixture sanity: the click target must sit left of \
                         the centred popup, not under it"
                    );
                    driver.click(body.x + 1.0, body.y);
                    driver.render();

                    let screen = driver.screen();
                    assert!(
                        !screen.contains("Open Tabs"),
                        "a click outside the tab-switcher popup must dismiss \
                         it too; screen:\n{screen}"
                    );
                    assert!(
                        !screen.contains("Ln 1, Col 1"),
                        "an outside click must propagate to the editor \
                         underneath and move the cursor off line 1; \
                         screen:\n{screen}"
                    );
                },
            );
        }

        // Unified-picker row clicks

        /// Mirrors `shell_app.rs`'s test of the same name: a click on the
        /// *already selected* palette row confirms it and closes the
        /// palette.
        #[test]
        fn second_click_on_a_picker_row_confirms_it_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "fn main() {}\n");
            engine.open_picker(crate::core::engine::PickerSource::LineEndings);
            assert_eq!(
                engine.picker_selected, 0,
                "fixture assumes the palette opens on row 0, so row 1 is a \
                 not-yet-selected row"
            );
            let title = engine.picker_title.clone();
            let row1_label = engine.picker_items[1].display.clone();
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::second_click_on_a_picker_row_confirms_it_via_shell_app",
                || {
                    let row1 = driver
                        .find_bounds(&row1_label)
                        .unwrap_or_else(|| panic!("the palette must paint its {row1_label:?} row"));

                    driver.click(row1.x, row1.y);
                    assert!(
                        driver.screen_contains(&title),
                        "the first click only selects — the palette must \
                         still be painted; screen:\n{}",
                        driver.screen()
                    );

                    driver.click(row1.x, row1.y);
                    let screen = driver.screen();
                    assert!(
                        !screen.contains(&title),
                        "a second click on the already-selected row must \
                         confirm it and close the palette; screen:\n{screen}"
                    );
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: a click
        /// outside the painted picker popup must dismiss it.
        #[test]
        fn click_outside_picker_popup_dismisses_it_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "fn main() {}\n");
            engine.open_picker(crate::core::engine::PickerSource::LineEndings);
            let title = engine.picker_title.clone();
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::click_outside_picker_popup_dismisses_it_via_shell_app",
                || {
                    assert!(
                        driver.screen_contains(&title),
                        "precondition: the picker's title must paint; \
                         screen:\n{}",
                        driver.screen()
                    );

                    driver.click(6.0, 10.0);
                    driver.render();
                    let screen = driver.screen();
                    assert!(
                        !screen.contains(&title),
                        "a click outside the painted popup must dismiss the \
                         picker (route_modal_overlay_click's \
                         PickerRoute::Dismiss arm); screen:\n{screen}"
                    );
                },
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Pickers (folder/workspace picker — #1431 tranche 2)
    // ─────────────────────────────────────────────────────────────────────────
    mod pickers {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name: the shared
        /// folder/workspace picker must actually paint its entries through
        /// `FolderPickerController::render`, typing must reach
        /// `FolderPickerController::handle` and filter the list, and Esc
        /// must dismiss it.
        ///
        /// Needs [`harness_with_folder_picker`] rather than [`harness`]: the
        /// picker has to exist on `App` *before* it is moved into
        /// `driver_with_shell`, and `ConformanceHarness` gives no hook back
        /// to the (by-then-moved) `App` once [`harness`] returns — see
        /// `crate::tui_main::testing::conformance_harness_with_folder_picker`'s
        /// own doc, added by this same change alongside
        /// `crate::gtk::testing::conformance_harness_with_folder_picker`,
        /// which this scenario doesn't yet have a `gtk`-side twin test for.
        fn harness_with_folder_picker(
            dir: std::path::PathBuf,
        ) -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            crate::tui_main::testing::conformance_harness_with_folder_picker(
                plain_engine(),
                dir,
                100,
                24,
            )
        }

        #[test]
        fn folder_picker_paints_and_filters_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1431_folder_picker_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("distinctive_child_dir_1431")).unwrap();
            std::fs::create_dir_all(dir.join("another_unrelated_dir_1431")).unwrap();

            let mut h = harness_with_folder_picker(dir.clone());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::folder_picker_paints_and_filters_via_shell_app",
                || {
                    assert!(
                        driver.screen_contains("distinctive_child_dir_1431"),
                        "the open picker must paint its entries via \
                         `FolderPickerController::render`; screen:\n{}",
                        driver.screen()
                    );
                    assert!(
                        driver.screen_contains("another_unrelated_dir_1431"),
                        "screen:\n{}",
                        driver.screen()
                    );

                    for c in "distinctive".chars() {
                        driver.type_char(c);
                    }
                    assert!(
                        driver.screen_contains("distinctive_child_dir_1431"),
                        "typing must reach `FolderPickerController::handle` \
                         and keep matching entries visible; screen:\n{}",
                        driver.screen()
                    );
                    assert!(
                        !driver.screen_contains("another_unrelated_dir_1431"),
                        "typing a query that only matches one entry must \
                         filter the other one out; screen:\n{}",
                        driver.screen()
                    );

                    driver.press_named(quadraui::NamedKey::Escape);
                    assert!(
                        !driver.screen_contains("distinctive_child_dir_1431"),
                        "Esc must dismiss the picker; screen:\n{}",
                        driver.screen()
                    );
                },
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// Flipping `show_hidden_files` on by default must not leak
        /// `.git/` internals into quick-open. The `explorer_exclude` list is
        /// the single source of truth for that noise, and
        /// `picker_populate_files` prunes it from the `ignore` walk — so the
        /// *painted* picker list shows an ordinary dotfile but no `.git`
        /// entry.
        ///
        /// Confirmed RED against the branch without the
        /// `picker_populate_files` `filter_entry` prune (which is the state
        /// unfixed `develop` + the default flip produces): the picker paints
        /// `.git/HEAD1545` and the second assertion fails. The same
        /// regression is what made `test_picker_files_populates_preview`
        /// fail with `"ref: refs/heads/main"` instead of the real file's
        /// first line.
        #[test]
        fn quick_open_hides_git_internals_but_shows_dotfiles_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vc1545qo_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join(".git")).unwrap();
            std::fs::write(dir.join(".git").join("HEAD1545"), b"ref: refs/heads/main\n").unwrap();
            std::fs::write(dir.join(".dotrc1545"), b"kept\n").unwrap();

            // Default settings only — the shipped `show_hidden_files` /
            // `explorer_exclude` defaults are the thing under test.
            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.open_picker(crate::core::engine::PickerSource::Files);
            let h = harness(engine);
            let driver = &h.driver;

            assert!(
                driver.screen_contains(".dotrc1545"),
                "quick-open must list dotfiles now that `show_hidden_files` \
                 defaults on (#1545); screen:\n{}",
                driver.screen()
            );
            assert!(
                !driver.screen_contains("HEAD1545"),
                "quick-open must not list `.git/` internals — `explorer_exclude` \
                 prunes them from the walk (#1545); screen:\n{}",
                driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// The quick-open preview pane must run the selected file's text
        /// through the same syntax-highlighting pipeline as the main
        /// editor, not paint it as one uniform color. `picker_populate_files`
        /// -> `picker_load_preview` -> `syntax_highlights_for_path` feeds
        /// `PickerPreview::highlights` from the real tree-sitter grammar for
        /// `.rs`, and `picker_panel_to_palette` turns each highlighted byte
        /// range into its own `StyledSpan::with_fg(..., theme.scope_color(..))`
        /// — so a string-literal token's cell must carry a different `fg`
        /// than a plain, unstyled cell elsewhere on screen (the picker's own
        /// file-name entry, built with `StyledText::plain`).
        ///
        /// Confirmed red against the unfixed preview rendering, which built
        /// every preview line with `StyledText::plain` regardless of
        /// `highlights`: both cells then shared the same fallback
        /// `theme.surface_fg`, so this `assert_ne!` failed.
        #[test]
        fn quick_open_preview_pane_syntax_highlights_source_text_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vc1926_synhi_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join(".git")).unwrap();
            std::fs::write(
                dir.join("hello.rs"),
                b"fn main() {\n    let s = \"ZQX_MARK\";\n}\n",
            )
            .unwrap();
            // The popup's height tracks the result count, and the preview
            // pane is only as tall as the list beside it — so the fixture
            // needs enough entries for the marker on the file's second line
            // to be painted. `hello.rs` sorts first, so it stays selected
            // and its preview is the one shown.
            for i in 0..6 {
                std::fs::write(dir.join(format!("zpad{i}.txt")), b"pad\n").unwrap();
            }

            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.open_picker(crate::core::engine::PickerSource::Files);
            let h = harness(engine);
            let driver = &h.driver;

            let marker_bounds = driver.find_bounds("ZQX_MARK").unwrap_or_else(|| {
                panic!(
                    "preview pane must show the string literal's text; screen:\n{}",
                    driver.screen()
                )
            });
            let item_bounds = driver.find_bounds("hello.rs").unwrap_or_else(|| {
                panic!(
                    "picker list must show the plain file-name entry; screen:\n{}",
                    driver.screen()
                )
            });

            let marker_style = driver
                .style_at(marker_bounds.x as u16, marker_bounds.y as u16)
                .expect("marker cell must be on screen");
            let item_style = driver
                .style_at(item_bounds.x as u16, item_bounds.y as u16)
                .expect("item cell must be on screen");

            assert_ne!(
                marker_style.fg,
                item_style.fg,
                "preview pane's string-literal token must render in a \
                 different color than the plain, unstyled file-name entry \
                 — got the same fg ({:?}) for both, meaning the preview \
                 text is still painted as one uniform color; screen:\n{}",
                marker_style.fg,
                driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// `picker_panel_to_palette` must not duplicate preview text when a
        /// line's tree-sitter captures overlap or coincide byte-for-byte —
        /// which is the common case for a method call (`@property` and
        /// `@method.call` both matching the same `field_identifier`) or an
        /// escape sequence inside a string literal (`@escape` nesting inside
        /// `@string`). A per-line builder that pushes a colored span for
        /// every highlight entry unconditionally (instead of resolving each
        /// byte to at most one scope) re-emits the overlapped text, so the
        /// rendered line grows longer than the source line and later
        /// characters shift off past where they belong.
        ///
        /// Confirmed red against the unfixed sequential-cursor builder: the
        /// `push_str` call's `_str` suffix and the escape sequence's
        /// characters were re-emitted into the rendered row, so the painted
        /// line no longer matched the source line byte-for-byte and
        /// `push_str` appeared twice.
        #[test]
        fn quick_open_preview_pane_does_not_duplicate_overlapping_highlight_text() {
            let dir = std::env::temp_dir().join(format!(
                "vc1926_synhi_overlap_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join(".git")).unwrap();
            // The second line packs both overlap shapes into one short row
            // that fits the narrow preview pane: `push_str` is a method call
            // through a field expression, so its `field_identifier` is
            // captured by both `@property` and `@method.call` over identical
            // byte ranges, and the `\n` escape nests inside the `@string`
            // capture that spans it.
            std::fs::write(
                dir.join("hello.rs"),
                b"fn f(s: &mut String) {\ns.push_str(\"a\\nEM\");\n}\n",
            )
            .unwrap();
            // Pad the result list so the popup is tall enough to paint the
            // second preview line. `hello.rs` sorts first and stays selected.
            for i in 0..6 {
                std::fs::write(dir.join(format!("zpad{i}.txt")), b"pad\n").unwrap();
            }

            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.open_picker(crate::core::engine::PickerSource::Files);
            let h = harness(engine);
            let driver = &h.driver;

            let screen = driver.screen();
            let call_line = screen
                .lines()
                .find(|l| l.contains("push_str"))
                .unwrap_or_else(|| {
                    panic!("preview must show the method-call line; screen:\n{screen}")
                });
            assert!(
                call_line.contains("s.push_str(\"a\\nEM\");"),
                "overlapping @property/@method.call captures on the same \
                 `push_str` field identifier, and the nested @escape inside \
                 the @string capture, must not duplicate any of the line's \
                 text — got:\n{call_line}\nfull screen:\n{screen}"
            );
            assert_eq!(
                call_line.matches("push_str").count(),
                1,
                "`push_str` must appear exactly once in its preview row, not \
                 duplicated by overlapping highlight spans; got:\n{call_line}\n\
                 full screen:\n{screen}"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Tab bar
    // ─────────────────────────────────────────────────────────────────────────
    mod tab_bar {
        use super::*;

        /// Read back the *painted* tab-bar row for `group_id` — scoped to
        /// exactly the cells `ScreenLayout::group_tab_bars`' `bounds`
        /// reserves for that group's tab bar, not the whole screen. A bare
        /// `screen_has(label)`/`screen_contains` check is satisfied just as
        /// well by the Explorer sidebar row or the breadcrumb segment
        /// painting the same filename.
        ///
        /// `bounds.x`/`bounds.width` are *display columns* (the activity
        /// bar's "☰" hamburger glyph is double-width), so this walks the
        /// row accumulating [`quadraui::tui::char_cell_width`] rather than
        /// `.chars().skip(n)` — the same char-vs-column distinction
        /// `build_tab_bar_icons`'s own doc traces to width calculations.
        ///
        /// Callers must set `engine.settings.breadcrumbs = false` first:
        /// at the default `breadcrumbs = true` the tab bar's band is two
        /// rows tall and `bounds.y - 1` no longer reliably lands on the
        /// label row.
        pub(super) fn active_group_tab_bar_row(
            h: &crate::harness::ConformanceHarness<
                quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            >,
            group_id: crate::core::window::GroupId,
        ) -> String {
            let bounds = {
                let layout = h.screen_layout.borrow();
                let layout = layout.as_ref().expect("a frame must have been painted");
                layout
                    .group_tab_bars
                    .iter()
                    .find(|gtb| gtb.group_id == group_id)
                    .map(|gtb| gtb.bounds)
                    .expect("the group must have its own tab bar entry")
            };
            let tab_row_idx = (bounds.y - 1.0).round() as usize;
            let col_start = bounds.x.round() as usize;
            let col_width = bounds.width.round() as usize;

            let screen = h.driver.screen();
            let full_row = screen.lines().nth(tab_row_idx).unwrap_or_else(|| {
                panic!(
                    "row {tab_row_idx} (group {group_id:?}'s tab row) is \
                     off-screen; screen:\n{screen}"
                )
            });

            let mut col = 0usize;
            let mut out = String::new();
            for c in full_row.chars() {
                let w = quadraui::tui::char_cell_width(c) as usize;
                if col >= col_start && col < col_start + col_width {
                    out.push(c);
                }
                col += w;
                if col >= col_start + col_width {
                    break;
                }
            }
            out
        }

        /// Mirrors `shell_app.rs`'s test of the same name: the unsplit
        /// case must paint exactly one full-width tab bar, on row 0.
        #[test]
        fn render_content_paints_single_group_tab_bar_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "short\n");
            assert_eq!(
                engine.group_layout.leaf_count(),
                1,
                "this test covers the unsplit case"
            );
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            // `App` reserves its own permanent menu-bar row (File Edit
            // View Go Run Terminal Help) on every backend; the shipped TUI
            // shell only ever showed that row in vscode-mode or when
            // Alt-revealed, so its tab bar position differs.
            known_bug_gate(
                "app_on_tui::render_content_paints_single_group_tab_bar_via_shell_app",
                || {
                    let screen = driver.screen();
                    let tab_row = screen
                        .lines()
                        .next()
                        .expect("the screen must paint at least one row");
                    let count = tab_row.matches("[No Name]").count();
                    assert_eq!(
                        count, 1,
                        "an unsplit editor must paint exactly one tab bar, on row 0; \
                 row:\n{tab_row}"
                    );
                },
            );
        }

        /// A vertical split (`open_editor_group`) must paint two independent
        /// tab bars, one per pane.
        #[test]
        fn two_groups_paint_two_tab_bars() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "short\n");
            engine.open_editor_group(SplitDirection::Vertical);
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            // Same permanent-menu-bar-row divergence as
            // `render_content_paints_single_group_tab_bar_via_shell_app`
            known_bug_gate("app_on_tui::two_groups_paint_two_tab_bars", || {
                let screen = driver.screen();
                let tab_row = screen
                    .lines()
                    .next()
                    .expect("the screen must paint at least one row");
                let count = tab_row.matches("[No Name]").count();
                assert_eq!(
                    count, 2,
                    "a vertical split must paint two tab bars, one per pane; \
                 row:\n{tab_row}"
                );
            });
        }

        /// Two real, distinctly-named tabs must both paint their labels on the
        /// tab bar.
        #[test]
        fn two_tabs_paint_both_labels() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1425_two_tabs_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let a = dir.join("zqxwA1425.txt");
            let b = dir.join("zqxwB1425.txt");
            std::fs::write(&a, "AAAA\n").unwrap();
            std::fs::write(&b, "BBBB\n").unwrap();

            let mut engine = plain_engine();
            engine.new_tab(Some(&a));
            engine.new_tab(Some(&b));
            engine.goto_tab(0);
            engine.close_tab();
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            // Both tabs' labels must paint on the tab bar.
            assert!(
                driver.screen_has("zqxwA1425.txt") && driver.screen_has("zqxwB1425.txt"),
                "both tabs' labels must paint on the tab bar; screen:\n{}",
                driver.screen()
            );
        }

        /// Double-clicking a *second* file in the Explorer sidebar,
        /// while a first file is already open in a real (non-preview) tab,
        /// must open it in a **second** tab — not swap the first tab's
        /// buffer in place. Reads back the tab bar's *own* painted row and
        /// requires it contain both labels: a buffer-swap-in-place bug
        /// leaves exactly one label painted on that row, never both.
        /// Scoping the read to the tab bar's own cells (not a bare
        /// `screen_has`/`screen_contains` call) matters because both
        /// filenames are *also* painted by the Explorer sidebar row and
        /// the breadcrumb segment, which stay open/visible throughout this
        /// test.
        #[test]
        fn explorer_double_click_opens_second_file_in_a_second_tab_1798() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1798_explorer_dblclick_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let sample = dir.join("sample.txt");
            let main_rs = dir.join("main.rs");
            std::fs::write(&sample, "sample contents\n").unwrap();
            std::fs::write(&main_rs, "fn main() {}\n").unwrap();

            let mut engine = plain_engine();
            engine.settings.breadcrumbs = false; // one-row tab bar, simplest geometry
            engine.cwd = dir.clone();
            engine.open_file_in_tab(&sample);
            // `open_file_in_tab` always *appends* a new tab for a file not
            // already open anywhere — unlike `open_file_with_mode`'s
            // `OpenMode::Permanent` path (the single-click twin below uses
            // that one via `startup_without_session_restore`), it never
            // reuses `plain_engine()`'s initial pristine `[No Name]`
            // scratch tab. Left in place, that scratch tab is still open
            // alongside sample.txt here, so close it the same way
            // `two_tabs_paint_both_labels` above does — otherwise the
            // double-click below lands on 3 tabs, not the 2 the bug
            // report's "sample.txt was the only tab open" setup describes.
            engine.goto_tab(0);
            engine.close_tab();
            engine.explorer_expanded.insert(dir.clone());
            engine.explorer_rebuild_rows();
            // `engine.session.explorer_visible` drives the TUI sidebar's
            // painted visibility here.
            engine.session.explorer_visible = true;
            let active_group = engine.active_group;
            let mut h = harness(engine);

            // Scoped to the tab bar's own row (same helper the main
            // assertions below use), not a bare `screen_has`.
            let precondition_row = active_group_tab_bar_row(&h, active_group);
            assert!(
                precondition_row.contains("sample.txt"),
                "precondition: sample.txt's tab must already be painted on \
                 the tab bar itself; tab bar row:\n{precondition_row}"
            );

            let driver = &mut h.driver;
            let (x, y) = driver
                .find("main.rs")
                .expect("the populated explorer row must paint main.rs's file name");
            driver.double_click(x, y);
            driver.render();

            assert_eq!(
                h.engine.borrow().active_group().tabs.len(),
                2,
                "double-clicking a second file in the Explorer must open a \
                 second tab, not replace the first one in place"
            );
            let tab_row = active_group_tab_bar_row(&h, active_group);
            assert!(
                tab_row.contains("sample.txt") && tab_row.contains("main.rs"),
                "the tab bar's own row must paint both the originally-open \
                 sample.txt tab and the newly double-clicked main.rs tab's \
                 labels — not just the sidebar or breadcrumb painting the \
                 same filename elsewhere on screen; tab bar row:\n{tab_row}"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// The bugbash report names *single*-click as an alternate
        /// trigger ("Single- or double-clicking a different file ... updated
        /// the breadcrumb/content/status bar ... but the tab strip kept
        /// showing only 'sample.txt x'"). A single click routes through
        /// `Engine::open_file_preview` (preview-tab mode) rather than
        /// `open_file_in_tab` — a different engine function from the
        /// double-click test above, so it needs its own coverage rather
        /// than assuming the double-click proof covers it.
        ///
        /// Primed via `Engine::startup_without_session_restore`, the same
        /// CLI-argument path (`open_file_with_mode` /
        /// `OpenMode::Permanent`, pristine-scratch-buffer reuse) the real
        /// report's "Open sample.txt" step used — not `open_file_in_tab`
        /// directly.
        ///
        /// Reads the painted labels back via `active_group_tab_bar_row`
        /// (scoped to the tab bar's own cells), not a bare
        /// `screen_has`/`screen_contains` call, for the same reason the
        /// double-click test above does — see its own doc.
        #[test]
        fn explorer_single_click_opens_second_file_in_a_second_tab_1798() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1798_explorer_click_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let sample = dir.join("sample.txt");
            let main_rs = dir.join("main.rs");
            std::fs::write(&sample, "sample contents\n").unwrap();
            std::fs::write(&main_rs, "fn main() {}\n").unwrap();

            let mut engine = plain_engine();
            engine.settings.breadcrumbs = false; // one-row tab bar, simplest geometry
            engine.cwd = dir.clone();
            engine.startup_without_session_restore(Some(&sample));
            engine.explorer_expanded.insert(dir.clone());
            engine.explorer_rebuild_rows();
            // See the double-click test above: `app_shell.show_panel` was
            // confirmed a no-op in this harness and is deliberately not
            // called here either.
            engine.session.explorer_visible = true;
            let active_group = engine.active_group;
            let mut h = harness(engine);

            // Scoped to the tab bar's own row, not a bare `screen_has` —
            // same reasoning as the double-click test above.
            let precondition_row = active_group_tab_bar_row(&h, active_group);
            assert!(
                precondition_row.contains("sample.txt"),
                "precondition: sample.txt's tab must already be painted on \
                 the tab bar itself; tab bar row:\n{precondition_row}"
            );

            let driver = &mut h.driver;
            let (x, y) = driver
                .find("main.rs")
                .expect("the populated explorer row must paint main.rs's file name");
            driver.click(x, y);
            driver.render();

            assert_eq!(
                h.engine.borrow().active_group().tabs.len(),
                2,
                "single-clicking a second file in the Explorer must open a \
                 second (preview) tab, not replace the first one in place"
            );
            let tab_row = active_group_tab_bar_row(&h, active_group);
            assert!(
                tab_row.contains("sample.txt") && tab_row.contains("main.rs"),
                "the tab bar's own row must paint both the originally-open \
                 sample.txt tab and the newly clicked main.rs preview \
                 tab's labels — not just the sidebar or breadcrumb painting \
                 the same filename elsewhere on screen; tab bar row:\n{tab_row}"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// A *stacked* split (`Ctrl+W s`) must paint the lower group's own
        /// tab row with its active tab's label — not overwritten by the
        /// group-boundary divider line. The divider's position coincides
        /// exactly with the lower group's tab-row band; the fix simply never
        /// paints a line for that direction.
        ///
        /// Deliberately reads the *painted* row the current frame's own
        /// `ScreenLayout::group_tab_bars` says the bottom group's tab bar
        /// occupies, rather than a hardcoded row number or a bare
        /// `screen_has` — a bare `screen_has(label)` would pass even on the
        /// unfixed bug, since the breadcrumb row directly below the
        /// (missing) tab row already shows the same filename.
        ///
        /// Uses a `(80, 25)` driver, not this module's usual `(80, 24)`:
        /// at `24` rows the boundary happens to land on an exact `.5` row,
        /// and the tab-bar rung *rounds* that to place the label row while
        /// the divider rung *truncates* it to place its own — two different
        /// conventions that, on a `.5` tie, land one row apart and
        /// accidentally dodge the collision. `25` rows lands the boundary
        /// on a whole row, where both conventions agree.
        #[test]
        fn stacked_groups_bottom_tab_row_shows_its_label_1586() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1586_stacked_tabs_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let top = dir.join("zqxwTOP1586.txt");
            let bottom = dir.join("zqxwBOTTOM1586.txt");
            std::fs::write(&top, "TOP\n").unwrap();
            std::fs::write(&bottom, "BOTTOM\n").unwrap();

            let mut engine = plain_engine();
            engine.settings.breadcrumbs = false; // one-row tab bar, simplest geometry
            engine.new_tab(Some(&top));
            // `open_editor_group`'s new group is always the *second* child
            // (`split_at(..., new_first: false)`) — for `Horizontal` that is
            // the bottom pane, and it becomes the active group.
            engine.open_editor_group(SplitDirection::Horizontal);
            engine.new_tab(Some(&bottom));
            let bottom_group = engine.active_group;

            let mut h = crate::tui_main::testing::conformance_harness(engine, 80, 25);
            collapse_sidebar(&mut h.driver);
            h.driver.render();

            let bounds = {
                let layout = h.screen_layout.borrow();
                let layout = layout.as_ref().expect("a frame must have been painted");
                layout
                    .group_tab_bars
                    .iter()
                    .find(|gtb| gtb.group_id == bottom_group)
                    .map(|gtb| gtb.bounds)
                    .expect("the bottom group must have its own tab bar entry")
            };
            let tab_row_idx = (bounds.y - 1.0).round() as usize;

            let screen = h.driver.screen();
            let tab_row = screen.lines().nth(tab_row_idx).unwrap_or_else(|| {
                panic!(
                    "row {tab_row_idx} (bottom group's tab row) is off-screen; screen:\n{screen}"
                )
            });
            assert!(
                tab_row.contains("zqxwBOTTOM1586.txt"),
                "the bottom group's own tab row (row {tab_row_idx}) must show its \
                 active tab's label, not a group-divider line; row:\n{tab_row}\n\
                 full screen:\n{screen}"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// A single-group frame must not paint a group-divider glyph ('│')
        /// anywhere to the right of the tab label.
        #[test]
        fn unsplit_editor_paints_no_group_divider_glyph() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "fn main() {}\n");
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::unsplit_editor_paints_no_group_divider_glyph",
                || {
                    let screen = driver.screen();
                    // Locate the tab row by content, not by assuming row 0 — `App`
                    // reserves its own permanent menu-bar row above the tab bar,
                    // unlike the shipped TUI.
                    let (tab_y, tab_start) = screen
                        .lines()
                        .enumerate()
                        .find_map(|(y, line)| line.find("[No Name]").map(|x| (y, x)))
                        .expect("the tab bar must paint \"[No Name]\" somewhere on screen");
                    for (y, line) in screen.lines().enumerate().skip(tab_y + 1).take(15) {
                        let stray = line.chars().skip(tab_start).find(|&c| c == '\u{2502}');
                        assert!(
                            stray.is_none(),
                            "row {y}: unexpected group-divider glyph with a single \
                     editor group; line:\n{line}"
                        );
                    }
                },
            );
        }

        /// `:set nonerdfont` (already the default here) must not change the
        /// tab bar's own geometry between two otherwise-identical frames —
        /// paint determinism smoke, the `App` twin of `shell_app.rs`'s
        /// `nerd_fonts_off_keeps_tab_bar_geometry_byte_identical_via_shell_app`.
        #[test]
        fn repeated_paint_is_byte_identical() {
            fn fixture() -> crate::core::Engine {
                let mut engine = plain_engine();
                engine.buffer_mut().insert(0, "short\n");
                engine
            }
            let h1 = harness(fixture());
            let h2 = harness(fixture());

            known_bug_gate("app_on_tui::repeated_paint_is_byte_identical", || {
                assert_eq!(
                    h1.driver.screen(),
                    h2.driver.screen(),
                    "two harnesses built from the same fixture must paint byte-\
                 identical screens"
                );
            });
        }

        // Tab drag / hover / :tabonly

        /// Mirrors `shell_app.rs`'s test of the same name: the
        /// tab-drag ghost overlay must paint a third `"[No Name]"` occurrence
        /// (the two static tab labels, plus the drag ghost) once a drag has
        /// started but not yet released.
        #[test]
        fn render_content_paints_tab_drag_ghost_via_shell_app() {
            let mut engine = plain_engine();
            engine.new_tab(None);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::render_content_paints_tab_drag_ghost_via_shell_app",
                || {
                    let (tx, ty) = driver
                        .find("[No Name]")
                        .expect("tab label should be painted on screen");
                    driver.mouse_down(tx, ty);
                    driver.mouse_move(tx + 4.0, ty + 3.0);

                    let screen = driver.screen();
                    let occurrences = screen.matches("[No Name]").count();
                    assert!(
                        occurrences >= 3,
                        "expected the two static tab labels plus a drag-ghost \
                         label (>= 3 occurrences of \"[No Name]\"), got \
                         {occurrences}; screen:\n{screen}"
                    );
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: dragging a
        /// tab past a neighbour on the unsplit tab bar must actually reorder
        /// the painted tab labels, not just paint a ghost overlay.
        #[test]
        fn tui_tab_drag_past_a_neighbour_reorders_the_painted_tab_bar() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1431_tui_tab_drag_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let a = dir.join("zqa1431.txt");
            let b = dir.join("zqb1431.txt");
            std::fs::write(&a, "a\n").unwrap();
            std::fs::write(&b, "b\n").unwrap();

            let mut engine = plain_engine();
            engine.new_tab(Some(&a));
            engine.new_tab(Some(&b));
            assert_eq!(
                engine.group_layout.leaf_count(),
                1,
                "this test covers the unsplit single-group tab bar arm"
            );
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::tui_tab_drag_past_a_neighbour_reorders_the_painted_tab_bar",
                || {
                    let left_before = driver
                        .find_bounds("zqa1431.txt")
                        .expect("tab a should be painted on the tab bar");
                    let right_before = driver
                        .find_bounds("zqb1431.txt")
                        .expect("tab b should be painted on the tab bar");
                    assert!(
                        left_before.x < right_before.x,
                        "new_tab appends, so a's tab should paint left of \
                         b's; a={left_before:?} b={right_before:?}"
                    );

                    let from = (
                        left_before.x + left_before.width / 2.0,
                        left_before.y + left_before.height / 2.0,
                    );
                    let to = (
                        right_before.x + right_before.width * 0.75,
                        right_before.y + right_before.height / 2.0,
                    );
                    driver.mouse_down(from.0, from.1);
                    driver.mouse_move(to.0, to.1);
                    driver.mouse_move(to.0, to.1);
                    driver.mouse_up(to.0, to.1);

                    let left_after = driver
                        .find_bounds("zqa1431.txt")
                        .expect("tab a should still be painted after the drop");
                    let right_after = driver
                        .find_bounds("zqb1431.txt")
                        .expect("tab b should still be painted after the drop");
                    assert!(
                        left_after.x > right_after.x,
                        "dragging a onto b must repaint it to the right of b \
                         (was {} < {}, now {} vs {})",
                        left_before.x,
                        right_before.x,
                        left_after.x,
                        right_after.x
                    );
                },
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// Mirrors `shell_app.rs`'s test of the same name: the
        /// tab-hover tooltip must paint from `engine.tab_hover_tooltip`
        /// alone, with no live hover-dwell timer needed.
        #[test]
        fn render_content_paints_tab_hover_tooltip_via_shell_app() {
            let mut engine = plain_engine();
            engine.tab_hover_tooltip = Some("ZQXW_609_TOOLTIP_MARKER".to_string());
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::render_content_paints_tab_hover_tooltip_via_shell_app",
                || {
                    assert!(
                        driver.screen_has("ZQXW_609_TOOLTIP_MARKER"),
                        "tab-hover tooltip should paint via \
                         App::render_content; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: `:tabonly`
        /// must collapse the painted tab bar to a single `"[No Name]"`
        /// label — located by content, not assumed to be row 0 (see
        /// `render_content_paints_single_group_tab_bar_via_shell_app`'s own
        /// doc on why).
        #[test]
        fn ex_tabonly_collapses_tab_bar_via_shell_app() {
            let mut engine = plain_engine();
            engine.settings.hide_single_tab = false;
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::ex_tabonly_collapses_tab_bar_via_shell_app",
                || {
                    for _ in 0..2 {
                        driver.type_char(':');
                        for c in "tabnew".chars() {
                            driver.type_char(c);
                        }
                        driver.press_named(quadraui::NamedKey::Enter);
                    }
                    driver.render();

                    let screen = driver.screen();
                    let sanity_row = screen
                        .lines()
                        .find(|line| line.contains("[No Name]"))
                        .unwrap_or("");
                    assert_eq!(
                        sanity_row.matches("[No Name]").count(),
                        3,
                        "sanity: 3 tabs must be open before :tabonly; \
                         row:\n{sanity_row}"
                    );

                    driver.type_char(':');
                    for c in "tabonly".chars() {
                        driver.type_char(c);
                    }
                    driver.press_named(quadraui::NamedKey::Enter);
                    driver.render();

                    let screen = driver.screen();
                    let tab_row = screen
                        .lines()
                        .find(|line| line.contains("[No Name]"))
                        .unwrap_or("");
                    assert_eq!(
                        tab_row.matches("[No Name]").count(),
                        1,
                        ":tabonly must collapse the tab bar to a single tab; \
                         row:\n{tab_row}"
                    );
                },
            );
        }

        /// Locate `needle` painted as the Explorer sidebar's *own* row —
        /// not the tab bar's label (row 0) and not the status bar's
        /// "current file" segment (the second-to-last row), both of which
        /// also paint the bare filename as an exact, standalone text run.
        ///
        /// Built on [`quadraui::testing::ConformanceDriver::inventory`]'s
        /// `text_runs()` rather than a bare `driver.find(needle)`: once the
        /// Explorer reveals the active file, its name paints in *three*
        /// places at once (tab label, status bar, Explorer row), and
        /// `find` always returns the first (tab-bar) hit — never the
        /// sidebar one this helper's callers need. Filtering runs to
        /// `text == needle` (the sidebar paints the filename as its own
        /// `StyledText` segment, separate from its icon — see
        /// `build_explorer_tree_rows` in `src/render/sidebar_tree_population.rs`)
        /// plus a row range that excludes the tab bar (`y == 0`) and the
        /// status/command-line rows at the bottom of this module's fixed
        /// `(80, 24)` harness leaves only a genuine sidebar match.
        fn find_below_tab_bar(
            h: &crate::harness::ConformanceHarness<
                quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            >,
            needle: &str,
        ) -> Option<(u16, u16)> {
            h.driver
                .inventory()
                .text_runs()
                .iter()
                .find(|r| {
                    // Row 0 is the tab bar itself (its own label is an exact
                    // match too); rows 22-23 are the status line and
                    // command line, where the status bar's "current file"
                    // segment paints the bare filename as its own run, just
                    // like the Explorer's filename-only run does. Only a
                    // match strictly between those — inside the sidebar's
                    // body rows — is a genuine Explorer reveal.
                    r.text == needle && r.bounds.y > 0.5 && r.bounds.y < 21.5
                })
                .map(|r| (r.bounds.x.round() as u16, r.bounds.y.round() as u16))
        }

        /// VS Code's `explorer.autoReveal` (default on) expands the
        /// active file's ancestor folders, selects its row and scrolls it
        /// into view in the Explorer sidebar whenever the active editor
        /// changes — including a plain tab-bar click, not just opening a
        /// file for the first time.
        ///
        /// Both tabs are opened through *relative* paths, with the process
        /// working directory moved into the fixture, because that is the
        /// shape a real `cd myrepo && vimcode sub/nested.rs` launch (and
        /// every later relative `:e`) hands the engine: the buffer's
        /// `file_path` stays relative while `engine.cwd` and
        /// `explorer_rows` are absolute. [`crate::test_cwd::CwdGuard`] is
        /// scoped to the setup block below — `harness` takes a
        /// `CwdReadGuard`, and one thread must never hold both (see that
        /// module's doc).
        ///
        /// Reads the sidebar's painted background colour back via
        /// [`find_below_tab_bar`] + `style_at`, the same proof
        /// `explorer_unfocused_selected_row_paints_sidebar_sel_bg_inactive_via_shell_app`
        /// (`mod sidebar_panels`) uses for a direct `explorer_reveal_path`
        /// call — this test's only difference is the trigger: a genuine
        /// `driver.click` on the tab bar's own painted label.
        ///
        /// Observed red against unfixed `develop`:
        /// `explorer_reveal_active_file` there hands the buffer's raw
        /// `file_path` (`sub/nested.rs`) to `explorer_reveal_path`, which
        /// compares it against the always-absolute `self.cwd` and
        /// `explorer_rows` without absolutizing it, so `rel` is `None`,
        /// nothing is expanded or selected, and the `find_below_tab_bar`
        /// lookup after the click panics with "must auto-reveal it".
        /// Confirmed failing, then restored.
        #[test]
        fn tab_bar_click_reveals_and_highlights_active_file_in_explorer_1903() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1903_tab_click_reveal_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("sub")).unwrap();
            // Canonical, so it compares equal to the `engine.cwd` the
            // engine reads back out of the process below.
            let dir = std::fs::canonicalize(&dir).unwrap();
            let sub = dir.join("sub");
            let nested = sub.join("nested.rs");
            let nested_rel = std::path::PathBuf::from("sub").join("nested.rs");
            std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
            std::fs::write(&nested, "// nested\n").unwrap();

            let (engine, active_group) = {
                let _cwd = crate::test_cwd::CwdGuard::new();
                std::env::set_current_dir(&dir).expect("chdir into the fixture directory");

                let mut engine = plain_engine();
                engine.settings.colorscheme = "vscode-light".to_string();
                engine.settings.breadcrumbs = false; // one-row tab bar, simplest geometry
                assert_eq!(
                    engine.cwd, dir,
                    "sanity: Engine::new_for_test must pick up the process \
                     cwd this test just moved to, the same way Engine::new \
                     would on a real `cd dir && vimcode ...` launch"
                );
                engine.open_file_in_tab(std::path::Path::new("main.rs"));
                // `open_file_in_tab` appends rather than reusing
                // `plain_engine`'s pristine `[No Name]` scratch tab — close
                // it so the tab bar only shows the two tabs this test cares
                // about, same as
                // `explorer_double_click_opens_second_file_in_a_second_tab_1798`.
                engine.goto_tab(0);
                engine.close_tab();
                // Opening `nested` makes it the active tab and auto-reveals
                // it (expanding `sub`); immediately undo both so the click
                // below is the thing that re-expands/re-selects it, not a
                // leftover from setup.
                engine.open_file_in_tab(&nested_rel);
                engine.goto_tab(0);
                engine.explorer_expanded.remove(&sub);
                engine.explorer_rebuild_rows();
                engine.app_shell.show_panel(&quadraui::WidgetId::new(
                    crate::core::engine::sidebar::PANEL_EXPLORER,
                ));
                engine.session.explorer_visible = true;
                engine.explorer_has_focus = false;
                let active_group = engine.active_group;
                (engine, active_group)
            };
            let mut h = harness(engine);

            let precondition_row = active_group_tab_bar_row(&h, active_group);
            assert!(
                precondition_row.contains("main.rs") && precondition_row.contains("nested.rs"),
                "precondition: both tabs must already be painted on the tab \
                 bar; tab bar row:\n{precondition_row}"
            );
            assert!(
                find_below_tab_bar(&h, "nested.rs").is_none(),
                "precondition: `sub` must start collapsed, so nested.rs \
                 does not yet paint anywhere below the tab bar; screen:\n{}",
                h.driver.screen()
            );

            let driver = &mut h.driver;
            let (x, y) = driver
                .find("nested.rs")
                .expect("nested.rs's tab label must paint on the tab bar");
            driver.click(x, y);
            driver.render();

            assert_eq!(
                h.engine.borrow().file_path(),
                Some(&nested_rel),
                "clicking nested.rs's tab must make it the active file, \
                 still under the relative path it was opened with"
            );

            let (rx, ry) = find_below_tab_bar(&h, "nested.rs").unwrap_or_else(|| {
                panic!(
                    "clicking nested.rs's tab must auto-reveal it in the \
                     Explorer sidebar (expanding `sub`); screen:\n{}",
                    h.driver.screen()
                )
            });
            let style = h
                .driver
                .style_at(rx, ry)
                .expect("the revealed row must paint a styled cell");
            let expected_bg =
                quadraui::tui::ratatui_color(crate::render::Theme::light().sidebar_sel_bg_inactive);
            assert_eq!(
                style.bg,
                expected_bg,
                "the revealed row must be selected/highlighted with \
                 `sidebar_sel_bg_inactive`; screen:\n{}",
                h.driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// `explorer_auto_reveal` (default on, mirroring VS Code's
        /// `explorer.autoReveal`) must be possible to turn off — with it
        /// off, the same tab-bar click as
        /// `tab_bar_click_reveals_and_highlights_active_file_in_explorer_1903`
        /// must still switch the active tab (that behaviour predates and is
        /// independent of this setting) but must leave the Explorer sidebar
        /// alone: `sub` stays collapsed and nested.rs never paints there.
        ///
        /// Opens both files through absolute paths, unlike
        /// `tab_bar_click_reveals_and_highlights_active_file_in_explorer_1903`
        /// — the reveal has to be the thing the setting suppresses, so the
        /// reveal must be capable of succeeding in the first place.
        ///
        /// Observed red against unfixed `develop` (`explorer_ops.rs`
        /// restored, `settings.rs` kept so the field still compiles): with
        /// no `explorer_auto_reveal` check in `explorer_reveal_path`,
        /// `goto_tab`'s reveal runs regardless of the setting, `sub` is
        /// expanded and nested.rs paints in the sidebar, so the final
        /// `find_below_tab_bar(...).is_none()` assertion fails. Confirmed
        /// failing, then restored.
        #[test]
        fn tab_bar_click_does_not_reveal_when_explorer_auto_reveal_is_off_1903() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1903_tab_click_noreveal_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            let sub = dir.join("sub");
            std::fs::create_dir_all(&sub).unwrap();
            let main_rs = dir.join("main.rs");
            let nested = sub.join("nested.rs");
            std::fs::write(&main_rs, "fn main() {}\n").unwrap();
            std::fs::write(&nested, "// nested\n").unwrap();

            let mut engine = plain_engine();
            engine.settings.colorscheme = "vscode-light".to_string();
            engine.settings.breadcrumbs = false;
            engine.cwd = dir.clone();
            engine.open_file_in_tab(&main_rs);
            engine.goto_tab(0);
            engine.close_tab();
            engine.open_file_in_tab(&nested);
            engine.goto_tab(0);
            engine.explorer_expanded.remove(&sub);
            engine.explorer_rebuild_rows();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            engine.session.explorer_visible = true;
            engine.explorer_has_focus = false;
            engine.settings.explorer_auto_reveal = false;
            let active_group = engine.active_group;
            let mut h = harness(engine);

            let precondition_row = active_group_tab_bar_row(&h, active_group);
            assert!(
                precondition_row.contains("main.rs") && precondition_row.contains("nested.rs"),
                "precondition: both tabs must already be painted on the tab \
                 bar; tab bar row:\n{precondition_row}"
            );
            assert!(
                find_below_tab_bar(&h, "nested.rs").is_none(),
                "precondition: `sub` must start collapsed; screen:\n{}",
                h.driver.screen()
            );

            let driver = &mut h.driver;
            let (x, y) = driver
                .find("nested.rs")
                .expect("nested.rs's tab label must paint on the tab bar");
            driver.click(x, y);
            driver.render();

            assert_eq!(
                h.engine.borrow().file_path(),
                Some(&nested),
                "clicking nested.rs's tab must still switch the active \
                 file — only the Explorer reveal is suppressed"
            );
            assert!(
                find_below_tab_bar(&h, "nested.rs").is_none(),
                "with `explorer_auto_reveal` off, clicking the tab must \
                 NOT expand `sub` or reveal nested.rs in the Explorer \
                 sidebar; screen:\n{}",
                h.driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// Bringing the Explorer panel to the front is an "active
        /// editor changed, as far as the Explorer is concerned" moment too —
        /// VS Code reveals the active file when the Explorer view becomes
        /// visible, not only when the editor switches while it is already
        /// open. `explorer_expanded` is in-memory only (never persisted to
        /// the session) and `explorer_collapse_all` wipes it, so without
        /// that trigger a user who collapses the tree, works in another
        /// panel and comes back finds the Explorer showing a bare root with
        /// the active file nowhere in sight and nothing highlighted.
        ///
        /// Drives the real gesture: a `driver.click` on the Explorer
        /// activity-bar icon's chrome zone (same lookup as
        /// [`collapse_sidebar`]), through `AppShell::handle_activity_click`
        /// and the backend-neutral `render::apply_activity_panel_switch`
        /// rung, then reads the painted sidebar row's background back with
        /// `style_at`.
        ///
        /// Observed red against unfixed `develop`: showing a panel is not a
        /// reveal trigger there at all, so the click brings up an Explorer
        /// still collapsed to its root, `nested.rs` has no row to find and
        /// the `find_below_tab_bar` lookup panics with "must auto-reveal the
        /// active file". Confirmed failing, then restored.
        #[test]
        fn activity_bar_click_reveals_active_file_in_explorer_1903() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1903_activity_reveal_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            let sub = dir.join("sub");
            std::fs::create_dir_all(&sub).unwrap();
            let nested = sub.join("nested.rs");
            std::fs::write(&nested, "// nested\n").unwrap();

            let mut engine = plain_engine();
            engine.settings.colorscheme = "vscode-light".to_string();
            engine.settings.breadcrumbs = false;
            engine.cwd = dir.clone();
            engine.open_file_in_tab(&nested);
            // The user collapsed the whole tree while the Explorer was
            // hidden (or it simply never expanded in this session —
            // `explorer_expanded` is in-memory only), so nested.rs's
            // ancestor folder is shut and its row does not exist.
            engine.explorer_collapse_all();
            let mut h = harness(engine);

            assert!(
                find_below_tab_bar(&h, "nested.rs").is_none(),
                "precondition: with the tree collapsed, nested.rs must not \
                 paint below the tab bar yet; screen:\n{}",
                h.driver.screen()
            );

            // One click: the runner's fresh `AppShell` starts with the
            // hamburger panel active (see [`collapse_sidebar`]), so the
            // first click on Explorer's icon activates + shows it.
            let zone = h
                .driver
                .inventory()
                .zones()
                .iter()
                .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_EXPLORER)
                .map(|z| z.bounds)
                .expect("the Explorer activity-bar icon must register a chrome zone");
            h.driver
                .click(zone.x + zone.width / 2.0, zone.y + zone.height / 2.0);
            h.driver.render();

            let (rx, ry) = find_below_tab_bar(&h, "nested.rs").unwrap_or_else(|| {
                panic!(
                    "showing the Explorer must auto-reveal the active file \
                     (expanding `sub`); screen:\n{}",
                    h.driver.screen()
                )
            });
            let style = h
                .driver
                .style_at(rx, ry)
                .expect("the revealed row must paint a styled cell");
            // Clicking the icon focuses the panel, so the revealed row is
            // the *focused* tree selection — quadraui's `selected_bg`, not
            // the faint `inactive_selected_bg` an unfocused Explorer paints
            // (see `explorer_unfocused_selected_row_paints_sidebar_sel_bg_
            // inactive_via_shell_app`).
            let qtheme = crate::render::to_quadraui_theme(&crate::render::Theme::light());
            assert_eq!(
                style.bg,
                quadraui::tui::ratatui_color(qtheme.selected_bg),
                "the revealed row must paint the focused selection \
                 background; screen:\n{}",
                h.driver.screen()
            );
            assert_ne!(
                style.bg,
                quadraui::tui::ratatui_color(qtheme.inactive_selected_bg),
                "sanity: focused and unfocused selection backgrounds must \
                 differ for this assertion to mean anything"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// The reveal-on-panel-show in
        /// `activity_bar_click_reveals_active_file_in_explorer_1903` obeys
        /// `explorer_auto_reveal` like every other reveal trigger: with the
        /// setting off, showing the Explorer must leave the collapsed tree
        /// exactly as the user left it.
        ///
        /// Unfixed `develop` passes this one vacuously — nothing reveals on
        /// panel show there, so there is nothing for the setting to
        /// suppress. Its red-capability was observed the other way round,
        /// against this branch with the `explorer_auto_reveal` check deleted
        /// from `explorer_reveal_path`: the reveal then runs regardless of
        /// the setting, `sub` is expanded and the final
        /// `find_below_tab_bar(...).is_none()` assertion fails. Confirmed
        /// failing, then restored.
        #[test]
        fn activity_bar_click_does_not_reveal_when_auto_reveal_is_off_1903() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1903_activity_noreveal_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            let sub = dir.join("sub");
            std::fs::create_dir_all(&sub).unwrap();
            let nested = sub.join("nested.rs");
            std::fs::write(&nested, "// nested\n").unwrap();

            let mut engine = plain_engine();
            engine.settings.colorscheme = "vscode-light".to_string();
            engine.settings.breadcrumbs = false;
            engine.cwd = dir.clone();
            engine.open_file_in_tab(&nested);
            engine.explorer_collapse_all();
            engine.settings.explorer_auto_reveal = false;
            let mut h = harness(engine);

            let zone = h
                .driver
                .inventory()
                .zones()
                .iter()
                .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_EXPLORER)
                .map(|z| z.bounds)
                .expect("the Explorer activity-bar icon must register a chrome zone");
            h.driver
                .click(zone.x + zone.width / 2.0, zone.y + zone.height / 2.0);
            h.driver.render();

            assert!(
                h.driver.find("sub").is_some(),
                "the Explorer panel must actually be showing (its root's \
                 `sub` folder row paints); screen:\n{}",
                h.driver.screen()
            );
            assert!(
                find_below_tab_bar(&h, "nested.rs").is_none(),
                "with `explorer_auto_reveal` off, showing the Explorer must \
                 not expand `sub` or reveal nested.rs; screen:\n{}",
                h.driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// Regression guard: `Engine::explorer_reveal_path` is handed
        /// whatever `target` the caller has, not necessarily an absolute
        /// one. The single most common real launch — `cd myrepo && vimcode
        /// src/main.rs` — hands `Engine::startup` a path relative to the
        /// process's (and therefore `engine.cwd`'s) working directory, all
        /// the way from `Args::parse` in `src/main.rs`.
        /// [`crate::test_cwd::CwdGuard`] moves the *real* process working
        /// directory (not just `engine.cwd`) into the fixture for the setup
        /// block, so `plain_engine`'s `Engine::new_for_test` picks it up
        /// exactly the way `Engine::new` would on a real launch, and
        /// `startup_without_session_restore` is then driven with a genuinely
        /// relative `file_path` — the same shape a real CLI argument has.
        /// The guard is scoped to that block and dropped before `harness`,
        /// which takes a `CwdReadGuard`: one thread must never hold both
        /// (see `crate::test_cwd`'s module doc).
        ///
        /// Observed red against unfixed `develop`: with
        /// `explorer_reveal_path` comparing `target` to `self.cwd`/
        /// `explorer_rows` as literal `PathBuf`s (no absolutization), the
        /// relative `target` never shares a prefix with the always-absolute
        /// `self.cwd`, so `rel` is `None`, nothing gets expanded or
        /// selected, and this test's `find_below_tab_bar` lookup below
        /// fails with "must auto-reveal it". Confirmed failing, then
        /// restored.
        #[test]
        fn startup_with_relative_file_path_reveals_it_in_explorer_1903() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1903_startup_relative_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("rel")).unwrap();
            // Canonical, so it compares equal to the `engine.cwd` the engine
            // reads back out of the process below.
            let dir = std::fs::canonicalize(&dir).unwrap();
            std::fs::write(dir.join("rel").join("relative.rs"), "// relative\n").unwrap();

            let engine = {
                let _cwd = crate::test_cwd::CwdGuard::new();
                std::env::set_current_dir(&dir).expect("chdir into the fixture directory");

                let mut engine = plain_engine();
                engine.settings.colorscheme = "vscode-light".to_string();
                engine.settings.breadcrumbs = false; // one-row tab bar, simplest geometry
                assert_eq!(
                    engine.cwd, dir,
                    "sanity: Engine::new_for_test must pick up the process cwd \
                     this test just moved to, the same way Engine::new would on \
                     a real `cd dir && vimcode ...` launch"
                );

                engine
                    .startup_without_session_restore(Some(std::path::Path::new("rel/relative.rs")));

                engine.app_shell.show_panel(&quadraui::WidgetId::new(
                    crate::core::engine::sidebar::PANEL_EXPLORER,
                ));
                engine.session.explorer_visible = true;
                engine
            };
            let h = harness(engine);

            assert!(
                h.driver.screen_has("relative.rs"),
                "startup with a relative file_path must still open that \
                 file (onto the tab bar); screen:\n{}",
                h.driver.screen()
            );

            let (rx, ry) = find_below_tab_bar(&h, "relative.rs").unwrap_or_else(|| {
                panic!(
                    "startup with a relative file_path must auto-reveal it \
                     in the Explorer sidebar (expanding `rel`); screen:\n{}",
                    h.driver.screen()
                )
            });
            let style = h
                .driver
                .style_at(rx, ry)
                .expect("the revealed row must paint a styled cell");
            let expected_bg =
                quadraui::tui::ratatui_color(crate::render::Theme::light().sidebar_sel_bg_inactive);
            assert_eq!(
                style.bg,
                expected_bg,
                "the revealed row must be selected/highlighted with \
                 `sidebar_sel_bg_inactive`; screen:\n{}",
                h.driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Bottom band (quickfix / debug output / terminal / status)
    // ─────────────────────────────────────────────────────────────────────────
    mod bottom_band {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name: the Debug
        /// Output branch of the bottom panel must paint its content.
        #[test]
        fn render_content_paints_bottom_panel_debug_output_via_shell_app() {
            let mut engine = plain_engine();
            engine.bottom_panel_open = true;
            engine.bottom_panel_kind = crate::render::BottomPanelKind::DebugOutput;
            engine
                .dap_output_lines
                .push("ZQXW_608_DEBUG_OUTPUT_MARKER".to_string());
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::render_content_paints_bottom_panel_debug_output_via_shell_app",
                || {
                    assert!(
                        driver.screen_has("ZQXW_608_DEBUG_OUTPUT_MARKER"),
                        "bottom panel debug-output content should paint; \
                     screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: the terminal
        /// branch of the bottom panel must paint its "Terminal" tab-strip
        /// label without panicking (spawns a real PTY, same pattern the
        /// mirrored test uses).
        #[test]
        fn render_content_paints_bottom_panel_terminal_via_shell_app() {
            let mut engine = plain_engine();
            engine.terminal_new_tab(80, 10);
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::render_content_paints_bottom_panel_terminal_via_shell_app",
                || {
                    assert!(
                        driver.screen_has("Terminal"),
                        "bottom panel terminal tab bar should paint; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// The bottom panel tab strip must offer more than a single bare
        /// "Terminal" label: at minimum "Terminal" (active) and "Problems"
        /// must both paint when the terminal is open, so the panel reads as
        /// a switchable VS Code-style tab bar rather than a plain label.
        #[test]
        fn render_content_paints_bottom_panel_problems_tab_via_shell_app() {
            let mut engine = plain_engine();
            engine.terminal_new_tab(80, 10);
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::render_content_paints_bottom_panel_problems_tab_via_shell_app",
                || {
                    assert!(
                        driver.screen_has("Terminal"),
                        "bottom panel should still paint the active Terminal tab; \
                         screen:\n{}",
                        driver.screen()
                    );
                    assert!(
                        driver.screen_has("Problems"),
                        "bottom panel tab strip should also offer a Problems tab; \
                         screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// A click on the "Problems" tab must switch which panel is
        /// **painted**, through `App::handle_dispatch`'s shared
        /// `render::route_bottom_panel_click` -> `render::apply_bottom_panel_
        /// route` -> `Engine::handle_bottom_tab_bar_click` chain (the same
        /// chain `gtk::testing::bottom_panel_tab_strip_click_switches_the_
        /// painted_panel` drives on the other backend). With only a
        /// terminal open (no debug output), `build_bottom_panel_tab_bar`
        /// paints three slots: Problems, Output, Terminal — clicking
        /// "Problems" must swap the terminal content for the Problems
        /// placeholder.
        ///
        /// The index-to-kind mapping inside
        /// `Engine::handle_bottom_tab_bar_click` is the load-bearing half:
        /// it rebuilds the very sequence `build_bottom_panel_tab_bar` paints,
        /// so slot 0 resolves to `Problems`. Any drift between those two
        /// orders leaves the terminal content on screen after the click and
        /// trips the final assertion.
        #[test]
        fn bottom_panel_tab_strip_click_switches_the_painted_panel_via_shell_app() {
            let mut engine = plain_engine();
            engine.terminal_new_tab(80, 10);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            assert!(
                !driver.screen_has("No problems have been detected"),
                "precondition: the Terminal tab owns the panel body; screen:\n{}",
                driver.screen()
            );

            let (x, y) = driver
                .find("Problems")
                .expect("the bottom panel tab strip must paint a Problems tab");
            driver.click(x, y);

            assert!(
                driver.screen_has("No problems have been detected"),
                "clicking the Problems tab must repaint the panel body with the \
                 Problems placeholder; screen:\n{}",
                driver.screen()
            );
        }

        /// The same click-to-switch chain, driven onto the *second* always-
        /// present slot: with only a terminal open the strip paints
        /// Problems, Output, Terminal, so "Output" is slot 1 and clicking it
        /// must paint the Output placeholder instead of the terminal body.
        /// Covers the index arithmetic one step past slot 0 — an off-by-one
        /// in either half of the lockstep order would land on Problems or
        /// Terminal here.
        #[test]
        fn bottom_panel_tab_strip_click_switches_to_the_output_panel_via_shell_app() {
            let mut engine = plain_engine();
            engine.terminal_new_tab(80, 10);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            assert!(
                !driver.screen_has("No output channels are available"),
                "precondition: the Terminal tab owns the panel body; screen:\n{}",
                driver.screen()
            );

            let (x, y) = driver
                .find("Output")
                .expect("the bottom panel tab strip must paint an Output tab");
            driver.click(x, y);

            assert!(
                driver.screen_has("No output channels are available"),
                "clicking the Output tab must repaint the panel body with the \
                 Output placeholder; screen:\n{}",
                driver.screen()
            );
        }

        /// Clicking the bottom panel's tab strip at all — even the already-
        /// active "Terminal" tab, a no-op for which panel is painted — goes
        /// through `App::handle_mouse_click_msg`'s unconditional pre-dispatch
        /// `Engine::clear_sidebar_focus()` call before the click is routed
        /// to `handle_bottom_tab_bar_click`. `clear_sidebar_focus` must leave
        /// `terminal_has_focus` alone (it only clears the sidebar-band
        /// flags), so a terminal that was genuinely focused before the click
        /// must still be focused after it.
        #[test]
        fn bottom_panel_terminal_tab_click_leaves_the_terminal_focused_via_shell_app() {
            let mut engine = plain_engine();
            engine.terminal_new_tab(80, 10);
            let mut h = harness_no_sidebar(engine);
            // `harness_no_sidebar`'s sidebar-collapse clicks land on the
            // Explorer activity-bar icon, outside the bottom panel, so they
            // legitimately clear `terminal_has_focus` themselves. Set focus
            // only after that setup, right before the click under test.
            h.engine.borrow_mut().terminal_has_focus = true;
            let driver = &mut h.driver;

            let (x, y) = driver
                .find("Terminal")
                .expect("the bottom panel tab strip must paint a Terminal tab");
            driver.click(x, y);

            assert!(
                h.engine.borrow().terminal_has_focus,
                "clicking the bottom panel's tab strip must not steal focus \
                 from an already-focused terminal"
            );
        }

        /// An **open but empty** quickfix list must reserve no rows — paint-
        /// only twin of `shell_app.rs`'s
        /// `empty_quickfix_does_not_displace_the_terminal_band_via_shell_app`:
        /// with a terminal open and an empty quickfix, the terminal's own
        /// "Terminal" tab-strip label must still paint (an empty quickfix
        /// wrongly reserving six rows would push it out of the fixed 24-row
        /// viewport).
        #[test]
        fn empty_quickfix_does_not_displace_the_terminal_band() {
            let mut engine = plain_engine();
            engine.terminal_new_tab(80, 8);
            engine.quickfix.open = true;
            engine.quickfix.items.clear();
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::empty_quickfix_does_not_displace_the_terminal_band",
                || {
                    assert!(
                        driver.screen_has("Terminal"),
                        "an empty-but-open quickfix must reserve no rows, so the \
                 terminal tab strip must still paint; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// With both a quickfix item and an open bottom panel, the quickfix
        /// content must paint strictly above the bottom panel's own content —
        /// paint-only twin of `shell_app.rs`'s
        /// `separated_status_paints_below_the_bottom_panel_via_shell_app`
        /// (that test additionally inspects the private `composed_bottom_band`
        /// field; this only reads painted geometry, via `find_bounds`).
        #[test]
        fn quickfix_paints_above_the_bottom_panel() {
            let mut engine = plain_engine();
            // Short filename/marker text — see `quickfix_with_item_paints_row`'s
            // own comment on why: this module's `(80, 24)` size leaves far less
            // row width than `crate::harness`'s usual `(800, 480)`.
            engine
                .quickfix
                .items
                .push(crate::core::project_search::ProjectMatch {
                    file: std::path::PathBuf::from("a.rs"),
                    line: 0,
                    col: 0,
                    line_text: "QFORDER".to_string(),
                });
            engine.quickfix.open = true;
            engine.bottom_panel_open = true;
            engine.bottom_panel_kind = crate::render::BottomPanelKind::DebugOutput;
            engine.dap_output_lines.push("DBGORDER".to_string());
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate("app_on_tui::quickfix_paints_above_the_bottom_panel", || {
                let screen = driver.screen();
                let qf = driver
                    .find_bounds("QFORDER")
                    .unwrap_or_else(|| panic!("quickfix must have painted; screen:\n{screen}"));
                let dbg = driver
                    .find_bounds("DBGORDER")
                    .unwrap_or_else(|| panic!("bottom panel must have painted; screen:\n{screen}"));
                assert!(
                    qf.y < dbg.y,
                    "the quickfix panel must paint above the bottom panel \
                 (quickfix at row {}, debug output at row {}); screen:\n{screen}",
                    qf.y,
                    dbg.y
                );
            });
        }

        /// The global status bar's `Ln N, Col N` cursor segment must paint —
        /// the signal `crate::harness::activity_bar_click_focuses_search_panel`
        /// itself asserts on for `gtk`/`tui_prod`.
        #[test]
        fn status_bar_paints_cursor_position() {
            let h = harness_no_sidebar(plain_engine());
            let driver = &h.driver;

            // `App::render_content` reserves the px profile's
            // tab/breadcrumb row heights as cell-grid rows.
            assert!(
                driver.screen_has("Ln 1,"),
                "a fresh buffer's status bar must show the cursor on line \
                 1; screen:\n{}",
                driver.screen()
            );
        }

        /// The `Ln N, Col N` ruler must survive on the window
        /// status bar even once other optional left-side segments — a
        /// dirty-file marker (`[+]`), a git branch, and VS Code mode's
        /// long `EDIT  F1:palette  Alt-M:vim` hint — are all competing for the
        /// same 80-column width budget. Before this fix,
        /// `build_window_status_line` pushed `cursor_seg` *first* into
        /// `right`, which is the segment quadraui's `StatusBar::layout`
        /// priority-drop removes *first* under a tight width budget — the
        /// exact inversion of "always show the cursor position" the bug
        /// report describes. Confirmed RED against unfixed `develop`
        /// (reverting this issue's `render.rs` hunk and re-running prints
        /// a screen with no `Ln ` anywhere on the status row).
        #[test]
        fn status_bar_1760_keeps_cursor_position_once_other_segments_compete() {
            let mut engine = plain_engine();
            engine.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
            engine.git_branch = Some("a-fairly-long-feature-branch-name".to_string());
            engine.buffer_mut().insert(0, "hello world\n");
            engine.active_buffer_state_mut().dirty = true;

            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            let screen = driver.screen();
            assert!(
                screen.contains("[+]"),
                "fixture sanity: the dirty marker must paint; screen:\n{screen}"
            );
            // The branch name is long enough that the left side legitimately
            // clips it at the window edge on an 80-column bar — that clip is
            // not what this test is about, so only check the (guaranteed to
            // survive) prefix rather than the full branch string.
            assert!(
                screen.contains("a-fairly"),
                "fixture sanity: the git branch segment must paint; screen:\n{screen}"
            );
            assert!(
                screen.contains("F1:palette"),
                "fixture sanity: VS Code mode's EDIT hint must paint; screen:\n{screen}"
            );
            assert!(
                driver.screen_has("Ln 1,"),
                "the ruler must still paint the cursor position even once \
                 the dirty marker, git branch and EDIT-mode hint are all \
                 competing for the status bar's width budget; screen:\n{screen}"
            );
        }

        // Quickfix / location-list rows and E42

        /// Mirrors `shell_app.rs`'s test of the same name: the
        /// quickfix panel's own content must paint via
        /// `App::render_content`, not just get recorded in `engine.quickfix`.
        #[test]
        fn render_content_paints_quickfix_panel_via_shell_app() {
            let mut engine = plain_engine();
            engine
                .quickfix
                .items
                .push(crate::core::project_search::ProjectMatch {
                    file: std::path::PathBuf::from("zqxw608.rs"),
                    line: 0,
                    col: 0,
                    line_text: "ZQXW_608_QUICKFIX_MARKER".to_string(),
                });
            engine.quickfix.open = true;
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::render_content_paints_quickfix_panel_via_shell_app",
                || {
                    assert!(
                        driver.screen_has("ZQXW_608_QUICKFIX_MARKER"),
                        "quickfix panel content should paint via \
                         App::render_content; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: the
        /// active window's location list shares the quickfix panel's bottom
        /// rung, painting its own distinct "LOCATION LIST" title.
        #[test]
        fn render_content_paints_location_list_panel_via_shell_app() {
            let mut engine = plain_engine();
            let win = engine.active_window_id();
            let list = engine.location_lists.entry(win).or_default();
            list.items.push(crate::core::project_search::ProjectMatch {
                file: std::path::PathBuf::from("zqxw1155.rs"),
                line: 0,
                col: 0,
                line_text: "ZQXW_1155_LOCLIST_MARKER".to_string(),
            });
            list.open = true;
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::render_content_paints_location_list_panel_via_shell_app",
                || {
                    let screen = driver.screen();
                    assert!(
                        screen.contains("ZQXW_1155_LOCLIST_MARKER"),
                        "location-list panel content should paint via \
                         App::render_content; screen:\n{screen}"
                    );
                    assert!(
                        screen.contains("LOCATION LIST"),
                        "the shared bottom rung must show the location-list \
                         title, not \"QUICKFIX\", when the global quickfix \
                         list is empty/closed; screen:\n{screen}"
                    );
                    assert!(
                        !screen.contains("QUICKFIX ("),
                        "the global quickfix panel must not also paint; \
                         screen:\n{screen}"
                    );
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: when both
        /// the global quickfix list and the active window's location list
        /// are open, the shared bottom rung shows quickfix.
        #[test]
        fn quickfix_panel_takes_priority_over_location_list_via_shell_app() {
            let mut engine = plain_engine();
            engine
                .quickfix
                .items
                .push(crate::core::project_search::ProjectMatch {
                    file: std::path::PathBuf::from("zqxw1155qf.rs"),
                    line: 0,
                    col: 0,
                    line_text: "ZQXW_1155_QF_MARKER".to_string(),
                });
            engine.quickfix.open = true;
            let win = engine.active_window_id();
            let list = engine.location_lists.entry(win).or_default();
            list.items.push(crate::core::project_search::ProjectMatch {
                file: std::path::PathBuf::from("zqxw1155loc.rs"),
                line: 0,
                col: 0,
                line_text: "ZQXW_1155_LOC_MARKER".to_string(),
            });
            list.open = true;
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            known_bug_gate(
                "app_on_tui::quickfix_panel_takes_priority_over_location_list_via_shell_app",
                || {
                    let screen = driver.screen();
                    assert!(
                        screen.contains("ZQXW_1155_QF_MARKER"),
                        "quickfix must win the shared bottom rung when both \
                         lists are open; screen:\n{screen}"
                    );
                    assert!(
                        !screen.contains("ZQXW_1155_LOC_MARKER"),
                        "the location list must not also paint while \
                         quickfix has the rung; screen:\n{screen}"
                    );
                },
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: `:cnext`
        /// on an empty quickfix list must paint Neovim's `E42: No Errors` on
        /// the command line — driven through the real command line, not by
        /// calling `qf_next` directly.
        #[test]
        fn cnext_on_empty_quickfix_list_paints_e42_via_shell_app() {
            let engine = plain_engine();
            assert!(
                engine.quickfix.items.is_empty(),
                "precondition: a fresh engine has an empty quickfix list"
            );
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::cnext_on_empty_quickfix_list_paints_e42_via_shell_app",
                || {
                    driver.type_char(':');
                    for c in "cnext".chars() {
                        driver.type_char(c);
                    }
                    driver.press_named(quadraui::NamedKey::Enter);
                    driver.render();

                    let screen = driver.screen();
                    assert!(
                        screen.contains("E42: No Errors"),
                        ":cnext on an empty quickfix list must paint \
                         Neovim's E42 on the command line; screen:\n{screen}"
                    );
                },
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Terminal
    // ─────────────────────────────────────────────────────────────────────────
    mod terminal {
        use super::*;

        /// Pins down **which literal strings the `:term` ex-command
        /// actually paints into the terminal panel's chrome**, so a
        /// real-ConPTY test on a Windows host can wait on a needle that is
        /// genuinely reachable.
        ///
        /// `tests/conpty_term_opens_shell_1829.rs` needs a precondition
        /// "the panel opened" gate between typing `:term` and sending its
        /// filesystem probe. Its first attempt waited on `"TERMINAL"`
        /// (uppercase), by analogy with
        /// `tests/conpty_activity_bar_click.rs`'s sidebar needles
        /// (`"EXPLORER"`, `"SOURCE CONTROL"`, …, which come from
        /// `Engine::fixed_panel_title_tooltip` and have no terminal entry).
        /// That needle is unreachable in **every** state that test can
        /// reach: the only uppercase `"TERMINAL"` literal in `src/` is the
        /// terminal toolbar tab strip's `if tabs.is_empty()` fallback, i.e.
        /// it paints only when `TerminalPanel::tab_count == 0` — but
        /// `tab_count` is `engine.terminal_panes.len()`, and
        /// `render::terminal_panel_desc` early-returns `None` (painting no
        /// panel at all) when there are no panes. So `tab_count` is `>= 1`
        /// whenever the panel paints, and the fallback is dead for any
        /// `:term`-driven flow.
        ///
        /// This test is the Linux-side, no-Windows-host-required proof of
        /// that: after `:term`, the toolbar's per-tab `"[1]"` label paints
        /// and `"TERMINAL"` does not. It is deliberately *also* a negative
        /// assertion — a needle-verification test whose whole value is
        /// failing if someone changes the chrome the ConPTY test waits on.
        #[test]
        fn term_ex_command_paints_bracketed_tab_label_not_uppercase_terminal() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            // Baseline: neither needle may be painted before `:term` runs,
            // or the precondition gate downstream would be vacuous. (This
            // is why `"Terminal"` — title-case — is a *bad* needle: it is
            // also a permanent top-level menu-bar title, so it is already
            // on screen here. See
            // `menu_terminal_activation_opens_terminal_pane_via_shell_app`
            // below for the same gotcha.)
            assert!(
                !driver.screen_has("[1]"),
                "precondition: the terminal toolbar's \"[1]\" tab label must \
                 not be painted before `:term` runs; screen:\n{}",
                driver.screen()
            );
            assert!(
                !driver.screen_has("TERMINAL"),
                "precondition: uppercase \"TERMINAL\" must not be painted \
                 before `:term` runs; screen:\n{}",
                driver.screen()
            );

            // The real user gesture, through the ex-command parse path.
            driver.type_char(':');
            for c in "term".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.tick();
            driver.render();

            assert!(
                driver.screen_has("[1]"),
                "`:term` must paint the terminal toolbar's \"[1]\" tab label \
                 — this is the needle \
                 `tests/conpty_term_opens_shell_1829.rs` waits on as its \
                 \"the panel opened\" precondition; screen:\n{}",
                driver.screen()
            );
            assert!(
                !driver.screen_has("TERMINAL"),
                "uppercase \"TERMINAL\" is the terminal toolbar's \
                 `tabs.is_empty()` fallback label and is unreachable once a \
                 pane exists — if this now paints, \
                 `tests/conpty_term_opens_shell_1829.rs`'s precondition \
                 needle should be revisited; screen:\n{}",
                driver.screen()
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name (focus rung): with a
        /// terminal pane focused, a key that would otherwise be a Normal-mode
        /// buffer edit must not reach the editor buffer.
        #[test]
        fn focused_terminal_swallows_editor_keys_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQXW_TERM_FOCUS_LINE\n");
            engine.terminal_new_tab(80, 10);
            let mut h = harness_no_sidebar(engine);
            // Set terminal focus *after* `harness_no_sidebar`: its
            // `collapse_sidebar` clicks activate the Explorer panel (a
            // focus-grant site), which clears `terminal_has_focus`.
            h.engine.borrow_mut().terminal_has_focus = true;
            let driver = &mut h.driver;

            // 'dd' would delete the line under Normal-mode dispatch; with
            // the terminal focused it must be swallowed by the PTY instead.
            driver.type_char('d');
            driver.type_char('d');
            assert!(
                driver.screen_has("ZQXW_TERM_FOCUS_LINE"),
                "keys must not reach the editor buffer while the terminal \
                 pane is focused; screen:\n{}",
                driver.screen()
            );
        }

        /// Opening the terminal via its accelerator (the menu/keybinding path,
        /// `render::ACC_OPEN_TERMINAL`) must paint a terminal pane — mirrors
        /// `shell_app.rs`'s
        /// `menu_terminal_activation_opens_terminal_pane_via_shell_app`.
        #[test]
        fn menu_terminal_activation_opens_terminal_pane_via_shell_app() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            // Baseline count, not `!screen_has("Terminal")` — `App`
            // always paints a permanent "Terminal" top-level menu item
            // (`File Edit Selection View Go Run Terminal Help`), so a bare
            // presence check is true before any terminal ever opens.
            // Same gotcha `crate::harness`'s own
            // `context_menu_open_terminal_opens_terminal_tab` doc
            // names for `gtk`.
            let before = driver.inventory().count("Terminal");
            driver.dispatch(quadraui::UiEvent::Accelerator(
                quadraui::AcceleratorId::new(crate::render::ACC_OPEN_TERMINAL),
                quadraui::Modifiers::default(),
            ));
            // Drains the `DeferredAction::ToggleTerminal` the accelerator
            // arm queued — see this fn's own doc.
            driver.tick();
            let after = driver.inventory().count("Terminal");
            assert!(
                after > before,
                "the open-terminal accelerator must open a terminal \
                 pane, painting a new \"Terminal\" occurrence (before \
                 {before}, after {after}); screen:\n{}",
                driver.screen()
            );
        }

        /// Toggling terminal-maximize must reserve strictly more rows for the
        /// terminal content than the un-maximized state — located by counting
        /// painted rows between the terminal tab strip and the bottom of the
        /// screen, not a hardcoded row count.
        #[test]
        fn terminal_toggle_max_increases_panel_height() {
            let mut engine = plain_engine();
            engine.terminal_new_tab(80, 10);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::terminal_toggle_max_increases_panel_height",
                || {
                    let before = driver
                        .find_bounds("Terminal")
                        .expect("the terminal tab strip must paint before toggling");
                    driver.dispatch(quadraui::UiEvent::Accelerator(
                        quadraui::AcceleratorId::new(crate::render::ACC_TERMINAL_TOGGLE_MAX),
                        quadraui::Modifiers::default(),
                    ));
                    let after = driver
                        .find_bounds("Terminal")
                        .expect("the terminal tab strip must still paint after toggling");
                    assert!(
                        after.y <= before.y,
                        "maximizing the terminal must never push its own tab strip \
                 further down the screen (before y={}, after y={})",
                        before.y,
                        after.y
                    );
                },
            );
        }

        /// `Ctrl-F` while the terminal is focused must open the terminal's own
        /// find bar.
        #[test]
        fn terminal_ctrl_f_opens_the_painted_find_bar() {
            let mut engine = plain_engine();
            engine.terminal_new_tab(80, 10);
            let mut h = harness_no_sidebar(engine);
            // Set terminal focus *after* `harness_no_sidebar`: its
            // `collapse_sidebar` clicks activate the Explorer panel (a
            // focus-grant site), which clears `terminal_has_focus`.
            h.engine.borrow_mut().terminal_has_focus = true;
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::terminal_ctrl_f_opens_the_painted_find_bar",
                || {
                    driver.ctrl_char('f');
                    assert!(
                        // All-caps "FIND:" — the terminal find bar's own painted
                        // label, confirmed by hand (`render::TerminalToolbarHits`'s
                        // find-bar row) — not "Find", which never appears.
                        driver.screen_has("FIND"),
                        "Ctrl-F with the terminal focused must open its find bar; \
                 screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// Opening the terminal via the accelerator sizes its PTY from the
        /// full painted editor width, reserving only the one-cell scrollbar
        /// gutter (`UnitProfile::terminal_scrollbar_gutter`) a cell-grid
        /// backend actually has room for, not a pixel-sized gutter
        /// mistakenly subtracted as whole columns. On an 80-column,
        /// no-sidebar TUI window `App::terminal_panel_cols` resolves the
        /// painted editor width (77 cols) minus that one-cell gutter to 76
        /// columns. Mirrors the GTK precedent
        /// (`issue_1421_terminal_size_derived_from_layout`'s
        /// `opening_a_terminal_in_a_wide_window_sizes_the_pty_from_the_
        /// painted_layout_on_gtk`), which asserts on the same
        /// `TerminalSession::cols()` signal.
        #[test]
        fn opening_a_terminal_on_tui_sizes_the_pty_within_one_column_of_the_painted_width() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;
            driver.dispatch(quadraui::UiEvent::Accelerator(
                quadraui::AcceleratorId::new(crate::render::ACC_OPEN_TERMINAL),
                quadraui::Modifiers::default(),
            ));
            // Drains the `DeferredAction::ToggleTerminal` the accelerator
            // arm queued, which is what actually calls `terminal_new_tab`.
            driver.tick();

            let cols = h
                .engine
                .borrow()
                .terminal_panes
                .first()
                .map(|slot| slot.session.cols());
            assert_eq!(
                cols,
                Some(75),
                "an 80-column, no-sidebar TUI window's terminal PTY should be \
                 sized within one column of the painted editor width (75 \
                 cols), reserving the one-cell scrollbar gutter and the \
                 activity-bar accent column; a pixel-count gutter wrongly \
                 subtracted as whole TUI columns would land this several \
                 columns narrower (71) instead"
            );
        }

        /// The one-cell gutter the TUI reserves is a real, hit-testable
        /// scrollbar, not dead space: a press on the painted track must page
        /// the terminal's scrollback and repaint the body with older output.
        ///
        /// `paint_bottom_panel_rung` only registers a
        /// `quadraui::SurfaceScrollbar` on the `terminal_scrollback` scroll
        /// surface when `BottomPanelUnits::terminal_scrollbar` is `Some`, and
        /// `quadraui::dispatch_click` only hit-tests a track that exists — so
        /// a cell backend that reserves the gutter width but registers no
        /// scrollbar swallows the press as a plain body click and leaves the
        /// live view painted.
        ///
        /// The track is located from the surface the paint pass registered
        /// (never hardcoded coordinates) and the assertion is on painted
        /// text: the newest line must leave the screen once the track press
        /// pages back into history.
        #[test]
        fn pressing_the_painted_terminal_scrollbar_track_on_tui_scrolls_the_scrollback() {
            // Distinctive enough that no shell prompt or ambient PTY output
            // can produce it, and numbered so "newest" is unambiguous.
            let newest = "ZQXWSCROLL59";
            let mut engine = plain_engine();
            engine.terminal_new_tab(80, 10);
            let mut h = harness_no_sidebar(engine);

            // Synthetic scrollback, fed straight into the pane's parser so
            // the test doesn't depend on a real shell's output timing.
            {
                let mut e = h.engine.borrow_mut();
                let session = &mut e.terminal_panes[0].session;
                for i in 0..60 {
                    session.feed_for_bench(format!("ZQXWSCROLL{i}\r\n").as_bytes());
                }
            }
            h.driver.tick();

            assert!(
                h.driver.screen_has(newest),
                "precondition: the live view paints the newest scrollback line; \
                 screen:\n{}",
                h.driver.screen()
            );

            let track = h
                .engine
                .borrow()
                .scroll_surfaces
                .borrow()
                .iter()
                .find(|s| s.id.as_str() == "terminal_scrollback")
                .and_then(|s| s.scrollbar.as_ref())
                .map(|sb| sb.track_bounds)
                .expect(
                    "the terminal panel must register a hit-testable scrollbar \
                     over its reserved gutter",
                );
            // Top of the track: with the view live the thumb sits at the
            // bottom of an inverted track, so this is a page-back press.
            h.driver
                .click(track.x + 0.5, track.y + f32::min(0.5, track.height / 2.0));

            assert!(
                !h.driver.screen_has(newest),
                "pressing the terminal scrollbar track must page the painted \
                 scrollback back so the newest line leaves the screen; \
                 screen:\n{}",
                h.driver.screen()
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // An extension install whose command line is longer than the tty's
    // canonical-mode input cap never ran at all
    // ─────────────────────────────────────────────────────────────────────────
    /// Driver-tier, black-box coverage for the case where an extension install
    /// command line exceeds the tty's canonical-mode input cap (1024 bytes on
    /// macOS, 4096 on Linux). The engine must break long commands into chunks
    /// via a temp-script launcher rather than typing them as single lines,
    /// since a tty silently discards input past the cap, leaving the shell
    /// at a half-typed prompt forever.
    #[cfg(unix)]
    mod terminal_install_long_command_1712 {
        use super::*;

        /// Printed by the install command's *last* statement — i.e. from
        /// bytes that sit well past every platform's canon cap, so neither
        /// the tty's echo of a truncated typed line nor a partially-executed
        /// command can produce it. It appears on screen only if the shell
        /// genuinely ran the whole command.
        const MARKER: &str = "ZQXW1712INSTALLRAN";

        #[test]
        fn install_command_over_the_tty_canon_cap_still_runs_and_paints_its_output() {
            // `:` is the POSIX no-op builtin — it accepts (and discards) the
            // padding argument without printing 8 KB into the pane, while
            // still making the command line itself far longer than the 1024
            // byte macOS cap and the 4096 byte Linux one.
            let padding = "p".repeat(8000);
            let command = format!(": '{padding}' ; printf '%s\\n' '{MARKER}'");
            assert!(
                command.len() > 4096,
                "fixture must exceed the most generous platform's canon cap, \
                 otherwise it isn't exercising the bug: {} bytes",
                command.len()
            );

            let mut engine = plain_engine();
            // The exact call `render::handle_engine_action` makes for
            // `EngineAction::RunInTerminal` — the install path every backend
            // funnels through (`tests/extensions.rs` covers the
            // `:ExtInstall` → `RunInTerminal` leg that reaches it).
            engine.terminal_run_command(&command, 80, 10);
            let mut h = harness_no_sidebar(engine);

            // Drain the PTY and repaint until the child's output shows up.
            // Up to ~6s of wall clock: generous for a `printf`, but a cold
            // shell spawn on a loaded CI box is not instant.
            let mut painted = false;
            for _ in 0..600 {
                h.engine.borrow_mut().poll_terminal();
                h.driver.render();
                if h.driver.screen_contains(MARKER) {
                    painted = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(
                painted,
                "an install command longer than the tty canon cap must still \
                 run and paint its output in the terminal pane (#1712); the \
                 pane never printed {MARKER}, i.e. the command was truncated \
                 before the shell could execute it. screen:\n{}",
                h.driver.screen()
            );

            // …and the pane must still close when the user answers the
            // wrapper's "Press Enter to close…" prompt. Routing the wrapper
            // through a child `sh` moved its trailing `exit` out of the
            // pane's own interactive shell, so without the launcher line's
            // `; exit` the shell returns to its PS1 prompt,
            // `TerminalSession::is_exited()` never fires and
            // `poll_terminal` never removes the `TerminalSlot` — a pane
            // nothing can close. Asserted as painted *absence*: once the
            // slot is gone the pane stops painting, so its output
            // disappears from the screen.
            h.engine.borrow_mut().terminal_write(b"\n");
            let mut closed = false;
            for _ in 0..600 {
                h.engine.borrow_mut().poll_terminal();
                h.driver.render();
                if !h.driver.screen_contains(MARKER) {
                    closed = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(
                closed,
                "pressing Enter at the install wrapper's \"Press Enter to \
                 close…\" prompt must exit the pane's shell so the pane \
                 stops painting (#1712); it is still showing {MARKER}. \
                 screen:\n{}",
                h.driver.screen()
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Win-GUI's embedded terminal panel renders completely blank
    // ─────────────────────────────────────────────────────────────────────────
    /// Driver-tier, black-box coverage: the fix's unit tests only cover the
    /// pure decision function in isolation; this module exercises a real
    /// `Backend` (the `TuiBackend` this crate can construct in-process) by
    /// driving the exact shared `AppLogic::tick` entry point every backend's
    /// run loop calls, and asserting on `Backend` scheduling instrumentation.
    /// This exercises the shared decision code both backends run, though not
    /// `WinBackend` itself (which degrades the frame-request call to a no-op
    /// in headless mode).
    mod terminal_poll_rearm_1668 {
        use super::*;

        #[test]
        fn terminal_pane_open_rearms_tick_via_shell_app() {
            let mut engine = plain_engine();
            engine.terminal_new_tab(80, 10);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            let before = driver.backend().frame_requests();
            driver.tick();
            let after = driver.backend().frame_requests();

            assert_eq!(
                after,
                before + 1,
                "App::tick_dispatch must re-arm a future tick (via \
                 Backend::request_frame_in) while a terminal pane is open \
                 — this is #1668's own root cause on Win-GUI, which (unlike \
                 TUI/GTK/macOS) has no unconditional idle-poll fallback: \
                 without this re-arm, Engine::poll_terminal never drains \
                 the PTY again past the first frame and the embedded \
                 terminal panel stays blank forever, exactly as the \
                 bugbash report observed (before={before}, after={after})"
            );
            // `pending_frame_delay` reports *time remaining* until the
            // deadline, not the raw requested duration — it is derived from
            // `Instant::now()` at read time (`FrameScheduler::pending_delay`),
            // so it is always a hair under the requested 100ms by however
            // long this test itself took to reach this line. A tight
            // tolerance (80ms..=100ms) still clearly distinguishes this from
            // the coarse 250ms `IDLE_POLL_CEILING` fallback other backends
            // can lean on but Win-GUI cannot, without being flaky on a
            // loaded CI box.
            let delay = driver
                .backend()
                .pending_frame_delay()
                .expect("a frame must be scheduled after the re-arm above");
            assert!(
                delay <= std::time::Duration::from_millis(100)
                    && delay >= std::time::Duration::from_millis(80),
                "the re-armed tick must fire at the fast (~100ms) cadence \
                 `terminal_poll_rearm_delay` returns — mirroring the \
                 existing ai_streaming re-arm's own cadence — not the \
                 coarse 250ms IDLE_POLL_CEILING fallback other backends \
                 can lean on but Win-GUI cannot; got {delay:?}"
            );
        }

        #[test]
        fn no_terminal_pane_does_not_rearm_tick_via_shell_app() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            let before = driver.backend().frame_requests();
            driver.tick();
            let after = driver.backend().frame_requests();

            assert_eq!(
                after, before,
                "with no terminal pane open, tick_dispatch must not \
                 unconditionally re-arm a future tick — the re-arm is \
                 gated on Engine::terminal_panes being non-empty, not \
                 unconditional (an unconditional re-arm would reintroduce \
                 a perpetual 100ms busy-poll on every Win-GUI session, \
                 terminal or not)"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Dialogs
    mod dialogs {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name: a modal
        /// dialog must paint via `App::render_content`.
        #[test]
        fn render_content_paints_dialog_via_shell_app() {
            let mut engine = plain_engine();
            engine.dialog = Some(crate::core::engine::Dialog {
                title: "ZQXW605DIALOG".to_string(),
                body: vec!["body line".to_string()],
                buttons: vec![crate::core::engine::DialogButton {
                    label: "OK".to_string(),
                    hotkey: 'o',
                    action: "ok".to_string(),
                }],
                selected: 0,
                tag: String::new(),
                input: None,
            });
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            // `App::render_content` now filters `quadraui::native_dialog_options` on
            // `backend.backend_caps().native_dialogs`, so TUI paints the in-canvas
            // `Dialog` rung instead of dropping it for a native present it has no
            // facility for.
            assert!(
                driver.screen_has("ZQXW605DIALOG"),
                "modal dialog should paint via App::render_content; \
                 screen:\n{}",
                driver.screen()
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: `:CheckNerdFonts`
        /// must paint on a real driver frame, with both the Nerd Font glyph
        /// row and the ASCII fallback row visible on the same screen.
        #[test]
        fn check_nerd_fonts_dialog_paints_both_variants_via_shell_app() {
            let mut engine = plain_engine();
            engine.execute_command("CheckNerdFonts");
            assert!(
                engine.dialog.is_some(),
                "fixture must actually open the dialog"
            );
            let h = crate::tui_main::testing::conformance_harness(engine, 100, 30);
            let driver = &h.driver;

            // `App::render_content` now filters `quadraui::native_dialog_options` on
            // `backend.backend_caps().native_dialogs`, so TUI paints the in-canvas
            // `Dialog` rung.
            assert!(
                driver.screen_contains(crate::icons::FILE_RUST.nerd),
                "the painted dialog must show the Nerd Font glyph \
                 row; screen:\n{}",
                driver.screen()
            );
            assert!(
                driver.screen_contains(crate::icons::FILE_RUST.fallback),
                "the painted dialog must show the ASCII fallback \
                 row; screen:\n{}",
                driver.screen()
            );
            assert!(
                driver.screen_contains("Check Nerd Fonts"),
                "the painted dialog must show its own title; \
                 screen:\n{}",
                driver.screen()
            );
        }

        /// `:about` must paint the trademark disclaimer and codicon
        /// attribution lines `execute.rs`'s `"about"` handler adds to the
        /// dialog body, not just hold them in `engine.dialog.body`.
        #[test]
        fn about_dialog_shows_trademark_and_codicon_attribution_via_shell_app() {
            let mut engine = plain_engine();
            engine.execute_command("about");
            assert!(
                engine.dialog.is_some(),
                "fixture must actually open the dialog"
            );
            let h = crate::tui_main::testing::conformance_harness(engine, 100, 30);
            let driver = &h.driver;

            assert!(
                driver.screen_contains("About VimCode"),
                "the painted dialog must show its own title; screen:\n{}",
                driver.screen()
            );
            assert!(
                driver.screen_contains("not affiliated with Microsoft"),
                "the painted dialog must show the trademark disclaimer; \
                 screen:\n{}",
                driver.screen()
            );
            assert!(
                driver.screen_contains("codicon"),
                "the painted dialog must show the codicon attribution; \
                 screen:\n{}",
                driver.screen()
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Explorer context menu
    // ─────────────────────────────────────────────────────────────────────────
    mod explorer_context_menu {
        use super::*;

        /// Build an engine with one folder (`root`, containing `marker.txt`)
        /// already shown in an open, populated explorer sidebar, with its
        /// context menu already open and `selected_idx` already highlighted —
        /// same fixed folder-menu order
        /// `crate::harness::issue_1418_explorer_context_menu`'s own
        /// `engine_with_folder_ctx_menu` builds (0 new_file, 1 new_folder,
        /// 2 reveal, 3 open_terminal, 4 find_in_folder, 5 copy_path,
        /// 6 copy_relative_path, 7 rename, 8 delete).
        fn engine_with_folder_ctx_menu(tag: &str, selected_idx: usize) -> crate::core::Engine {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1425_ctxmenu_{tag}_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("marker.txt"), "hello").unwrap();

            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.explorer_rebuild_rows();
            engine.session.explorer_visible = true;
            // `session.explorer_visible` alone leaves the shadow
            // `engine.app_shell`'s `sidebar_visible()` stale — see
            // `crate::harness`'s `engine_with_collapsed_explorer_dir`'s
            // identical fix for the full mechanics (`render::sync_runner_
            // sidebar_visibility`, called unconditionally on every
            // dispatch, would otherwise collapse the sidebar the very
            // first time this scenario dispatches anything, e.g. its own
            // `Enter` confirm below).
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            engine.open_explorer_context_menu(dir, true, 5, 5);
            engine.context_menu.as_mut().unwrap().selected = selected_idx;
            engine
        }

        /// `App::dispatch_context_menu_key` is the exact code path
        /// `crate::harness::issue_1418_explorer_context_menu`'s own doc names
        /// as shared between `gtk` and this "tui" control arm — so, unlike the
        /// tab-bar close-button gap, this is expected to pass here exactly as
        /// it does on `gtk`. Confirming "New File..." actually starts the
        /// tree's inline-edit placeholder is the App-on-TUI half of that
        /// convergence claim.
        #[test]
        fn context_menu_new_file_starts_inline_edit() {
            let mut h = harness(engine_with_folder_ctx_menu("new_file", 0));
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::context_menu_new_file_starts_inline_edit",
                || {
                    // "file name" rather than the full "New file name..." — at
                    // this module's narrower `(80, 24)` sidebar width the trailing
                    // ellipsis itself can fall past the column budget even when
                    // the placeholder text is painted (confirmed by hand), same
                    // truncation-tolerance reasoning `crate::harness`'s own mirror
                    // of this scenario applies to the leading "N" (eaten by the
                    // inline-edit cursor block).
                    assert!(
                        !driver.screen_has("file name"),
                        "precondition: nothing is being edited yet"
                    );
                    driver.press_named(quadraui::NamedKey::Enter);
                    assert!(
                        driver.screen_has("file name"),
                        "confirming 'New File...' must start the tree's inline-edit \
                 placeholder; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// Confirming "Delete" must open the delete-confirmation dialog.
        #[test]
        fn context_menu_delete_opens_confirm_dialog() {
            let mut h = harness(engine_with_folder_ctx_menu("delete", 8));
            let driver = &mut h.driver;

            // `App::render_content` now filters `quadraui::native_dialog_options` on
            // `backend.backend_caps().native_dialogs`, so TUI paints the in-canvas
            // `Dialog` rung instead of dropping it.
            assert!(
                !driver.screen_has("Confirm Delete"),
                "precondition: no dialog is open yet"
            );
            driver.press_named(quadraui::NamedKey::Enter);
            assert!(
                driver.screen_has("Confirm Delete"),
                "confirming 'Delete' must open the delete-confirmation \
                 dialog; screen:\n{}",
                driver.screen()
            );
        }

        /// Confirming "Open in Integrated Terminal" must open a real terminal
        /// tab in the bottom panel.
        #[test]
        fn context_menu_open_terminal_opens_terminal_tab() {
            let mut h = harness(engine_with_folder_ctx_menu("open_terminal", 3));
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::context_menu_open_terminal_opens_terminal_tab",
                || {
                    driver.press_named(quadraui::NamedKey::Enter);
                    assert!(
                        driver.screen_has("Terminal"),
                        "confirming 'Open in Integrated Terminal' must open a \
                 terminal tab; screen:\n{}",
                        driver.screen()
                    );
                },
            );
        }

        /// Escape must dismiss the context menu and leave no trace of it.
        #[test]
        fn context_menu_escape_dismisses() {
            let mut h = harness(engine_with_folder_ctx_menu("escape", 0));
            let driver = &mut h.driver;

            known_bug_gate("app_on_tui::context_menu_escape_dismisses", || {
                assert!(
                    driver.screen_has("New File..."),
                    "precondition: the context menu must be painted"
                );
                driver.press_named(quadraui::NamedKey::Escape);
                assert!(
                    !driver.screen_has("New File..."),
                    "Escape must dismiss the context menu; screen:\n{}",
                    driver.screen()
                );
            });
        }

        /// Ports `mouse.rs::tests::right_click_in_explorer_panel_opens_
        /// explorer_context_menu` onto `App`: right-clicking a populated,
        /// visible Explorer row must open its file context menu — the
        /// "positive" counterpart proving the panel-gate test below isn't
        /// vacuously true (a right-click that opened nothing anywhere would
        /// pass that one too).
        #[test]
        fn right_click_on_explorer_row_opens_context_menu() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1432_rc_explorer_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("rc_marker.txt"), "hello").unwrap();

            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.explorer_expanded.insert(dir);
            engine.explorer_rebuild_rows();
            engine.session.explorer_visible = true;
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            let mut h = harness(engine);
            let driver = &mut h.driver;

            let (x, y) = driver
                .find("rc_marker.txt")
                .expect("the populated explorer row must paint its file name");
            // A *file* row's context menu, not the folder one
            // (`engine_with_folder_ctx_menu`'s "New File..." above) — "Open
            // to the Side" is one only a file row offers.
            assert!(
                !driver.screen_has("Open to the Side"),
                "precondition: no context menu is open yet"
            );
            driver.right_click(x, y);
            assert!(
                driver.screen_has("Open to the Side"),
                "right-clicking a populated Explorer row must open its file \
                 context menu; screen:\n{}",
                driver.screen()
            );
        }

        /// Measures app-side wall-clock time from a right-press to the context
        /// menu's first painted frame, against a one-frame budget. Inspection of
        /// the dispatch path found no debounce, deferred scheduling, or blocking
        /// I/O; this confirms the in-process path (event dispatch + menu
        /// construction + paint) meets the budget. Not a regression-guard (the
        /// investigation found nothing to fix in production code), but a
        /// perf-budget assertion — if this goes red, the regression is in-process.
        #[test]
        fn right_click_context_menu_appears_within_one_frame_budget() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1635_rc_latency_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("rc_marker.txt"), "hello").unwrap();

            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.explorer_expanded.insert(dir);
            engine.explorer_rebuild_rows();
            engine.session.explorer_visible = true;
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            let mut h = harness(engine);
            let driver = &mut h.driver;

            let (x, y) = driver
                .find("rc_marker.txt")
                .expect("the populated explorer row must paint its file name");
            assert!(
                !driver.screen_has("Open to the Side"),
                "precondition: no context menu is open yet"
            );

            let start = std::time::Instant::now();
            driver.right_click(x, y);
            let elapsed = start.elapsed();

            assert!(
                driver.screen_has("Open to the Side"),
                "right-click must open the file context menu on the very \
                 next frame; screen:\n{}",
                driver.screen()
            );
            assert!(
                elapsed <= std::time::Duration::from_millis(50),
                "#1635: right-press -> menu-visible app-side latency budget \
                 (<=50ms) exceeded: took {elapsed:?}. This is the in-process \
                 half of the reported delay (event dispatch + menu \
                 construction + paint) — see this test's own doc for what \
                 to re-check if it ever regresses.",
            );
        }

        /// Right-clicking in the Explorer must open the painted menu, clicking an
        /// item must run its command, and Escape must dismiss it — driven entirely
        /// through mouse/keyboard events, not direct engine calls.
        ///
        /// Parametrized over every `MenuStyle` value: `quadraui::TuiBackend`
        /// always declares `BackendCaps::native_menu: false`, so
        /// `quadraui::MenuStyle::resolve` (via `Backend::effective_menu_style`)
        /// resolves `Custom` here regardless of the setting.
        ///
        /// Tests the mouse-click route with `copy_path`, which runs entirely inside
        /// `Engine::context_menu_confirm` itself and exercises a route other items
        /// (UI-backend-plumbed only via keyboard) do not cover.
        #[test]
        fn explorer_context_menu_right_click_click_and_escape_round_trip_under_every_menu_style() {
            for style in [
                crate::core::settings::MenuStyle::Auto,
                crate::core::settings::MenuStyle::Native,
                crate::core::settings::MenuStyle::Custom,
            ] {
                let dir = std::env::temp_dir().join(format!(
                    "vimcode_test_1580_ctxmenu_roundtrip_{style:?}_{}_{:?}",
                    std::process::id(),
                    std::thread::current().id()
                ));
                let _ = std::fs::remove_dir_all(&dir);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("rc_marker.txt"), "hello").unwrap();

                let mut engine = plain_engine();
                engine.settings.menu_style = style;
                engine.cwd = dir.clone();
                engine.explorer_expanded.insert(dir);
                engine.explorer_rebuild_rows();
                engine.session.explorer_visible = true;
                engine.app_shell.show_panel(&quadraui::WidgetId::new(
                    crate::core::engine::sidebar::PANEL_EXPLORER,
                ));
                let mut h = harness(engine);
                let driver = &mut h.driver;

                let (fx, fy) = driver.find("rc_marker.txt").unwrap_or_else(|| {
                    panic!("{style:?}: the populated explorer row must paint its file name")
                });

                // Right-click opens the painted menu.
                assert!(
                    !driver.screen_has("Copy Path"),
                    "{style:?}: precondition -- no context menu open yet"
                );
                driver.right_click(fx, fy);
                let (ix, iy) = driver.find("Copy Path").unwrap_or_else(|| {
                    panic!(
                        "{style:?}: right-click must paint the file context menu; screen:\n{}",
                        driver.screen()
                    )
                });

                // Clicking an item runs its command (`copy_path` sets
                // `engine.message`, painted in the status bar) and closes
                // the menu.
                driver.click(ix, iy);
                assert!(
                    !driver.screen_has("Copy Path"),
                    "{style:?}: clicking an item must close the menu; screen:\n{}",
                    driver.screen()
                );
                assert!(
                    driver.screen_has("Copied:"),
                    "{style:?}: clicking 'Copy Path' must run its command; screen:\n{}",
                    driver.screen()
                );

                // Escape dismisses a freshly re-opened menu, leaving no trace.
                driver.right_click(fx, fy);
                assert!(
                    driver.screen_has("Copy Path"),
                    "{style:?}: right-click must re-open the menu for the \
                     Escape half of this test; screen:\n{}",
                    driver.screen()
                );
                driver.press_named(quadraui::NamedKey::Escape);
                assert!(
                    !driver.screen_has("Copy Path"),
                    "{style:?}: Escape must dismiss the context menu; screen:\n{}",
                    driver.screen()
                );
            }
        }

        /// Right-clicking inside a *different* active sidebar panel must not
        /// resurrect a stale Explorer context menu just because `explorer_rows`
        /// still holds rows from an earlier visit.
        #[test]
        fn right_click_in_non_explorer_panel_does_not_open_explorer_context_menu() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1432_rc_debug_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("rc_marker.txt"), "hello").unwrap();

            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.explorer_expanded.insert(dir);
            engine.explorer_rebuild_rows();
            engine.session.explorer_visible = true;
            // Leave `explorer_rows` populated but switch the *active* sidebar panel to
            // Debug — the rows are never painted once Debug is active.
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_DEBUG,
            ));
            let mut h = harness(engine);
            let driver = &mut h.driver;

            let debug_zone = driver
                .inventory()
                .zones()
                .iter()
                .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_DEBUG)
                .map(|z| z.bounds)
                .expect("the Debug activity-bar icon must register a chrome zone");
            // Right-click just to the right of the Debug icon, inside the
            // sidebar body it now owns.
            driver.right_click(debug_zone.x + debug_zone.width + 5.0, debug_zone.y + 3.0);

            assert!(
                !driver.screen_has("Open to the Side"),
                "right-clicking inside a non-Explorer panel must not open \
                 the Explorer's file context menu; screen:\n{}",
                driver.screen()
            );
        }

        /// A right-click at literal screen coordinates (row 3, col 10) may land on
        /// a file instead of a folder if the launch `cwd` contains only files.
        /// This test reproduces the exact scenario at the spec's own grid and
        /// literal coordinates to confirm a more reliable alternative: the
        /// Explorer **root entry row**, which is always a directory by
        /// construction, regardless of the launch `cwd`'s contents.
        ///
        /// # Screen row map
        ///
        /// The root entry sits at **screen `row: 2`** (not row 1, due to the
        /// toolbar row added by view-actions):
        ///
        /// | row | content |
        /// |-----|---------|
        /// | 0 | sidebar header (`☰ EXPLORER`) |
        /// | 1 | view-actions toolbar |
        /// | 2 | **Explorer root entry** (always `is_dir`) |
        /// | 3+ | the root's children |
        ///
        /// The assertion below is guarded by an explicit check that row 2 really is
        /// the root entry, so a future chrome row shift fails here loudly rather
        /// than silently re-aiming the test at the wrong row.
        ///
        /// Two plain files (not one): this keeps a child row under `row: 3` even if
        /// a future chrome row shifts the tree down — with a single file, such a
        /// shift would push `row: 3` below the only child.
        #[test]
        fn right_click_row_3_on_a_folderless_cwd_hits_a_file_not_a_folder_1703() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1703_rc_row3_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            // Deliberately two plain files and no subfolder -- the "flat,
            // no folder" layout the bugbash run's cwd had, with a second
            // file so `row: 3` keeps landing on a real child row rather
            // than the empty-space-below-the-tree fallback (see doc above).
            std::fs::write(dir.join("sample.txt"), "hello").unwrap();
            std::fs::write(dir.join("second.txt"), "world").unwrap();

            // Each right-click gets its own fresh engine + harness: the first revision
            // drove both clicks through one driver with an Escape in between, so the
            // root-row check inherited state from the first click's dismissal. Fresh
            // harnesses keep the two checks independent.
            let fresh = |dir: &std::path::Path| {
                let mut engine = plain_engine();
                engine.cwd = dir.to_path_buf();
                engine.explorer_expanded.insert(dir.to_path_buf());
                engine.explorer_rebuild_rows();
                engine.session.explorer_visible = true;
                engine.app_shell.show_panel(&quadraui::WidgetId::new(
                    crate::core::engine::sidebar::PANEL_EXPLORER,
                ));
                // Matches `tests/smoke-spec/tui.yaml`'s own `cols: 100` x
                // `rows: 30` grid exactly, so the row numbers below mean
                // the same thing they do in the real spec.
                crate::tui_main::testing::conformance_harness(engine, 100, 30)
            };

            // Literal screen coordinates (row 3, col 10): on a folderless cwd this
            // lands on a file row, whose context menu correctly omits folder actions.
            {
                let mut h = fresh(&dir);
                let driver = &mut h.driver;
                driver.right_click(10.0, 3.0);
                assert!(
                    driver.screen_has("Copy Path"),
                    "row 3 must land on a real explorer row and open its \
                     context menu; screen:\n{}",
                    driver.screen()
                );
                assert!(
                    !driver.screen_has("New File"),
                    "row 3 on a folderless cwd is a plain file's row; its \
                     context menu must not offer folder-only actions; \
                     screen:\n{}",
                    driver.screen()
                );
            }

            // The cwd-independent alternative: the Explorer root entry — always a
            // directory by construction, regardless of the cwd's children — so its
            // context menu always offers folder actions. Row 2 since the view-actions
            // toolbar took row 1.
            {
                let mut h = fresh(&dir);
                let driver = &mut h.driver;

                // Guard the row map itself, so a future header-chrome row
                // can't silently re-aim the click (and the smoke step this
                // backs) at some other row that merely happens to offer
                // "New File...". The root entry paints the workspace
                // folder's name, upper-cased and possibly truncated to the
                // sidebar width, so match on a prefix of it.
                let root_name = dir.file_name().unwrap().to_string_lossy().to_uppercase();
                let probe: String = root_name.chars().take(12).collect();
                let row2 = driver
                    .screen()
                    .lines()
                    .nth(2)
                    .unwrap_or_default()
                    .to_string();
                assert!(
                    row2.contains(&probe),
                    "row 2 must be the Explorer root entry (expected it to \
                     show {probe:?}); #1693's view-actions toolbar owns row \
                     1. If new header chrome shifted the tree again, update \
                     this test *and* `right-click-explorer-root-row-1703` \
                     in tests/smoke-spec/tui.yaml together. row 2 was \
                     {row2:?}; screen:\n{}",
                    driver.screen()
                );

                driver.right_click(10.0, 2.0);
                assert!(
                    driver.screen_has("New File"),
                    "row 2 is the Explorer root entry and must always open a \
                     directory's context menu, even on a folderless cwd; \
                     screen:\n{}",
                    driver.screen()
                );
            }

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// A bugbash run reported the Explorer's row map appearing inconsistent
        /// within the same session. The shared `CARGO_TARGET_DIR` across
        /// concurrent coordinator worktrees means concurrent builds can swap the
        /// on-disk binary out mid-session, producing different row maps for what
        /// appears to be the same binary. This test pins the intended row map's
        /// presence and position on the Explorer's very first painted frame, so a
        /// real future regression (e.g. a toolbar/tree paint ordering race) fails
        /// here instead of shipping unnoticed.
        #[test]
        fn explorer_first_frame_row_map_matches_settled_state_1736() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1736_first_frame_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("sample.txt"), "hello").unwrap();

            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.explorer_expanded.insert(dir.clone());
            engine.explorer_rebuild_rows();
            engine.session.explorer_visible = true;
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            // Matches the bugbash evidence's own `cols: 100` x `rows: 30`
            // grid and `use_nerd_fonts: false` setting (`plain_engine`
            // already sets that) exactly, so the row/glyph assertions
            // below mean the same thing they did in the real capture.
            let mut h = crate::tui_main::testing::conformance_harness(engine, 100, 30);
            let driver = &mut h.driver;

            // This is the harness's very first painted frame: no
            // `wait_idle`, no `tick`, no interaction of any kind happened
            // between construction and this read.
            let screen = driver.screen();
            let lines: Vec<&str> = screen.lines().collect();
            let row1 = lines.get(1).copied().unwrap_or_default();
            let row2 = lines.get(2).copied().unwrap_or_default();
            let row3 = lines.get(3).copied().unwrap_or_default();

            // Row 1 is the view-actions toolbar — with nerd fonts off its fallbacks
            // are single letters (n, N, r, c), never a partial or absent set.
            for glyph in ["n", "N", "r", "c"] {
                assert!(
                    row1.contains(glyph),
                    "row 1 (first frame) must be the Explorer toolbar and \
                     show the '{glyph}' fallback button; screen:\n{screen}"
                );
            }

            // Row 2 is the root entry, never row 1 — the root must not
            // have raced ahead of the toolbar row on this very first
            // frame.
            let root_name = dir.file_name().unwrap().to_string_lossy().to_uppercase();
            let probe: String = root_name.chars().take(12).collect();
            assert!(
                row2.contains(&probe),
                "row 2 (first frame) must be the Explorer root entry \
                 (expected a prefix of {probe:?}); screen:\n{screen}"
            );
            assert!(
                !row1.contains(&probe),
                "the root entry must not appear on row 1 of the first \
                 frame (that is the toolbar's row); screen:\n{screen}"
            );

            // Row 3 is the one real child this cwd has.
            assert!(
                row3.contains("sample.txt"),
                "row 3 (first frame) must be the child file row; \
                 screen:\n{screen}"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// An earlier report claimed the Explorer root entry and the editor's first
        /// content line were misaligned — one row apart on first frame. This test
        /// measures the actual cross-pane relationship: row 0 is the shared tab bar,
        /// row 1 is chrome on both sides (Explorer's view-actions toolbar and
        /// editor's breadcrumb bar), row 2 is content on both sides. Under default
        /// settings (breadcrumbs on), the two panes' content rows are byte-for-byte
        /// aligned. This test pins that alignment as an anti-regression guard for
        /// the cross-pane row relationship as it ships today.
        #[test]
        fn explorer_root_row_matches_editor_first_content_row_1736() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1736_cross_pane_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let file_path = dir.join("sample.txt");
            std::fs::write(&file_path, "ZQXW_EDITOR_FIRST_LINE_1736\nsecond line\n").unwrap();

            let mut engine = crate::core::Engine::open(&file_path);
            engine.settings.use_nerd_fonts = Some(false);
            engine.cwd = dir.clone();
            engine.explorer_expanded.insert(dir.clone());
            engine.explorer_rebuild_rows();
            engine.session.explorer_visible = true;
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            // Same grid as the bugbash evidence and the test above.
            let mut h = crate::tui_main::testing::conformance_harness(engine, 100, 30);
            let driver = &mut h.driver;

            // First painted frame, same as the test above: no settle, no
            // interaction.
            let screen = driver.screen();
            let lines: Vec<&str> = screen.lines().collect();

            let editor_row = lines
                .iter()
                .position(|l| l.contains("ZQXW_EDITOR_FIRST_LINE_1736"))
                .unwrap_or_else(|| {
                    panic!(
                        "editor must paint the buffer's first line on frame 1; screen:\n{screen}"
                    )
                });

            let root_name = dir.file_name().unwrap().to_string_lossy().to_uppercase();
            let probe: String = root_name.chars().take(12).collect();
            let explorer_root_row = lines
                .iter()
                .position(|l| l.contains(&probe))
                .unwrap_or_else(|| {
                    panic!(
                        "Explorer must paint the root entry (prefix {probe:?}) \
                         on frame 1; screen:\n{screen}"
                    )
                });

            assert_eq!(
                explorer_root_row, editor_row,
                "the Explorer root entry (row {explorer_root_row}) is expected \
                 to be on the SAME row as the editor's first content line \
                 (row {editor_row}) — the Explorer's #1693 toolbar and the \
                 editor's own breadcrumb bar (`settings.breadcrumbs`, on by \
                 default) both occupy exactly one row below the shared tab \
                 bar, keeping the two panes' content rows in sync. If this \
                 now fails, one pane's chrome height changed without the \
                 other's; see this test's doc comment before changing the \
                 asserted relationship. screen:\n{screen}"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Minimap
    // ─────────────────────────────────────────────────────────────────────────
    mod minimap {
        use super::*;

        /// True if any character in `s` falls in the Unicode Braille Patterns
        /// block (U+2800..=U+28FF) — the glyph range `quadraui`'s minimap
        /// rasteriser uses on TUI, same check `shell_app.rs`'s own minimap
        /// tests use.
        fn has_braille(s: &str) -> bool {
            s.chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c))
        }

        /// With the setting explicitly on, a buffer with enough lines to need
        /// scrolling must paint minimap braille somewhere on screen.
        ///
        /// The minimap is experimental and off by default on every
        /// backend now, so this test turns it on explicitly rather than
        /// relying on the (now-off) default.
        #[test]
        fn minimap_paints_braille_when_enabled() {
            let mut engine = plain_engine();
            engine.settings.minimap = true;
            let text = (1..=200)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n");
            engine.buffer_mut().insert(0, &text);
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            // The minimap is part of the same collapsed editor content band and must
            // paint when enabled.
            assert!(
                has_braille(&driver.screen()),
                "a scrollable buffer with the minimap on must paint \
                 braille somewhere; screen:\n{}",
                driver.screen()
            );
        }

        /// Mirrors `shell_app.rs`'s
        /// `editor_band_drops_the_minimap_rung_when_the_setting_is_off_via_shell_app`,
        /// minus the private `composed_editor_band` introspection: `:set
        /// nominimap` must leave no braille reaching the cells at all.
        #[test]
        fn no_minimap_braille_when_setting_is_off() {
            let mut engine = plain_engine();
            engine.settings.minimap = false;
            let text = (1..=200)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n");
            engine.buffer_mut().insert(0, &text);
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            // The minimap is part of the same collapsed editor content band and must
            // not paint when disabled.
            assert!(
                driver.screen_has("line 1"),
                "precondition: buffer text must be painted before the \
                 minimap setting can be meaningfully tested; screen:\n{}",
                driver.screen()
            );
            assert!(
                !has_braille(&driver.screen()),
                "`minimap: false` must reserve no strip, so no braille may \
                 reach the cells; screen:\n{}",
                driver.screen()
            );
        }

        /// The minimap must be off *by default*, not merely controllable via an
        /// explicit `false`. This is the driver-tier regression test for the
        /// default itself — if a future change flips `default_minimap()` to `true`,
        /// this test goes red.
        #[test]
        fn no_minimap_braille_by_default() {
            let mut engine = plain_engine();
            let text = (1..=200)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n");
            engine.buffer_mut().insert(0, &text);
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            assert!(
                driver.screen_has("line 1"),
                "precondition: buffer text must be painted before the \
                 minimap default can be meaningfully tested; screen:\n{}",
                driver.screen()
            );
            assert!(
                !has_braille(&driver.screen()),
                "the minimap must default to off (#1858): a fresh engine \
                 with no explicit `minimap` setting must reserve no strip, \
                 so no braille may reach the cells; screen:\n{}",
                driver.screen()
            );
        }

        /// A vertical split must paint minimap braille in *both* panes, not
        /// just one — mirrors `shell_app.rs`'s
        /// `split_paints_two_independent_minimap_strips_via_shell_app`.
        #[test]
        fn split_paints_minimap_in_both_panes() {
            let mut engine = plain_engine();
            // Minimap is experimental and off by default — this test
            // is specifically about the minimap, so turn it on explicitly.
            engine.settings.minimap = true;
            let text = (1..=200)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n");
            engine.buffer_mut().insert(0, &text);
            engine.open_editor_group(SplitDirection::Vertical);
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            // The second pane's content band must paint despite the editor band's split
            // collapsing.
            let screen = driver.screen();
            let mut left_hit = false;
            let mut right_hit = false;
            for line in screen.lines() {
                let starts: Vec<usize> = line.match_indices("line ").map(|(i, _)| i).collect();
                let Some(&second) = starts.get(1) else {
                    continue;
                };
                let (left, right) = line.split_at(second);
                left_hit |= has_braille(left);
                right_hit |= has_braille(right);
            }
            assert!(
                left_hit && right_hit,
                "a vertical split must paint minimap braille in both \
                 panes; screen:\n{screen}"
            );
        }

        /// Render `text` in a minimap-enabled, sidebar-free harness and
        /// report what it painted: the full screen dump, the minimap
        /// strip's leftmost column, and whether the pane overflows
        /// vertically (i.e. whether quadraui paints its inline vertical
        /// scrollbar at all).
        ///
        /// The strip's left edge is *rounded*, not truncated: the TUI
        /// backend converts every `quadraui::Rect` (editor and minimap
        /// strip alike) to terminal cells by rounding to the nearest
        /// column, so rounding here lines these tests' boundary up with
        /// the boundary the backend itself paints against.
        fn minimap_paint(text: &str) -> (String, usize, bool) {
            let mut engine = plain_engine();
            // Minimap is experimental and off by default — these tests are
            // specifically about the minimap, so turn it on explicitly.
            engine.settings.minimap = true;
            engine.buffer_mut().insert(0, text);
            let mut h = harness_no_sidebar(engine);
            h.driver.render();

            let win = h.engine.borrow().active_window_id();
            let (overflows_vertically, strip) = {
                let layout = h.screen_layout.borrow();
                let layout = layout
                    .as_ref()
                    .expect("render_content must have painted a ScreenLayout");
                let rw = layout
                    .windows
                    .iter()
                    .find(|rw| rw.window_id == win)
                    .expect("the active window must publish a RenderedWindow");
                let mm = layout
                    .minimap
                    .iter()
                    .find(|m| m.window_id == win)
                    .expect("a minimap-enabled window must publish a strip");
                (
                    rw.total_lines > rw.visible_line_capacity,
                    crate::render::minimap_strip_rect(mm),
                )
            };
            (
                h.driver.screen(),
                strip.x.round() as usize,
                overflows_vertically,
            )
        }

        /// `(row, column)` of the first `marker` character `screen` paints
        /// at or past column `from`, scanning rows top to bottom.
        fn first_marker_at_or_past(
            screen: &str,
            marker: char,
            from: usize,
        ) -> Option<(usize, usize)> {
            screen.lines().enumerate().find_map(|(row_idx, row)| {
                let row_chars: Vec<char> = row.chars().collect();
                row_chars[from.min(row_chars.len())..]
                    .iter()
                    .position(|&ch| ch == marker)
                    .map(|rel_col| (row_idx, from + rel_col))
            })
        }

        /// The editor's text rect must be narrowed by the minimap's
        /// reserved width, not merely painted under a strip drawn on top of
        /// it: a long, unwrapped line's marker characters must never reach
        /// the strip's own columns, for *every* row — not just the rows the
        /// strip happens to paint over this frame. A buggy build that still
        /// hands `Surface::Editor` the window's full, un-narrowed `rect`
        /// renders this marker line out to the pane's full width, so it
        /// collides with the strip's left edge; that's the overlap this
        /// test catches.
        ///
        /// The fixture is deliberately *short* — a single line, so the pane
        /// does not overflow and quadraui paints no inline vertical
        /// scrollbar — because that is what keeps the overlap observable.
        /// The strip is painted after (over) the text and its braille fills
        /// every column it covers, including the all-clear `U+2800` cells,
        /// so the only text columns a buggy build can still be *caught* in
        /// are the ones past the strip's own right edge: the pane's very
        /// last column, which un-narrowed text reaches only when no
        /// scrollbar column holds it one cell short. The vertically
        /// overflowing sibling fixture — where the scrollbar does exist, and
        /// text therefore stops under the strip — is
        /// [`minimap_narrowing_moves_the_vertical_scrollbar_flush_against_text`].
        #[test]
        fn minimap_does_not_overlap_buffer_text() {
            // One long, unwrapped line of a marker character that would
            // reach the pane's full 80-column width if the editor's paint
            // rect were not narrowed for the minimap strip.
            let (screen, strip_left, overflows_vertically) =
                minimap_paint(&format!("{}\n", "Z".repeat(200)));
            assert!(
                !overflows_vertically,
                "fixture must *not* overflow the pane vertically: an inline \
                 vertical scrollbar would hold even un-narrowed text one \
                 column short of the pane's right edge, inside the band the \
                 strip paints over, leaving this assertion unable to observe \
                 the overlap at all; screen:\n{screen}"
            );

            let violation = first_marker_at_or_past(&screen, 'Z', strip_left);
            assert!(
                violation.is_none(),
                "{}: buffer text marker 'Z' painted at or past the minimap \
                 strip's left edge (x={strip_left}); the editor text rect \
                 must be narrowed by the minimap's reserved width so text \
                 and the strip occupy disjoint columns; screen:\n{screen}",
                violation
                    .map(|(row_idx, col)| format!("row {row_idx} col {col}"))
                    .unwrap_or_default(),
            );
        }

        /// Narrowing the editor's paint rect for the minimap also moves
        /// TUI's inline vertical scrollbar, and this pins where to:
        /// `quadraui::tui::draw_editor` draws it at the last column of
        /// whatever `area` it is handed, so a narrowed `area` pulls the
        /// scrollbar one column left, flush against the text and
        /// immediately before the strip rather than at the pane's true
        /// right edge. That relocation is current, intended behaviour for
        /// *painting*.
        ///
        /// Click/drag geometry (`app_support::editor_scrollbar_layout`,
        /// `render::hit_test_geometry::window_zone_hit_test`) still targets
        /// the un-narrowed true edge, one column further right than where
        /// this test finds the scrollbar glyphs — a real but separate
        /// defect tracked as a quadraui gap in
        /// `docs/PENDING_QUADRAUI_ISSUES.md` (TUI `draw_editor` has no
        /// paint-side "reserve" parameter to decouple "how much width text
        /// gets" from "where the scrollbar anchors", the rasteriser sibling
        /// of the already-tracked `Editor::layout` gap).
        #[test]
        fn minimap_narrowing_moves_the_vertical_scrollbar_flush_against_text() {
            // Far more lines than the harness terminal's 24 rows, so the
            // window genuinely overflows and grows a vertical scrollbar.
            let text = "line\n".repeat(100);
            let (screen, strip_left, overflows_vertically) = minimap_paint(&text);
            assert!(
                overflows_vertically,
                "fixture must overflow the pane vertically so a vertical \
                 scrollbar is painted at all; screen:\n{screen}"
            );

            // '█'/'░' are the thumb/track glyphs
            // `quadraui::tui::draw_scrollbar` paints.
            let scrollbar_col = strip_left.saturating_sub(1);
            let scrollbar_col_has_glyph = screen.lines().any(|row| {
                row.chars()
                    .nth(scrollbar_col)
                    .is_some_and(|ch| ch == '█' || ch == '░')
            });
            assert!(
                scrollbar_col_has_glyph,
                "expected the vertical scrollbar to paint at column \
                 {scrollbar_col} (one column left of the minimap strip at \
                 x={strip_left}); screen:\n{screen}"
            );
        }

        /// The lowest `line N` number among the frame's painted text —
        /// the TUI twin of `gtk::testing`'s identically-named helper on its
        /// own pair of acceptance tests below.
        fn top_line(screen: &str) -> Option<usize> {
            screen
                .split_whitespace()
                .collect::<Vec<_>>()
                .windows(2)
                .filter(|w| w[0] == "line")
                .filter_map(|w| w[1].parse::<usize>().ok())
                .min()
        }

        /// Minimum combined |Δr| + |Δg| + |Δb| for two backgrounds to
        /// count as "clearly visually distinct" rather than merely "not
        /// bit-identical". Chosen well below the onedark delta the
        /// `minimap_viewport_highlight_band_paints_a_distinct_background`
        /// test observes (strip `rgb(26,26,26)` vs. band `rgb(44,63,79)`,
        /// combined delta 18+37+53=108) so a real regression toward
        /// low-contrast has room to be caught before it reaches full
        /// identity.
        const MIN_PERCEPTIBLE_RGB_DELTA: i32 = 24;

        /// Fail unless `a` and `b` differ by at least
        /// `MIN_PERCEPTIBLE_RGB_DELTA` combined RGB units — stronger than
        /// `assert_ne!`, which a one-unit rounding difference would
        /// satisfy while still reading as flat on real hardware. Falls
        /// back to plain inequality for non-`Rgb` `Color` variants (named
        /// colours, indexed, reset), where no component-wise delta is
        /// meaningful.
        fn assert_clearly_distinct(a: ratatui::style::Color, b: ratatui::style::Color, ctx: &str) {
            use ratatui::style::Color;
            if let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (a, b) {
                let delta = (r1 as i32 - r2 as i32).abs()
                    + (g1 as i32 - g2 as i32).abs()
                    + (b1 as i32 - b2 as i32).abs();
                assert!(
                    delta >= MIN_PERCEPTIBLE_RGB_DELTA,
                    "{ctx}: {a:?} vs {b:?} differ by only {delta} combined RGB \
                     units, below the {MIN_PERCEPTIBLE_RGB_DELTA}-unit threshold \
                     for a reliably perceptible difference on real hardware"
                );
            } else {
                assert_ne!(a, b, "{ctx}: colours must differ ({a:?} vs {b:?})");
            }
        }

        /// The minimap's viewport-highlight band must paint a **distinct background
        /// colour** from the rest of the strip — confirmed by reading back styles
        /// via `TuiDriver::style_at`, not just verifying the geometry. The known
        /// defect this guards against is the band being blended indistinguishably
        /// from the background. This test confirms the band clears a perceptible
        /// per-channel RGB delta under the dark `onedark` theme.
        #[test]
        fn minimap_viewport_highlight_band_paints_a_distinct_background() {
            const TOTAL_LINES: usize = 2000;
            let mut engine = plain_engine();
            // Minimap is experimental and off by default — this test
            // is specifically about the minimap, so turn it on explicitly.
            engine.settings.minimap = true;
            let text: String = (0..TOTAL_LINES)
                .map(|i| format!("line {i} content\n"))
                .collect();
            engine.buffer_mut().insert(0, &text);
            let mut h = harness_no_sidebar(engine);
            h.driver.render();

            // Helper: read back the strip rect + resolved viewport-highlight
            // rect for the active window's minimap, as last painted.
            let strip_and_highlight =
                |h: &crate::harness::ConformanceHarness<quadraui::tui::testing::TuiDriver<_>>| {
                    let layout = h.screen_layout.borrow();
                    let win = h.engine.borrow().active_window_id();
                    let mm = layout
                        .as_ref()
                        .expect("render_content must have painted a ScreenLayout")
                        .minimap
                        .iter()
                        .find(|m| m.window_id == win)
                        .expect("a long, scrolling buffer must publish a minimap strip");
                    let strip = crate::render::minimap_strip_rect(mm);
                    let hl = mm
                        .resolved_layout
                        .borrow()
                        .as_ref()
                        .expect("draw_minimap_strip must have painted a resolved layout")
                        .viewport_highlight;
                    (strip, hl)
                };

            // Scrolled to the top: the highlight band sits at the strip's
            // own top rows, with un-highlighted strip rows below it.
            let (strip, highlight) = strip_and_highlight(&h);
            assert!(
                highlight.height > 0.0,
                "a 2000-line buffer taller than the strip must produce a \
                 nonzero-height viewport highlight; strip={strip:?} \
                 highlight={highlight:?}"
            );
            let strip_x = (strip.x as u16) + (strip.width as u16 / 2);
            // Probe the band's *midpoint* row, not its top edge: quadraui's
            // rasteriser decides row membership on `row_mid` (the row's
            // vertical centre), so a non-integral `highlight.y` can place
            // the top edge's row just outside the band while the midpoint
            // is reliably inside it (quadraui/src/tui/minimap.rs:237).
            let in_band_y = (highlight.y as u16 + (highlight.height as u16 / 2))
                .min((strip.y as u16) + (strip.height as u16) - 1);
            let out_band_y = (strip.y as u16) + (strip.height as u16) - 1;
            let in_style = h
                .driver
                .style_at(strip_x, in_band_y)
                .expect("the highlight row must be a painted cell");
            let out_style = h
                .driver
                .style_at(strip_x, out_band_y)
                .expect("the strip's last row must be a painted cell");
            assert_clearly_distinct(
                in_style.bg,
                out_style.bg,
                &format!(
                    "the minimap's viewport-highlight band at column \
                     {strip_x} (inside row {in_band_y}, outside row \
                     {out_band_y})"
                ),
            );

            // Scroll to the bottom and re-check — the band must still be distinct
            // and track the viewport position.
            let win = h.engine.borrow().active_window_id();
            h.engine
                .borrow_mut()
                .set_scroll_top_for_window(win, TOTAL_LINES - 1);
            h.driver.render();
            let (strip2, highlight2) = strip_and_highlight(&h);
            assert!(
                highlight2.height > 0.0,
                "scrolled to the bottom, the viewport highlight must still \
                 have nonzero height; strip={strip2:?} highlight={highlight2:?}"
            );
            let strip_x2 = (strip2.x as u16) + (strip2.width as u16 / 2);
            let in_band_y2 = (highlight2.y as u16 + (highlight2.height as u16 / 2))
                .min((strip2.y as u16) + (strip2.height as u16) - 1);
            let out_band_y2 = strip2.y as u16;
            let in_style2 = h
                .driver
                .style_at(strip_x2, in_band_y2)
                .expect("the highlight row must be a painted cell");
            let out_style2 = h
                .driver
                .style_at(strip_x2, out_band_y2)
                .expect("the strip's first row must be a painted cell");
            assert_clearly_distinct(
                in_style2.bg,
                out_style2.bg,
                &format!(
                    "after scrolling to the bottom, the minimap's \
                     viewport-highlight band at column {strip_x2} (inside \
                     row {in_band_y2}, outside row {out_band_y2})"
                ),
            );
        }

        /// Light-theme sibling of `minimap_viewport_highlight_band_paints_a_distinct_background`:
        /// confirms the perceptible-delta threshold holds under both dark and light
        /// themes.
        #[test]
        fn minimap_viewport_highlight_band_paints_a_distinct_background_under_light_theme() {
            const TOTAL_LINES: usize = 2000;
            let mut engine = plain_engine();
            engine.settings.colorscheme = "vscode-light".to_string();
            // Minimap is experimental and off by default — this test
            // is specifically about the minimap, so turn it on explicitly.
            engine.settings.minimap = true;
            let text: String = (0..TOTAL_LINES)
                .map(|i| format!("line {i} content\n"))
                .collect();
            engine.buffer_mut().insert(0, &text);
            let mut h = harness_no_sidebar(engine);
            h.driver.render();

            let layout = h.screen_layout.borrow();
            let win = h.engine.borrow().active_window_id();
            let mm = layout
                .as_ref()
                .expect("render_content must have painted a ScreenLayout")
                .minimap
                .iter()
                .find(|m| m.window_id == win)
                .expect("a long, scrolling buffer must publish a minimap strip");
            let strip = crate::render::minimap_strip_rect(mm);
            let highlight = mm
                .resolved_layout
                .borrow()
                .as_ref()
                .expect("draw_minimap_strip must have painted a resolved layout")
                .viewport_highlight;
            assert!(
                highlight.height > 0.0,
                "a 2000-line buffer taller than the strip must produce a \
                 nonzero-height viewport highlight under vscode-light too; \
                 strip={strip:?} highlight={highlight:?}"
            );
            let strip_x = (strip.x as u16) + (strip.width as u16 / 2);
            let in_band_y = (highlight.y as u16 + (highlight.height as u16 / 2))
                .min((strip.y as u16) + (strip.height as u16) - 1);
            let out_band_y = (strip.y as u16) + (strip.height as u16) - 1;
            let in_style = h
                .driver
                .style_at(strip_x, in_band_y)
                .expect("the highlight row must be a painted cell");
            let out_style = h
                .driver
                .style_at(strip_x, out_band_y)
                .expect("the strip's last row must be a painted cell");
            assert_clearly_distinct(
                in_style.bg,
                out_style.bg,
                &format!(
                    "under vscode-light, the minimap's viewport-highlight \
                     band at column {strip_x} (inside row {in_band_y}, \
                     outside row {out_band_y})"
                ),
            );
        }

        /// **Holding Alt** while dragging the minimap's viewport-highlight thumb from
        /// the top of the strip to the bottom must scroll through virtually the whole
        /// file — exactly like dragging the real vertical scrollbar handle the same
        /// distance. TUI counterpart of the GTK test — both backends share the one
        /// resolver, so this confirms the Alt-gating reaches production through TUI's
        /// `App::handle_dispatch` path too.
        #[test]
        fn dragging_the_minimap_viewport_highlight_scrolls_the_whole_file_with_alt_held() {
            const TOTAL_LINES: usize = 200_000;
            let mut engine = plain_engine();
            // Minimap is experimental and off by default — this test
            // is specifically about the minimap, so turn it on explicitly.
            engine.settings.minimap = true;
            let text: String = (0..TOTAL_LINES)
                .map(|i| format!("line {i} content\n"))
                .collect();
            engine.buffer_mut().insert(0, &text);

            let mut h = harness_no_sidebar(engine);
            let win = h.engine.borrow().active_window_id();
            h.driver.render();

            assert_eq!(
                h.engine.borrow().windows.get(&win).unwrap().view.scroll_top,
                0,
                "fixture must start at the top of the file"
            );

            let strip = {
                let layout = h.screen_layout.borrow();
                let mm = layout
                    .as_ref()
                    .expect("render_content must have painted a ScreenLayout")
                    .minimap
                    .iter()
                    .find(|m| m.window_id == win)
                    .expect("a 200,000-line buffer must publish a minimap strip");
                crate::render::minimap_strip_rect(mm)
            };

            let x = strip.x + strip.width / 2.0;
            h.driver.click_with(
                quadraui::MouseButton::Left,
                quadraui::Modifiers {
                    alt: true,
                    ..Default::default()
                },
                x,
                strip.y + 1.0,
            );
            h.driver.mouse_move(x, strip.y + strip.height - 1.0);
            h.driver.mouse_up(x, strip.y + strip.height - 1.0);
            h.driver.render();

            let screen = h.driver.screen();
            let top = top_line(&screen).unwrap_or_else(|| {
                panic!("the editor must still paint line numbers; screen:\n{screen}")
            });

            assert!(
                top > TOTAL_LINES * 9 / 10,
                "an Alt-held drag from the highlight's top edge to the \
                 strip's bottom row must scroll through virtually the \
                 whole file in one gesture, not crawl within one strip \
                 window — landed on line {top} of {TOTAL_LINES}; \
                 screen:\n{screen}"
            );
        }

        /// A plain (no-modifier) minimap thumb drag uses file-wide geometry —
        /// the full strip maps to the full file, matching VS Code. TUI
        /// counterpart of the GTK test.
        #[test]
        fn dragging_the_minimap_viewport_highlight_traverses_the_whole_file() {
            const TOTAL_LINES: usize = 200_000;
            let mut engine = plain_engine();
            // Minimap is experimental and off by default — this test
            // is specifically about the minimap, so turn it on explicitly.
            engine.settings.minimap = true;
            let text: String = (0..TOTAL_LINES)
                .map(|i| format!("line {i} content\n"))
                .collect();
            engine.buffer_mut().insert(0, &text);

            let mut h = harness_no_sidebar(engine);
            let win = h.engine.borrow().active_window_id();
            h.driver.render();

            assert_eq!(
                h.engine.borrow().windows.get(&win).unwrap().view.scroll_top,
                0,
                "fixture must start at the top of the file"
            );

            let strip = {
                let layout = h.screen_layout.borrow();
                let mm = layout
                    .as_ref()
                    .expect("render_content must have painted a ScreenLayout")
                    .minimap
                    .iter()
                    .find(|m| m.window_id == win)
                    .expect("a 200,000-line buffer must publish a minimap strip");
                crate::render::minimap_strip_rect(mm)
            };

            let x = strip.x + strip.width / 2.0;
            // Plain press/drag — no modifier.
            h.driver.mouse_down(x, strip.y + 1.0);
            h.driver.mouse_move(x, strip.y + strip.height - 1.0);
            h.driver.mouse_up(x, strip.y + strip.height - 1.0);
            h.driver.render();

            let screen = h.driver.screen();
            let top = top_line(&screen).unwrap_or_else(|| {
                panic!("the editor must still paint line numbers; screen:\n{screen}")
            });

            assert!(
                top > TOTAL_LINES * 9 / 10,
                "a plain (no-modifier) drag from the highlight's top edge \
                 to the strip's bottom row must traverse virtually the \
                 whole file (file-wide geometry, matching VS Code) — \
                 landed on line {top} of {TOTAL_LINES}; \
                 screen:\n{screen}"
            );
        }

        /// A plain (no-modifier) **click away from the thumb** — a "jump" to
        /// a point near the bottom of the strip, not a drag of the highlight
        /// itself — must not scroll past the painted window's own local
        /// ceiling, `base + (span - viewport_lines)`: the highest
        /// `scroll_top` that still shows a line within `[base, base + span)`
        /// as the viewport's last visible row. The strip only ever paints
        /// lines `[base, base + span)`, so letting a bottom-edge click push
        /// `scroll_top` any higher scrolls past content the minimap never
        /// represented. Reads `base`/`span`/`viewport_lines` back from the
        /// actually-painted window instead of a hardcoded guess, so the
        /// expectation always matches whatever window size the minimap
        /// happens to choose.
        #[test]
        fn clicking_the_minimap_strip_near_its_bottom_edge_does_not_overshoot_the_window() {
            const TOTAL_LINES: usize = 200_000;
            let mut engine = plain_engine();
            // Minimap is experimental and off by default — this test
            // is specifically about the minimap, so turn it on explicitly.
            engine.settings.minimap = true;
            let text: String = (0..TOTAL_LINES)
                .map(|i| format!("line {i} content\n"))
                .collect();
            engine.buffer_mut().insert(0, &text);

            let mut h = harness_no_sidebar(engine);
            let win = h.engine.borrow().active_window_id();
            h.driver.render();

            let (strip, base, span) = {
                let layout = h.screen_layout.borrow();
                let mm = layout
                    .as_ref()
                    .expect("render_content must have painted a ScreenLayout")
                    .minimap
                    .iter()
                    .find(|m| m.window_id == win)
                    .expect("a 200,000-line buffer must publish a minimap strip");
                let first = mm
                    .minimap
                    .lines
                    .first()
                    .expect("a painted window has at least one line");
                let last = mm
                    .minimap
                    .lines
                    .last()
                    .expect("a painted window has at least one line");
                let base = first.line_idx;
                let span = last.line_idx + 1 - base;
                (crate::render::minimap_strip_rect(mm), base, span)
            };
            let viewport_lines = h
                .engine
                .borrow()
                .windows
                .get(&win)
                .unwrap()
                .view
                .viewport_lines;

            let x = strip.x + strip.width / 2.0;
            // The highlight band sits at the strip's own top (scroll_top
            // starts at 0), so a press one row above the strip's bottom
            // edge lands well outside the band — a jump, not a thumb
            // drag — and a follow-up move to the same point exercises the
            // armed scrollbar's own clamping instead of the jump's
            // one-shot centring.
            let py = strip.y + strip.height - 1.0;
            h.driver.mouse_down(x, py);
            h.driver.mouse_move(x, py);
            h.driver.mouse_up(x, py);
            h.driver.render();

            let top = h.engine.borrow().windows.get(&win).unwrap().view.scroll_top;
            let local_ceiling = base + span.saturating_sub(viewport_lines);

            assert!(
                top <= local_ceiling,
                "a plain click near the minimap strip's bottom edge must \
                 not scroll past the painted window's own local ceiling \
                 (base {base}, span {span}, viewport_lines {viewport_lines}, \
                 local ceiling {local_ceiling}) — landed on line {top} of \
                 {TOTAL_LINES}"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Dividers
    // ─────────────────────────────────────────────────────────────────────────
    mod dividers {
        use super::*;

        /// The column of the first `'│'` divider glyph on `cells`, scanning
        /// only from `after` onward — the TUI twin `shell_app.rs`'s own test
        /// module uses (`divider_col_on_row`), reproduced here since that one
        /// is private to `shell_app.rs`'s test module.
        fn divider_col_on_row<S>(cells: &[(char, S)], after: usize) -> Option<usize> {
            cells
                .iter()
                .enumerate()
                .skip(after)
                .find(|(_, (c, _))| *c == '\u{2502}')
                .map(|(i, _)| i)
        }

        /// Mirrors `shell_app.rs`'s test of the same name: a vertical group
        /// split must paint a `'│'` divider glyph between the two panes' tab
        /// labels.
        #[test]
        fn render_content_paints_group_divider_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "short\n");
            engine.open_editor_group(SplitDirection::Vertical);
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            // The split's second pane's tab bar must paint despite the editor band
            // collapsing.
            let screen = driver.screen();
            // Locate the tab row by content, not row 0 — see
            // `ctrl_w_v_reserves_one_column_for_the_divider_via_shell_app`'s
            // own comment on why.
            let (tab_y, tab_row) = screen
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains("[No Name]"))
                .unwrap_or((0, ""));
            let starts: Vec<usize> = tab_row.match_indices("[No Name]").map(|(i, _)| i).collect();
            assert_eq!(starts.len(), 2, "expected two tab bars; row:\n{tab_row}");
            let (left_tab_start, right_tab_start) = (starts[0], starts[1]);

            let mut found_divider = false;
            for (y, line) in screen.lines().enumerate().skip(tab_y + 1).take(15) {
                let chars: Vec<char> = line.chars().collect();
                let Some(col) = chars
                    .iter()
                    .enumerate()
                    .skip(left_tab_start)
                    .find(|(_, &c)| c == '\u{2502}')
                    .map(|(i, _)| i)
                else {
                    continue;
                };
                found_divider = true;
                assert!(
                    col > left_tab_start && col <= right_tab_start,
                    "row {y}: divider at col {col} should land between the \
                     two panes' tab labels; line:\n{line}"
                );
            }
            assert!(
                found_divider,
                "expected the group divider glyph to paint; screen:\n{screen}"
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: dragging the
        /// group divider must actually move it, repainted further in the drag
        /// direction.
        #[test]
        fn group_divider_drag_moves_the_painted_divider_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "short\n");
            engine.open_editor_group(SplitDirection::Vertical);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // The vertical group split must paint a divider glyph despite the editor
            // band collapsing.
            driver.mouse_up(1.0, 1.0); // settle the layout, see mirrored test's own doc
            let (tab_x, _) = driver
                .find("[No Name]")
                .expect("each pane paints its own tab label");
            let after = tab_x as usize;
            let row = 5_usize;
            let before = divider_col_on_row(&driver.styled_row(row as u16), after)
                .expect("the vertical group split must paint a divider glyph");

            let target = before.saturating_sub(4);
            driver.mouse_down(before as f32, row as f32);
            driver.mouse_move(target as f32, row as f32);
            driver.mouse_up(target as f32, row as f32);

            let moved = divider_col_on_row(&driver.styled_row(row as u16), after)
                .expect("the divider must still be painted after the drag");
            let screen = driver.screen();
            assert!(
                moved < before,
                "dragging the divider from col {before} to col {target} \
                     must repaint it further left, but it stayed at col \
                     {moved}; screen:\n{screen}"
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name: a press and
        /// release with no intervening move must leave the divider exactly
        /// where it was.
        #[test]
        fn group_divider_click_without_move_leaves_the_divider_put_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "short\n");
            engine.open_editor_group(SplitDirection::Vertical);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // The vertical group split must paint a divider glyph even when the editor
            // content band collapses to zero rows.
            driver.mouse_up(1.0, 1.0);
            let (tab_x, _) = driver
                .find("[No Name]")
                .expect("each pane paints its own tab label");
            let after = tab_x as usize;
            let row = 5_usize;
            let before = divider_col_on_row(&driver.styled_row(row as u16), after)
                .expect("the vertical group split must paint a divider glyph");

            driver.mouse_down(before as f32, row as f32);
            driver.mouse_up(before as f32, row as f32);

            let after_cells = driver.styled_row(row as u16);
            let screen = driver.screen();
            assert_eq!(
                divider_col_on_row(&after_cells, after),
                Some(before),
                "a press-and-release on the divider with no drag must \
                     not move it; screen:\n{screen}"
            );
        }

        /// A `Ctrl-W v` vertical split (a vim **window** split, not an editor-group
        /// split) must reserve exactly one column for the divider. The gate's own
        /// assertion was checking for the wrong shape: `Ctrl-W v` shares its
        /// group's one tab bar (unlike `open_editor_group`'s group splits, which
        /// paint one tab bar *per group*). Fixed to assert the correct shape.
        #[test]
        fn ctrl_w_v_reserves_one_column_for_the_divider_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "short\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            driver.ctrl_char('w');
            driver.type_char('v');
            driver.mouse_up(1.0, 1.0); // settle the layout, see mirrored test's own doc

            let screen = driver.screen();
            // Exactly one tab bar (a window split shares its group's tab
            // bar) — proves this is a window split, not an editor-group
            // split, before checking the divider shape below.
            let tab_row = screen
                .lines()
                .find(|line| line.contains("[No Name]"))
                .unwrap_or("");
            let tab_starts: Vec<usize> =
                tab_row.match_indices("[No Name]").map(|(i, _)| i).collect();
            assert_eq!(
                tab_starts.len(),
                1,
                "'Ctrl-W v' is a window split and must share its group's \
                 single tab bar; row:\n{tab_row}"
            );

            // The content row: both window panes paint the buffer's own
            // text, exactly once each, with exactly one divider column
            // between them.
            let content_row = screen
                .lines()
                .find(|line| line.match_indices("short").count() >= 2)
                .unwrap_or_else(|| {
                    panic!(
                        "expected a row with both window panes' content \
                         painted; screen:\n{screen}"
                    )
                });
            let first_end = content_row.find("short").unwrap() + "short".len();
            let second_start = content_row[first_end..].find("short").unwrap() + first_end;
            let between = &content_row[first_end..second_start];
            assert_eq!(
                between.matches('\u{2502}').count(),
                1,
                "'Ctrl-W v' must reserve exactly one column for the \
                 divider between the two panes; between-text: {between:?}; \
                 row:\n{content_row}"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Terminal-shell behaviours keyed on `BackendCaps`
    // ─────────────────────────────────────────────────────────────────────────
    //
    // Terminal-only behaviours now shared with `App`: capabilities read from
    // `Backend::backend_caps` or no-ops on GUI backends, wired into the shared
    // `App`/`render.rs`.
    mod terminal_shell_behaviours {
        use super::*;

        // ── 1: `keyboard_enhanced` (kitty-keyboard protocol) ────────────

        /// `App::setup` must read `App::keyboard_enhanced` from the *live*
        /// `Backend::backend_caps().kitty_keyboard` answer, not a hardcoded
        /// constant — wrong on a terminal without the kitty protocol — and that
        /// value must reach `render::engine_key_from_ui` at the named-key decode
        /// call site. Deterministic in both directions via `TuiBackend::
        /// set_kitty_keyboard`.
        #[test]
        fn setup_reads_keyboard_enhanced_from_live_backend_caps() {
            let engine = std::rc::Rc::new(std::cell::RefCell::new(plain_engine()));
            let backend_handle: std::rc::Rc<std::cell::RefCell<Box<dyn quadraui::Backend>>> =
                std::rc::Rc::new(std::cell::RefCell::new(Box::new(
                    quadraui::tui::TuiBackend::new(),
                )));
            let (mut app, _config) = crate::harness::build_app_and_config(
                engine,
                backend_handle,
                crate::render::UnitProfile::cell(),
            );

            let ctrl_4 = quadraui::Key::Char('4');
            let ctrl_mods = quadraui::Modifiers {
                ctrl: true,
                ..Default::default()
            };

            let mut backend = quadraui::tui::TuiBackend::new();
            backend.set_kitty_keyboard(false);
            quadraui::ShellApp::setup(&mut app, &mut backend);
            assert!(
                !app.keyboard_enhanced,
                "setup() must read kitty_keyboard=false off the live backend"
            );
            assert_eq!(
                crate::render::engine_key_from_ui(&ctrl_4, ctrl_mods, app.keyboard_enhanced),
                Some(("backslash".to_string(), Some('4'), true)),
                "without kitty-keyboard support, Ctrl+4 is genuinely \
                 ambiguous with Ctrl+\\ and must resolve to \"backslash\""
            );

            backend.set_kitty_keyboard(true);
            quadraui::ShellApp::setup(&mut app, &mut backend);
            assert!(
                app.keyboard_enhanced,
                "setup() must read kitty_keyboard=true off the live backend"
            );
            assert_eq!(
                crate::render::engine_key_from_ui(&ctrl_4, ctrl_mods, app.keyboard_enhanced),
                Some(("4".to_string(), Some('4'), true)),
                "with kitty-keyboard support, Ctrl+4 is unambiguous and must \
                 resolve to literal \"4\""
            );
        }

        // ── 2: caret shape per mode ──────────────────────────────────────

        /// The caret-shape decision `App::tick_dispatch` feeds `Backend::
        /// set_caret_shape` must match the shared `render::caret_shape_for_mode`.
        /// Pure-function coverage: the actual `backend.set_caret_shape` write is
        /// gated behind `App::live`.
        #[test]
        fn caret_shape_for_mode_tracks_engine_mode_and_sidebar_focus() {
            let mut engine = plain_engine();
            assert_eq!(
                crate::render::caret_shape_for_mode(&engine, engine.sidebar_has_focus()),
                quadraui::EditorCursorShape::Block,
                "Normal mode, no sidebar focus, no pending replace -> Block"
            );

            engine.mode = crate::core::Mode::Insert;
            assert_eq!(
                crate::render::caret_shape_for_mode(&engine, engine.sidebar_has_focus()),
                quadraui::EditorCursorShape::Bar,
                "Insert mode -> Bar"
            );

            engine.mode = crate::core::Mode::Normal;
            engine.pending_key = Some('r');
            assert_eq!(
                crate::render::caret_shape_for_mode(&engine, engine.sidebar_has_focus()),
                quadraui::EditorCursorShape::Underline,
                "pending replace-char ('r') -> Underline"
            );

            engine.pending_key = None;
            engine.mode = crate::core::Mode::Insert;
            engine.explorer_has_focus = true;
            assert_eq!(
                crate::render::caret_shape_for_mode(&engine, engine.sidebar_has_focus()),
                quadraui::EditorCursorShape::Block,
                "a focused sidebar panel overrides Insert-mode Bar -> Block"
            );
        }

        // ── 3: Ctrl-L full repaint ────────────────────────────────────────

        /// Requires a vt100-backed observer (`TuiVtDriver`) to prove `Backend:::
        /// request_full_repaint` actually cleared a stale cell — mirrors
        /// `conformance_harness` using `driver_with_shell` in place of the
        /// `TestBackend` variant.
        fn vt_driver(
            engine: crate::core::Engine,
        ) -> quadraui::tui::vt_testing::TuiVtDriver<impl quadraui::AppLogic> {
            let engine = std::rc::Rc::new(std::cell::RefCell::new(engine));
            let backend: std::rc::Rc<std::cell::RefCell<Box<dyn quadraui::Backend>>> =
                std::rc::Rc::new(std::cell::RefCell::new(Box::new(
                    quadraui::tui::TuiBackend::new(),
                )));
            let (app, config) = crate::harness::build_app_and_config(
                engine,
                backend,
                crate::render::UnitProfile::cell(),
            );
            quadraui::tui::vt_testing::driver_with_shell(app, config, 80, 24)
        }

        /// The bottom-right cell — never painted by the status bar or any
        /// popup at 80x24 on either backend (mirrors
        /// `tui_main::shell_app`'s identical `STALE_CELL_ANSI_MOVE`
        /// constant and its own doc for why that coordinate is safe).
        const STALE_CELL_ANSI_MOVE: &[u8] = b"\x1b[24;80H";

        /// Ctrl+L must call `Backend::request_full_repaint` and force the next
        /// paint to wipe a cell an incremental diff would otherwise skip.
        #[test]
        fn ctrl_l_repaints_a_stale_cell_an_incremental_diff_would_skip_via_shell_app() {
            let mut driver = vt_driver(plain_engine());

            driver.inject_raw(STALE_CELL_ANSI_MOVE);
            driver.inject_raw(b"Z");
            assert!(
                driver.screen_contains("Z"),
                "sanity: the injected stale glyph must be visible before \
                 either render — screen:\n{}",
                driver.screen()
            );

            // Plain redraw, nothing pending: `App` never paints that
            // corner, so ratatui's diff still believes it's unchanged and
            // sends nothing for it — the stale glyph survives. Proves this
            // test can go RED.
            driver.render();
            assert!(
                driver.screen_contains("Z"),
                "a redraw with no full-repaint request pending must not \
                 touch cells the diff cache believes are unchanged — \
                 screen:\n{}",
                driver.screen()
            );

            let reaction = driver.ctrl_char('l');
            assert_eq!(
                reaction,
                quadraui::Reaction::Redraw,
                "Ctrl+L must request a redraw"
            );
            assert!(
                !driver.screen_contains("Z"),
                "Ctrl+L must call Backend::request_full_repaint and force \
                 the stale glyph to clear — screen:\n{}",
                driver.screen()
            );
        }

        /// Companion to
        /// `opening_a_line_below_the_last_line_paints_every_line_in_order_
        /// 1779` above, using `vt_driver` (a real `CrosstermBackend` whose
        /// writes are parsed by a real `vt100::Parser`) instead of plain
        /// `TuiDriver`/`TestBackend`, in case the bug were in what ANSI
        /// bytes ratatui's incremental diff emits rather than in its own
        /// in-memory `Buffer` (the `request_full_repaint`-shaped gap
        /// `ctrl_l_repaints_a_stale_cell_...` above exists to catch for a
        /// different trigger).
        ///
        /// Unlike the `TestBackend` twin above, this one genuinely
        /// **cannot** be fixed to interleave a tick the same way:
        /// `quadraui::tui::vt_testing::TuiVtDriver` (at this repo's pinned
        /// rev) has no public `tick()` — only `TuiDriver`
        /// (`quadraui::tui::testing`) does. Confirmed by trying it: adding
        /// `driver.tick()` here is a compile error
        /// (`no method named 'tick' found for struct 'TuiVtDriver<A>'`),
        /// not a passing-but-uninformative test, so the earlier "Confirmed
        /// NOT RED, same as the `TestBackend` twin" framing was doubly
        /// wrong — the real reason this one can't reproduce the bug
        /// in-process is a missing driver API, not a gap in what key
        /// sequence it sends. Kept as-is (no tick) as evidence the ANSI
        /// diff/vt100 path isn't itself where the bug lives; the real
        /// end-to-end RED-verified coverage is
        /// `tests/pty_open_line_below_paints_all_lines.rs`, which drives an
        /// actual `vcd` over a real pty and needs no driver `tick()` at
        /// all because the real runner ticks on its own.
        #[test]
        fn opening_a_line_below_the_last_line_paints_every_line_in_order_via_vt_driver_1779() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQXW_FOO\n");
            let mut driver = vt_driver(engine);

            assert!(
                driver.screen_contains("ZQXW_FOO"),
                "precondition: the single existing line must be painted; \
                 screen:\n{}",
                driver.screen()
            );

            driver.type_char('o');
            for c in "ZQXW_BAR".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Escape);
            driver.render();

            let screen = driver.screen();
            let foo_pos = driver.find("ZQXW_FOO");
            let bar_pos = driver.find("ZQXW_BAR");
            assert!(
                foo_pos.is_some() && bar_pos.is_some(),
                "both the original first line and the line opened below it \
                 with 'o' must be painted on the real terminal grid, not \
                 just in ratatui's own diff buffer; screen:\n{screen}"
            );
            let (_, foo_y) = foo_pos.unwrap();
            let (_, bar_y) = bar_pos.unwrap();
            assert_eq!(
                bar_y,
                foo_y + 1.0,
                "the new line must paint immediately below the first line, \
                 on the actual vt100-observed terminal grid, with no blank \
                 row between them and no row vanishing; foo_y={foo_y} \
                 bar_y={bar_y} screen:\n{screen}"
            );
        }

        // ── 4: terminal PTY resize on WindowResized ──────────────────────

        /// `App::handle_dispatch`'s `WindowResized` arm must forward the resize to
        /// any open terminal PTY. Two `WindowResized` dispatches: the first resize
        /// reads the *pre*-resize painted width (no live pixel width is available),
        /// sets `draw_needed`, and the repaint updates the cached bounds for the
        /// *second* dispatch. This mirrors how a real live resize settles over
        /// consecutive `WindowResized` events.
        #[test]
        fn window_resized_resizes_the_open_terminal_pty() {
            let mut engine = plain_engine();
            engine.terminal_new_tab(74, 24);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            let before = h.engine.borrow().terminal_panes[0].session.cols();

            driver.resize(40, 24);
            for _ in 0..2 {
                driver.dispatch(quadraui::UiEvent::WindowResized {
                    viewport: quadraui::Viewport::new(40.0, 24.0, 1.0),
                });
            }

            let after = h.engine.borrow().terminal_panes[0].session.cols();
            assert!(
                after < before,
                "narrowing the window must shrink the open terminal pane's \
                 PTY column count (before={before}, after={after})"
            );
        }

        // ── 5: nerd-font startup notice ──────────────────────────────────

        /// `App::tick_dispatch` must drain `App::pending_startup_msg` into
        /// `engine.message` exactly as the pre-#1434 TUI shell's `tick` does. The
        /// natural trigger is platform-gated, so this test seeds the field directly
        /// to isolate the drain, not the platform-specific resolution.
        #[test]
        fn tick_drains_the_pending_nerd_font_startup_message_via_shell_app() {
            let engine = std::rc::Rc::new(std::cell::RefCell::new(plain_engine()));
            let backend: std::rc::Rc<std::cell::RefCell<Box<dyn quadraui::Backend>>> =
                std::rc::Rc::new(std::cell::RefCell::new(Box::new(
                    quadraui::tui::TuiBackend::new(),
                )));
            let (mut app, config) = crate::harness::build_app_and_config(
                engine,
                backend,
                crate::render::UnitProfile::cell(),
            );
            app.pending_startup_msg = Some("ASCII fallback icons ZQXW1428".to_string());
            let mut driver = quadraui::tui::testing::driver_with_shell(app, config, 80, 24);

            let reaction = driver.tick();
            assert_eq!(
                reaction,
                quadraui::Reaction::Redraw,
                "draining the startup message must request a redraw"
            );
            assert!(
                driver.screen_contains("ASCII fallback icons ZQXW1428"),
                "the startup nudge must reach engine.message and paint on \
                 the status/command line; screen:\n{}",
                driver.screen()
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Mouse wheel scroll polarity
    // ─────────────────────────────────────────────────────────────────────────
    mod mouse_scroll {
        use super::*;
        use quadraui::{Point, ScrollDelta, UiEvent};

        /// Buffer long enough that a viewport scroll cannot be clamped away
        /// — the TUI-on-`App` twin of `crate::gtk::testing`'s identically
        /// purposed `engine_with_long_buffer`.
        fn engine_with_long_buffer() -> crate::core::Engine {
            let mut engine = plain_engine();
            let text: String = (0..500).map(|i| format!("line {i}\n")).collect();
            engine.buffer_mut().insert(0, &text);
            engine
        }

        /// Wheel-down must scroll the viewport down (negating back to GTK's raw
        /// polarity). Also exercises the `App::handle_mouse_scroll_msg` hovered-window
        /// lookup refactored onto `render::find_window_at`.
        #[test]
        fn wheel_down_scrolls_the_viewport_down_not_up() {
            let mut h = harness(engine_with_long_buffer());
            let win = h.engine.borrow().active_window_id();
            let rect = h
                .screen_layout
                .borrow()
                .as_ref()
                .and_then(|s| s.windows.first().map(|w| w.rect))
                .expect("the editor window must have painted a rect");
            let x = (rect.x + rect.width / 2.0) as f32;
            let y = (rect.y + rect.height / 2.0) as f32;

            // quadraui convention: negative delta.y = scroll down.
            h.driver.dispatch(UiEvent::Scroll {
                widget: None,
                position: Point::new(x, y),
                delta: ScrollDelta::new(0.0, -1.0),
            });
            let after_down = h.engine.borrow().windows[&win].view.scroll_top;
            assert!(
                after_down > 0,
                "wheel down must move the viewport DOWN (scroll_top 0 -> >0), \
                 got {after_down} — direction is inverted (#554)"
            );

            // ...and the opposite notch walks it back, so this cannot pass
            // by a consumer that ignores the sign entirely.
            h.driver.dispatch(UiEvent::Scroll {
                widget: None,
                position: Point::new(x, y),
                delta: ScrollDelta::new(0.0, 1.0),
            });
            let after_up = h.engine.borrow().windows[&win].view.scroll_top;
            assert!(
                after_up < after_down,
                "wheel up must move the viewport back UP ({after_down} -> {after_up})"
            );
        }

        /// `App::handle_mouse_scroll_msg`'s hovered-window lookup must resolve the
        /// *unfocused* pane the pointer is actually over, not fall back to the active
        /// window.
        #[test]
        fn wheel_scrolls_the_pane_under_the_pointer_not_the_focused_one() {
            let mut h = harness(engine_with_long_buffer());
            h.engine
                .borrow_mut()
                .split_window(SplitDirection::Horizontal, None);
            // Repaint so `cached_screen_layout` carries both panes' rects.
            h.driver.render();

            let focused = h.engine.borrow().active_window_id();
            let unfocused = *h
                .engine
                .borrow()
                .windows
                .keys()
                .find(|id| **id != focused)
                .expect("`:split` must produce a second window");

            let unfocused_rect = h
                .screen_layout
                .borrow()
                .as_ref()
                .and_then(|s| s.windows.iter().find(|w| w.window_id == unfocused))
                .map(|w| w.rect)
                .expect("the unfocused pane must have painted a rect");
            let ux = (unfocused_rect.x + unfocused_rect.width / 2.0) as f32;
            let uy = (unfocused_rect.y + unfocused_rect.height / 2.0) as f32;

            h.driver.dispatch(UiEvent::Scroll {
                widget: None,
                position: Point::new(ux, uy),
                delta: ScrollDelta::new(0.0, -1.0),
            });

            assert!(
                h.engine.borrow().windows[&unfocused].view.scroll_top > 0,
                "wheel over the unfocused pane must scroll it (scroll_top stayed 0)"
            );
            assert_eq!(
                h.engine.borrow().windows[&focused].view.scroll_top,
                0,
                "wheel over the unfocused pane must NOT scroll the focused pane"
            );
            assert_eq!(
                h.engine.borrow().active_window_id(),
                focused,
                "hovering to scroll must not move focus"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Editor scrollbars
    // ─────────────────────────────────────────────────────────────────────────
    mod editor_scrollbars {
        use super::*;

        /// TUI counterpart of `crate::gtk::testing`'s
        /// `horizontal_scrollbar_thumb_click_scrolls_with_sidebar_open_on_gtk`
        /// — same scenario (a click on the h-scrollbar's empty track, with the
        /// explorer sidebar open, must page `scroll_left`), driven through
        /// `TuiDriver` instead of a Cairo `ImageSurface`. Both tests exist
        /// because `App::handle_mouse_click_msg`/`App::editor_scrollbar_press`
        /// (`src/app.rs`) is one shared dispatch for *both* backends now —
        /// #1433 flipped TUI's `run` onto it and #1434 deleted the
        /// independently hand-written production TUI shell (this module's own
        /// top doc) — so CLAUDE.md's "cover both backends" rule for a shared
        /// dispatch code path applies here, not just on GTK.
        ///
        /// Before #1493, `app.rs`'s "H scrollbar hit-test" rung rebuilt window
        /// rects via `compute_editor_window_rects(&engine, width, height, lh)`,
        /// which always assumes the editor area starts at column 0 — true only
        /// with the activity bar/sidebar at zero width. With the explorer
        /// open, the real editor rect starts to the right of column 0 (this
        /// test's own `rect.x > 0.0` fixture-sanity check below), so every
        /// click coordinate this test sends landed *outside* the phantom
        /// `x = 0` rect and never reached the h-scrollbar hit-test at all —
        /// #1493's fix reads `painted_editor_bounds()` for both axes through
        /// the shared `App::editor_scrollbar_press`, so the same click now
        /// resolves.
        ///
        /// **Verified RED against unfixed `develop`**: reverting this test's
        /// two `app.rs` call sites to call `compute_editor_window_rects(&engine,
        /// width, height, lh)` directly (the pre-#1493 horizontal rung's own
        /// rect source) instead of `self.painted_editor_bounds()`, leaving
        /// everything else (including this test) unmodified, and re-running
        /// this test — `scroll_left` stayed `0`: the click missed the
        /// h-scrollbar entirely and landed on ordinary editor text instead
        /// (cursor moved, scrollbar untouched). Restored before landing.
        #[test]
        fn horizontal_scrollbar_thumb_click_scrolls_with_sidebar_open() {
            let mut engine = plain_engine();
            // No line numbers: the gutter is then exactly one column wide
            // (the fold indicator `render::calculate_gutter_cols` always
            // reserves) — matches the GTK counterpart's fixture.
            engine.settings.line_numbers = crate::core::settings::LineNumberMode::None;
            // Off (default on): the per-window status line paints in the
            // same bottom row as the h-scrollbar and would otherwise claim
            // that row via the chrome rung before this issue's scrollbar
            // rung ever runs — see the GTK counterpart's identical note.
            engine.settings.window_status_line = false;
            // One very long line plus a couple of short ones: needs an
            // h-scrollbar but never a v-scrollbar, isolating this test to
            // the one axis #1493 fixes.
            let long_line = "x".repeat(500);
            engine
                .buffer_mut()
                .insert(0, &format!("{long_line}\nshort\nshort\n"));
            let win = engine.active_window_id();
            let buffer_id = engine.windows.get(&win).unwrap().buffer_id;
            // `max_col` (what the scrollbar geometry reads) is a cache
            // refreshed by `update_syntax`, not by a raw `Buffer::insert`.
            engine
                .buffer_manager
                .get_mut(buffer_id)
                .unwrap()
                .update_syntax();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            engine.session.explorer_visible = true;

            let mut h = crate::tui_main::testing::conformance_harness(engine, 120, 24);
            h.driver.render();

            let rect = h
                .screen_layout
                .borrow()
                .as_ref()
                .and_then(|s| s.windows.iter().find(|w| w.window_id == win))
                .map(|w| w.rect)
                .expect("the editor window must have painted a rect");
            assert!(
                rect.x > 0.0,
                "fixture sanity: with the explorer open the editor must not \
                 start at column 0 (got x={}) — this offset is exactly what \
                 the pre-#1493 horizontal rung ignored",
                rect.x
            );

            let before = h
                .engine
                .borrow()
                .windows
                .get(&win)
                .unwrap()
                .view
                .scroll_left;
            assert_eq!(before, 0, "fixture sanity: must start unscrolled");

            // Click one cell short of the window's right edge, on its bottom
            // row — where the h-scrollbar's track paints with no per-window
            // status line to claim that row first — strictly past the thumb
            // (which starts at the track's own left edge, `scroll_left`
            // being 0), resolving to a page-jump toward the click.
            let click_x = (rect.x + rect.width - 1.0) as f32;
            let click_y = (rect.y + rect.height - 1.0) as f32;

            h.driver.click(click_x, click_y);
            h.driver.render();

            let after = h
                .engine
                .borrow()
                .windows
                .get(&win)
                .unwrap()
                .view
                .scroll_left;
            assert!(
                after > 0,
                "a click on the h-scrollbar's empty track must page \
                 scroll_left forward; got {after} — with the sidebar open, \
                 this is the exact click the pre-#1493 horizontal rung's \
                 `x = 0`-assuming window rects missed entirely"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // `:set wrap linebreak` scroll drift
    // ─────────────────────────────────────────────────────────────────────────
    mod word_wrap {
        use super::*;

        /// The column count the painter wrapped the active window at on the
        /// last render — `render.rs`'s `render_viewport_cols`, recorded into
        /// `Engine::paint_viewport_cols`. Fixture measurement only; the
        /// tests' assertions are on the rendered screen.
        fn painted_wrap_cols(
            h: &crate::harness::ConformanceHarness<
                quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            >,
        ) -> usize {
            let engine = h.engine.borrow();
            let wid = engine.active_window_id();
            let vp = engine
                .paint_viewport_cols
                .borrow()
                .get(&wid)
                .copied()
                .unwrap_or(0);
            assert!(vp > 4, "fixture sanity: implausible painted width {vp}");
            vp
        }

        /// A line whose text exactly fills its last wrapped row must not
        /// paint an extra blank continuation row after it. The painter used
        /// to wrap the line *including* its trailing `\n`, so the newline
        /// spilled into a row of its own — a row `ensure_cursor_visible_wrap`
        /// (which counts rows on the EOL-stripped text) never accounted
        /// for, drifting `G` off the bottom of the viewport.
        ///
        /// RED verified: reverting `render.rs`'s `wrap_text_len` trim makes
        /// the next line paint two rows below the wrapped one, not one.
        #[test]
        fn exactly_full_wrapped_line_paints_no_trailing_blank_row() {
            let _settings_guard = crate::core::settings::TestSettingsPathGuard::install(
                std::env::temp_dir().join(format!(
                    "vimcode_test_1496_no_settings_{}_{:?}.json",
                    std::process::id(),
                    std::thread::current().id()
                )),
            );
            let mut engine = plain_engine();
            engine.settings.minimap = false;
            engine.settings.wrap = true;
            engine.buffer_mut().insert(0, "x\nZQXW_NEXT_LINE\n");
            let mut h = harness_no_sidebar(engine);
            h.driver.render();
            h.driver.tick();
            let vp = painted_wrap_cols(&h);

            // Exactly two full rows of 'b', then the marker line.
            let text = format!("{}\nZQXW_NEXT_LINE\n", "b".repeat(2 * vp));
            let cur_len = h.engine.borrow().buffer().len_chars();
            h.engine.borrow_mut().buffer_mut().delete_range(0, cur_len);
            h.engine.borrow_mut().buffer_mut().insert(0, &text);
            h.driver.render();

            let screen = h.driver.screen();
            let rows: Vec<&str> = screen.lines().collect();
            let last_b_row = rows
                .iter()
                .rposition(|r| r.contains("bbbb"))
                .expect("wrapped line must paint");
            let marker_row = rows
                .iter()
                .position(|r| r.contains("ZQXW_NEXT_LINE"))
                .expect("next line must paint");
            assert_eq!(
                marker_row,
                last_b_row + 1,
                "the line after an exactly-full wrapped line must paint on \
                 the very next row (no blank EOL row); screen:\n{screen}"
            );
        }

        /// `ensure_cursor_visible_wrap`'s per-line visual-row count used to
        /// be a plain length-based `div_ceil`, blind to `'linebreak'` — see
        /// `core::engine::mod::engine_visual_rows_for_line`'s own doc. With
        /// `'linebreak'` on, a word-boundary break can back up off the
        /// viewport edge, needing *more* wrapped rows for a line than the
        /// length-only count predicted; the scroll-to-cursor walk in
        /// `ensure_cursor_visible_wrap` then thinks more buffer lines fit
        /// above the cursor than the viewport actually has room for once
        /// they're really wrapped, landing the cursor's own line below the
        /// bottom of the viewport.
        ///
        /// Builds a buffer of lines each engineered so that a hard wrap
        /// needs exactly 2 rows but a `'linebreak'`-aware wrap needs 3:
        /// `vp - 2` filler chars, a single space, then `vp + 1` more filler
        /// chars — `2 * vp` chars total, so the hard cut lands exactly on a
        /// row boundary (2 rows, no remainder) while `'linebreak'` backs
        /// the first break up to the space, leaving one char too many to
        /// fit the remainder in a single second row. Repeated across many
        /// lines so the total drift is large enough to push the marker
        /// line off *any* plausible viewport height.
        ///
        /// `vp` here is the width the painter actually wrapped at (see
        /// [`painted_wrap_cols`]), not a count of visible filler chars on
        /// screen: the scrollbar overdraws the text area's last column, so
        /// a screen scan reads one short and the fixture lines end up not
        /// wrapping the way this doc describes.
        ///
        /// RED verified against unfixed `develop`: reverting
        /// `engine_visual_rows_for_line` to its pre-#1496 `div_ceil`
        /// (linebreak-blind) form makes this fail — `G` lands the cursor
        /// below the bottom of the viewport and the marker line never
        /// appears on screen.
        #[test]
        fn linebreak_scroll_to_last_line_shows_it() {
            const DRIFT_LINES: usize = 30;
            const PROBE_LEN: usize = 500;

            // `tick()` below polls the on-disk settings file and reloads it
            // if it looks newer — without this guard that's the developer's
            // real `~/.config/vimcode/settings.json`, which can silently
            // flip 'wrap'/'linebreak' back off and make this test pass
            // without exercising anything (it did, on a machine with a
            // user settings file, while failing on CI's empty `$HOME`).
            // Point it at a path that doesn't exist instead.
            let _settings_guard = crate::core::settings::TestSettingsPathGuard::install(
                std::env::temp_dir().join(format!(
                    "vimcode_test_1496_no_settings_{}_{:?}.json",
                    std::process::id(),
                    std::thread::current().id()
                )),
            );

            let mut engine = plain_engine();
            // Minimap off, and 'wrap'+'linebreak' on for the probe render
            // too (a horizontal-scrollbar column reserved only when 'wrap'
            // is off would otherwise make the probe measurement disagree
            // with the real run below) — both irrelevant to what this test
            // is about, so set directly rather than through more `:set`
            // round trips.
            engine.settings.minimap = false;
            engine.settings.wrap = true;
            engine.settings.linebreak = true;
            // Short placeholder lines so the total line count already
            // matches the real fixture below — keeps the gutter's digit
            // width identical between the probe render and the real one.
            let mut probe_text = String::new();
            for _ in 0..=DRIFT_LINES {
                probe_text.push_str("x\n");
            }
            let initial_len = engine.buffer().len_chars();
            engine.buffer_mut().delete_range(0, initial_len);
            engine.buffer_mut().insert(0, &probe_text);

            let mut h = harness_no_sidebar(engine);
            h.driver.render();
            h.driver.tick();
            let vp = painted_wrap_cols(&h);
            // `vp - 2` filler + one space + `vp + 1` filler = exactly
            // `2 * vp` chars total (see this fn's own doc above).
            let drift_line = format!("{}{}{}", "a".repeat(vp - 2), " ", "a".repeat(vp + 1));
            let mut text = String::new();
            for _ in 0..DRIFT_LINES {
                text.push_str(&drift_line);
                text.push('\n');
            }
            text.push_str("ZQXW1496_LAST_LINE");
            let cur_len = h.engine.borrow().buffer().len_chars();
            h.engine.borrow_mut().buffer_mut().delete_range(0, cur_len);
            h.engine.borrow_mut().buffer_mut().insert(0, &text);
            {
                let mut engine = h.engine.borrow_mut();
                let win = engine.active_window_id();
                let view = &mut engine.windows.get_mut(&win).unwrap().view;
                view.cursor.line = 0;
                view.cursor.col = 0;
                view.scroll_top = 0;
            }
            h.driver.render();
            h.driver.tick();

            let driver = &mut h.driver;
            // Real, black-box `:set` round trip (already true from the
            // probe setup above, so this is a confirmatory no-op on the
            // fields — but it's the actual user gesture the acceptance
            // scenario names, and exercises the ex-command parse path too).
            driver.type_char(':');
            for c in "set wrap linebreak".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            driver.type_char('G');
            driver.render();

            let screen = driver.screen();
            assert!(
                screen.contains("ZQXW1496_LAST_LINE"),
                "'G' with 'wrap' and 'linebreak' on must scroll the last \
                 line into view; screen:\n{screen}"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Line numbers default to absolute (VS Code/Neovim hybrid default)
    // ─────────────────────────────────────────────────────────────────────────
    mod line_numbers_default {
        use super::*;

        /// A fresh editor with untouched settings (no `:set number`, no
        /// `settings.json` override) must paint the absolute line-number
        /// gutter out of the box — VS Code shows line numbers from the
        /// first frame, and vimcode is a VS Code/Neovim hybrid. Asserts on
        /// the painted gutter text itself, not on `settings.line_numbers`
        /// being populated (that field could be right while paint ignores
        /// it, as #587/#592 found for other `ScreenLayout`-adjacent state).
        ///
        /// Scoped to the gutter column specifically (not an unscoped
        /// whole-screen substring search): the default-`ruler`-on status
        /// bar always paints a literal "1" (e.g. "Ln 1, Col 1 (3 lines)"),
        /// so an unscoped `find_bounds("1").is_some()` can't actually
        /// distinguish "gutter painted the digit" from "the ruler always
        /// has a 1 in it". Instead this pins each digit to the same row as
        /// — and strictly left of — that line's own text, which only the
        /// gutter satisfies.
        ///
        /// **Verified RED against unfixed `develop`**: with
        /// `Settings::default`'s `line_numbers` reverted to
        /// `LineNumberMode::None`, this test fails — the gutter paints only
        /// the fold-indicator column, so `find_bounds("1")` never lands on
        /// the "aaa" row.
        #[test]
        fn fresh_engine_paints_absolute_line_numbers_by_default() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "aaa\nbbb\nccc\n");
            let mut h = harness_no_sidebar(engine);
            h.driver.render();

            let screen = h.driver.screen();
            let aaa = h.driver.find_bounds("aaa").unwrap_or_else(|| {
                panic!("first line's text should be painted; screen:\n{screen}")
            });
            let bbb = h.driver.find_bounds("bbb").unwrap_or_else(|| {
                panic!("second line's text should be painted; screen:\n{screen}")
            });
            let one = h
                .driver
                .find_bounds("1")
                .unwrap_or_else(|| panic!("gutter should paint line number 1; screen:\n{screen}"));
            let two = h
                .driver
                .find_bounds("2")
                .unwrap_or_else(|| panic!("gutter should paint line number 2; screen:\n{screen}"));

            assert_eq!(
                one.y, aaa.y,
                "line number 1 must be on the same row as the first line's text; screen:\n{screen}"
            );
            assert!(
                one.x < aaa.x,
                "line number 1 must sit in the gutter, left of the line text; screen:\n{screen}"
            );
            assert_eq!(
                two.y, bbb.y,
                "line number 2 must be on the same row as the second line's text; screen:\n{screen}"
            );
            assert!(
                two.x < bbb.x,
                "line number 2 must sit in the gutter, left of the line text; screen:\n{screen}"
            );
        }
    }

    mod fold_controls {
        use super::*;

        /// The default `fold_controls = "mouseover"` paints the open-fold
        /// `-` gutter marker only while the pointer is over that window's
        /// gutter. Both backends route through the same shared
        /// `App::handle_dispatch` `MouseMoved` arm (`render::
        /// route_gutter_hover`), which sets `Engine::gutter_hover_window` —
        /// read back by `render::build_rendered_window` on the very next
        /// paint — so no per-backend hover geometry exists for this at all.
        ///
        /// Reads the exact gutter cell the fold indicator paints into (the
        /// leftmost column of the block-opener line's own row, resolved
        /// from the just-painted `RenderedWindow`, never a hardcoded
        /// coordinate, #555) — not `gutter_hover_window` being `Some`
        /// (#587/#592's lesson: state can be right while paint ignores it).
        ///
        /// **Verified RED against unfixed `develop`**: reverting `render::
        /// fold_indicator_char` to always show `-` for a genuine block
        /// opener (dropping the `open_markers_visible` gate this issue
        /// adds) makes the "before hover" assertion below fail — the
        /// marker paints immediately, with no hover required.
        #[test]
        fn open_fold_marker_paints_only_while_the_gutter_is_hovered_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "fn foo() {\n    body\n}\n");
            let mut h = harness_no_sidebar(engine);
            h.driver.render();

            let (row, col) = {
                let layout = h.screen_layout.borrow();
                let layout = layout.as_ref().expect("a frame must have painted");
                let rw = &layout.windows[0];
                let view_row = rw
                    .lines
                    .iter()
                    .position(|rl| rl.line_idx == 0)
                    .expect("the block-opener line must be in the painted viewport");
                (rw.rect.y as usize + view_row, rw.rect.x as usize)
            };

            let screen_before = h.driver.screen();
            let before = screen_before
                .lines()
                .nth(row)
                .and_then(|l| l.chars().nth(col))
                .unwrap_or(' ');
            assert_ne!(
                before,
                '-',
                "no pointer has moved onto the gutter yet, so the open-fold \
                 marker must not be painted under the default \
                 `fold_controls = mouseover`; row {row}: {:?}",
                screen_before.lines().nth(row)
            );

            h.driver.mouse_move(col as f32, row as f32);

            let screen_after = h.driver.screen();
            let after = screen_after
                .lines()
                .nth(row)
                .and_then(|l| l.chars().nth(col))
                .unwrap_or(' ');
            assert_eq!(
                after,
                '-',
                "hovering this window's gutter must paint the `-` open-fold \
                 marker on its block-opener line; row {row}: {:?}",
                screen_after.lines().nth(row)
            );
        }

        /// Non-blocking review follow-up: the settings round-trip
        /// test in `core::settings` only proves `fold_controls = "always"`
        /// stores/parses the right string — it never renders a frame. This
        /// exercises the actual paint path: `Always` must show the `-`
        /// open-fold marker on a genuine block-opener line **without** any
        /// hover at all (matching pre-#1544 behavior, restored on request).
        ///
        /// **Verified RED**: temporarily making `Always` route through the
        /// same hover check as `Mouseover` (in `render::
        /// build_rendered_window`'s `fold_open_markers_visible` match) made
        /// this fail. Restored before committing.
        #[test]
        fn fold_controls_always_shows_open_fold_marker_without_hover_via_shell_app() {
            let mut engine = plain_engine();
            engine.settings.fold_controls = crate::core::settings::FoldControlsMode::Always;
            engine.buffer_mut().insert(0, "fn foo() {\n    body\n}\n");
            let mut h = harness_no_sidebar(engine);
            h.driver.render();

            let (row, col) = {
                let layout = h.screen_layout.borrow();
                let layout = layout.as_ref().expect("a frame must have painted");
                let rw = &layout.windows[0];
                let view_row = rw
                    .lines
                    .iter()
                    .position(|rl| rl.line_idx == 0)
                    .expect("the block-opener line must be in the painted viewport");
                (rw.rect.y as usize + view_row, rw.rect.x as usize)
            };

            let screen = h.driver.screen();
            let ch = screen
                .lines()
                .nth(row)
                .and_then(|l| l.chars().nth(col))
                .unwrap_or(' ');
            assert_eq!(
                ch,
                '-',
                "`fold_controls = always` must paint the open-fold marker \
                 immediately, with no hover required; row {row}: {:?}",
                screen.lines().nth(row)
            );
        }

        /// Non-blocking review follow-up: same gap as above for
        /// `Never`. Covers both marker kinds — the open-fold `-` on a
        /// block-opener line, and the closed-fold `+` on a manually closed
        /// fold header (`zfj`) — neither should ever paint under `Never`,
        /// hovered or not.
        ///
        /// **Verified RED**: temporarily disabling `fold_indicator_char`'s
        /// top `controls == FoldControlsMode::Never` short-circuit made
        /// this fail (the closed `+` marker painted again). Restored before
        /// committing.
        #[test]
        fn fold_controls_never_hides_open_and_closed_fold_markers_via_shell_app() {
            let mut engine = plain_engine();
            engine.settings.fold_controls = crate::core::settings::FoldControlsMode::Never;
            engine
                .buffer_mut()
                .insert(0, "fn foo() {\n    body\n}\nfn bar() {\n    body2\n}\n");
            // Manually close the second block into a fold header so its
            // gutter would otherwise show `+`.
            engine.view_mut().cursor.line = 3;
            engine.feed_keys("zfj");
            assert!(
                engine.view().fold_at(3).is_some(),
                "test setup sanity: `zfj` must create a closed fold at line 3"
            );
            let mut h = harness_no_sidebar(engine);
            h.driver.render();

            let (open_row, closed_row, col) = {
                let layout = h.screen_layout.borrow();
                let layout = layout.as_ref().expect("a frame must have painted");
                let rw = &layout.windows[0];
                let open_view_row = rw
                    .lines
                    .iter()
                    .position(|rl| rl.line_idx == 0)
                    .expect("the open block-opener line must be in the painted viewport");
                let closed_view_row = rw
                    .lines
                    .iter()
                    .position(|rl| rl.line_idx == 3)
                    .expect("the closed fold header line must be in the painted viewport");
                (
                    rw.rect.y as usize + open_view_row,
                    rw.rect.y as usize + closed_view_row,
                    rw.rect.x as usize,
                )
            };

            // Hover the open block-opener's gutter cell too, to prove
            // `Never` wins even over an active hover.
            h.driver.mouse_move(col as f32, open_row as f32);

            let screen = h.driver.screen();
            let open_ch = screen
                .lines()
                .nth(open_row)
                .and_then(|l| l.chars().nth(col))
                .unwrap_or(' ');
            let closed_ch = screen
                .lines()
                .nth(closed_row)
                .and_then(|l| l.chars().nth(col))
                .unwrap_or(' ');
            assert_ne!(
                open_ch,
                '-',
                "`fold_controls = never` must blank the open-fold marker \
                 even while the gutter is hovered; row {open_row}: {:?}",
                screen.lines().nth(open_row)
            );
            assert_ne!(
                closed_ch,
                '+',
                "`fold_controls = never` must also blank an already-closed \
                 fold's marker; row {closed_row}: {:?}",
                screen.lines().nth(closed_row)
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Sticky scroll: pin enclosing-scope header lines at the top of
    // the editor pane while scrolling.
    // ─────────────────────────────────────────────────────────────────────────
    mod sticky_scroll {
        use super::*;

        /// Builds `fn STICKYOUTER1546() {` / `    fn STICKYINNER1546() {`
        /// followed by 40 body lines and closing braces, scrolled so both
        /// headers are far above the viewport (`scroll_top = 20`, cursor at
        /// line 25 — safely below the pinned band so the "never hide the
        /// cursor" guard doesn't suppress it).
        fn nested_scopes_engine() -> crate::core::Engine {
            let mut engine = plain_engine();
            let mut text = String::from("fn STICKYOUTER1546() {\n    fn STICKYINNER1546() {\n");
            for i in 0..40 {
                text.push_str(&format!("        body line {i}\n"));
            }
            text.push_str("    }\n}\n");
            engine.buffer_mut().insert(0, &text);
            engine.view_mut().scroll_top = 20;
            engine.view_mut().cursor.line = 25;
            engine.view_mut().cursor.col = 0;
            engine
        }

        /// Scrolling past a nested scope's opener lines must pin them at the
        /// top of the viewport instead of letting them scroll away — VS
        /// Code's `editor.stickyScroll.enabled`. Reads the actual painted
        /// `RenderedWindow` for the row → buffer-line mapping (never a
        /// hardcoded coordinate, #555) and cross-checks the character grid
        /// itself (#587/#592's lesson: assert on paint, not engine state).
        ///
        /// **Verified RED against unfixed `develop`**: with `render::
        /// build_rendered_window`'s sticky-scroll splice block deleted
        /// entirely, rows 0/1 show `body line 18`/`body line 19` (buffer
        /// lines 20/21 — `scroll_top`'s own real top-of-viewport content,
        /// since the two header lines shift every body line's buffer index
        /// up by 2 from its `{i}` suffix) instead of the two headers, and
        /// the `line_idx`/`screen_contains` assertions below fail. Restored
        /// before committing.
        #[test]
        fn scrolling_past_enclosing_scopes_pins_their_headers_via_shell_app() {
            let mut h = harness_no_sidebar(nested_scopes_engine());
            h.driver.render();

            let (row0_idx, row1_idx, row2_idx, row0_text, row1_text) = {
                let layout = h.screen_layout.borrow();
                let layout = layout.as_ref().expect("a frame must have painted");
                let rw = &layout.windows[0];
                (
                    rw.lines[0].line_idx,
                    rw.lines[1].line_idx,
                    rw.lines[2].line_idx,
                    rw.lines[0].raw_text.clone(),
                    rw.lines[1].raw_text.clone(),
                )
            };

            assert_eq!(
                row0_idx, 0,
                "row 0 must be pinned to the outer scope's header line (buffer line 0)"
            );
            assert!(
                row0_text.contains("STICKYOUTER1546"),
                "row 0's pinned text: {row0_text:?}"
            );
            assert_eq!(
                row1_idx, 1,
                "row 1 must be pinned to the inner scope's header line (buffer line 1)"
            );
            assert!(
                row1_text.contains("STICKYINNER1546"),
                "row 1's pinned text: {row1_text:?}"
            );
            // Rows past the pinned band are untouched — row 2 is still the
            // *third* row of the original `scroll_top`-anchored viewport
            // (buffer line 22), not shifted down to compensate for the
            // splice. The two rows the headers displaced (buffer lines
            // 20/21) are simply covered, not moved elsewhere.
            assert_eq!(
                row2_idx, 22,
                "row 2 (untouched by the splice) must still be the third \
                 row of the scroll_top-anchored viewport (buffer line 22)"
            );

            assert!(
                h.driver.screen_contains("STICKYOUTER1546"),
                "the pinned outer header must reach the painted character grid:\n{}",
                h.driver.screen()
            );
            assert!(
                h.driver.screen_contains("STICKYINNER1546"),
                "the pinned inner header must reach the painted character grid:\n{}",
                h.driver.screen()
            );
            // `scroll_top`'s own line (buffer line 20 = body index 18,
            // since the two header lines shift every body line's buffer
            // index up by 2) must no longer occupy row 0/1 — it was pushed
            // down behind the pinned band, not lost.
            assert!(
                !h.driver.screen_contains("body line 18"),
                "the real top-of-viewport content (`body line 18`, buffer \
                 line 20 = scroll_top) must be covered by the pinned \
                 headers, not painted alongside them:\n{}",
                h.driver.screen()
            );
        }

        /// `sticky_scroll = false` must turn the whole feature off: the
        /// scrolled-to body line paints at the true top of the viewport
        /// again, with neither header pinned. Exists so the test above
        /// can't pass merely because `STICKYOUTER1546`/`STICKYINNER1546`
        /// happen to appear somewhere else on screen (they don't, but this
        /// closes that loophole directly) and to prove the setting itself
        /// is load-bearing, not just plumbed through `Settings` with no
        /// paint-side effect.
        #[test]
        fn sticky_scroll_false_disables_the_pinned_band_via_shell_app() {
            let mut engine = nested_scopes_engine();
            engine.settings.sticky_scroll = false;
            let mut h = harness_no_sidebar(engine);
            h.driver.render();

            let row0_idx = {
                let layout = h.screen_layout.borrow();
                let layout = layout.as_ref().expect("a frame must have painted");
                layout.windows[0].lines[0].line_idx
            };
            assert_eq!(
                row0_idx, 20,
                "with sticky_scroll off, row 0 must show the real scrolled-to \
                 top line (buffer line 20, `scroll_top`), not a pinned header"
            );
            assert!(
                !h.driver.screen_contains("STICKYOUTER1546"),
                "no header should be pinned when sticky_scroll is off:\n{}",
                h.driver.screen()
            );
        }

        /// Clicking a pinned sticky-scroll header must jump the cursor to
        /// that line — reuses the exact same `RenderedLine::line_idx` click
        /// hit-test every other row goes through
        /// (`render::window_zone_hit_test`), proved end-to-end through a
        /// real click rather than asserting on the hit-test function in
        /// isolation.
        #[test]
        fn clicking_a_pinned_sticky_header_jumps_the_cursor_to_it_via_shell_app() {
            let mut h = harness_no_sidebar(nested_scopes_engine());
            h.driver.render();

            let (x, y) = h
                .driver
                .find("STICKYINNER1546")
                .expect("the pinned inner header must paint somewhere on screen");
            h.driver.click(x, y);

            assert_eq!(
                h.engine.borrow().view().cursor.line,
                1,
                "clicking the pinned inner-scope header must move the cursor \
                 to its real buffer line (1), not wherever it visually sits \
                 in the scrolled viewport"
            );
        }
    }

    mod acp_mcp_servers {
        use super::*;
        use std::time::{Duration, Instant};

        /// #1487 acceptance: the `:AiAgent` status line's
        /// `" | MCP: <names>"` suffix must actually paint on screen, not
        /// just be true of `Engine::acp_agent_registry_status_line`'s
        /// return value in isolation. Drives a real session start through
        /// `:AI hi` (the fixture agent advertises `mcpCapabilities.http`
        /// via `$ACP_FAKE_MCP_HTTP`, so the configured `http` server is
        /// sent, not dropped), then types `:AiAgent` itself and reads the
        /// painted command line — the same "engine.message renders
        /// verbatim" surface `render_content_paints_command_line_via_
        /// shell_app` establishes.
        ///
        /// RED verified: reverting `Engine::acp_begin_session` to send
        /// `vec![]` unconditionally (so `AcpSession::active_mcp_servers`
        /// stays empty) makes this fail — the `:AiAgent` screen shows no
        /// `MCP:` suffix at all.
        #[cfg(unix)]
        #[test]
        fn ai_agent_status_line_shows_active_mcp_server_via_shell_app() {
            let mut engine = plain_engine();
            // `Engine::new_for_test` seeds `settings_mtime: None`, which
            // `App`'s `settings_file_changed` (run unconditionally every
            // `tick()`, see `handle_poll_tick`'s own comment) treats as
            // "reload unconditionally" the first time it runs — it would
            // otherwise stomp the `acp_agents`/`acp_mcp_servers` set below
            // with whatever this *machine's real*
            // `~/.config/vimcode/settings.json` happens to contain the
            // first time `driver.tick()` runs (mirrors `shell_app.rs`'s
            // identical `check_settings_reload` dance for the same
            // reason). Consume that one-shot reload here, before
            // configuring the fixture agent.
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));

            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec![
                    "ACP_FAKE_NO_TOOL_REQUEST=1".to_string(),
                    "ACP_FAKE_MCP_HTTP=1".to_string(),
                    "ACP_FAKE_AGENT_LABEL=Mcp1487".to_string(),
                ],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();
            engine.settings.acp_mcp_servers = vec![crate::core::acp::AcpMcpServerConfig {
                name: "remoteMcp1487".to_string(),
                transport: "http".to_string(),
                url: "https://mcp.example.com".to_string(),
                ..Default::default()
            }];

            let mut h = harness(engine);
            let driver = &mut h.driver;
            driver.press_named(quadraui::NamedKey::Escape);

            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("Hello_Mcp1487") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("Hello_Mcp1487"),
                "session must start and reply within 5s; screen:\n{screen}"
            );

            // `:AI hi`'s own one-shot "focus the panel for further typing"
            // reveal (`Engine::acp_focus_ai_panel_for_keyboard`,
            // `sidebar_focus_requested`, drained by `run_post_key_epilogue`
            // right after the command ran) leaves `ai_has_focus` set —
            // *persistently*, by design (the whole point of the reveal),
            // unlike the one-shot *request* itself. On `App` (unlike
            // the pre-#1434 TUI shell's own separate `TuiSidebar::has_focus` latch),
            // `route_focus_key`'s `sidebar_band_focused` gate is literally
            // `Engine::sidebar_has_focus()` — the disjunction that includes
            // `ai_has_focus` — so every keystroke, including `:`, now
            // routes to the AI panel's own chat input
            // (`FocusKeyRoute::Ai`/`route_ai_chat_event`) instead of
            // opening the Normal-mode command line, exactly like GTK
            // already does (same comment, `App::handle_key_press`). Escape
            // is how a user leaves that focus on every other panel too
            // (`dispatch_ai_chat_event`'s `Cancelled` arm clears
            // `ai_has_focus`) — not a workaround, the same keystroke a real
            // user would press to go back to typing `:AiAgent` at all.
            driver.press_named(quadraui::NamedKey::Escape);

            driver.type_char(':');
            for c in "AiAgent".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();

            let screen = driver.screen();
            assert!(
                screen.contains("MCP: remoteMcp1487"),
                "`:AiAgent`'s painted status line must name the MCP server \
                 the live session actually started with; screen:\n{screen}"
            );
        }

        /// #1487 acceptance, the drop-warning half: when the agent does
        /// NOT advertise `mcpCapabilities.http`, a configured `http` MCP
        /// server is dropped and the warning must reach the painted
        /// command line, not merely `engine.message` in isolation.
        /// Session start is driven through the real `:AI hi` ex command,
        /// same as the sibling test above, minus `$ACP_FAKE_MCP_HTTP`.
        ///
        /// RED verified: reverting `Engine::acp_begin_session` to send
        /// `vec![]` unconditionally (so the drop warning is never assigned
        /// to `self.message`) makes this fail — the screen right after
        /// `:AI hi` shows no `ACP:`/`dropped` text at all.
        #[cfg(unix)]
        #[test]
        fn ai_agent_warns_about_dropped_mcp_server_via_shell_app() {
            let mut engine = plain_engine();
            // See the sibling test above for why this must run before the
            // fixture agent/MCP servers are configured.
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));

            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec!["ACP_FAKE_NO_TOOL_REQUEST=1".to_string()],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();
            engine.settings.acp_mcp_servers = vec![crate::core::acp::AcpMcpServerConfig {
                name: "droppedMcp1487".to_string(),
                transport: "http".to_string(),
                url: "https://mcp.example.com".to_string(),
                ..Default::default()
            }];

            // 140 columns, not this module's usual 80: the command line
            // shares its row with the AI sidebar's right border, leaving
            // well under 80 columns of free width — not enough to fit
            // "ACP: agent doesn't support dropped MCP server(s):
            // droppedMcp1487" without truncating the server name
            // off-screen (mirrors `shell_app.rs`'s identical width bump
            // for the same reason).
            let mut h = crate::tui_main::testing::conformance_harness(engine, 140, 24);
            let driver = &mut h.driver;
            driver.press_named(quadraui::NamedKey::Escape);

            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("droppedMcp1487") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("droppedMcp1487"),
                "the painted command line must name the dropped MCP server \
                 within 5s of session start; screen:\n{screen}"
            );
            assert!(
                screen.contains("ACP:") && screen.contains("dropped"),
                "the painted command line must warn that the server was \
                 dropped, not merely mention its name; screen:\n{screen}"
            );
        }
    }

    mod issue_1519_acp_conformance {
        use super::*;
        use std::time::{Duration, Instant};

        /// #1519 acceptance: an agent -> client request whose method this
        /// client has no handler for must be answered immediately with
        /// JSON-RPC `-32601`, never left parked forever. The fixture's
        /// `$ACP_FAKE_UNKNOWN_REQUEST` sends one mid-turn and BLOCKS reading
        /// the reply before it can send `stopReason: end_turn` — pre-#1519,
        /// the client's `_ => {}` no-op left that request unanswered, so the
        /// fixture (and the whole turn) hung forever and the reply text
        /// never painted. Drives a real turn through `:AI hi`, the same
        /// entry point `acp_mcp_servers`'s tests above use, and reads the
        /// painted transcript.
        ///
        /// RED verified: reverting the `_` arm in `Engine::
        /// acp_dispatch_events`'s `ClientRequest` match to `_ => {}` makes
        /// this fail — the screen never shows "Hello world" within the
        /// deadline below (in fact it hangs for the fixture's remaining
        /// lifetime, since the unanswered request blocks its own `read`).
        #[cfg(unix)]
        #[test]
        fn unknown_agent_request_is_answered_not_left_parked_via_shell_app() {
            let mut engine = plain_engine();
            // See `acp_mcp_servers`'s sibling tests for why this must run
            // before the fixture agent is configured.
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));

            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec!["ACP_FAKE_UNKNOWN_REQUEST=1".to_string()],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();

            let mut h = harness(engine);
            let driver = &mut h.driver;
            driver.press_named(quadraui::NamedKey::Escape);

            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("ANSWERED1519") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("ANSWERED1519"),
                "the fixture's post-reply chunk (only sent once the client \
                 has actually answered the unknown-method request) must \
                 paint within 5s -- it must be answered immediately, not \
                 left parked; screen:\n{screen}"
            );
        }

        /// #1519 acceptance, fix 1: a `tool_call_update` carrying a
        /// DIFFERENT `content`/`locations` than the original `tool_call`
        /// must REPLACE what's painted, not accumulate both — the exact
        /// spec deviation #1519 fixes (#955 originally appended). The
        /// fixture's `$ACP_FAKE_TOOL_CALL_REPLACE` emits a `tool_call`
        /// pointing at `src/first.rs`, then a `tool_call_update` pointing
        /// at `src/second.rs`.
        ///
        /// #1511 moved locations out of the collapsed one-line summary
        /// (`tool_call_title_line`, just `{glyph} {kind}: {title}`) into the
        /// card's *expanded* body (`tool_call_expanded_text`) — every card
        /// now starts collapsed, so this test clicks the card open (on its
        /// title text, which is stable across the replace) before checking
        /// which location is showing.
        ///
        /// RED verified against this expand-then-check shape: with
        /// `call.content.append(&mut blocks)` / `call.locations.append(...)`
        /// restored in place of the plain assignment in `Engine::
        /// acp_apply_tool_call_update`, this fails — the expanded card shows
        /// both `first.rs` and `second.rs` instead of just the latter.
        #[cfg(unix)]
        #[test]
        fn tool_call_update_replace_paints_only_the_new_location_via_shell_app() {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));

            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec!["ACP_FAKE_TOOL_CALL_REPLACE=1".to_string()],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();

            let mut h = harness(engine);
            let driver = &mut h.driver;
            driver.press_named(quadraui::NamedKey::Escape);

            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("edit: Edit") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("edit: Edit"),
                "the tool call's collapsed card must paint within 5s; \
                 screen:\n{screen}"
            );

            // Expand the card — collapsed cards show only
            // `tool_call_title_line` (no locations at all).
            driver.click_text("edit: Edit");
            driver.tick();

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("second.rs") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("second.rs"),
                "the tool_call_update's new location must paint within 5s \
                 of expanding the card; screen:\n{screen}"
            );
            assert!(
                !screen.contains("first.rs"),
                "the original tool_call's location must be gone once the \
                 update replaces it, not still visible alongside the new \
                 one; screen:\n{screen}"
            );
        }

        /// #1518 acceptance: a `session/request_permission` request for an
        /// `edit` tool call whose `toolCall.content` carries a `diff` block
        /// must paint the actual proposed change (an added and a removed
        /// line) into the permission dialog, not just its title/kind/
        /// locations — a human must not have to approve blind. The
        /// fixture's `$ACP_FAKE_REQUEST_PERMISSION_DIFF` emits that shaped
        /// request.
        ///
        /// RED verified: with the `acp_permission_dialog_parts` diff-block
        /// branch short-circuited (`let diff_blocks: Vec<..> = Vec::new()`,
        /// same technique as `acp_ops.rs`'s own unit-test RED
        /// verification), this fails — neither "- old line" nor "+ new
        /// line" appears anywhere in the unmodified title/kind/locations
        /// body the dialog still paints.
        #[cfg(unix)]
        #[test]
        fn permission_dialog_previews_the_proposed_diff_via_shell_app() {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));

            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec!["ACP_FAKE_REQUEST_PERMISSION_DIFF=1".to_string()],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();

            let mut h = harness(engine);
            let driver = &mut h.driver;
            driver.press_named(quadraui::NamedKey::Escape);

            driver.type_char(':');
            for c in "AI please edit".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("Tool kind: edit") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("Tool kind: edit"),
                "the permission dialog must be open within 5s; screen:\n{screen}"
            );
            assert!(
                screen.contains("- old line"),
                "the removed line must be painted in the dialog: {screen}"
            );
            assert!(
                screen.contains("+ new line"),
                "the added line must be painted in the dialog: {screen}"
            );
        }

        /// #1518 acceptance: `acp_permission_default = allow_all` must
        /// auto-answer a `session/request_permission` request with the
        /// first `allow_*` option *without ever painting the dialog at
        /// all* — `Engine::acp_permission_default_option`'s own unit tests
        /// already cover the internal-state half (`engine.dialog.is_none()`
        /// after the turn settles); this covers the black-box half the
        /// #1518 review found missing, driving a real turn through the
        /// same `$ACP_FAKE_REQUEST_PERMISSION_DIFF` fixture request
        /// `permission_dialog_previews_the_proposed_diff_via_shell_app`
        /// above uses to prove the dialog *does* open under the default
        /// `ask` setting — here `Tool kind:` (that dialog's own body text)
        /// must never paint, for the whole real subprocess round trip, and
        /// the turn must still resume to completion on its own.
        ///
        /// RED verified: with the `acp_permission_default` gate in
        /// `Engine::acp_handle_permission_request` deleted (falling
        /// straight through to `show_dialog`/parking, the pre-#1518
        /// behaviour), the wait loops below observe "Tool kind: edit" paint
        /// and fail immediately instead of running the turn out with it
        /// absent.
        ///
        /// #1732, two full-suite-only flake sources, both fixed here (the
        /// test failed once under the whole lib lane and passed every time
        /// in isolation, including under 2x-core CPU saturation):
        ///
        /// 1. `App::handle_poll_tick` runs `settings_file_changed` ->
        ///    `Engine::check_settings_reload` on *every* `driver.tick()`,
        ///    and that reload replaces `engine.settings` wholesale. The
        ///    one-shot `check_settings_reload()` below only consumes the
        ///    *first* mtime change; anything that moved this machine's real
        ///    `~/.config/vimcode/settings.json` mtime later in the run would
        ///    reload it mid-turn and reset `acp_permission_default` to `Ask`
        ///    — so `Engine::acp_handle_permission_request` (which reads
        ///    `self.settings.acp_permission_default` live, at request time)
        ///    would open the dialog and "Tool kind: edit" would paint. The
        ///    `TestSettingsPathGuard` below points `settings_file_path()` at
        ///    a per-process path that cannot exist, so the per-tick poll
        ///    takes its `mtime.is_none()` early return forever and nothing
        ///    outside this test can rewrite the settings it just injected.
        ///    (Same reasoning as `word_wrap`'s own
        ///    `TestSettingsPathGuard` use.)
        /// 2. The completion wait was 5s for a *whole* real subprocess
        ///    round trip — `sh` fork/exec, `initialize`, `session/new`,
        ///    `session/prompt`, `session/request_permission`, the
        ///    auto-answer, then `end_turn`. That is the longest-latency
        ///    wait of any ACP test in this module, and 5s of it is a budget,
        ///    not a behavioural assertion: the loops exit the instant the
        ///    condition flips, so a wider deadline costs a green run
        ///    nothing and only stops a loaded machine from being reported as
        ///    a regression.
        ///
        /// The wait is also split in two so it cannot pass *vacuously*. The
        /// old single `while ai_streaming` loop would fall straight through
        /// if the turn had not started yet, and its `!ai_streaming`
        /// assertion would then hold for the wrong reason. Phase 1 waits for
        /// the agent's own streamed `Hello_Perm1518` greeting to paint
        /// (the fixture emits it *before* it issues the permission request),
        /// proving the session really is mid-turn; only then does phase 2
        /// wait for the turn to settle, which the fixture only ever does
        /// after it has read a reply to that request. Both loops assert the
        /// dialog is absent on every frame.
        #[cfg(unix)]
        #[test]
        fn acp_permission_default_allow_all_skips_the_dialog_via_shell_app() {
            let _settings_guard = crate::core::settings::TestSettingsPathGuard::install(
                std::env::temp_dir().join(format!(
                    "vimcode_test_1732_no_settings_{}_{:?}.json",
                    std::process::id(),
                    std::thread::current().id()
                )),
            );

            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));

            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec![
                    "ACP_FAKE_REQUEST_PERMISSION_DIFF=1".to_string(),
                    // One unbroken token, so phase 1's wait cannot match a
                    // bare "Hello" painted by anything else.
                    "ACP_FAKE_AGENT_LABEL=Perm1518".to_string(),
                ],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();
            engine.settings.acp_permission_default =
                crate::core::settings::AcpPermissionDefault::AllowAll;

            let mut h = harness(engine);
            let driver = &mut h.driver;
            driver.press_named(quadraui::NamedKey::Escape);

            driver.type_char(':');
            for c in "AI please edit".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            // Phase 1: the turn is genuinely under way — the agent's own
            // streamed greeting has painted, which the fixture emits on the
            // way to issuing `session/request_permission`.
            let greeting_deadline = Instant::now() + Duration::from_secs(30);
            let mut screen = driver.screen();
            while !screen.contains("Hello_Perm1518") && Instant::now() < greeting_deadline {
                driver.tick();
                screen = driver.screen();
                assert!(
                    !screen.contains("Tool kind:"),
                    "allow_all must never paint the permission dialog; \
                     screen:\n{screen}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                screen.contains("Hello_Perm1518"),
                "the session must start and stream the agent's reply within \
                 30s, or the dialog-absence assertions below prove nothing; \
                 screen:\n{screen}"
            );

            // Phase 2: and it settles on its own, without a human ever being
            // asked — the fixture only answers `session/prompt` with
            // `end_turn` after it has read a reply to its permission
            // request, so `ai_streaming` going false *is* the auto-answer
            // having reached the wire.
            let settle_deadline = Instant::now() + Duration::from_secs(30);
            while h.engine.borrow().acp().ai_streaming && Instant::now() < settle_deadline {
                driver.tick();
                let screen = driver.screen();
                assert!(
                    !screen.contains("Tool kind:"),
                    "allow_all must never paint the permission dialog; \
                     screen:\n{screen}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                !h.engine.borrow().acp().ai_streaming,
                "the auto-approved request must let the turn resume to \
                 completion within 30s, not hang waiting for a human who is \
                 never asked (acp_permission_default is {:?} at this point — \
                 if it is not AllowAll, a settings reload stomped it)",
                h.engine.borrow().settings.acp_permission_default
            );
            let screen = driver.screen();
            assert!(
                !screen.contains("Tool kind:"),
                "the permission dialog must never have painted at all \
                 under allow_all; final screen:\n{screen}"
            );
        }

        /// #1519 acceptance, fix 3: `session/cancel` must mark every
        /// still-unfinished tool call `Cancelled`, painted as the `[-]`
        /// glyph, not left showing `[~]` (in-progress) forever. The
        /// fixture's `$ACP_FAKE_TOOL_CALL_HANGS` announces one
        /// `in_progress` call and then never answers `session/prompt` at
        /// all, so the only way this call's painted glyph ever changes is
        /// the client-side sweep `Engine::acp_cancel_turn` does. Drives a
        /// real Ctrl+C through the driver (the same key a human would
        /// press) rather than calling `acp_cancel_turn()` directly.
        ///
        /// RED verified: with the `for call in ... is_unfinished()` sweep
        /// removed from `acp_cancel_turn`, this fails — the screen still
        /// shows `[~]`, never `[-]`, after Ctrl+C.
        #[cfg(unix)]
        #[test]
        fn cancelled_turn_paints_unfinished_tool_call_as_cancelled_glyph_via_shell_app() {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));

            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec!["ACP_FAKE_TOOL_CALL_HANGS=1".to_string()],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();

            let mut h = harness(engine);
            let driver = &mut h.driver;
            driver.press_named(quadraui::NamedKey::Escape);

            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("[~] execute: Run") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("[~] execute: Run"),
                "sanity: the in-progress tool call must paint before it can \
                 be cancelled; screen:\n{screen}"
            );

            driver.ctrl_char('c');

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("[-] execute: Run") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("[-] execute: Run"),
                "cancelling the turn must repaint the still-unfinished tool \
                 call with the Cancelled glyph within 5s; screen:\n{screen}"
            );
        }

        /// #1519 acceptance, fix 4: a mismatched `initialize` `protocolVersion`
        /// must abort the handshake and surface a warning that actually
        /// paints, not just live in `engine.message` in isolation. The
        /// fixture's `$ACP_FAKE_PROTOCOL_VERSION=99` claims a protocol
        /// version this client doesn't speak.
        ///
        /// RED verified: deleting the `if protocol_version !=
        /// PROTOCOL_VERSION` guard in `Engine::acp_dispatch_events`'s
        /// `Initialized` arm makes this fail — the screen never shows
        /// "protocol version" and instead the turn proceeds to a normal
        /// "Hello world" reply.
        #[cfg(unix)]
        #[test]
        fn protocol_version_mismatch_paints_a_refusal_warning_via_shell_app() {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));

            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec!["ACP_FAKE_PROTOCOL_VERSION=99".to_string()],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();

            let mut h = harness(engine);
            let driver = &mut h.driver;
            driver.press_named(quadraui::NamedKey::Escape);

            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("protocol version") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("protocol version"),
                "a protocolVersion mismatch must paint a refusal warning \
                 within 5s, not just set `engine.message` in isolation; \
                 screen:\n{screen}"
            );
            assert!(
                !screen.contains("Hello world"),
                "the handshake must abort before ever reaching a normal \
                 reply; screen:\n{screen}"
            );
        }

        /// #1519 acceptance, fix 5: a `session_info_update`-learned title
        /// must paint on `:AiSessions`' picker in place of the first
        /// prompt, not merely sit in `AcpSessionIndex`/`picker_all_items`
        /// unrendered. Sends a prompt whose text is deliberately different
        /// from the fixture's `$ACP_FAKE_SESSION_TITLE`, so a screen
        /// showing the title text (and not the prompt text) can only mean
        /// the title actually painted and was preferred.
        ///
        /// RED verified: with `parse_session_info_update`'s call site
        /// removed from `Engine::acp_handle_session_update`, this fails —
        /// the picker's painted row still reads the literal prompt text,
        /// never "Fix the login bug".
        #[cfg(unix)]
        #[test]
        fn session_info_update_title_paints_on_the_sessions_picker_via_shell_app() {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));

            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec![
                    "ACP_FAKE_SESSION_TITLE=Fix the login bug".to_string(),
                    "ACP_FAKE_LOAD_SESSION=1".to_string(),
                ],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();

            let mut h = harness(engine);
            let driver = &mut h.driver;
            driver.press_named(quadraui::NamedKey::Escape);

            driver.type_char(':');
            for c in "AI a typed prompt distinct from the title".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("Hello world") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("Hello world"),
                "sanity: the turn must complete within 5s before the \
                 session is recorded/titled; screen:\n{screen}"
            );

            driver.press_named(quadraui::NamedKey::Escape);
            driver.type_char(':');
            for c in "AiClear".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();
            assert!(
                !driver.screen().contains("a typed prompt"),
                ":AiClear must wipe the transcript before the picker check \
                 below can prove anything"
            );

            driver.type_char(':');
            for c in "AiSessions".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();

            let screen = driver.screen();
            assert!(
                screen.contains("Fix the login bug"),
                "the picker must show the learned title within 5s; \
                 screen:\n{screen}"
            );
            assert!(
                !screen.contains("a typed prompt"),
                "the picker must show the title INSTEAD of the first \
                 prompt, not alongside it; screen:\n{screen}"
            );
        }
    }

    mod issue_1520_acp_config_options {
        use super::*;
        use std::time::{Duration, Instant};

        /// #1520 acceptance: "session config options — :AiModel". ACP v1
        /// has no dedicated `session/set_model`; the fixture's
        /// `$ACP_FAKE_SESSION_CONFIG_OPTIONS` `category: "model"` config
        /// option is the only model picker the spec offers at all. Drives
        /// the real `:AiModel <name>` ex-command path (not a direct
        /// `Engine::acp_set_model` call) and confirms the header's
        /// "model: ..." segment only flips once the fixture's
        /// `config_option_update` notification lands — never
        /// optimistically on the `session/set_config_option` request
        /// alone. TUI's twin of `gtk::testing::sidebar_panel_clicks::
        /// ai_panel_mode_switch_round_trips_via_session_set_mode`'s own
        /// round-trip shape, for the config-option mechanism instead of
        /// `modes`.
        ///
        /// RED verified: with `Engine::acp_handle_session_update`'s
        /// `config_option_update` dispatch arm deleted (so the
        /// notification is silently dropped as an unrecognized update
        /// kind, the same "forward-compatible no-op" fate an unknown kind
        /// already gets), this test fails — the header stays on "model:
        /// Claude Sonnet" forever even though the fixture did reply to
        /// `session/set_config_option` and did emit the notification.
        #[cfg(unix)]
        #[test]
        fn ai_model_switch_round_trips_via_session_set_config_option_via_shell_app() {
            let mut engine = plain_engine();
            // See `acp_mcp_servers`'s sibling tests for why this must run
            // before the fixture agent is configured.
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));

            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec![
                    "ACP_FAKE_SESSION_CONFIG_OPTIONS=1".to_string(),
                    "ACP_FAKE_NO_TOOL_REQUEST=1".to_string(),
                ],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();

            // The default 20-cell sidebar width clips the
            // status header's own text mid-word (confirmed while writing
            // this test — "model: Claude Sonnet" reads as truncated "· m"
            // at the default width), the exact same clipping
            // `ai_panel_harness_widened`'s own doc names for the
            // send/stop/leave hint. Widened the same way: a wider
            // terminal plus 40 real `Alt+Right` "resize sidebar" presses
            // (`render::alt_resized_sidebar_width`) rather than a bespoke
            // fixed-width workaround.
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            let driver = &mut h.driver;
            driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }

            // Drive the full handshake (spawn -> initialize -> session/new
            // -> session/prompt) via a real sent message, the same
            // ":AI <text>" entry point `acp_mcp_servers`/
            // `issue_1519_acp_conformance`'s tests above use — the
            // fixture's `configOptions` only ever lands on the
            // `session/new` response, so there is no earlier trigger to
            // drive this from.
            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("Hello world") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("Hello world"),
                "sanity: the turn must complete within 5s before the \
                 model round trip below can prove anything; \
                 screen:\n{screen}"
            );
            assert!(
                screen.contains("model: Claude Sonnet"),
                "session/new's configOptions' current value must render \
                 in the status header once the handshake completes; \
                 screen:\n{screen}"
            );

            // The real ex-command path, not a direct `acp_set_model` call.
            // `Escape` first: the chat input still has keyboard focus after
            // submitting the first message, so ':' would otherwise be typed
            // as literal chat text instead of opening the command line —
            // the same leave-chat-focus step
            // `session_info_update_title_paints_on_the_sessions_picker_
            // via_shell_app` above takes before its own second `:` command.
            driver.press_named(quadraui::NamedKey::Escape);
            driver.type_char(':');
            for c in "AiModel opus".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();
            assert!(
                driver.screen().contains("model: Claude Sonnet"),
                "the displayed model must NOT change optimistically just \
                 because the request was sent -- only \
                 config_option_update may change it"
            );

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("model: Claude Opus") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("model: Claude Opus"),
                "the fixture's config_option_update notification must \
                 flip the displayed model within 5s of the \
                 session/set_config_option round trip; screen:\n{screen}"
            );
        }
    }

    mod app_on_tui_seam {
        use crate::tui_main::testing::tui_driver_with;

        /// Same marker `seam_657.rs` uses: cannot occur in a restored
        /// session, a settings file, or any chrome vimcode paints, so
        /// finding it on screen can only mean the frame rendered the
        /// buffer seeded via [`tui_driver_with`]'s `setup` hook.
        const MARKER: &str = "SEAM657MARKER";

        /// `tui_driver(None, 80, 24)` → `render()` → the screen has exactly
        /// 24 rows and is not blank — the `tui_driver` twin of
        /// `seam_657.rs`'s `tui_backend_paints_a_full_frame_from_an_
        /// integration_test`.
        #[test]
        fn tui_driver_paints_a_full_frame() {
            let mut driver = crate::tui_main::testing::tui_driver(None, 80, 24);
            driver.render();
            let screen = driver.screen();

            let rows: Vec<&str> = screen.lines().collect();
            assert_eq!(
                rows.len(),
                24,
                "expected a 24-row frame, got {}:\n{screen}",
                rows.len()
            );
            assert!(
                screen.chars().any(|c| !c.is_whitespace()),
                "the frame painted nothing at all:\n{screen}"
            );
        }

        /// Seeding `SEAM657MARKER\n` at buffer offset 0 with every window's
        /// scroll pinned to 0, at 120x24, must reach the painted character
        /// grid — the `tui_driver_with` twin of `seam_657.rs`'s
        /// `tui_backend_paints_seeded_buffer_text`.
        #[test]
        fn tui_driver_with_paints_seeded_buffer_text() {
            let mut driver = tui_driver_with(None, 120, 24, |engine| {
                engine.buffer_mut().insert(0, &format!("{MARKER}\n"));
                for window in engine.windows.values_mut() {
                    window.view.scroll_top = 0;
                }
            });
            driver.render();

            assert!(
                driver.screen_contains(MARKER),
                "seeded buffer text never reached the character grid:\n{}",
                driver.screen()
            );
        }

        /// `Engine::new()` seeds its initial `explorer_rows` cache from
        /// `std::env::current_dir()` before any CLI-supplied path is
        /// applied; `build_portable_engine`'s `Engine::new()` →
        /// `e.startup(file_path)` ordering is exactly what every real
        /// backend runs, and `tui_driver_with` drives that same ordering,
        /// so this exercises the actual startup path rather than poking
        /// `explorer_rows`/`explorer_expanded` directly the way other
        /// sidebar tests in this file do.
        #[test]
        fn tui_driver_opening_a_directory_shows_that_directorys_contents_not_the_process_cwd() {
            let dir = std::env::temp_dir().join(format!(
                "vc1924expl_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            // Short name, same reasoning as `render_content_paints_
            // explorer_sidebar_content_via_shell_app`'s `mk1425.txt`: the
            // narrow sidebar at this module's `(80, 24)` size truncates a
            // longer filename before `screen_contains` would ever see it.
            std::fs::write(dir.join("mk1924.txt"), b"").unwrap();

            let mut driver = tui_driver_with(Some(dir.clone()), 80, 24, |engine| {
                engine.session.explorer_visible = true;
                engine.app_shell.show_panel(&quadraui::WidgetId::new(
                    crate::core::engine::sidebar::PANEL_EXPLORER,
                ));
            });
            driver.render();

            assert!(
                driver.screen_contains("mk1924.txt"),
                "opening a directory on the command line must show that \
                 directory's own contents in the Explorer on the very \
                 first rendered frame, not whatever directory the \
                 process happened to start in; screen:\n{}",
                driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    mod file_encoding {
        use super::*;

        use crate::core::Engine;

        /// Opening a UTF-16LE-with-BOM file (Notepad's/PowerShell's
        /// "Unicode" default on Windows) through `Engine::open` — the exact
        /// fixed read path (`Buffer::from_file` → `decode_file_bytes`) —
        /// must paint the decoded text on screen, with no trace of the raw
        /// decode error.
        #[test]
        fn opening_utf16le_bom_file_shows_decoded_content_via_shell_app() {
            let path = std::env::temp_dir().join(format!(
                "vimcode_test_1560_utf16le_{}_{:?}.txt",
                std::process::id(),
                std::thread::current().id()
            ));
            let mut bytes: Vec<u8> = vec![0xFF, 0xFE]; // UTF-16LE BOM
            for unit in "UTF16MARKER98431".encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            std::fs::write(&path, &bytes).unwrap();

            let engine = Engine::open(&path);
            let h = crate::tui_main::testing::conformance_harness(engine, 80, 24);

            assert!(
                !h.driver.screen_has("stream did not contain valid UTF-8"),
                "must not surface the raw decode error on screen:\n{}",
                h.driver.screen()
            );
            assert!(
                h.driver.screen_has("UTF16MARKER98431"),
                "the decoded UTF-16LE-BOM file content must reach the \
                 painted character grid; screen:\n{}",
                h.driver.screen()
            );

            let _ = std::fs::remove_file(&path);
        }

        /// Mirror of the above for a UTF-8-with-BOM file.
        #[test]
        fn opening_utf8_bom_file_shows_decoded_content_via_shell_app() {
            let path = std::env::temp_dir().join(format!(
                "vimcode_test_1560_utf8bom_{}_{:?}.txt",
                std::process::id(),
                std::thread::current().id()
            ));
            let mut bytes: Vec<u8> = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
            bytes.extend_from_slice(b"UTF8BOMMARKER24601");
            std::fs::write(&path, &bytes).unwrap();

            let engine = Engine::open(&path);
            let h = crate::tui_main::testing::conformance_harness(engine, 80, 24);

            assert!(
                !h.driver.screen_has("stream did not contain valid UTF-8"),
                "must not surface the raw decode error on screen:\n{}",
                h.driver.screen()
            );
            assert!(
                h.driver.screen_has("UTF8BOMMARKER24601"),
                "the decoded UTF-8-BOM file content must reach the painted \
                 character grid; screen:\n{}",
                h.driver.screen()
            );

            let _ = std::fs::remove_file(&path);
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // Idle-stability — comment text must not flash, and an idle
    // frame must not repaint at all when nothing render-relevant changed.
    //
    // Root cause: `Engine::poll_lsp`'s `WorkProgressBegin`/`WorkProgressReport`/
    // `WorkProgressEnd` arms (`src/core/engine/panels.rs`) unconditionally set
    // `redraw = true` on *any* `$/progress` notification, even a server's
    // byte-identical repeat of a payload it already reported. A server that
    // is confused about its workspace root (rust-analyzer, pointed at a
    // buffer with no enclosing Cargo project — exactly what surfaced this
    // during investigation) can emit such repeats indefinitely, at roughly
    // the ~1Hz cadence LSP servers commonly throttle `$/progress` to —
    // forcing a full repaint every tick forever, not just during genuine
    // indexing. `LspManager::work_progress_begin/report/end` now return
    // `bool` ("did the stored snapshot actually change") and
    // `Engine::poll_lsp` only requests a redraw when one of them does.
    //
    // This test drives the exact production path a real server's messages
    // take (`LspManager::poll_events` → `Engine::poll_lsp`'s event match)
    // via `LspManager::test_send_event` (a `#[cfg(test)]`-only seam that
    // pushes an `LspEvent` through the same channel a spawned server
    // writes to) rather than calling `work_progress_report` etc. directly,
    // which would bypass `poll_lsp`'s redraw decision entirely and prove
    // nothing about the user-visible symptom (repaint / flashing).
    //
    // RED against unfixed develop: reverting just the
    // `work_progress_begin`/`report`/`end` return-value + call-site change
    // (keeping this test) turns every iteration's `Reaction::Continue`
    // assertion into an observed `Reaction::Redraw` — confirmed by hand
    // before writing this comment.
    //
    // Scope note (review, iteration 1): this test independently confirms
    // (see the failure-collecting loop below, which checks all three
    // signals every tick rather than short-circuiting on the first
    // mismatch) that the unfixed handlers force a wasted repaint on every
    // identical `$/progress` repeat — the redraw-storm / "app feels slow"
    // mechanism is real and closed. It does **not** independently
    // demonstrate that this specific redraw storm is what produces the
    // *reported* comment fg colour alternating white/grey: reverting the
    // fix and re-running this test (done by hand for this note) shows
    // `Reaction::Redraw` every tick but `screen()`/`style_at()` unchanged
    // frame-to-frame in this synthetic scenario — i.e. the backend
    // faithfully repaints the *same* colours it already had, it just does
    // so wastefully. The colour-divergence mechanism the issue describes
    // (a frame painting plain `fg` before the comment scope colour comes
    // back, or two highlight sources disagreeing) is therefore still
    // unconfirmed by any test in this repo; only the wasted-redraw/
    // performance half of the bug report is verified fixed here. Treat
    // "comment text flashes" as reproduced-but-not-yet-root-caused if it
    // resurfaces after this fix ships.
    mod idle_stability_1583 {
        use super::*;
        use crate::core::lsp::LspEvent;
        use crate::core::lsp_manager::LspManager;
        use crate::core::Engine;
        use quadraui::Reaction;

        /// Open a comment-heavy buffer with LSP auto-start disabled (so the
        /// test never depends on a real language server binary existing on
        /// the machine), in an isolated `$HOME` (so a real
        /// `~/.config/vimcode/settings.json` on a shared dev machine can't
        /// leak in mid-test — see `core::paths::set_test_home`'s own doc for
        /// why a thread-local override, not `std::env::set_var`, is
        /// required here).
        fn open_comment_heavy_buffer() -> (
            crate::test_paint::PaintGuard,
            crate::test_cwd::CwdReadGuard,
            crate::core::paths::TestHomeGuard,
            std::path::PathBuf,
            Engine,
        ) {
            let paint = crate::test_paint::PaintGuard::acquire();
            let cwd = crate::test_cwd::CwdReadGuard::acquire();
            let home = std::env::temp_dir().join(format!(
                "vimcode_test_1583_home_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&home);
            std::fs::create_dir_all(&home).unwrap();
            let home_guard = crate::core::paths::set_test_home(&home);

            let path = std::env::temp_dir().join(format!(
                "vimcode_test_1583_{}_{:?}.rs",
                std::process::id(),
                std::thread::current().id()
            ));
            let mut text = String::new();
            for i in 0..20 {
                text.push_str(&format!("// this is a documentation comment line {i}\n"));
            }
            text.push_str("fn main() {}\n");
            std::fs::write(&path, &text).unwrap();

            let mut engine = Engine::new_for_test();
            engine.settings.lsp_enabled = false;
            let old_id = engine.active_buffer_id();
            let _ = engine.buffer_manager.delete(old_id, true);
            let buffer_id = engine.buffer_manager.open_file(&path).unwrap();
            engine
                .buffer_manager
                .apply_language_map(buffer_id, &engine.settings.language_map);
            if let Some(window) = engine.windows.get_mut(&engine.active_window_id()) {
                window.buffer_id = buffer_id;
            }
            let view = engine.restore_file_position(buffer_id);
            if let Some(window) = engine.windows.get_mut(&engine.active_window_id()) {
                window.view = view;
            }
            engine.plugin_init();

            (paint, cwd, home_guard, path, engine)
        }

        #[test]
        fn idle_ticks_with_repeated_lsp_progress_do_not_repaint_or_change_colors() {
            let (_paint, _cwd, _home, path, mut engine) = open_comment_heavy_buffer();

            // Install a manager the way `Engine::ensure_lsp_manager` would,
            // without spawning any real server — this test drives its event
            // channel directly via `LspManager::test_send_event`.
            engine.lsp_manager = Some(LspManager::new(std::env::temp_dir(), &[]));

            let h = crate::tui_main::testing::conformance_harness(engine, 80, 24);
            // `ConformanceHarness::engine` is the same `Rc<RefCell<Engine>>`
            // the driver's `App` holds (`conformance_harness`'s own body
            // clones it into both before moving one half into the driver),
            // so sending an event on `engine_rc.borrow().lsp_manager` here
            // is visible to the exact `Engine::poll_lsp` call
            // `driver.tick()` below drives.
            let engine_rc = h.engine.clone();
            let mut driver = h.driver;

            let (cx, cy) = driver
                .find("documentation comment line 0")
                .expect("comment text must be on screen");
            let (cx, cy) = (cx.round() as u16, cy.round() as u16);

            // A genuine first progress notification IS a real change (a
            // brand-new token) — settle past it before asserting stability,
            // per this test class's "every frame after the first settle"
            // contract.
            engine_rc
                .borrow()
                .lsp_manager
                .as_ref()
                .expect("manager installed above")
                .test_send_event(LspEvent::WorkProgressBegin {
                    server_id: 0,
                    token: "indexing".to_string(),
                    title: Some("Indexing".to_string()),
                    message: Some("1/10".to_string()),
                    percentage: Some(10),
                });
            driver.tick();

            let screen0 = driver.screen();
            let style0 = driver.style_at(cx, cy);
            assert!(
                screen0.contains("documentation comment line 0"),
                "settle frame must still show the comment text:\n{screen0}"
            );

            // Now repeat the *exact same* progress payload — a chatty (or
            // workspace-confused) server re-sending an unchanged `$/progress`
            // report, the shape that forced a redraw every idle tick before
            // this fix. Interleave real sleeps so this also crosses the
            // syntax-debounce (150ms) and idle-file-check (2s) windows —
            // the "several ticks, enough to cross every periodic timer"
            // the idle-stability contract asks for.
            //
            // Review fix (#1583 iter 1): capture all three signals
            // (reaction, screen, cell style) *independently* every tick
            // rather than `assert_eq!`-ing them one after another, so a
            // pre-fix run's reaction failure can't short-circuit the loop
            // before the colour check for that same tick ever runs. Without
            // this, "confirmed RED" only ever demonstrated the wasted
            // repaint (`Reaction::Redraw`), not the reported symptom itself
            // (the comment fg actually alternating colour) — the two are
            // logically distinct claims and this loop now checks both on
            // every iteration, panicking with the full picture at the end
            // rather than on the first mismatch.
            let mut failures: Vec<String> = Vec::new();
            for n in 0..8 {
                engine_rc
                    .borrow()
                    .lsp_manager
                    .as_ref()
                    .expect("manager installed above")
                    .test_send_event(LspEvent::WorkProgressReport {
                        server_id: 0,
                        token: "indexing".to_string(),
                        message: Some("1/10".to_string()),
                        percentage: Some(10),
                    });
                std::thread::sleep(std::time::Duration::from_millis(300));
                let reaction = driver.tick();
                let screen_n = driver.screen();
                let style_n = driver.style_at(cx, cy);

                if reaction != Reaction::Continue {
                    failures.push(format!(
                        "tick {n}: an unchanged $/progress repeat must not force a repaint \
                         (this is the #1583 root cause: WorkProgress* handlers used to set \
                         redraw=true unconditionally) — got {reaction:?}"
                    ));
                }
                if screen_n != screen0 {
                    failures.push(format!(
                        "tick {n}: rendered text must not change with no input"
                    ));
                }
                if style_n != style0 {
                    failures.push(format!(
                        "tick {n}: comment cell fg/bg must not flash between frames \
                         (settle style {style0:?}, this tick {style_n:?})"
                    ));
                }
            }
            assert!(
                failures.is_empty(),
                "idle-stability violated:\n{}",
                failures.join("\n")
            );

            let _ = std::fs::remove_file(&path);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Idle stability with the Explorer sidebar open
    // ─────────────────────────────────────────────────────────────────────────
    /// The sidebar-open sibling of [`idle_stability_1583`]. That
    /// module's fixture has no sidebar, so it never crossed
    /// `render::run_shared_tick_chores`'s *other* periodic timer: the 2-second
    /// source-control auto-refresh, gated only on
    /// `engine.app_shell.sidebar_visible()` — true in the default startup
    /// state. Before this fix that block set `needs_redraw = true`
    /// unconditionally on every kickoff, *and* `Engine::poll_sc_refresh`
    /// reported "changed" unconditionally on every snapshot arrival, so an
    /// idle `vcd` forced a real `ratatui::Terminal::draw` every ~2 seconds
    /// forever — and `ratatui-crossterm`'s `CrosstermBackend::draw`/
    /// `hide_cursor` emit an SGR-reset + cursor-hide escape burst on every
    /// `draw` even for a zero-cell diff, which is the non-silent idle byte
    /// stream `tests/smoke-spec/tui.yaml`'s `idle-truly-silent` step caught.
    ///
    /// Driver-tier, not state-tier: the assertions below read the
    /// `Reaction` the driver's own tick returns (the signal the runner's
    /// `needs_redraw` gate turns into a real `draw` call, i.e. into the
    /// escape burst) plus the painted screen text, exactly the two signals
    /// [`idle_stability_1583`] asserts on. Neither is "an engine field got
    /// populated".
    ///
    /// RED-verified against unfixed `develop`: with *either* half of the fix
    /// reverted — `needs_redraw = true` restored in
    /// `render::run_shared_tick_chores`'s sidebar block, or
    /// `Engine::poll_sc_refresh`'s `changed` computation replaced by a bare
    /// `true` — [`idle_ticks_with_explorer_sidebar_open_do_not_repaint`]
    /// fails with `an idle tick must not force a repaint` entries for the
    /// ticks that land on the 2-second boundary. Both were reverted, observed
    /// red, and restored before committing.
    mod idle_stability_1650 {
        use super::*;
        use quadraui::Reaction;

        /// Explorer sidebar open on a scratch directory containing one
        /// marker file.
        ///
        /// Primes the source-control cache *synchronously*
        /// (`Engine::sc_refresh`) before the harness is built, so the
        /// snapshots the periodic `sc_refresh_async` delivers during the
        /// observation window are identical to what is already cached from
        /// the very first arrival — without this, the first arrival would
        /// legitimately differ from an empty cache (and legitimately
        /// repaint), and whether it did would depend on whether the
        /// machine's temp dir happens to sit inside a git repo.
        fn engine_with_explorer_sidebar_open(tag: &str) -> crate::core::Engine {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1650_{tag}_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("marker.txt"), "hello").unwrap();

            let mut engine = plain_engine();
            engine.cwd = dir.clone();
            engine.explorer_expanded.insert(dir.clone());
            engine.explorer_rebuild_rows();
            engine.session.explorer_visible = true;
            // `session.explorer_visible` alone leaves the shadow
            // `engine.app_shell`'s `sidebar_visible()` stale, and
            // `render::sync_runner_sidebar_visibility` would then collapse
            // the sidebar on the first dispatch — which would silently turn
            // this test into a no-sidebar rerun of `idle_stability_1583`.
            // The painted-sidebar precondition below is what keeps that
            // honest.
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_EXPLORER,
            ));
            engine.sc_refresh();
            engine
        }

        #[test]
        fn idle_ticks_with_explorer_sidebar_open_do_not_repaint() {
            let mut h = engine_with_explorer_sidebar_open("idle");
            h.settings.lsp_enabled = false;
            let mut h = harness(h);
            let driver = &mut h.driver;

            // Settle past the startup paint before asserting stability —
            // same contract as `idle_stability_1583`'s own settle frame.
            for _ in 0..3 {
                driver.tick();
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            let screen0 = driver.screen();
            assert!(
                driver.screen_contains("marker"),
                "precondition: the Explorer sidebar must actually be painted, \
                 otherwise this test silently degrades into a no-sidebar rerun \
                 of `idle_stability_1583` and can never see #1650's 2s \
                 source-control tick at all; screen:\n{screen0}"
            );

            // Observe across several 2-second `run_shared_tick_chores`
            // source-control auto-refresh boundaries (#1650's own issue text
            // asks for a ≥10s idle window on the real-pty side). #1702's
            // real-pty bugbash run caught the exact pre-#1650 symptom —
            // paired SGR-reset/hide-cursor bursts repeating every ~2.007s —
            // persisting for "10+ seconds straight" before 9+ follow-up
            // attempts (including fully isolated fresh-HOME runs) failed to
            // reproduce it again; root cause was never pinned down beyond
            // "matches the exact mechanism #1650 already fixed here"
            // (confirmed by re-reading this function and `poll_sc_refresh`:
            // neither has regressed since). 40 ticks * 300ms ≈ 12s covers
            // six full 2s cycles, comfortably past #1702's observed window,
            // in case a cycle-count-dependent drift exists that the
            // original 5.4s/two-cycle window couldn't see.
            let mut failures: Vec<String> = Vec::new();
            for n in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let reaction = driver.tick();
                let screen_n = driver.screen();

                if reaction != Reaction::Continue {
                    failures.push(format!(
                        "tick {n}: an idle tick must not force a repaint with the \
                         Explorer sidebar open and nothing on disk changed (#1650: \
                         `run_shared_tick_chores`'s 2s source-control refresh used \
                         to set needs_redraw unconditionally, and \
                         `Engine::poll_sc_refresh` used to report changed \
                         unconditionally) — got {reaction:?}"
                    ));
                }
                if screen_n != screen0 {
                    failures.push(format!(
                        "tick {n}: rendered text must not change with no input"
                    ));
                }
            }
            assert!(
                failures.is_empty(),
                "idle-stability violated:\n{}",
                failures.join("\n")
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Idle stability with several tabs open
    // ─────────────────────────────────────────────────────────────────────────
    /// #1722's own acceptance bar: "a headless `App` with multiple tabs open and no input must
    /// return `Reaction::Continue` from repeated `tick()` calls once settled." Sibling of
    /// [`idle_stability_1583`]/[`idle_stability_1650`] — same shape (settle, snapshot,
    /// tick-and-compare in a loop), this time with six tabs open on the unsplit tab bar. It serves
    /// as a black-box regression guard for multi-tab idle stability in the TUI backend, even though
    /// the specific culprit found in other backends (`Engine::post_draw_apply_widths` forcing a
    /// redraw on a cosmetic-only tab-bar width change) cannot be reproduced here (TUI's char-cell
    /// widths are exact integers that never wobble).
    mod idle_stability_1722 {
        use super::*;
        use quadraui::Reaction;

        #[test]
        fn idle_ticks_with_six_tabs_open_do_not_repaint() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1722_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let mut engine = plain_engine();
            for i in 0..6 {
                let p = dir.join(format!("tab_number_{i}.txt"));
                std::fs::write(&p, "hello\n").unwrap();
                engine.new_tab(Some(&p));
            }
            engine.settings.lsp_enabled = false;
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // Settle past the startup paint before asserting stability —
            // same contract [`idle_stability_1583`]/[`idle_stability_1650`]
            // use.
            for _ in 0..3 {
                driver.tick();
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let screen0 = driver.screen();
            assert!(
                screen0.contains("tab_number_5.txt"),
                "precondition: the sixth tab must actually be painted, or \
                 this test silently degrades into a one-tab rerun of \
                 idle_stability_1583; screen:\n{screen0}"
            );

            let mut failures: Vec<String> = Vec::new();
            for n in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let reaction = driver.tick();
                let screen_n = driver.screen();
                if reaction != Reaction::Continue {
                    failures.push(format!(
                        "tick {n}: an idle tick must not force a repaint \
                         with six tabs open and nothing changed (#1722) — \
                         got {reaction:?}"
                    ));
                }
                if screen_n != screen0 {
                    failures.push(format!(
                        "tick {n}: rendered text must not change with no input"
                    ));
                }
            }
            assert!(
                failures.is_empty(),
                "idle-stability violated:\n{}",
                failures.join("\n")
            );
        }
    }

    /// A second real-pty bugbash run reported the *exact* #1650/#1702
    /// byte pattern (paired SGR-reset + hide-cursor bursts roughly every 2s,
    /// persisting "at least 10 more seconds") — this time caught by
    /// `tests/smoke-spec/tui.yaml`'s `idle-truly-silent` *control* step,
    /// which runs with the Explorer/Git sidebar **closed** (the default
    /// startup state) before any panel click ever happens. That is a
    /// scenario [`idle_stability_1650`] does not cover (it opens the
    /// sidebar specifically to exercise #1650's 2s source-control
    /// auto-refresh), and [`idle_stability_1583`] only observes for ~2.4s
    /// (8 ticks * 300ms) — far short of the "10+ seconds straight" both
    /// #1702's and #1737's bugbash reports describe.
    ///
    /// Direct investigation for this PR: built this branch's real `vcd`
    /// binary and drove it under a real OS pty (`pty.fork()`/`os.openpty`,
    /// not the coord harness) for 8 consecutive 16-second idle
    /// observations, zero input, default settings, no sidebar. All 8 were
    /// silent after startup settled, except for exactly one spurious
    /// repaint ~250ms after the first frame in every run — traced (temporary
    /// `std::env::var_os`-gated instrumentation in `Engine::poll_idle` and
    /// `render::run_shared_tick_chores`, removed before this commit) to
    /// `Engine::poll_ext_registry` consuming the startup extension-registry
    /// fetch's result on the first `poll_idle` tick after the first paint.
    /// That is a real, single state change (the registry genuinely arrived)
    /// correctly producing exactly one redraw — not a bug — but it does
    /// explain the *shape* of #1737's own evidence capture: its first
    /// logged write (`t=4.496s`, alone, no partner) is a lone burst,
    /// consistent with this mechanism. The *recurring* pairs reported
    /// afterward were never reproduced in any of those 8 runs, matching
    /// #1702's own "9+ follow-up attempts ... failed to reproduce"
    /// conclusion for the sidebar-open sibling.
    ///
    /// No production code change (same verdict #1702 reached for the
    /// sidebar-open case): every 2-second periodic chore
    /// `render::run_shared_tick_chores`/`Engine::poll_idle` runs
    /// (`check_file_changes`, `tick_git_branch`, the source-control
    /// auto-refresh gate) gates its redraw on an actual before/after
    /// difference, confirmed by direct re-read for this PR. This test
    /// widens coverage of the *no-sidebar* idle scenario #1737's own
    /// reproduction steps describe to the same ~12s/40-tick duration #1702
    /// already applied to the sidebar-open sibling
    /// ([`idle_stability_1650`]), so a future regression in either scenario
    /// has a comparable chance of being caught here before it ever reaches
    /// a real-pty bugbash again.
    mod idle_stability_1737 {
        use super::*;
        use quadraui::Reaction;

        #[test]
        fn idle_ticks_with_no_sidebar_and_zero_interaction_do_not_repaint() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "alpha\nbeta\ngamma\n");
            engine.settings.lsp_enabled = false;
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // Settle past the startup paint (and the one legitimate
            // extension-registry-fetch redraw documented above) before
            // asserting stability — same contract every sibling in this
            // family uses.
            for _ in 0..3 {
                driver.tick();
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let screen0 = driver.screen();
            assert!(
                screen0.contains("alpha"),
                "precondition: the buffer text must actually be painted; screen:\n{screen0}"
            );

            // #1702/#1737 both report the recurring pattern persisting "10+
            // seconds straight" — 40 ticks * 300ms ≈ 12s, matching
            // idle_stability_1650's own widened window, covers six full 2s
            // cycles of every periodic chore in `Engine::poll_idle`.
            let mut failures: Vec<String> = Vec::new();
            for n in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let reaction = driver.tick();
                let screen_n = driver.screen();
                if reaction != Reaction::Continue {
                    failures.push(format!(
                        "tick {n}: an idle tick with no sidebar and zero \
                         interaction must not force a repaint (#1737) — \
                         got {reaction:?}"
                    ));
                }
                if screen_n != screen0 {
                    failures.push(format!(
                        "tick {n}: rendered text must not change with no input"
                    ));
                }
            }
            assert!(
                failures.is_empty(),
                "idle-stability violated:\n{}",
                failures.join("\n")
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Mechanism documentation: a silence check with no settle margin
    // observes the ext-registry fetch's one post-first-paint repaint; one
    // that settles past it first does not
    // ─────────────────────────────────────────────────────────────────────────
    /// `tests/smoke-spec/tui.yaml`'s Tier-2 `idle-truly-silent`
    /// control failed on 3 consecutive real-pty runs (2168-2377 bytes
    /// received during the supposedly-silent 3s window) because it started
    /// measuring immediately after the first real frame painted, with no
    /// settle margin for `Engine::ext_refresh`'s background
    /// extension-registry fetch — spawned unconditionally at real `vcd`
    /// startup, never by the deterministic test entry points (see
    /// [`super::idle_stability_1737`]'s module doc for the identical
    /// mechanism, traced there to `Engine::poll_ext_registry` consuming the
    /// fetch result on the first `poll_idle` tick after first paint) — to
    /// land and produce its one legitimate repaint. This PR's actual fix is
    /// to the Tier-2 YAML: a `wait_idle` settle step
    /// (`settle-ext-registry-fetch-1741`) between first-paint and the
    /// silence check, in `tests/smoke-spec/tui.yaml`.
    ///
    /// **This is a characterisation / mechanism-documentation test, not a
    /// regression guard for #1741.** The real, failing-first oracle for
    /// #1741 is the pre-existing Tier-2 `idle-truly-silent` step itself,
    /// which failed 3/3 real-pty runs before this PR's `wait_idle` step was
    /// inserted and is expected to stay green now that it has settle margin.
    /// This in-process test cannot play that role: it never calls the real
    /// `Engine::ext_refresh()` (the fetch here is armed by hand — see
    /// [`arm_simulated_registry_fetch`] — not a real network call), and its
    /// "zero settle" half deliberately asserts that the fetch's repaint *is*
    /// observed, so it is green both on unfixed `develop` and after this PR.
    /// A regression that reinstated the Tier-2 step without the settle
    /// margin would **not** turn this test red.
    ///
    /// What it does verify, directly against the real, unmodified
    /// `Engine::poll_ext_registry`/`ext_registry_rx` plumbing: (1) a check
    /// that starts measuring immediately after first paint does observe the
    /// fetch's repaint (confirms the mechanism #1741's root cause describes
    /// is real), and (2) a check that first settles past the fetch's delay
    /// does not (confirms the shape of the YAML fix is sound). The two
    /// halves each arm their own, independent simulated fetch, so the
    /// second half's "settle, then measure" isn't just re-observing the
    /// first half's already-drained state.
    mod idle_silence_settle_1741 {
        use super::*;
        use quadraui::Reaction;

        /// How long the simulated background fetch takes to resolve after
        /// being armed — long enough that a zero-settle poll immediately
        /// after first paint reliably has not consumed it yet (so the
        /// "fetch still in flight, about to repaint" window this test
        /// exercises is real, not a race it happens to win), short enough
        /// to keep the test fast.
        const SIMULATED_FETCH_DELAY: std::time::Duration = std::time::Duration::from_millis(120);

        /// Arms a hand-driven stand-in for `Engine::ext_refresh`'s
        /// background registry fetch: spawns a thread that sleeps
        /// [`SIMULATED_FETCH_DELAY`] then sends an empty, successful
        /// result through the exact same channel plumbing `ext_refresh`
        /// itself sets up (`ext_registry_rx`/`ext_registry_fetching`), so
        /// `Engine::poll_ext_registry` — the real, unmodified production
        /// code — consumes it exactly as it would a real fetch that
        /// happened to resolve this fast. No real network I/O.
        ///
        /// The caller must hold a `crate::core::paths::TestHomeGuard` for
        /// the engine's lifetime: the `Some(entries)` branch this always
        /// sends is consumed by `Engine::poll_ext_registry`'s production
        /// code unmodified, which calls `registry::save_cache(&entries)` →
        /// `paths::vimcode_config_dir()`. Without the override that writes
        /// `[]` over the *real* `~/.config/vimcode/registry_cache.json` on
        /// whatever machine runs the suite — see [`zero_settle_after_
        /// first_paint_sees_the_fetch_repaint_but_a_settled_check_does_not`]
        /// for where the guard is taken.
        fn arm_simulated_registry_fetch(engine: &mut crate::core::Engine) {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                std::thread::sleep(SIMULATED_FETCH_DELAY);
                let _ = tx.send(Some(Vec::new()));
            });
            engine.ext_registry_rx = Some(rx);
            engine.ext_registry_fetching = true;
        }

        #[test]
        fn zero_settle_after_first_paint_sees_the_fetch_repaint_but_a_settled_check_does_not() {
            // Hermetic: `arm_simulated_registry_fetch`'s resolved fetch is
            // consumed by real `registry::save_cache` code, which must not
            // be allowed to touch the real `~/.config/vimcode/` on whatever
            // machine runs this suite.
            let home = std::env::temp_dir().join(format!(
                "vimcode_test_1741_home_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&home);
            std::fs::create_dir_all(&home).unwrap();
            let _home_guard = crate::core::paths::set_test_home(&home);

            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "alpha\nbeta\ngamma\n");
            engine.settings.lsp_enabled = false;
            let mut h = harness_no_sidebar(engine);

            // First real frame — mirrors the real startup order #1741
            // traces: `ext_refresh()` is already in flight by the time the
            // first frame paints.
            h.driver.tick();
            assert!(
                h.driver.screen_has("alpha"),
                "precondition: the buffer text must actually be painted"
            );

            // Half 1 ("zero settle", mirrors #1741's real-pty failure):
            // arm a fetch and poll for "silence" starting immediately. The
            // fetch resolves partway through and `Engine::poll_ext_registry`
            // reports a real change, so this must observe the fetch's own
            // status message specifically (not just "some repaint
            // happened" — matches the repo's "assert on rendered output"
            // bar) well before this loop's own budget, several multiples
            // of `SIMULATED_FETCH_DELAY`, runs out.
            arm_simulated_registry_fetch(&mut h.engine.borrow_mut());
            let mut saw_fetch_repaint = false;
            let start = std::time::Instant::now();
            while start.elapsed() < SIMULATED_FETCH_DELAY * 3 {
                std::thread::sleep(std::time::Duration::from_millis(20));
                h.driver.tick();
                if h.driver.screen_has("Extension registry updated") {
                    saw_fetch_repaint = true;
                    break;
                }
            }
            assert!(
                saw_fetch_repaint,
                "a silence check with zero settle margin after first paint \
                 must observe the extension-registry fetch's one legitimate \
                 repaint (#1741) — its status message was never painted \
                 within the budget; either the simulated fetch never \
                 resolved or this test's own timing assumption is stale"
            );

            // Half 2 ("settled", mirrors this PR's `wait_idle` fix): arm a
            // *second*, independent fetch (half 1 already consumed its
            // own, so re-arming keeps the two halves from measuring the
            // exact same already-drained state), settle past its delay
            // first, *then* measure for silence — exactly what the Tier-2
            // `settle-ext-registry-fetch-1741` step does before
            // `idle-truly-silent` in `tests/smoke-spec/tui.yaml`.
            arm_simulated_registry_fetch(&mut h.engine.borrow_mut());
            std::thread::sleep(SIMULATED_FETCH_DELAY * 2);
            h.driver.tick(); // drains this second fetch's own repaint
            assert!(
                h.driver.screen_has("Extension registry updated"),
                "precondition: settling past the delay must have let the \
                 second simulated fetch resolve and repaint"
            );
            let screen1 = h.driver.screen();
            let mut failures: Vec<String> = Vec::new();
            for n in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let reaction = h.driver.tick();
                if reaction != Reaction::Continue {
                    failures.push(format!(
                        "tick {n}: unexpected repaint after settling past \
                         the fetch — got {reaction:?}"
                    ));
                }
                if h.driver.screen() != screen1 {
                    failures.push(format!(
                        "tick {n}: rendered text changed after settling past the fetch"
                    ));
                }
            }
            assert!(
                failures.is_empty(),
                "idle-silence should hold once settled past the fetch's \
                 own one-time repaint:\n{}",
                failures.join("\n")
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // The startup extension-registry refresh must never surface a
    // status message or force a repaint at all, no matter how long its
    // background fetch takes
    // ─────────────────────────────────────────────────────────────────────────
    /// #1741 treated the startup registry fetch's post-first-paint repaint as a single, legitimate,
    /// unavoidable event and widened the Tier-2 YAML's settle margin to absorb it. A real-pty
    /// bugbash run later caught the same mechanism still breaking the `idle-no-repaint-bytes-when-
    /// idle` journey's "perfectly silent once idle" contract on a fresh `$HOME` (no
    /// `registry_cache.json`): the "Extension registry updated (N extensions)" status message
    /// landed well outside any settle window `registry::fetch_registry`'s own `curl --max-time 15`
    /// allows, because the fetch's completion time is bounded only by that 15s ceiling — not by
    /// anything a fixed settle margin can assume. This PR removes the message (and the repaint it
    /// forces) entirely from the *automatic* startup refresh, via `Engine::ext_refresh_quiet`
    /// (`src/core/engine/lsp_ops.rs`) — see that method's doc for why an explicit, user-requested
    /// refresh (`Engine::ext_refresh`, unchanged) keeps its message.
    ///
    /// Unlike [`idle_silence_settle_1741`], this calls the real, unmodified `Engine::
    /// ext_refresh_quiet` production entry point (not a hand-rolled channel) — with
    /// `extension_registries` cleared first so the background thread's real-filesystem/network-
    /// shaped step, `registry::fetch_registry`, has no URL to call and resolves near-instantly with
    /// an empty, successful registry. That keeps the test hermetic and fast while still exercising
    /// `poll_ext_registry`'s real message-suppression branch end to end.
    mod quiet_startup_registry_refresh_1761 {
        use super::*;

        /// Point `$HOME` at a fresh, unique temp dir for the duration of the
        /// guard it returns. Hermetic: `Engine::ext_refresh_quiet`'s real
        /// production code calls `registry::save_cache` on a successful
        /// fetch, which must not touch the real `~/.config/vimcode/` on
        /// whatever machine runs this suite (mirrors the #1741 review fix).
        fn fresh_test_home(tag: &str) -> crate::core::paths::TestHomeGuard {
            let home = std::env::temp_dir().join(format!(
                "vimcode_test_1761_home_{tag}_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&home);
            std::fs::create_dir_all(&home).unwrap();
            crate::core::paths::set_test_home(&home)
        }

        /// Poll `h` for up to 2s — generous relative to the near-instant
        /// empty-registry fetch both tests in this module arm, nowhere near
        /// wide enough to mask a real #1761 regression (which would show the
        /// banned message on the very first tick that drains the channel,
        /// long before any 15s-scale delay could matter). Stops early once
        /// the fetch has resolved (`ext_registry` populated) plus a short
        /// confirmation margin, rather than always burning the full budget —
        /// and stops at the very first violation so the caller's assertion
        /// message doesn't accumulate ~100 duplicate lines for one
        /// violating tick. Panics with every failure found (normally at
        /// most one, given the early-break) if the idle-silence contract
        /// was violated.
        fn assert_registry_fetch_settles_silently<L: quadraui::AppLogic>(
            h: &mut crate::harness::ConformanceHarness<quadraui::tui::testing::TuiDriver<L>>,
            screen0: &str,
        ) {
            // At most one violation is ever recorded — each arm below breaks
            // out of the loop — so this is an `Option`, not a `Vec`.
            let mut failure: Option<&str> = None;
            let start = std::time::Instant::now();
            let mut resolved_at: Option<std::time::Instant> = None;
            let confirmation_margin = std::time::Duration::from_millis(200);
            while start.elapsed() < std::time::Duration::from_secs(2) {
                std::thread::sleep(std::time::Duration::from_millis(20));
                h.driver.tick();
                if h.driver.screen_has("Extension registry updated") {
                    failure = Some(
                        "the automatic startup registry refresh must never \
                         surface a status message (#1761) — found \
                         'Extension registry updated' on screen",
                    );
                    break;
                }
                if h.driver.screen() != screen0 {
                    failure = Some(
                        "rendered text changed as a result of the \
                         automatic startup registry refresh (#1761)",
                    );
                    break;
                }
                let fetch_resolved = h.engine.borrow().ext_registry.is_some();
                if fetch_resolved {
                    match resolved_at {
                        None => resolved_at = Some(std::time::Instant::now()),
                        Some(t) if t.elapsed() >= confirmation_margin => break,
                        Some(_) => {}
                    }
                }
            }
            assert!(
                failure.is_none(),
                "idle-silence violated by the startup registry refresh: {}",
                failure.unwrap_or_default()
            );
        }

        #[test]
        fn startup_registry_refresh_never_shows_a_message_or_forces_a_repaint() {
            let _home_guard = fresh_test_home("direct");

            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "alpha\nbeta\ngamma\n");
            engine.settings.lsp_enabled = false;
            let mut h = harness_no_sidebar(engine);

            // First real frame, same as every sibling in this family.
            h.driver.tick();
            assert!(
                h.driver.screen_has("alpha"),
                "precondition: the buffer text must actually be painted"
            );
            let screen0 = h.driver.screen();

            // Arm the real startup entry point — no URLs configured, so
            // the background thread's `fetch_registry` loop has nothing to
            // call and the channel send happens almost immediately, but
            // still asynchronously through the exact same
            // `ext_registry_rx`/`poll_ext_registry` plumbing a real,
            // slow (`curl --max-time 15`) fetch would use.
            {
                let mut engine = h.engine.borrow_mut();
                engine.settings.extension_registries = Vec::new();
                engine.ext_refresh_quiet();
            }

            assert_registry_fetch_settles_silently(&mut h, &screen0);

            // Precondition check, after the loop above: the fetch must
            // actually have completed (not just never started), or the
            // silence observed above would be vacuous.
            assert_eq!(
                h.engine.borrow().ext_registry.as_ref().map(|v| v.len()),
                Some(0),
                "precondition: the quiet startup fetch must have resolved \
                 (ext_registry populated with the empty registry) within \
                 the 2s budget — either it never completed or this test's \
                 own timing assumption is stale"
            );
        }

        /// The test above drives `Engine::ext_refresh_quiet()` directly, so
        /// it would stay green even if the one call site that actually
        /// fixes #1761 — `Engine::startup_inner`'s `self.ext_refresh()` →
        /// `self.ext_refresh_quiet()` (`src/core/engine/mod.rs`) — reverted
        /// to the non-quiet call. This test instead drives the real public
        /// `Engine::startup` entry point (production's own call path, same
        /// one both the TUI and GTK front ends use), so a regression at
        /// that specific call site is caught here too, not only by
        /// inspection.
        #[test]
        fn public_startup_entry_point_uses_the_quiet_refresh() {
            let _home_guard = fresh_test_home("via_startup");

            // Build and arm *before* `startup()` — `startup_inner` fires
            // `ext_refresh_quiet()` synchronously as part of the call, so
            // `extension_registries` must already be empty when it runs
            // (same reasoning as the direct-call test: an empty registry
            // list makes the background fetch resolve near-instantly
            // without any network access).
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "alpha\nbeta\ngamma\n");
            engine.settings.lsp_enabled = false;
            engine.settings.extension_registries = Vec::new();
            // `load_ambient_state = true`'s other effect, `plugin_init()`,
            // is harmless here: the fresh test `$HOME` has no
            // `plugins`/`extensions` directories to load from. The session
            // restore `startup(None)` also performs reads from the same
            // fresh, session-file-less `$HOME`, so it's a no-op too.
            engine.startup(None);

            let mut h = harness_no_sidebar(engine);
            h.driver.tick();
            assert!(
                h.driver.screen_has("alpha"),
                "precondition: the buffer text must actually be painted"
            );
            let screen0 = h.driver.screen();

            assert_registry_fetch_settles_silently(&mut h, &screen0);

            assert_eq!(
                h.engine.borrow().ext_registry.as_ref().map(|v| v.len()),
                Some(0),
                "precondition: the quiet startup fetch armed by the real \
                 `Engine::startup()` entry point must have resolved \
                 (ext_registry populated with the empty registry) within \
                 the 2s budget — either it never completed or this test's \
                 own timing assumption is stale"
            );
        }

        /// The complement of the two tests above, and the driver-tier guard
        /// for the #1761 *review*'s dedupe finding: an explicit,
        /// user-requested refresh issued **while the quiet startup fetch is
        /// still in flight** dedupes against it, and must still surface its
        /// status message when that fetch lands. Before the
        /// `if !quiet { self.ext_registry_quiet = false; }` upgrade in
        /// `ext_refresh_inner`'s early return, the explicit refresh silently
        /// inherited startup's silence policy and the user got no feedback
        /// at all.
        ///
        /// Asserts on *painted* output rather than the `ext_registry_quiet`
        /// flag (CLAUDE.md: "assert on rendered output — never on state being
        /// populated"); `engine::lsp_ops`'s
        /// `explicit_refresh_upgrades_an_in_flight_quiet_fetch` covers the
        /// flag itself. This calls the same `ext_refresh()` the Extensions
        /// sidebar's `r` key handler calls (`core/engine/ext_panel.rs`),
        /// skipping only the keybinding table lookup.
        #[test]
        fn explicit_refresh_during_the_quiet_startup_fetch_still_paints_its_message() {
            let _home_guard = fresh_test_home("explicit_during_quiet");

            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "alpha\nbeta\ngamma\n");
            engine.settings.lsp_enabled = false;
            let mut h = harness_no_sidebar(engine);

            h.driver.tick();
            assert!(
                h.driver.screen_has("alpha"),
                "precondition: the buffer text must actually be painted"
            );

            {
                let mut engine = h.engine.borrow_mut();
                engine.settings.extension_registries = Vec::new();
                // Arm startup's quiet fetch, then issue the explicit refresh
                // *without* an intervening tick, so `poll_ext_registry` has
                // not yet drained the channel and `ext_registry_fetching` is
                // still true — i.e. we are genuinely on the dedupe path.
                engine.ext_refresh_quiet();
                assert!(
                    engine.ext_registry_fetching,
                    "precondition: the quiet fetch must still be in flight, \
                     otherwise the explicit refresh below would spawn its own \
                     fetch and this test would not exercise the dedupe path"
                );
                engine.ext_refresh();
            }

            // Pump until the deduped fetch lands and paints its message.
            let start = std::time::Instant::now();
            let mut painted = false;
            while start.elapsed() < std::time::Duration::from_secs(2) {
                std::thread::sleep(std::time::Duration::from_millis(20));
                h.driver.tick();
                if h.driver.screen_has("Extension registry updated") {
                    painted = true;
                    break;
                }
            }

            assert_eq!(
                h.engine.borrow().ext_registry.as_ref().map(|v| v.len()),
                Some(0),
                "precondition: the deduped fetch must have resolved within \
                 the 2s budget, or the silence below would be vacuous"
            );
            assert!(
                painted,
                "an explicit refresh that deduped against the in-flight quiet \
                 startup fetch must still paint its status message when the \
                 fetch lands (#1761 review) — screen:\n{}",
                h.driver.screen()
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // A `MouseMoved` that changes no hover target must not repaint
    // ─────────────────────────────────────────────────────────────────────────
    /// #1722's second acceptance bullet: "a `MouseMoved` that doesn't change
    /// any hover target must return `Continue`." Audited
    /// `App::handle_dispatch`'s `MouseMoved` arm (`src/app.rs`) for this
    /// issue: `render::route_gutter_hover`'s caller already gates
    /// `draw_needed` on an actual before/after difference
    /// (`engine.gutter_hover_window != was`), and the window-edge
    /// resize-cursor hint and `mouse_pos_cell` bookkeeping above it never
    /// touch `draw_needed` at all.
    ///
    /// This fixture has no sidebar (`harness_no_sidebar`), so it structurally
    /// never reaches `render::route_sidebar_hover` at all — that rung's own
    /// coverage, including the bug a prior revision of this PR incorrectly
    /// claimed didn't exist, is
    /// [`sidebar_panels::mouse_moved_to_the_same_sidebar_row_does_not_repaint`].
    /// This test pins down the plain-editor half as a black-box regression
    /// guard rather than leaving the audit as only a sentence in a PR
    /// description.
    #[test]
    fn mouse_moved_to_the_same_plain_editor_cell_does_not_repaint() {
        let mut engine = plain_engine();
        engine.buffer_mut().insert(0, "hello world\n");
        let mut h = harness_no_sidebar(engine);
        let driver = &mut h.driver;

        let (x, y) = driver
            .find("hello world")
            .expect("buffer text must be painted");

        // First move establishes whatever hover state a move to this cell
        // implies (there is none here — no gutter fold marker, no sidebar
        // popup — but settle it before taking the comparison snapshot).
        hover_move(driver, x, y);
        let screen0 = driver.screen();

        // Second move to the exact same cell: nothing about the pointer
        // target changed, so no hover state can have changed either.
        let reaction = hover_move(driver, x, y);
        assert_eq!(
            reaction,
            quadraui::Reaction::Continue,
            "a MouseMoved that lands on the same cell as the previous one, \
             touching no hover target, must not force a repaint (#1722)"
        );
        assert_eq!(
            driver.screen(),
            screen0,
            "rendered text must not change from a no-op mouse move"
        );
    }

    /// The recommended-extension install offer, TUI half of the black-box coverage — GTK's twin is
    /// `crate::gtk::testing::issue_1577_ext_install_offer_toast`. `ConformanceHarness::engine`
    /// stays reachable here too, so this module reads engine state directly exactly like the GTK
    /// twin, rather than needing the `find`/style-probe workarounds this module's own header doc
    /// says older TUI-only harnesses used to require.
    mod issue_1577_ext_install_offer_toast {
        use super::*;

        /// Build a [`harness`] whose first frame already shows the
        /// install-offer toast for a synthetic extension — same
        /// fixture shape as the GTK twin's `harness_with_ext_install_offer`,
        /// including the two-"hello"-line buffer needed for the N-search test.
        fn harness_with_ext_install_offer(
            unique: &str,
        ) -> (
            crate::harness::ConformanceHarness<
                quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            >,
            String,
            String,
            std::path::PathBuf,
        ) {
            use crate::core::extensions::{ExtensionManifest, LspConfig};
            use std::io::Write;

            let mut engine = crate::core::Engine::new_for_test();
            let ext_name = format!("vimcode-test-tui-ext-1577-{unique}");
            let display_name = format!("X1577{unique}");
            let lang_id = format!("vimcode-test-tui-lang-1577-{unique}");
            let file_ext = format!("zqxg1577{unique}");

            engine
                .settings
                .language_map
                .insert(file_ext.clone(), lang_id.clone());
            engine.ext_registry = Some(vec![ExtensionManifest {
                name: ext_name.clone(),
                display_name: display_name.clone(),
                language_ids: vec![lang_id],
                lsp: LspConfig {
                    binary: "vc-tui-bin-1577".to_string(),
                    install: "true".to_string(),
                    ..Default::default()
                },
                ..Default::default()
            }]);

            let path = std::env::temp_dir().join(format!(
                "vimcode_test_tui_1577_{unique}_{}.{file_ext}",
                std::process::id()
            ));
            {
                let mut f = std::fs::File::create(&path).unwrap();
                f.write_all(b"hello\nworld\nhello\n").unwrap();
            }

            engine.open_file_in_tab(&path);
            assert!(
                !engine.toasts.is_empty(),
                "precondition: opening the file must queue the install offer toast"
            );

            let h = harness(engine);
            (h, ext_name, display_name, path)
        }

        /// Center of the toast's `index`-th action button (0 = Install, 1 =
        /// Don't ask again), read from the cached `toast_layout` — same
        /// rationale as the GTK twin's identically-named helper.
        fn toast_action_center(
            h: &crate::harness::ConformanceHarness<
                quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            >,
            index: usize,
        ) -> (f32, f32) {
            let engine = h.engine.borrow();
            let layout = engine.toast_layout.borrow();
            let vt = layout
                .as_ref()
                .and_then(|l| l.visible_toasts.first())
                .expect("toast must have painted a visible_toasts entry");
            let b = vt
                .action_rects
                .get(index)
                .copied()
                .unwrap_or_else(|| panic!("toast must have painted action button {index}"));
            (b.x + b.width / 2.0, b.y + b.height / 2.0)
        }

        /// Center of the toast's dismiss ×, same rationale as
        /// [`toast_action_center`].
        fn toast_dismiss_center(
            h: &crate::harness::ConformanceHarness<
                quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            >,
        ) -> (f32, f32) {
            let engine = h.engine.borrow();
            let layout = engine.toast_layout.borrow();
            let vt = layout
                .as_ref()
                .and_then(|l| l.visible_toasts.first())
                .expect("toast must have painted a visible_toasts entry");
            let b = vt
                .dismiss_bounds
                .expect("toast must have painted a dismiss ×");
            (b.x + b.width / 2.0, b.y + b.height / 2.0)
        }

        /// Core acceptance (TUI half): opening a file whose
        /// recommended extension isn't installed must paint a toast offer
        /// with separate, non-overlapping "Install" and "Don't ask again"
        /// buttons — not a missable status-line hint, and not shortcuts
        /// printed as body text.
        #[test]
        fn ext_install_offer_toast_paints_two_distinct_buttons_via_shell_app() {
            let (mut h, _ext_name, display_name, _path) = harness_with_ext_install_offer("rd");

            let want_title = format!("Install the {display_name} extension?");
            assert!(
                h.driver.screen_contains(&want_title),
                "install offer toast title must paint; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                h.driver.screen_contains("Install") && h.driver.screen_contains("Don't ask again"),
                "toast must paint both buttons; painted: {:?}",
                h.driver.screen()
            );
            let (ax, ay) = toast_action_center(&h, 0);
            let (dx, dy) = toast_action_center(&h, 1);
            assert!(ax > 0.0 && ay > 0.0, "Install button must have real bounds");
            assert!(
                dx > 0.0 && dy > 0.0,
                "Don't ask again button must have real bounds"
            );
            assert!(
                (ax - dx).abs() > 0.5 || (ay - dy).abs() > 0.5,
                "Install and Don't ask again must be distinct, \
                 non-overlapping buttons, not drawn on top of each other"
            );
        }

        /// The `N` key must NOT be hijacked by the toast.
        /// Pressing `N` in Normal mode — vim's own "previous search match" —
        /// while this (sticky, never-expiring) toast is displayed must search,
        /// not silently dismiss the extension. This test drives a real backward
        /// search (`*` then `N`) and asserts the cursor moves like vim `N`,
        /// AND that the toast and dismissal state are completely unaffected.
        #[test]
        fn ext_install_offer_toast_n_key_searches_backward_and_leaves_toast_untouched_via_shell_app(
        ) {
            let (mut h, ext_name, display_name, path) = harness_with_ext_install_offer("nk");
            let want_title = format!("Install the {display_name} extension?");
            assert!(
                h.driver.screen_contains(&want_title),
                "precondition: offer must be showing; painted: {:?}",
                h.driver.screen()
            );
            assert_eq!(h.engine.borrow().view().cursor.line, 0, "precondition");

            // `*`: search forward for the word under the cursor ("hello") —
            // wraps past "world" (line 1) to the next "hello" (line 2).
            h.driver.type_char('*');
            h.driver.render();
            assert_eq!(
                h.engine.borrow().view().cursor.line,
                2,
                "precondition: '*' must land on the next 'hello' match"
            );

            // `N`: previous match relative to '*'s forward search — must go
            // BACK to line 0, not dismiss the toast.
            h.driver.type_char('N');
            h.driver.render();
            assert_eq!(
                h.engine.borrow().view().cursor.line,
                0,
                "'N' must perform vim's own backward search, not be \
                 hijacked by the extension-offer toast"
            );
            assert!(
                h.driver.screen_contains(&want_title),
                "'N' must leave the toast exactly as it was; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                !h.engine.borrow().extension_state.is_dismissed(&ext_name),
                "'N' must not dismiss/persist anything — that is the \
                 \"Don't ask again\" button's job now, not a hijacked vim key"
            );

            let _ = std::fs::remove_file(&path);
        }

        /// Clicking "Install" starts the install
        /// (`Engine::ext_install_from_registry`), proven by the command
        /// line's "Extension '…' installed — …" outcome message.
        #[test]
        fn ext_install_offer_toast_click_install_starts_install_via_shell_app() {
            let (mut h, ext_name, display_name, _path) = harness_with_ext_install_offer("ci");
            let want_title = format!("Install the {display_name} extension?");

            let (ax, ay) = toast_action_center(&h, 0);
            h.driver.click(ax, ay);
            h.driver.render();

            let want_outcome = format!("Extension '{ext_name}' installed");
            assert!(
                h.driver.screen_contains(&want_outcome),
                "clicking Install must run ext_install_from_registry for the \
                 right extension; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                !h.driver.screen_contains(&want_title),
                "the toast must be gone once its action has run; painted: {:?}",
                h.driver.screen()
            );
        }

        /// Clicking "Don't ask again" persists the dismissal.
        #[test]
        fn ext_install_offer_toast_click_dont_ask_again_persists_via_shell_app() {
            let (mut h, ext_name, display_name, path) = harness_with_ext_install_offer("da");
            let want_title = format!("Install the {display_name} extension?");

            let (dx, dy) = toast_action_center(&h, 1);
            h.driver.click(dx, dy);
            h.driver.render();
            assert!(
                !h.driver.screen_contains(&want_title),
                "clicking Don't ask again must dismiss the toast; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                h.engine.borrow().extension_state.is_dismissed(&ext_name),
                "Don't ask again must persist the dismissal via \
                 ExtensionState::mark_dismissed"
            );

            h.engine.borrow_mut().open_file_in_tab(&path);
            h.driver.render();
            assert!(
                !h.driver.screen_contains(&want_title),
                "a persisted dismissal must not re-prompt on a later open; \
                 painted: {:?}",
                h.driver.screen()
            );

            let _ = std::fs::remove_file(&path);
        }

        /// Clicking × hides the toast without persisting anything.
        #[test]
        fn ext_install_offer_toast_click_dismiss_x_hides_without_persisting_via_shell_app() {
            let (mut h, ext_name, display_name, path) = harness_with_ext_install_offer("kx");
            let want_title = format!("Install the {display_name} extension?");

            let (dx, dy) = toast_dismiss_center(&h);
            h.driver.click(dx, dy);
            h.driver.render();
            assert!(
                !h.driver.screen_contains(&want_title),
                "clicking × must dismiss the toast; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                !h.engine.borrow().extension_state.is_dismissed(&ext_name),
                "× must not persist a dismissal — only session-scoped \
                 (prompted_extensions)"
            );

            h.engine.borrow_mut().open_file_in_tab(&path);
            h.driver.render();
            assert!(
                !h.driver.screen_contains(&want_title),
                "'Not now' must not re-prompt again this session \
                 (prompted_extensions); painted: {:?}",
                h.driver.screen()
            );

            let _ = std::fs::remove_file(&path);
        }

        /// Keyboard access via `Engine::focus_toast_stack`
        /// (`:Notifications` / `panel_keys.focus_notifications`) +
        /// quadraui's `ToastStackController` — Tab from the dismiss `×` to
        /// "Install" to "Don't ask again", then Enter — must have the same
        /// effect as clicking "Don't ask again" directly.
        #[test]
        fn ext_install_offer_toast_keyboard_focus_tab_enter_dont_ask_again_via_shell_app() {
            let (mut h, ext_name, display_name, path) = harness_with_ext_install_offer("kb");
            let want_title = format!("Install the {display_name} extension?");

            assert!(
                h.engine.borrow_mut().focus_toast_stack(),
                "focus_toast_stack must succeed while the toast is showing"
            );

            // Focus starts on the dismiss "×" — Tab once for "Install", Tab
            // again for "Don't ask again".
            h.driver.press_named(quadraui::NamedKey::Tab);
            h.driver.press_named(quadraui::NamedKey::Tab);
            h.driver.press_named(quadraui::NamedKey::Enter);
            h.driver.render();

            assert!(
                !h.driver.screen_contains(&want_title),
                "Enter on the focused 'Don't ask again' button must dismiss \
                 the toast; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                h.engine.borrow().extension_state.is_dismissed(&ext_name),
                "keyboard-driven 'Don't ask again' must persist the \
                 dismissal, exactly like clicking it"
            );

            h.engine.borrow_mut().open_file_in_tab(&path);
            h.driver.render();
            assert!(
                !h.driver.screen_contains(&want_title),
                "the persisted dismissal must not re-prompt; painted: {:?}",
                h.driver.screen()
            );

            let _ = std::fs::remove_file(&path);
        }

        /// `:Notifications` (the ex-command spelling of
        /// [`crate::core::Engine::focus_toast_stack`]) must give the stack
        /// keyboard focus exactly like the default keybinding does — driven
        /// through `execute_command`, the same path the command line uses.
        #[test]
        fn notifications_ex_command_focuses_toast_stack_via_shell_app() {
            let (mut h, ext_name, display_name, path) = harness_with_ext_install_offer("ex");
            let want_title = format!("Install the {display_name} extension?");

            let _ = h.engine.borrow_mut().execute_command("Notifications");
            assert!(
                h.engine.borrow().toast_focus.is_focused(),
                ":Notifications must give the toast stack keyboard focus"
            );

            h.driver.press_named(quadraui::NamedKey::Tab);
            h.driver.press_named(quadraui::NamedKey::Tab);
            h.driver.press_named(quadraui::NamedKey::Enter);
            h.driver.render();

            assert!(
                !h.driver.screen_contains(&want_title),
                "Enter on the focused 'Don't ask again' button (reached via \
                 :Notifications) must dismiss the toast; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                h.engine.borrow().extension_state.is_dismissed(&ext_name),
                "must persist the dismissal exactly like the mouse path"
            );

            let _ = std::fs::remove_file(&path);
        }
    }

    /// Driver-tier black-box coverage for the "package-manager
    /// acquire kind whose runtime is missing" fallback — CLAUDE.md's
    /// black-box coverage rule requires a test that drives the running app
    /// and asserts on rendered output, not just engine-internal state.
    /// `core::engine::lsp_ops::tests::missing_package_manager_runtime_falls_
    /// through_to_terminal_tier` already covers the same seam at the
    /// engine-internal-state level (`tool_acquire_groups`/
    /// `pending_terminal_command`); this module closes the gap the review
    /// flagged by asserting on `driver.screen()` instead, through the same
    /// `pub(crate) Engine::ext_install_from_registry_with_runtime_check`
    /// seam (stubbed `runtime_present`, deterministic regardless of what's
    /// actually on the machine running the suite — same rationale as that
    /// sibling test's own doc comment).
    mod issue_1346_package_manager_acquire_missing_runtime {
        use super::*;

        /// Build a [`harness`] wired with a manifest whose `[lsp.acquire]`
        /// names an `npm` package-manager kind, `unique`-suffixed so
        /// parallel test runs never collide on extension/language/file-ext
        /// names.
        fn harness_with_npm_acquire_manifest(
            unique: &str,
        ) -> (
            crate::harness::ConformanceHarness<
                quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            >,
            String,
        ) {
            use crate::core::extensions::{ExtensionManifest, LspConfig};
            use crate::core::tool_acquire::{AcquireConfig, AcquireKind};

            let mut engine = crate::core::Engine::new_for_test();
            let ext_name = format!("vc-tui-pkgmgr-fallback-1346-{unique}");
            engine.ext_registry = Some(vec![ExtensionManifest {
                name: ext_name.clone(),
                display_name: format!("Package-manager fallback test 1346-{unique}"),
                language_ids: vec![format!("vc-tui-pkgmgr-1346-lang-{unique}")],
                lsp: LspConfig {
                    binary: format!("vc-tui-pkgmgr-1346-lsp-{unique}"),
                    acquire: Some(AcquireConfig {
                        kind: AcquireKind::Npm,
                        package: format!("vc-tui-pkgmgr-1346-pkg-{unique}"),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }]);

            // No sidebar: the default explorer tree is tall enough (this
            // repo has ~20 top-level entries) to fill the whole 24-row
            // fixture terminal and share the bottom row with the command
            // line, truncating the status message before "needs npm" ever
            // reaches the screen. `harness_no_sidebar` gives the command
            // line the full row width instead.
            let h = harness_no_sidebar(engine);
            (h, ext_name)
        }

        /// A manifest's `[lsp.acquire]` naming a
        /// package-manager kind (`npm`) whose runtime isn't on PATH must
        /// paint a "needs npm" status line — the visible fallback, not a
        /// silent no-op and not a native-acquisition attempt that would
        /// just fail. `runtime_present` is stubbed to always return `false`
        /// via the `pub(crate)` `ext_install_from_registry_with_runtime_
        /// check` seam, exactly like the engine-internal-state sibling test
        /// in `core::engine::lsp_ops`.
        #[test]
        fn missing_npm_runtime_paints_needs_npm_status_line() {
            let (mut h, ext_name) = harness_with_npm_acquire_manifest("status");

            h.engine
                .borrow_mut()
                .ext_install_from_registry_with_runtime_check(&ext_name, |_| false);
            h.driver.render();

            assert!(
                h.driver.screen_contains("needs npm"),
                "a missing package-manager runtime must paint a visible \
                 'needs npm' fallback status line; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                h.engine.borrow().tool_acquire_groups.is_empty(),
                "a missing runtime must never spawn a native acquisition"
            );
        }

        /// The same missing-runtime fallback must also
        /// queue the terminal-tier command carrying the dependency hint —
        /// the install path the user actually runs once they've read the
        /// status line. This is exercised through the extension's DAP
        /// half (`[dap.acquire]`) so both `resolve_acquire_action` call
        /// sites in `lsp_ops.rs` get driver-tier coverage.
        #[test]
        fn missing_dotnet_runtime_falls_through_to_visible_terminal() {
            use crate::core::extensions::{DapConfig, ExtensionManifest};
            use crate::core::tool_acquire::{AcquireConfig, AcquireKind};

            let mut engine = crate::core::Engine::new_for_test();
            let ext_name = "vc-tui-pkgmgr-fallback-1346-dap";
            engine.ext_registry = Some(vec![ExtensionManifest {
                name: ext_name.to_string(),
                display_name: "Package-manager DAP fallback test (#1346)".to_string(),
                dap: DapConfig {
                    adapter: "vc-tui-pkgmgr-1346-adapter".to_string(),
                    binary: "vc-tui-pkgmgr-1346-dap-bin".to_string(),
                    acquire: Some(AcquireConfig {
                        kind: AcquireKind::DotnetTool,
                        package: "vc-tui-pkgmgr-1346-dap-pkg".to_string(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }]);

            let mut h = harness_no_sidebar(engine);
            h.engine
                .borrow_mut()
                .ext_install_from_registry_with_runtime_check(ext_name, |_| false);
            h.driver.render();

            assert!(
                h.driver.screen_contains("needs dotnet"),
                "a missing DAP-side package-manager runtime must also paint \
                 a visible fallback status line; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                h.engine
                    .borrow()
                    .pending_terminal_command
                    .clone()
                    .unwrap_or_default()
                    .contains("dotnet"),
                "the queued terminal command must carry the missing runtime's name"
            );
        }
    }

    /// Driver-tier black-box coverage for the "declared
    /// (or built-in) prerequisite is missing → block the install and paint
    /// an instruction instead" behaviour. The engine-internal-state level
    /// is covered by `core::engine::lsp_ops::tests`; this module covers the
    /// driver-tier gap by asserting on `driver.screen()` instead, through
    /// the `pub(crate) Engine::ext_install_from_registry_with_runtime_check`
    /// seam with stubbed `runtime_present` for deterministic testing.
    mod issue_1719_prerequisite_detect_before_install {
        use super::*;

        /// [`harness_no_sidebar`], but considerably wider than the default
        /// 80 columns. The "requires X — X: <install hint>" status line
        /// is long enough to get truncated at the default width; this harness
        /// width ensures it reaches the screen.
        fn wide_harness_no_sidebar(
            engine: crate::core::Engine,
        ) -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 24);
            collapse_sidebar(&mut h.driver);
            h
        }

        /// #1719 acceptance: a manifest's legacy `[lsp]` `install_*`
        /// string, gated by the new `lsp.dependencies` field, must paint
        /// the actionable "requires npm — ..." status line instead of
        /// silently queuing a doomed terminal command — the mirror-image
        /// LSP leg of `lsp_ops::tests::
        /// lsp_legacy_install_blocked_when_declared_dependency_missing`,
        /// now asserting on rendered output rather than engine state.
        ///
        /// Verified RED against a reintroduced regression (commenting out
        /// the `missing.is_empty()` gate in `lsp_ops.rs`'s legacy-install
        /// branch so it falls straight to the `else` with no dependency
        /// check): the screen painted "LSP: installing" instead of
        /// "requires npm", confirming this test catches a real
        /// behavioural break and not just a visibility change.
        #[test]
        fn lsp_legacy_install_blocked_when_dependency_missing_paints_status() {
            use crate::core::extensions::{ExtensionManifest, LspConfig};

            let mut engine = crate::core::Engine::new_for_test();
            let ext_name = "vc-tui-1719-lsp-missing-dep";
            engine.ext_registry = Some(vec![ExtensionManifest {
                name: ext_name.to_string(),
                display_name: "1719 TUI LSP missing-dep test".to_string(),
                language_ids: vec!["vc-tui-1719-lsp-lang".to_string()],
                lsp: LspConfig {
                    binary: "vc-tui-1719-lsp-bin".to_string(),
                    install_linux: "npm install -g vc-tui-1719-lsp-bin".to_string(),
                    install_macos: "npm install -g vc-tui-1719-lsp-bin".to_string(),
                    install_windows: "npm install -g vc-tui-1719-lsp-bin".to_string(),
                    dependencies: vec!["npm".to_string()],
                    ..Default::default()
                },
                ..Default::default()
            }]);

            let mut h = wide_harness_no_sidebar(engine);
            h.engine
                .borrow_mut()
                .ext_install_from_registry_with_runtime_check(ext_name, |_| false);
            h.driver.render();

            assert!(
                h.driver.screen_contains("requires npm"),
                "a missing declared LSP dependency must paint a visible \
                 'requires npm' instruction instead of dispatching a doomed \
                 install; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                h.engine.borrow().pending_terminal_command.is_none(),
                "a missing declared dependency must never dispatch an install"
            );
        }

        /// The built-in `delve` DAP adapter's hardcoded
        /// `go install ...` installer, gated by the new `dap_manager::
        /// adapter_dependencies` merge, must paint a "requires go"
        /// status line rather than queuing the install — the mirror-image
        /// DAP leg of `lsp_ops::tests::dap_builtin_delve_install_blocked_when_go_missing`.
        #[test]
        fn dap_builtin_delve_install_blocked_when_go_missing_paints_status() {
            use crate::core::extensions::{DapConfig, ExtensionManifest};

            let mut engine = crate::core::Engine::new_for_test();
            let ext_name = "vc-tui-1719-delve-missing-go";
            engine.ext_registry = Some(vec![ExtensionManifest {
                name: ext_name.to_string(),
                display_name: "1719 TUI delve missing-go test".to_string(),
                dap: DapConfig {
                    adapter: "delve".to_string(),
                    // Deliberately not the literal `dlv` binary name — see
                    // `lsp_ops::tests::dap_builtin_delve_install_blocked_
                    // when_go_missing`'s comment for why.
                    binary: "vc-tui-1719-nonexistent-dlv".to_string(),
                    ..Default::default()
                },
                ..Default::default()
            }]);

            let mut h = wide_harness_no_sidebar(engine);
            h.engine
                .borrow_mut()
                .ext_install_from_registry_with_runtime_check(ext_name, |_| false);
            h.driver.render();

            assert!(
                h.driver.screen_contains("requires go"),
                "delve's missing `go` prerequisite must paint a visible \
                 'requires go' instruction instead of dispatching the \
                 doomed `go install` command; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                h.engine.borrow().pending_terminal_command.is_none(),
                "delve's install must never run without `go` present"
            );
        }
    }

    /// A project-pinned LSP binary (e.g. `rust-analyzer` via a
    /// `rust-toolchain.toml` with no `rust-analyzer` component) that can't be
    /// resolved must paint a status-line hint naming the workspace root the
    /// install command needs to be run from — `rustup component add` only
    /// picks up a pinned toolchain from inside that directory, not wherever
    /// the editor's OS process cwd happens to be. Driver-tier coverage for
    /// `lsp_manager::not_found_hint`'s workspace-root-naming behaviour,
    /// asserting on `driver.screen()` rather than on `Engine`/`LspManager`
    /// internal state the way `lsp_manager::tests::
    /// not_found_hint_names_workspace_root` already does.
    mod issue_1905_lsp_not_found_hint_names_workspace_root {
        use super::*;

        /// [`crate::core::Engine::new_for_test`] rooted at `workspace_root`,
        /// with a fake, never-resolvable Rust extension manifest installed.
        /// The manifest's `[lsp.install]` mirrors the real rust-analyzer
        /// extension's `rustup component add rust-analyzer` so the hint
        /// built by `lsp_manager::not_found_hint` takes the same branch
        /// production code does. Wide (220 cols), sidebar-collapsed: the
        /// full hint (binary name + workspace root path + install command)
        /// is long enough to get truncated at the default 80-column width,
        /// same reasoning as `issue_1719_prerequisite_detect_before_install`'s
        /// `wide_harness_no_sidebar` above.
        fn harness_with_unresolvable_rust_manifest(
            workspace_root: &std::path::Path,
        ) -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            use crate::core::extensions::{ExtensionManifest, LspConfig};

            let mut engine = crate::core::Engine::new_for_test();
            engine.cwd = workspace_root.to_path_buf();
            let ext_name = "vc-tui-1905-rust";
            engine.ext_registry = Some(vec![ExtensionManifest {
                name: ext_name.to_string(),
                display_name: "1905 TUI Rust test fixture".to_string(),
                file_extensions: vec![".rs".to_string()],
                language_ids: vec!["rust".to_string()],
                lsp: LspConfig {
                    binary: "vc-tui-1905-rust-analyzer".to_string(),
                    install: "rustup component add vc-tui-1905-rust-analyzer".to_string(),
                    ..Default::default()
                },
                ..Default::default()
            }]);
            engine.extension_state.mark_installed(ext_name);

            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 24);
            collapse_sidebar(&mut h.driver);
            h
        }

        /// The hint must name the workspace root (`Run in <path>`), not just
        /// the binary and its install command.
        #[test]
        fn missing_pinned_lsp_binary_paints_workspace_root_in_hint() {
            let workspace_root = std::env::temp_dir().join(format!(
                "vimcode_test_1905_tui_root_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&workspace_root).expect("create fixture workspace root");
            let rs_path = workspace_root.join("main.rs");
            std::fs::write(&rs_path, "fn main() {}\n").expect("write fixture file");

            let mut h = harness_with_unresolvable_rust_manifest(&workspace_root);
            h.engine.borrow_mut().new_tab(Some(&rs_path));
            h.driver.render();

            let expected_root_hint = format!("Run in {}", workspace_root.display());
            assert!(
                h.driver.screen_contains(&expected_root_hint),
                "a project-pinned LSP binary that can't be resolved must \
                 name the workspace root (not just the install command) in \
                 its status-line hint; painted: {:?}",
                h.driver.screen()
            );
            assert!(
                h.engine
                    .borrow()
                    .message
                    .contains("vc-tui-1905-rust-analyzer not found"),
                "engine.message should carry the hint: {:?}",
                h.engine.borrow().message
            );

            let _ = std::fs::remove_dir_all(&workspace_root);
        }
    }

    /// An extension manifest's `requires_vimcode` semver requirement
    /// must be surfaced in the Extensions panel (so an incompatible
    /// extension reads as incompatible before the user tries to install
    /// it) and must block the install itself — the registry-fetch half of
    /// the acceptance bar ("a far-future version shows as incompatible...
    /// and its script does not run").
    mod issue_1807_requires_vimcode_gates_install {
        use super::*;

        /// A manifest requiring a vimcode version far beyond anything this
        /// binary will ever report (`env!("CARGO_PKG_VERSION")`), with one
        /// bundled script — so "its script does not run" is checkable by
        /// asserting the script never lands on disk.
        fn harness_with_far_future_requirement(
            unique: &str,
        ) -> (
            crate::harness::ConformanceHarness<
                quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            >,
            String,
        ) {
            use crate::core::extensions::ExtensionManifest;

            let mut engine = crate::core::Engine::new_for_test();
            let ext_name = format!("vc-tui-1807-future-{unique}");
            engine.ext_registry = Some(vec![ExtensionManifest {
                name: ext_name.clone(),
                display_name: format!("1807 Future Extension {unique}"),
                requires_vimcode: Some(">=9999.0.0".to_string()),
                scripts: vec!["init.lua".to_string()],
                registry_base_url: "https://example.invalid/never-fetched".to_string(),
                ..Default::default()
            }]);

            // Wide terminal + the real `Alt+Right` "resize sidebar" gesture
            // (`render::alt_resized_sidebar_width`), same pattern used
            // elsewhere in this module (e.g. the AI panel's "model: ..."
            // header) for a panel whose body text is wider than the
            // default ~20-cell sidebar: at the default width the row
            // truncates ("1807 Future Exte") before the incompatibility
            // reason ever reaches the screen.
            let h = crate::tui_main::testing::conformance_harness(engine, 220, 24);
            (h, ext_name)
        }

        /// Opening the Extensions panel must paint the incompatibility
        /// reason (`"requires vimcode"` and the declared bound) inline
        /// next to the extension's name, in the "available" section —
        /// readable as incompatible without ever attempting an install.
        #[test]
        fn far_future_requires_vimcode_shows_as_incompatible_in_extensions_panel() {
            let (mut h, _ext_name) = harness_with_far_future_requirement("panel");

            // `show_panel` + `explorer_visible`, not `focus_sidebar_panel`:
            // the latter also sets `ext_sidebar_has_focus`, which routes
            // every subsequent key (including the Alt+Right resize chord
            // below) through `dispatch_ext_sidebar_key_unified` instead of
            // the shared Alt rung — same reasoning
            // `ai_model_switch_round_trips_via_session_set_config_option_via_shell_app`
            // above uses for the AI panel.
            {
                let mut engine = h.engine.borrow_mut();
                engine.app_shell.show_panel(&quadraui::WidgetId::new(
                    crate::core::engine::sidebar::PANEL_EXTENSIONS,
                ));
                engine.session.explorer_visible = true;
            }
            for _ in 0..150 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            h.driver.render();

            let screen = h.driver.screen();
            assert!(
                screen.contains("requires vimcode"),
                "an extension whose requires_vimcode constraint isn't met \
                 must show that reason in the Extensions panel; painted:\n{screen}"
            );
            assert!(
                screen.contains("9999.0.0"),
                "the painted reason must name the required version; \
                 painted:\n{screen}"
            );
        }

        /// Attempting to install a `requires_vimcode`-incompatible
        /// extension must refuse with a clear message and must never
        /// download its bundled script. A script that's never written to
        /// disk can never be loaded by `plugin_init`.
        #[test]
        fn far_future_requires_vimcode_blocks_install_with_clear_message() {
            let (mut h, ext_name) = harness_with_far_future_requirement("install");

            h.engine.borrow_mut().ext_install_from_registry(&ext_name);
            h.driver.render();

            // Primary (black-box) assertion: the refusal message actually
            // reaches the painted screen via the status line
            // (`render.rs`'s status-line body reads `engine.message`) — not
            // just set as internal state (CLAUDE.md "Testing (CRITICAL)"
            // rule 1).
            let screen = h.driver.screen();
            assert!(
                screen.contains("Cannot install"),
                "installing an incompatible extension must paint a clear \
                 refusal message naming the requirement; painted:\n{screen}"
            );

            let engine = h.engine.borrow();
            assert!(
                engine.message.contains("Cannot install")
                    && engine.message.contains("requires vimcode"),
                "installing an incompatible extension must set a clear \
                 refusal message naming the requirement; message: {:?}",
                engine.message
            );
            assert!(
                !engine.extension_state.is_installed(&ext_name),
                "a refused install must never mark the extension installed"
            );
            let script_path = crate::core::paths::vimcode_config_dir()
                .join("extensions")
                .join(&ext_name)
                .join("init.lua");
            assert!(
                !script_path.exists(),
                "a refused install must never download the extension's script \
                 to disk — its script must never run"
            );
        }

        /// `requires_vimcode` absent — the default — must behave
        /// exactly as today: no incompatibility text in the panel, and
        /// install proceeds past the new gate (it may still fail further
        /// down for unrelated reasons).
        ///
        /// This test uses `set_test_home` to keep the persisted install
        /// state under a throwaway directory instead of the developer's real
        /// `~/.config/vimcode`, preventing spurious failures if a developer
        /// has a locally-installed extension declaring an unmet `requires_vimcode`.
        #[test]
        fn absent_requires_vimcode_shows_no_incompatibility_and_does_not_block_install() {
            use crate::core::extensions::ExtensionManifest;

            let home = std::env::temp_dir().join(format!(
                "vimcode_test_1807_absent_requires_home_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&home);
            std::fs::create_dir_all(&home).unwrap();
            let home_guard = crate::core::paths::set_test_home(&home);

            let mut engine = crate::core::Engine::new_for_test();
            let ext_name = "vc-tui-1807-legacy-no-constraint".to_string();
            engine.ext_registry = Some(vec![ExtensionManifest {
                name: ext_name.clone(),
                display_name: "1807 Legacy Extension".to_string(),
                ..Default::default()
            }]);

            let mut h = harness(engine);
            h.engine
                .borrow_mut()
                .focus_sidebar_panel(crate::core::engine::sidebar::PANEL_EXTENSIONS);
            h.driver.render();
            assert!(
                !h.driver.screen_contains("requires vimcode"),
                "an extension with no requires_vimcode field must not show \
                 any incompatibility text; painted:\n{}",
                h.driver.screen()
            );

            h.engine.borrow_mut().ext_install_from_registry(&ext_name);
            h.driver.render();
            assert!(
                !h.driver.screen_contains("Cannot install"),
                "an extension with no requires_vimcode field must never \
                 paint a refusal message; painted:\n{}",
                h.driver.screen()
            );
            assert!(
                !h.engine.borrow().message.contains("Cannot install"),
                "an extension with no requires_vimcode field must never be \
                 refused by the new gate; message: {:?}",
                h.engine.borrow().message
            );

            drop(h);
            drop(home_guard);
            let _ = std::fs::remove_dir_all(&home);
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // AI panel persistent send/stop/leave hint + `<leader>ai` focus
    // toggle
    // ─────────────────────────────────────────────────────────────────────
    mod ai_panel_hint_and_focus_toggle {
        use super::*;

        /// Build a fixture with the AI panel already the active sidebar
        /// panel, widened well past the default 20-cell content rect via
        /// the real `Alt+Right` "resize sidebar" gesture
        /// (`render::alt_resized_sidebar_width`) so the persistent hint's
        /// full text has room to paint instead of being clipped mid-word —
        /// the same clipping the built-in `TextInput` placeholder already
        /// suffers at the narrower default (confirmed by hand while writing
        /// this test: at the default width the placeholder itself reads
        /// "Type a m" before running out of room). 40 presses of the
        /// production `alt_resized_sidebar_width` (`current + 1`, clamped
        /// at `ALT_SIDEBAR_WIDTH_MAX = 150`) takes the 20-cell default to
        /// 60, comfortably past this hint's ~57-character width.
        pub(super) fn ai_panel_harness_widened() -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            h.driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            h
        }

        /// The send/stop/leave hint
        /// `render::populate_ai_chat_controller` pins to
        /// `ChatController::set_hint` must (a) actually reach the painted
        /// screen once the panel has keyboard focus and (b) stay on screen
        /// once the user starts typing a message — unlike the `TextInput`
        /// placeholder, which vanishes the moment the input buffer is
        /// non-empty. Focus is driven via the real `<leader>ai` key sequence
        /// so the test exercises the same production key-dispatch path a
        /// user's keystrokes take.
        #[test]
        fn hint_reaches_screen_and_survives_typing_via_shell_app() {
            let mut h = ai_panel_harness_widened();
            h.driver.type_char(' ');
            h.driver.type_char('a');
            h.driver.type_char('i');
            h.driver.render();

            assert!(
                h.engine.borrow().ai_has_focus,
                "setup: <leader>ai must focus the AI panel"
            );
            let screen = h.driver.screen();
            assert!(
                screen.contains("send") && screen.contains("stop") && screen.contains("editor"),
                "the persistent hint must paint the send/stop/leave keys \
                 once the panel has focus; screen:\n{screen}"
            );

            // The hint must persist after the user starts composing a message,
            // unlike the empty-input placeholder.
            for c in "hi".chars() {
                h.driver.type_char(c);
            }
            h.driver.render();
            let screen = h.driver.screen();
            assert!(
                screen.contains("send") && screen.contains("stop") && screen.contains("editor"),
                "the persistent hint must stay on screen while typing (the \
                 whole point of #1507 vs the old placeholder); screen:\n{screen}"
            );
        }

        /// Pressing `<leader>ai` again while the AI panel already has
        /// keyboard focus and its input is empty toggles focus back to the
        /// editor. This proves focus genuinely returned to the editor,
        /// not merely that a flag flipped: a subsequent `i` keystroke must
        /// enter the editor's own Insert mode (painted in the window status
        /// line's mode segment) — if focus were still on the chat panel,
        /// that `i` would be typed as literal text instead.
        #[test]
        fn leader_ai_toggles_focus_back_to_editor_via_shell_app() {
            let mut h = ai_panel_harness_widened();
            h.driver.type_char(' ');
            h.driver.type_char('a');
            h.driver.type_char('i');
            h.driver.render();
            assert!(
                h.engine.borrow().ai_has_focus,
                "setup: <leader>ai must focus the AI panel"
            );

            // Pressed again while the input is empty and the panel already
            // has focus: toggle back to the editor instead of typing
            // " ai" into the chat message.
            h.driver.type_char(' ');
            h.driver.type_char('a');
            h.driver.type_char('i');
            h.driver.render();

            assert!(
                !h.engine.borrow().ai_has_focus,
                "<leader>ai pressed again (panel focused, input empty) must \
                 hand focus back to the editor"
            );
            assert_eq!(
                h.engine.borrow().ai_chat.borrow().input_text(),
                "",
                "the toggle-back gesture must not leave stray text in the \
                 chat input"
            );

            h.driver.type_char('i');
            h.driver.render();
            let screen = h.driver.screen();
            assert!(
                screen.contains("INSERT"),
                "pressing 'i' after the toggle must enter the editor's \
                 Insert mode, proving focus is really back on the editor \
                 and not still consuming keys into the chat input; \
                 screen:\n{screen}"
            );
        }

        /// Typing the literal characters ` ai` into an in-progress
        /// message must never be swallowed as the focus-toggle gesture —
        /// `Engine::ai_leader_toggle_key` only engages while the input is
        /// empty, precisely so a real message containing those characters
        /// is never at risk.
        ///
        /// RED verified: with the `!self.ai_chat.borrow().input_text().
        /// is_empty()` guard removed from the top of `ai_leader_toggle_key`,
        /// this fails — typing "say ai please" toggles focus back to the
        /// editor partway through and the input reads back without the
        /// swallowed " ai".
        #[test]
        fn leader_ai_sequence_is_literal_text_once_input_is_non_empty_via_shell_app() {
            let mut h = ai_panel_harness_widened();
            h.driver.type_char(' ');
            h.driver.type_char('a');
            h.driver.type_char('i');
            h.driver.render();
            assert!(
                h.engine.borrow().ai_has_focus,
                "setup: panel must be focused"
            );

            for c in "say ai please".chars() {
                h.driver.type_char(c);
            }
            h.driver.render();

            assert!(
                h.engine.borrow().ai_has_focus,
                "typing a message that happens to contain \" ai\" must not \
                 toggle focus away from the panel"
            );
            assert_eq!(
                h.engine.borrow().ai_chat.borrow().input_text(),
                "say ai please",
                "every character of the message, including \" ai\", must \
                 land in the chat input verbatim"
            );
        }

        /// A partial `<leader>ai` match interrupted by a key
        /// `Engine::ai_leader_toggle_key` never sees (any `quadraui::Key::Named`)
        /// must be replayed into the chat input rather than silently dropped.
        /// For example: type the leader (Space) then `a`, both buffered, then
        /// press Enter before typing the rest of the message. This test forces
        /// `ai_chat_submit_on_enter` to `false` so plain Enter inserts a newline
        /// rather than submitting, focusing on the leader-prefix replay rather
        /// than which key sends.
        #[test]
        fn leader_prefix_interrupted_by_enter_is_replayed_not_dropped_via_shell_app() {
            let mut h = ai_panel_harness_widened();
            h.engine.borrow_mut().settings.ai_chat_submit_on_enter = false;
            // Gain focus first via a full `<leader>ai` (this goes through
            // `Engine::handle_leader_key`'s "ai" arm, not
            // `ai_leader_toggle_key` — the panel doesn't have focus yet, so
            // `route_ai_chat_event` isn't even reached). Only once focused
            // does typing the leader sequence again start the *toggle-back*
            // gesture this test is about.
            h.driver.type_char(' ');
            h.driver.type_char('a');
            h.driver.type_char('i');
            h.driver.render();
            assert!(
                h.engine.borrow().ai_has_focus,
                "setup: <leader>ai must focus the AI panel"
            );

            // Start a fresh toggle-back match, but interrupt it after 2 of
            // its 3 keys with Enter.
            h.driver.type_char(' ');
            h.driver.type_char('a');
            h.driver.render();
            assert_eq!(
                h.engine.borrow().ai_chat.borrow().input_text(),
                "",
                "setup: a partial match must not be visible in the input yet"
            );

            h.driver.press_named(quadraui::NamedKey::Enter);
            for c in "rest".chars() {
                h.driver.type_char(c);
            }
            h.driver.render();

            assert!(
                h.engine.borrow().ai_has_focus,
                "an interrupted partial match must not toggle focus"
            );
            assert_eq!(
                h.engine.borrow().ai_chat.borrow().input_text(),
                " a\nrest",
                "the buffered ' a' prefix must be replayed into the input \
                 before the interrupting Enter's newline, not silently \
                 dropped"
            );
        }

        /// A partial match abandoned via Escape (leaving the panel) must not
        /// survive to wrongly complete against an unrelated later message once
        /// the panel regains focus. The buffer is discarded by two independent
        /// layers: `dispatch_ai_chat_event`'s `Cancelled` arm and
        /// `route_ai_chat_event`'s `Escape`-clears-pending branch.
        #[test]
        fn leader_prefix_abandoned_via_escape_does_not_leak_into_next_session_via_shell_app() {
            let mut h = ai_panel_harness_widened();
            // Gain focus first via a full `<leader>ai` — the panel doesn't
            // have keyboard focus yet, so typing it now goes through
            // `Engine::handle_leader_key`'s "ai" arm, not
            // `ai_leader_toggle_key`/`ai_leader_toggle_pending` at all.
            h.driver.type_char(' ');
            h.driver.type_char('a');
            h.driver.type_char('i');
            h.driver.render();
            assert!(
                h.engine.borrow().ai_has_focus,
                "setup: <leader>ai must focus the AI panel"
            );

            // Now, with the panel already focused, start a *toggle-back*
            // match (this is the one that uses `ai_leader_toggle_pending`),
            // then abandon it by leaving the panel via Escape (`Cancelled`)
            // before it completes.
            h.driver.type_char(' ');
            h.driver.type_char('a');
            h.driver.press_named(quadraui::NamedKey::Escape);
            h.driver.render();
            assert!(
                !h.engine.borrow().ai_has_focus,
                "setup: Escape must leave the panel"
            );

            // Re-enter the panel and type an unrelated message that starts
            // with the single character the abandoned match was still
            // waiting for ('i', the third character of "<leader>ai").
            h.engine.borrow_mut().ai_has_focus = true;
            h.driver.type_char('i');
            for c in "gnore this".chars() {
                h.driver.type_char(c);
            }
            h.driver.render();

            assert!(
                h.engine.borrow().ai_has_focus,
                "a stale, abandoned partial match must not resurrect itself \
                 and toggle focus off against this unrelated message"
            );
            assert_eq!(
                h.engine.borrow().ai_chat.borrow().input_text(),
                "ignore this",
                "every character of the new, unrelated message must land \
                 in the chat input verbatim"
            );
        }
    }

    mod issue_1508_ai_panel_busy_status {
        use super::*;
        use std::time::{Duration, Instant};

        /// The AI panel's status strip must show the live in-progress tool
        /// call's title plus an elapsed-time reading while busy, replacing the
        /// old frozen `"(thinking\u{2026})"` literal, and the busy `Spinner`
        /// icon must animate. The fixture's `$ACP_FAKE_TOOL_CALL_HANGS` announces
        /// one `in_progress` tool call and then never answers `session/prompt`,
        /// so the scenario stays busy indefinitely.
        ///
        /// The new status-strip text is distinguished from the pre-existing
        /// transcript line: the status strip is suffixed with `"\u{b7} <N>s"`
        /// elapsed time, while the transcript line has only a bracketed glyph.
        #[cfg(unix)]
        #[test]
        fn busy_status_shows_running_tool_call_and_elapsed_time_via_shell_app() {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));

            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec!["ACP_FAKE_TOOL_CALL_HANGS=1".to_string()],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();

            // A wide sidebar (same `Alt+Right` gesture/target width as
            // `ai_panel_hint_and_focus_toggle::ai_panel_harness_widened`)
            // — the default 20-cell content rect clips this status line's
            // longer text down to a bare glyph, well before the "execute:
            // Run the tests \u{b7}" substring below it.
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            {
                let driver = &mut h.driver;
                driver.press_named(quadraui::NamedKey::Escape);
                for _ in 0..40 {
                    driver.dispatch(quadraui::UiEvent::KeyPressed {
                        key: quadraui::Key::Named(quadraui::NamedKey::Right),
                        modifiers: quadraui::Modifiers {
                            alt: true,
                            ..Default::default()
                        },
                        repeat: false,
                    });
                }
            }
            let driver = &mut h.driver;

            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(6);
            let mut screen = driver.screen();
            while !screen.contains("execute: Run the tests \u{b7}") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("execute: Run the tests \u{b7}"),
                "the status strip must show the live tool call's title \
                 plus an elapsed-time separator, distinct from the \
                 transcript's own bracketed `[~] execute: ...` line; \
                 screen:\n{screen}"
            );
            assert!(
                screen.contains("[~] execute: Run the tests"),
                "sanity: the transcript's own summary line must still \
                 paint too, unaffected by the status-strip change; \
                 screen:\n{screen}"
            );

            // The spinner glyph painted immediately before "execute: Run
            // the tests" must actually move on the *rendered surface* —
            // not just some internal engine counter (that was this
            // review's finding: an `Engine::ai_spinner_frame` assertion
            // can stay green even if `ChatController::render`/the TUI
            // rasteriser stops repainting the glyph, e.g. a stale
            // `set_spinner_frame` call or a layout cache suppressing the
            // repaint). Extract the literal glyph character painted on
            // `driver.screen()` right before the tool-call title and
            // sample it across several driven ticks.
            fn painted_spinner_glyph(screen: &str) -> char {
                let line = screen
                    .lines()
                    .find(|l| l.contains("execute: Run the tests"))
                    .unwrap_or_else(|| {
                        panic!(
                            "expected a screen line containing \
                             \"execute: Run the tests\"; screen:\n{screen}"
                        )
                    });
                let idx = line.find("execute: Run the tests").unwrap();
                line[..idx]
                    .trim_end()
                    .chars()
                    .next_back()
                    .unwrap_or_else(|| {
                        panic!(
                            "expected a spinner glyph immediately before \
                             \"execute: Run the tests\" on line {line:?}"
                        )
                    })
            }

            let glyph_a = painted_spinner_glyph(&driver.screen());
            let advance_deadline = Instant::now() + Duration::from_secs(2);
            let mut glyph_b = glyph_a;
            while glyph_b == glyph_a && Instant::now() < advance_deadline {
                driver.tick();
                driver.render();
                std::thread::sleep(Duration::from_millis(20));
                glyph_b = painted_spinner_glyph(&driver.screen());
            }
            assert_ne!(
                glyph_a, glyph_b,
                "the spinner glyph painted on screen right before the \
                 tool-call title must change every so often while a turn \
                 is streaming, not stay frozen at its starting glyph"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // AI panel adopts wrapping/auto-grow input and `Enter`-sends
    // (`submit_on_enter`) with a Send/Stop segment.
    // ─────────────────────────────────────────────────────────────────────
    mod issue_1509_ai_chat_submit_on_enter_and_stop_segment {
        use super::*;
        use std::time::{Duration, Instant};

        /// An ACP agent registry pointing at the shared echo fixture — every
        /// test below needs a live (fake) agent so a `Submit` actually
        /// completes a turn deterministically, with no real network call
        /// and no dependence on whether `ANTHROPIC_API_KEY` happens to be
        /// set in the ambient environment (`Engine::ai_send_message`'s
        /// direct-provider fallback would otherwise spawn a real `curl`).
        fn fixture_agent(extra_env: &[&str]) -> crate::core::acp::AcpAgentProfile {
            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: extra_env.iter().map(|s| s.to_string()).collect(),
                mcp_servers: Vec::new(),
            }
        }

        /// Same widening (`Alt+Right` x40) as
        /// `ai_panel_hint_and_focus_toggle::ai_panel_harness_widened`, plus
        /// a configured fixture agent — built fresh per test since each one
        /// wants different `extra_env`.
        fn widened_harness_with_agent(
            extra_env: &[&str],
        ) -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            engine.settings.acp_agents = vec![fixture_agent(extra_env)];
            engine.settings.acp_active_agent = "alpha".to_string();
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            h.driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            h
        }

        /// `ai_chat_submit_on_enter` defaults to `true` (Zed parity) — typing a
        /// message into the focused AI panel and pressing plain `Enter` must
        /// actually send it, not insert a newline. Proven by the fake agent's
        /// echoed "Hello world" reply reaching the screen: that text can only
        /// paint once `session/prompt` was actually dispatched.
        #[cfg(unix)]
        #[test]
        fn default_submit_on_enter_sends_message_on_plain_enter_via_shell_app() {
            let mut h = widened_harness_with_agent(&[]);
            let driver = &mut h.driver;

            driver.type_char(' ');
            driver.type_char('a');
            driver.type_char('i');
            driver.render();
            assert!(
                h.engine.borrow().ai_has_focus,
                "setup: <leader>ai must focus the AI panel"
            );

            for c in "hi".chars() {
                h.driver.type_char(c);
            }
            h.driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = h.driver.screen();
            while !screen.contains("Hello world") && Instant::now() < deadline {
                h.driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = h.driver.screen();
            }
            assert!(
                screen.contains("Hello world"),
                "plain Enter must submit the message by default (#1509) — \
                 the fake agent's echoed reply never reached the screen \
                 within the deadline; screen:\n{screen}"
            );
        }

        /// `ai_chat_submit_on_enter = false` must keep the pre-default behaviour
        /// — plain `Enter` inserts a newline instead of sending — while the
        /// always-available `Ctrl+S` chord still sends. This proves the setting
        /// genuinely gates the behaviour: the message must not reach the fake
        /// agent until `Ctrl+S` is pressed.
        #[cfg(unix)]
        #[test]
        fn submit_on_enter_false_keeps_enter_as_newline_via_shell_app() {
            let mut h = widened_harness_with_agent(&[]);
            h.engine.borrow_mut().settings.ai_chat_submit_on_enter = false;
            let driver = &mut h.driver;

            driver.type_char(' ');
            driver.type_char('a');
            driver.type_char('i');
            driver.render();
            assert!(
                h.engine.borrow().ai_has_focus,
                "setup: <leader>ai must focus the AI panel"
            );

            for c in "hi".chars() {
                h.driver.type_char(c);
            }
            h.driver.press_named(quadraui::NamedKey::Enter);
            h.driver.render();

            assert_eq!(
                h.engine.borrow().ai_chat.borrow().input_text(),
                "hi\n",
                "with the setting off, plain Enter must insert a newline \
                 into the input rather than submitting it"
            );

            // Give any wrongly-sent request a few ticks to arrive — it must
            // not, since nothing has submitted yet.
            for _ in 0..10 {
                h.driver.tick();
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                !h.driver.screen().contains("Hello world"),
                "the message must not have been sent yet — Enter only \
                 inserted a newline"
            );

            // `Ctrl+S` still sends in both `submit_on_enter` modes
            // (`ChatController::handle`'s own doc).
            h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char('s'),
                modifiers: quadraui::Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
                repeat: false,
            });

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = h.driver.screen();
            while !screen.contains("Hello world") && Instant::now() < deadline {
                h.driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = h.driver.screen();
            }
            assert!(
                screen.contains("Hello world"),
                "Ctrl+S must still submit the message even with \
                 ai_chat_submit_on_enter off; screen:\n{screen}"
            );
        }

        /// Clicking the Send/Stop segment while a turn is streaming (it reads
        /// "Stop" then) must abort the turn via the same `session/cancel` path
        /// as `Ctrl+C`. The fixture's `$ACP_FAKE_TOOL_CALL_HANGS` announces one
        /// `in_progress` tool call and then never answers `session/prompt`, so
        /// the scenario stays busy until this click cancels it. Asserts on
        /// rendered output: the transcript's `"[cancelled by user]"` line must
        /// paint, and the busy status line must be gone.
        #[cfg(unix)]
        #[test]
        fn clicking_stop_segment_cancels_the_turn_via_shell_app() {
            let mut h = widened_harness_with_agent(&["ACP_FAKE_TOOL_CALL_HANGS=1"]);
            let driver = &mut h.driver;

            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(6);
            let mut screen = driver.screen();
            while !screen.contains("execute: Run the tests \u{b7}") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("execute: Run the tests \u{b7}"),
                "setup: the fixture's hung tool call must show up as busy \
                 first; screen:\n{screen}"
            );

            let (x, y) = driver
                .find("Stop")
                .expect("the Send/Stop segment must read \"Stop\" while busy");
            driver.click(x, y);
            driver.render();

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("[cancelled by user]") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("[cancelled by user]"),
                "clicking the Stop segment must abort the turn via \
                 session/cancel, same as Ctrl+C; screen:\n{screen}"
            );
            assert!(
                !screen.contains("execute: Run the tests \u{b7}"),
                "the busy tool-call status line must be gone once the \
                 turn is cancelled; screen:\n{screen}"
            );
        }
    }

    /// Turn-end review, badge instead of auto-opening the
    /// full-screen review. TUI twin of `gtk::testing::
    /// issue_1515_acp_review_badge`'s two scenarios — see that module's
    /// own doc comments for the shared rationale, reused verbatim here.
    mod issue_1515_acp_review_badge {
        use super::*;

        /// A wide-enough (220x30, `Alt+Right` x40) harness with the AI
        /// panel shown — same widening `issue_1509_ai_chat_submit_on_
        /// enter_and_stop_segment::widened_harness_with_agent` uses, so
        /// the status-strip badge text isn't clipped by an 80-column
        /// terminal's much narrower sidebar.
        fn widened_ai_harness(
            engine: crate::core::Engine,
        ) -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            h.driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            h
        }

        /// GTK twin: `gtk::testing::issue_1515_acp_review_badge::
        /// badge_mode_suppresses_auto_open_and_shows_edited_summary_via_gtk_driver`
        /// — see that test's doc comment for the full rationale. Drives
        /// the same real `Engine::acp_write_text_file` + `Engine::
        /// acp_end_turn` calls (bypassing the ACP wire — the checkpoint
        /// bookkeeping under test doesn't care how it got populated), then
        /// asserts on the painted screen via the real `TuiDriver`, not
        /// `Engine::change_review` state.
        ///
        /// RED verified: with `Engine::acp_end_turn`'s `acp_review_on_
        /// turn_end` gate removed (always calling `acp_open_turn_review`,
        /// restoring the pre-#1515 behaviour), the full-viewport
        /// "Change 1/2" modal paints immediately after `acp_end_turn()`
        /// and the very first "must not auto-open" assertion below fails.
        #[test]
        fn badge_mode_suppresses_auto_open_and_shows_edited_summary_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1515_tui_badge_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let a = dir.join("a.txt");
            let b = dir.join("b.txt");
            std::fs::write(&a, "aaa1\n").unwrap();
            std::fs::write(&b, "bbb1\n").unwrap();

            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.settings.acp_review_on_turn_end =
                crate::core::settings::AcpReviewOnTurnEnd::Badge;
            engine.workspace_root = Some(dir.clone());
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            engine.acp_write_text_file(&a, "aaa2\nnew line\n").unwrap();
            engine.acp_write_text_file(&b, "bbb2\n").unwrap();
            engine.acp_end_turn();

            let mut h = widened_ai_harness(engine);
            h.driver.render();
            let screen = h.driver.screen();

            assert!(
                !screen.contains("Change 1/"),
                "badge mode must not auto-open the full-viewport turn \
                 review; screen:\n{screen}"
            );
            assert!(
                screen.contains("Edited 2 files"),
                "the status-strip badge must summarise how many files the \
                 turn touched; screen:\n{screen}"
            );
            assert!(
                screen.contains(":AiReview"),
                "the badge must name the command that opens the review; \
                 screen:\n{screen}"
            );

            h.driver.type_char(':');
            for c in "AiReview".chars() {
                h.driver.type_char(c);
            }
            h.driver.press_named(quadraui::NamedKey::Enter);
            h.driver.render();
            let screen = h.driver.screen();
            assert!(
                screen.contains("Change 1/2"),
                ":AiReview must open the turn review even in badge mode; \
                 screen:\n{screen}"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// Badge mode must paint gutter markers on agent-changed lines.
        /// Tested in a plain (non-git) temp directory, so any marker painted
        /// comes only from the ACP-turn overlay, never from `crate::core::git::compute_file_diff`.
        #[test]
        fn badge_mode_paints_gutter_markers_on_agent_changed_lines_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1515_tui_gutter_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let a = dir.join("a.txt");
            std::fs::write(&a, "one\ntwo\nthree\n").unwrap();

            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.settings.acp_review_on_turn_end =
                crate::core::settings::AcpReviewOnTurnEnd::Badge;
            engine.settings.line_numbers = crate::core::settings::LineNumberMode::Absolute;
            engine.workspace_root = Some(dir.clone());
            engine
                .acp_write_text_file(&a, "one\nTWO CHANGED\nthree\n")
                .unwrap();
            engine.acp_end_turn();
            let buf_id = engine.buffer_manager.open_file(&a).unwrap();
            let win_id = engine.active_window_id();
            engine.windows.get_mut(&win_id).unwrap().buffer_id = buf_id;

            let mut h = widened_ai_harness(engine);
            h.driver.render();
            let screen = h.driver.screen();

            assert!(
                screen.contains("\u{258c}"),
                "an agent-changed line must paint the same gutter marker \
                 glyph a real git-diff line uses, even outside a git repo; \
                 screen:\n{screen}"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// Off mode's contract is "neither the modal nor the badge/gutter nudge"
        /// — only an explicit `:AiReview` shows anything.
        #[test]
        fn off_mode_paints_no_gutter_markers_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1515_tui_off_gutter_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let a = dir.join("a.txt");
            std::fs::write(&a, "one\ntwo\nthree\n").unwrap();

            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.settings.acp_review_on_turn_end = crate::core::settings::AcpReviewOnTurnEnd::Off;
            engine.settings.line_numbers = crate::core::settings::LineNumberMode::Absolute;
            engine.workspace_root = Some(dir.clone());
            engine
                .acp_write_text_file(&a, "one\nTWO CHANGED\nthree\n")
                .unwrap();
            engine.acp_end_turn();
            let buf_id = engine.buffer_manager.open_file(&a).unwrap();
            let win_id = engine.active_window_id();
            engine.windows.get_mut(&win_id).unwrap().buffer_id = buf_id;

            let mut h = widened_ai_harness(engine);
            h.driver.render();
            let screen = h.driver.screen();

            assert!(
                !screen.contains("\u{258c}"),
                "off mode must not paint the ACP-turn gutter overlay at \
                 all; screen:\n{screen}"
            );
            assert!(
                !screen.contains("Edited 1 file"),
                "off mode must not show the status-strip badge either; \
                 screen:\n{screen}"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// In-buffer review of agent hunks — a virtual `[a] keep  [r]
    /// reject` action row painted right after each hunk in the *normal*
    /// editor view (no full-viewport modal), `<leader>ak`/`<leader>ar`
    /// acting on the hunk under the cursor, freely editable in between.
    /// GTK twin: `gtk::testing::issue_1517_acp_inline_review`.
    mod issue_1517_acp_inline_review {
        use super::*;

        /// Opens `path` in the active window under `badge` mode after a
        /// single agent write — the shared setup every scenario below
        /// starts from. Returns `(engine, buf_id)`.
        fn engine_with_buffer_open(
            dir: &std::path::Path,
            path: &std::path::Path,
        ) -> crate::core::Engine {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.settings.acp_review_on_turn_end =
                crate::core::settings::AcpReviewOnTurnEnd::Badge;
            engine.settings.line_numbers = crate::core::settings::LineNumberMode::Absolute;
            engine.workspace_root = Some(dir.to_path_buf());
            engine
                .acp_write_text_file(path, "one\nTWO CHANGED\nthree\nfour\n")
                .unwrap();
            engine.acp_end_turn();
            let buf_id = engine.buffer_manager.open_file(path).unwrap();
            let win_id = engine.active_window_id();
            engine.windows.get_mut(&win_id).unwrap().buffer_id = buf_id;
            engine
        }

        /// The virtual action row paints in the normal buffer view
        /// — no modal, no full-viewport surface — and the status message
        /// left by `Engine::acp_end_turn` invites `:AiReview` / `ga`
        /// as the alternative full-review path.
        #[test]
        fn virtual_action_row_paints_in_the_normal_buffer_view_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1517_tui_row_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let a = dir.join("a.txt");
            std::fs::write(&a, "one\ntwo\nthree\nfour\n").unwrap();

            let engine = engine_with_buffer_open(&dir, &a);
            assert!(
                engine.message.contains("1 agent hunk"),
                "unexpected message: {}",
                engine.message
            );

            let mut h = crate::tui_main::testing::conformance_harness(engine, 120, 30);
            h.driver.render();
            let screen = h.driver.screen();

            assert!(
                !screen.contains("Change 1/"),
                "the in-buffer surface must never open the full-viewport \
                 modal; screen:\n{screen}"
            );
            assert!(
                screen.contains("keep hunk") && screen.contains("reject hunk"),
                "the virtual action row must paint in the normal editor \
                 view; screen:\n{screen}"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// `<leader>ar` on the hunk under the cursor reverts it on disk
        /// and the action row disappears from the screen (nothing left to
        /// decide) — driven through the real key-dispatch path
        /// (`Engine::handle_leader_key`), not by calling the engine method
        /// directly.
        ///
        /// RED verified: with the `"ar"` leader-sequence arm removed from
        /// `keys.rs`, `<leader>ar` falls through as an unknown sequence and
        /// the on-disk content assertion below fails (still "TWO CHANGED").
        #[test]
        fn leader_ar_reverts_the_hunk_under_the_cursor_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1517_tui_ar_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let a = dir.join("a.txt");
            std::fs::write(&a, "one\ntwo\nthree\nfour\n").unwrap();

            let mut engine = engine_with_buffer_open(&dir, &a);
            engine.view_mut().cursor.line = 1; // the "TWO CHANGED" line

            let mut h = crate::tui_main::testing::conformance_harness(engine, 120, 30);
            h.driver.render();
            assert!(
                h.driver.screen().contains("reject hunk"),
                "setup: the action row must be visible before rejecting"
            );

            h.driver.type_char(' '); // leader
            h.driver.type_char('a');
            h.driver.type_char('r');
            h.driver.render();

            let on_disk = std::fs::read_to_string(&a).unwrap();
            assert_eq!(
                on_disk, "one\ntwo\nthree\nfour\n",
                "the hunk must be reverted to its pre-turn content on disk"
            );
            assert!(
                !h.driver.screen().contains("reject hunk"),
                "the action row must disappear once the only hunk is \
                 resolved; screen:\n{}",
                h.driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// `<leader>ak` keeps the hunk under the cursor — the file the
        /// agent already wrote is left untouched on disk, and the action
        /// row disappears the same way rejecting one does.
        ///
        /// RED verified: with the `"ak"` leader-sequence arm removed from
        /// `keys.rs`, `<leader>ak` falls through as an unknown sequence
        /// and the action row is never resolved — the "must disappear"
        /// assertion below fails (`"keep hunk"` is still on screen).
        #[test]
        fn leader_ak_keeps_the_hunk_under_the_cursor_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1517_tui_ak_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let a = dir.join("a.txt");
            std::fs::write(&a, "one\ntwo\nthree\nfour\n").unwrap();

            let mut engine = engine_with_buffer_open(&dir, &a);
            engine.view_mut().cursor.line = 1;

            let mut h = crate::tui_main::testing::conformance_harness(engine, 120, 30);
            h.driver.type_char(' ');
            h.driver.type_char('a');
            h.driver.type_char('k');
            h.driver.render();

            let on_disk = std::fs::read_to_string(&a).unwrap();
            assert_eq!(
                on_disk, "one\nTWO CHANGED\nthree\nfour\n",
                "keep must never touch disk — the agent's write is already correct"
            );
            assert!(
                !h.driver.screen().contains("keep hunk"),
                "the action row must disappear once the only hunk is \
                 resolved; screen:\n{}",
                h.driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// A human edit made to the hunk's range before deciding is what
        /// `<leader>ak` keeps — the action row's label switches to "(edited)"
        /// and disk ends up with the human's text, not the agent's original write.
        #[test]
        fn editing_a_hunk_before_keep_labels_it_edited_and_keeps_the_edit_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1517_tui_edit_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let a = dir.join("a.txt");
            std::fs::write(&a, "one\ntwo\nthree\nfour\n").unwrap();

            let mut engine = engine_with_buffer_open(&dir, &a);
            engine.view_mut().cursor.line = 1;

            let mut h = crate::tui_main::testing::conformance_harness(engine, 120, 30);
            h.driver.render();
            assert!(
                !h.driver.screen().contains("(edited)"),
                "setup: the hunk must not start out labelled edited"
            );

            // Human edits the agent's line in place, then saves — same
            // buffer-first contract every other turn-review read relies on.
            h.driver.type_char('A'); // append at end of "TWO CHANGED"
            for c in " BY HUMAN".chars() {
                h.driver.type_char(c);
            }
            h.driver.press_named(quadraui::NamedKey::Escape);
            for c in ":w".chars() {
                h.driver.type_char(c);
            }
            h.driver.press_named(quadraui::NamedKey::Enter);
            h.driver.render();

            assert!(
                h.driver.screen().contains("(edited)"),
                "the action row must relabel an edited hunk; screen:\n{}",
                h.driver.screen()
            );

            h.driver.type_char(' ');
            h.driver.type_char('a');
            h.driver.type_char('k');
            h.driver.render();

            let on_disk = std::fs::read_to_string(&a).unwrap();
            assert_eq!(
                on_disk, "one\nTWO CHANGED BY HUMAN\nthree\nfour\n",
                "keeping an edited hunk must keep the human's edit, not the \
                 agent's original write"
            );
            assert!(
                !h.driver.screen().contains("keep hunk"),
                "the action row must disappear once the only (edited) \
                 hunk is resolved — this is what actually proves \
                 `<leader>ak` ran, since keeping never touches disk; \
                 screen:\n{}",
                h.driver.screen()
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// Review-fix (iteration 1): `]c` must land on the *outstanding*
        /// in-buffer review hunk, not wherever the buffer's raw `git diff
        /// HEAD` markers happen to point — the ordinary case for a
        /// git-tracked file the agent just edited, where `has_git` in
        /// `Engine::jump_next_hunk` is `true` too (`git_diff` is populated
        /// on every buffer open, per `refresh_git_diff`). GTK twin:
        /// `gtk::testing::issue_1517_acp_inline_review::jump_next_hunk_
        /// prefers_the_outstanding_review_hunk_over_raw_git_diff_via_gtk_
        /// driver`.
        ///
        /// The fixture manufactures a case where the two sources
        /// genuinely disagree: turn 1 changes line 2 and is then *kept*
        /// (resolved, so no longer an outstanding review hunk) but never
        /// committed, so `git diff HEAD` still reports it; turn 2 changes
        /// line 8, the only hunk actually outstanding for review. `]c`
        /// from the top of the buffer must jump into that line-8 hunk
        /// (`quadraui::compute_hunks` pads it with surrounding unchanged
        /// context, so its `right_start` is line 5, not line 8 itself —
        /// 0-indexed line 4), never anywhere near the already-resolved
        /// line-2 change (0-indexed line 1).
        ///
        /// RED verified (manually, reverting `jump_next_hunk` to the old
        /// `if has_git { .. } else { <@@ fallback> }` order that made the
        /// in-buffer-review branch unreachable): with that ordering
        /// restored, `has_git` is `true` for this git-tracked, previously
        /// committed file, so `]c` lands on 0-indexed line 1 (the raw git
        /// diff's line-2 hunk) instead of line 4 (the actual outstanding
        /// review hunk) — this test's cursor-line assertion fails with
        /// `left: 1, right: 4`.
        #[test]
        fn jump_next_hunk_prefers_the_outstanding_review_hunk_over_raw_git_diff_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1517_tui_git_precedence_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();

            let run_git = |args: &[&str]| {
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(&dir)
                    .output()
                    .unwrap();
            };
            run_git(&["init"]);
            run_git(&["config", "user.email", "t@t.com"]);
            run_git(&["config", "user.name", "T"]);

            let a = dir.join("a.txt");
            let lines: Vec<String> = (1..=10).map(|n| format!("line{n}")).collect();
            std::fs::write(&a, format!("{}\n", lines.join("\n"))).unwrap();
            run_git(&["add", "."]);
            run_git(&["commit", "-m", "init"]);

            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.settings.acp_review_on_turn_end =
                crate::core::settings::AcpReviewOnTurnEnd::Badge;
            engine.workspace_root = Some(dir.clone());

            // Turn 1: agent changes line 2, turn ends, human keeps the hunk
            // (resolved but still uncommitted, so `git diff HEAD` still reports it).
            let mut turn1 = lines.clone();
            turn1[1] = "LINE2 CHANGED".to_string();
            engine
                .acp_write_text_file(&a, &format!("{}\n", turn1.join("\n")))
                .unwrap();
            engine.acp_end_turn();
            engine.acp_inline_review_keep_hunk_at_line(&a.to_string_lossy(), 2);

            // Turn 2: agent changes line 8 — the only hunk actually
            // outstanding for review afterward.
            let mut turn2 = turn1.clone();
            turn2[7] = "LINE8 CHANGED".to_string();
            engine
                .acp_write_text_file(&a, &format!("{}\n", turn2.join("\n")))
                .unwrap();
            engine.acp_end_turn();

            // Open through the real path so `refresh_git_diff` populates
            // `buffer_state.git_diff` from disk exactly like a normal
            // file open would — `git diff HEAD` at this point covers both
            // the (resolved) line-2 change and the (outstanding) line-8
            // change, since neither has been committed.
            engine
                .open_file_with_mode(&a, crate::core::engine::OpenMode::Permanent)
                .unwrap();

            let mut h = crate::tui_main::testing::conformance_harness(engine, 120, 30);
            h.driver.render();

            h.driver.type_char(']');
            h.driver.type_char('c');
            h.driver.render();

            let cursor_line = h.engine.borrow().view().cursor.line;
            assert_eq!(
                cursor_line, 4,
                "`]c` must land inside the outstanding review hunk \
                 (0-indexed line 4, the context-padded start of the \
                 line-8 change), not the raw git diff's line-2 hunk \
                 (0-indexed line 1)"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Hunk-level Keep/Reject on the ACP turn-review surface — `a`/
    /// `r` act on the hunk under the cursor, not the whole file. GTK twin:
    /// `gtk::testing::issue_1516_hunk_level_review`.
    mod issue_1516_hunk_level_review {
        use super::*;

        /// Widened (220x30) harness — the change-review surface is
        /// full-viewport (unlike the badge/gutter overlay `issue_1515_acp_
        /// review_badge::widened_ai_harness` widens the sidebar column
        /// for), so a plain wider terminal is all the new, longer
        /// hunk-level footer legend needs to paint without being clipped;
        /// no sidebar-collapse or panel-resize keypresses required. Also
        /// deliberately no leading `Escape`: this module opens the
        /// turn-review surface with `AcpReviewOnTurnEnd::Auto` *before*
        /// the harness is built, and an Escape here would immediately
        /// close the very surface under test
        /// (`Engine::handle_change_review_key`'s `Escape`/`q` arm).
        fn widened_harness(
            engine: crate::core::Engine,
        ) -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            crate::tui_main::testing::conformance_harness(engine, 220, 30)
        }

        /// A single file with two well-separated agent edits lands in two
        /// hunks; keeping hunk 0 (`a`) then reverting hunk 1 (`]` then `r`)
        /// leaves a genuinely mixed result on disk and the footer legend visible.
        #[test]
        fn keep_one_hunk_reject_the_other_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1516_tui_hunks_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let a = dir.join("a.txt");
            let lines: Vec<String> = (1..=20).map(|n| n.to_string()).collect();
            std::fs::write(&a, format!("{}\n", lines.join("\n"))).unwrap();

            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.settings.acp_review_on_turn_end =
                crate::core::settings::AcpReviewOnTurnEnd::Auto;
            engine.workspace_root = Some(dir.clone());
            let mut changed = lines;
            changed[2] = "AGENT-A".to_string();
            changed[15] = "AGENT-B".to_string();
            engine
                .acp_write_text_file(&a, &format!("{}\n", changed.join("\n")))
                .unwrap();
            engine.acp_end_turn();

            let mut h = widened_harness(engine);
            h.driver.render();
            let screen = h.driver.screen();
            assert!(
                screen.contains("a=keep-hunk") && screen.contains("r=revert-hunk"),
                "the new hunk-level footer legend must paint; screen:\n{screen}"
            );

            h.driver.type_char('a'); // keep hunk 0
            h.driver.render();
            assert!(
                h.driver.screen().contains("Change 1/1"),
                "hunk 1 is still pending — the review must still be open"
            );

            h.driver.type_char(']'); // move to hunk 1
            h.driver.type_char('r'); // revert hunk 1
            h.driver.render();

            let on_disk = std::fs::read_to_string(&a).unwrap();
            assert!(
                on_disk.contains("AGENT-A"),
                "the kept hunk's agent content must remain: {on_disk:?}"
            );
            assert!(
                !on_disk.contains("AGENT-B"),
                "the reverted hunk must be gone: {on_disk:?}"
            );
            assert!(
                on_disk.contains("\n16\n"),
                "the reverted hunk's line must be back to its pre-turn value \"16\": {on_disk:?}"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// AI panel assistant/thought turns render as markdown
    /// (`render_markdown_to_styled`) instead of raw markdown source; thought
    /// turns additionally collapse to a fixed one-line "Thinking..."
    /// summary. GTK twin: `gtk::testing::issue_1510_ai_panel_markdown_
    /// rendering`.
    mod issue_1510_ai_panel_markdown_rendering {
        use super::*;
        use std::time::{Duration, Instant};

        /// A wide (220x30, `Alt+Right` x40 — same widening every other AI
        /// panel test in this file uses) harness with a fixture agent
        /// configured to reply with `$ACP_FAKE_MARKDOWN_REPLY`'s markdown
        /// thought/message chunks and no follow-up tool-call machinery
        /// (`$ACP_FAKE_NO_TOOL_REQUEST`), so the turn ends cleanly.
        fn widened_markdown_harness() -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec![
                    "ACP_FAKE_MARKDOWN_REPLY=1".to_string(),
                    "ACP_FAKE_NO_TOOL_REQUEST=1".to_string(),
                ],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            h.driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            h
        }

        /// The assistant's markdown reply must paint as rendered markdown —
        /// headings and bold visible, with no literal `#`/`**` syntax
        /// characters on screen — proving the transcript goes through
        /// `quadraui::render_markdown_to_styled` rather than plain text.
        #[cfg(unix)]
        #[test]
        fn assistant_markdown_reply_renders_stripped_and_styled_via_shell_app() {
            let mut h = widened_markdown_harness();
            let driver = &mut h.driver;

            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("Heading") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("Heading"),
                "the rendered heading text must reach the screen; \
                 screen:\n{screen}"
            );
            assert!(
                screen.contains("bold"),
                "the rendered bold text must reach the screen; \
                 screen:\n{screen}"
            );
            assert!(
                !screen.contains("# Heading"),
                "the raw '#' heading marker must never reach the screen — \
                 markdown must be rendered, not shown verbatim; \
                 screen:\n{screen}"
            );
            assert!(
                !screen.contains("**bold**"),
                "the raw '**' emphasis markers must never reach the \
                 screen — markdown must be rendered, not shown verbatim; \
                 screen:\n{screen}"
            );
        }

        /// A thought turn's markdown content must never reach the screen —
        /// thought turns collapse by default to a fixed one-line "Thinking..."
        /// summary.
        #[cfg(unix)]
        #[test]
        fn thought_turn_collapses_to_thinking_summary_via_shell_app() {
            let mut h = widened_markdown_harness();
            let driver = &mut h.driver;

            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("Thinking") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("Thinking"),
                "a thought turn must collapse to a one-line 'Thinking...' \
                 summary; screen:\n{screen}"
            );
            assert!(
                !screen.contains("Pondering"),
                "the thought turn's own markdown content must not reach \
                 the screen while collapsed; screen:\n{screen}"
            );
            assert!(
                !screen.contains("deeply"),
                "the thought turn's own markdown content must not reach \
                 the screen while collapsed; screen:\n{screen}"
            );
        }

        /// Routing transcript turns through the markdown
        /// renderer must NOT cost plain replies their word wrapping.
        ///
        /// `ChatController::build_transcript_rows` picks its wrap policy
        /// off `ChatTurn::line_scales`: turns that carry per-line scales
        /// wrap with `WrapPolicy::Char` (mid-word, exactly at the
        /// display-width budget), turns without them word-wrap via
        /// `text_util::word_wrap`. Handing quadraui styled rows for a
        /// message containing no markdown at all therefore chopped every
        /// plain agent reply mid-word in the narrow default AI panel
        /// (`"Hello world ANSWE"` / `"RED1519"`), which is why
        /// `render::markdown_turn_styled_cached` returns empty
        /// `line_scales` when the render came back with nothing to style.
        ///
        /// Seeds the transcript on the `Engine` before the harness takes
        /// it (the driver owns the engine afterwards) — the same
        /// seeded-transcript shape `gtk::testing::…::
        /// ai_panel_scrolls_transcript` uses — and asserts each word of a
        /// reply long enough to need three rows survives whole. RED
        /// verified: with the `has_styling` branch removed from
        /// `markdown_turn_styled_cached` (so every turn carries
        /// `line_scales`), "charlie" is painted as "charl"/"ie" across two
        /// rows and this fails.
        #[test]
        fn plain_reply_word_wraps_not_mid_word_in_narrow_panel_via_shell_app() {
            let mut engine = plain_engine();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            for (role, content) in [
                ("user", "wrap me"),
                (
                    "assistant",
                    "alpha bravo charlie delta echo foxtrot golf hotel",
                ),
            ] {
                engine
                    .acp_mut()
                    .ai_messages
                    .push(crate::core::ai::AiMessage {
                        role: role.to_string(),
                        content: content.to_string(),
                    });
            }
            let mut h = harness(engine);
            h.driver.press_named(quadraui::NamedKey::Escape);
            let screen = h.driver.screen();

            for word in [
                "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel",
            ] {
                assert!(
                    screen.contains(word),
                    "a plain (non-markdown) reply must word-wrap, so every \
                     word stays whole on some row — {word:?} was split \
                     across rows; screen:\n{screen}"
                );
            }
        }

        /// Same widened harness as [`widened_markdown_harness`], but the
        /// fixture completes plainly (no `$ACP_FAKE_MARKDOWN_REPLY`) and
        /// `ai_messages` is pre-seeded with a cancelled turn 1 before the
        /// harness takes ownership of the engine — the fixture for
        /// `cancelled_turn_notice_stays_visible_after_a_later_reply_via_
        /// shell_app` below.
        fn widened_cancelled_then_reply_harness() -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec!["ACP_FAKE_NO_TOOL_REQUEST=1".to_string()],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();
            // Stand in for a turn 1 that was cancelled: a real user prompt
            // followed by the exact notice `Engine::acp_cancel_turn` pushes.
            engine.acp_mut().ai_messages = vec![
                crate::core::ai::AiMessage {
                    role: "user".to_string(),
                    content: "first, cancelled".to_string(),
                },
                crate::core::ai::AiMessage {
                    role: "assistant-thought".to_string(),
                    content: "[cancelled by user]".to_string(),
                },
            ];
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            h.driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            h
        }

        /// Review regression: `is_genuine_thought_chunk`'s original
        /// "does any LATER message in `ai_messages` have role `assistant`"
        /// heuristic scanned the *whole rest* of the conversation, not just
        /// the same turn — so a turn cancelled/stopped/failed mid-
        /// conversation (pushing its notice under `"assistant-thought"`)
        /// got permanently re-collapsed into "Thinking…" the instant a
        /// *later*, unrelated turn in the same session got a normal
        /// `"assistant"` reply. `Engine::acp_cancel_turn`/
        /// `AcpEvent::PromptStopped` (non-`end_turn`)/
        /// `AcpEvent::RequestFailed` never tear down the ACP session, so
        /// continuing the conversation after one of these is a normal,
        /// supported flow — not an edge case. GTK twin:
        /// `gtk::testing::issue_1510_ai_panel_markdown_rendering::
        /// cancelled_turn_notice_stays_visible_after_a_later_reply_via_
        /// gtk_driver`.
        ///
        /// Seeds a `"[cancelled by user]"` notice directly onto
        /// `ai_messages` (the same technique `plain_reply_word_wraps_not_
        /// mid_word_in_narrow_panel_via_shell_app` above uses to test the
        /// render layer independent of whatever transport produced the
        /// content) to stand in for turn 1's cancellation, then drives a
        /// REAL second turn through the fixture agent and asserts the
        /// seeded notice is still visible, verbatim, once the real reply
        /// lands.
        ///
        /// RED verified: reverting `is_genuine_thought_chunk` to its
        /// pre-fix `ai_messages[idx + 1..].iter().any(|m| m.role ==
        /// "assistant")` (scanning to the end of the conversation instead
        /// of stopping at the next `"user"` message) makes this fail —
        /// once turn 2's "Hello world" reply lands, the unscoped scan
        /// finds *that* assistant message and re-collapses turn 1's
        /// "[cancelled by user]" notice into "Thinking…", so the final
        /// `screen.contains("[cancelled by user]")` assertion below fails.
        #[cfg(unix)]
        #[test]
        fn cancelled_turn_notice_stays_visible_after_a_later_reply_via_shell_app() {
            let mut h = widened_cancelled_then_reply_harness();
            let driver = &mut h.driver;
            let screen = driver.screen();
            assert!(
                screen.contains("[cancelled by user]"),
                "setup: the seeded cancellation notice must paint in full \
                 before any second turn; screen:\n{screen}"
            );

            driver.type_char(':');
            for c in "AI second, real reply".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("Hello world") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("Hello world"),
                "setup: the second turn must actually complete with a \
                 real assistant reply; screen:\n{screen}"
            );
            assert!(
                screen.contains("[cancelled by user]"),
                "turn 1's cancellation notice must stay visible verbatim \
                 even after a later, unrelated turn gets a normal \
                 assistant reply — it must never be re-collapsed into \
                 \"Thinking…\"; screen:\n{screen}"
            );
        }
    }

    mod issue_1511_ai_panel_tool_call_cards {
        use super::*;
        use std::time::{Duration, Instant};

        /// A wide (220x32, `Alt+Right` x40 — same widening
        /// `issue_1510_ai_panel_markdown_rendering`'s harnesses use, so a
        /// card's title/location text never word-wraps and confuses a
        /// `screen.contains`/ordering check) harness with a fixture agent
        /// configured to reply via `$ACP_FAKE_TOOL_CALL_CARD` — see that
        /// env var's doc in `tests/fixtures/fake_acp_agent.sh` for the exact
        /// sequence (a `tool_call` carrying `rawInput`, a `tool_call_update`
        /// adding `rawOutput`, then a SECOND thought+message pair emitted
        /// afterward).
        ///
        /// 32 rows, not the 30 these tests were written against.
        /// `App::shell_config` now opts into quadraui's
        /// `with_command_line()`/`with_status_bar()` bands so the activity
        /// bar's bottom-pinned Settings button stops above vimcode's own
        /// status/command-line rows instead of being painted underneath
        /// them — and `AppShell::compute_layout` carves those same two
        /// rows out of `sidebar_content_bounds` too, which is where the AI
        /// panel (and so the tool-call card) paints. Two extra terminal
        /// rows give the panel back the exact viewport height it had
        /// before that reservation, so
        /// `tool_call_card_collapses_by_default_and_toggles_via_click_via_shell_app`
        /// can still see the expanded card's title line (the click target
        /// for the collapse half) rather than having it scrolled off the
        /// panel's top edge. Nothing about what these three tests assert
        /// changes with the taller terminal.
        fn widened_tool_call_card_harness() -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec!["ACP_FAKE_TOOL_CALL_CARD=1".to_string()],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();
            // 32, not 30 — see this fixture's doc.
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 32);
            h.driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            h
        }

        /// Send `"AI hi"` and block until the fixture's second reply
        /// ("Goodbye") has painted — i.e. the whole scripted turn (both
        /// replies AND the tool call between them) has fully streamed in.
        fn send_and_wait_for_turn_end(
            driver: &mut quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        ) -> String {
            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("Goodbye") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("Goodbye"),
                "setup: the fixture's second reply must paint within 5s; \
                 screen:\n{screen}"
            );
            screen
        }

        /// A tool-call card renders collapsed by default
        /// (just the `{glyph} {kind}: {title}` line — no `rawInput`/
        /// `rawOutput`), and a click expands the card to show both,
        /// then a second click collapses it again.
        #[cfg(unix)]
        #[test]
        fn tool_call_card_collapses_by_default_and_toggles_via_click_via_shell_app() {
            let mut h = widened_tool_call_card_harness();
            let driver = &mut h.driver;
            let screen = send_and_wait_for_turn_end(driver);

            assert!(
                screen.contains("edit: Edit files"),
                "the collapsed card's title line must paint; screen:\n{screen}"
            );
            assert!(
                !screen.contains("bytesWritten"),
                "a collapsed card must not show its rawOutput; \
                 screen:\n{screen}"
            );
            assert!(
                !screen.contains("Input:") && !screen.contains("Output:"),
                "a collapsed card must not show its rawInput/rawOutput \
                 section headers either; screen:\n{screen}"
            );

            driver.click_text("edit: Edit files");
            driver.tick();
            let screen = driver.screen();
            assert!(
                screen.contains("bytesWritten"),
                "clicking the collapsed card must expand it, revealing its \
                 rawOutput; screen:\n{screen}"
            );
            assert!(
                screen.contains("Input:") && screen.contains("Output:"),
                "the expanded card must label its rawInput/rawOutput \
                 sections; screen:\n{screen}"
            );

            driver.click_text("edit: Edit files");
            driver.tick();
            let screen = driver.screen();
            assert!(
                !screen.contains("bytesWritten"),
                "clicking an expanded card must collapse it again; \
                 screen:\n{screen}"
            );
        }

        /// The tool-call card renders between the two replies it
        /// chronologically belongs between, not after the whole conversation —
        /// maintaining strict chronological order as messages stream in.
        #[cfg(unix)]
        #[test]
        fn tool_call_card_interleaves_chronologically_between_replies_via_shell_app() {
            let mut h = widened_tool_call_card_harness();
            let driver = &mut h.driver;
            let screen = send_and_wait_for_turn_end(driver);

            let pos_hello = screen
                .find("Hello world")
                .expect("the first reply must paint");
            let pos_card = screen
                .find("edit: Edit files")
                .expect("the tool-call card must paint");
            let pos_goodbye = screen.find("Goodbye").expect("the second reply must paint");

            assert!(
                pos_hello < pos_card,
                "the tool call must paint after the first reply that \
                 preceded it over the wire; screen:\n{screen}"
            );
            assert!(
                pos_card < pos_goodbye,
                "the tool call must paint before the second reply that \
                 followed it over the wire — not after the whole \
                 conversation; screen:\n{screen}"
            );
        }

        /// `Tab` moves keyboard focus onto the tool-call card, and plain
        /// `Enter` toggles it open — the keyboard alternative to clicking.
        #[cfg(unix)]
        #[test]
        fn tab_enter_toggles_the_focused_tool_call_card_via_shell_app() {
            let mut h = widened_tool_call_card_harness();
            let driver = &mut h.driver;
            let _ = send_and_wait_for_turn_end(driver);

            // Cycle keyboard focus with `Tab` until the tool-call card is
            // reached — the fixture's fixed transcript is: user, thought,
            // "Hello world", the card, thought, "Goodbye" (6 turns), so 4
            // `Tab`s land on the card (0-indexed turn 3).
            for _ in 0..4 {
                driver.press_named(quadraui::NamedKey::Tab);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.tick();

            let screen = driver.screen();
            assert!(
                screen.contains("bytesWritten"),
                "Tab-focusing the tool-call card then pressing Enter must \
                 expand it, revealing its rawOutput; screen:\n{screen}"
            );
        }

        /// Clicking inside an expanded card's body (as opposed to its
        /// header, which toggles it) jumps to the tool call's location.
        #[cfg(unix)]
        #[test]
        fn expanded_card_click_jumps_to_the_tool_calls_location_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "issue-1511-tool-call-location-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let target = dir.join("target.txt");
            std::fs::write(&target, "one\ntwo\nthree\nfour\n").unwrap();
            let target_str = target.to_string_lossy().into_owned();

            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.workspace_root = Some(dir.clone());
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec![
                    "ACP_FAKE_TOOL_CALL_CARD=1".to_string(),
                    format!("ACP_FAKE_TOOL_CALL_PATH={target_str}"),
                ],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            h.driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            let driver = &mut h.driver;
            let _ = send_and_wait_for_turn_end(driver);

            // Expand the card (row 1 — the title line).
            driver.click_text("edit: Edit files");
            driver.tick();
            let screen = driver.screen();
            assert!(
                screen.contains("bytesWritten"),
                "setup: the card must be expanded before its body is \
                 clickable; screen:\n{screen}"
            );

            // Click inside the expanded body (well past row 1) — must jump
            // to the call's location (`target.txt`, line 3) rather than
            // re-collapsing the card.
            driver.click_text("bytesWritten");
            driver.tick();

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains("Ln 3") && Instant::now() < deadline {
                driver.tick();
                screen = driver.screen();
            }
            assert!(
                screen.contains("target.txt"),
                "clicking the expanded card's body must open the call's \
                 location's file; screen:\n{screen}"
            );
            assert!(
                screen.contains("Ln 3"),
                "clicking the expanded card's body must jump the cursor to \
                 the call's location's line (3); screen:\n{screen}"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // "follow the agent" — `acp_follow_agent` + `:AiFollow`, and the
    // "session waiting on a permission" toast. GTK twin:
    // `gtk::testing::acp_follow_agent_reveals_tool_call_location_without_a_
    // click_via_gtk_driver` / `gtk::testing::
    // permission_request_toasts_when_ai_panel_is_unfocused_via_gtk_driver`.
    // ─────────────────────────────────────────────────────────────────────
    mod issue_1514_acp_follow_agent {
        use super::*;
        use std::time::{Duration, Instant};

        /// Core acceptance: with `acp_follow_agent` on, a `tool_call`'s own
        /// `locations` (`$ACP_FAKE_TOOL_CALL_CARD`, the same fixture
        /// scenario `issue_1511_ai_panel_tool_call_cards`'s sibling test
        /// drives via an explicit click) must reveal the file at its line
        /// **without any click at all** — `Engine::acp_upsert_tool_call` ->
        /// `Engine::acp_follow_reveal`, not the user-initiated
        /// `Engine::ai_open_tool_call_location` path. Reuses the same
        /// painted "Ln 3"/`target.txt` signal
        /// `expanded_card_click_jumps_to_the_tool_calls_location_via_shell_app`
        /// established as unambiguous proof the editor actually opened and
        /// jumped, not just that some engine field got set.
        ///
        /// Also asserts the chat input never loses keyboard focus
        /// (`ai_has_focus`) — the issue's "never steal focus from the chat
        /// input" requirement.
        ///
        /// RED verified: on unfixed `develop` (no `acp_follow_agent`
        /// setting, no `Engine::acp_follow_reveal` call site), this fails —
        /// the screen never shows `target.txt`/`"Ln 3"` since nothing opens
        /// the file without a click.
        #[cfg(unix)]
        #[test]
        fn acp_follow_agent_reveals_tool_call_location_without_a_click_via_shell_app() {
            let dir = std::env::temp_dir()
                .join(format!("issue-1514-follow-agent-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let target = dir.join("target.txt");
            std::fs::write(&target, "one\ntwo\nthree\nfour\n").unwrap();
            let target_str = target.to_string_lossy().into_owned();

            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.settings.acp_follow_agent = true;
            engine.workspace_root = Some(dir.clone());
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec![
                    "ACP_FAKE_TOOL_CALL_CARD=1".to_string(),
                    format!("ACP_FAKE_TOOL_CALL_PATH={target_str}"),
                ],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            h.driver.press_named(quadraui::NamedKey::Escape);

            h.driver.type_char(':');
            for c in "AI hi".chars() {
                h.driver.type_char(c);
            }
            h.driver.press_named(quadraui::NamedKey::Enter);

            // Deliberately never click the card — the reveal must happen
            // purely from the `tool_call`'s own `locations`.
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = h.driver.screen();
            while !screen.contains("Ln 3") && Instant::now() < deadline {
                h.driver.tick();
                screen = h.driver.screen();
            }
            assert!(
                screen.contains("target.txt"),
                "acp_follow_agent must open the tool call's location's \
                 file with no click at all; screen:\n{screen}"
            );
            assert!(
                screen.contains("Ln 3"),
                "acp_follow_agent must move the cursor to the tool call's \
                 location's line (3); screen:\n{screen}"
            );
            assert!(
                h.engine.borrow().ai_has_focus,
                "revealing a file automatically must never steal keyboard \
                 focus away from the chat input"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// `:AiFollow` toggles `settings.acp_follow_agent` and echoes the
        /// new state on the command line — the only UI this ex command has.
        ///
        /// RED verified: `:AiFollow` doesn't exist on unfixed `develop`, so
        /// the command line would show an "Unknown command" style message
        /// instead of either of the asserted strings.
        #[test]
        fn ai_follow_command_toggles_and_echoes_status_via_shell_app() {
            let engine = plain_engine();
            let mut h = harness(engine);
            assert!(!h.engine.borrow().settings.acp_follow_agent);

            h.driver.type_char(':');
            for c in "AiFollow".chars() {
                h.driver.type_char(c);
            }
            h.driver.press_named(quadraui::NamedKey::Enter);
            h.driver.render();
            assert!(h.engine.borrow().settings.acp_follow_agent);
            let screen = h.driver.screen();
            assert!(
                screen.contains("follow-the-agent mode: on"),
                "toggling :AiFollow on must echo confirmation on the \
                 command line; screen:\n{screen}"
            );

            h.driver.type_char(':');
            for c in "AiFollow".chars() {
                h.driver.type_char(c);
            }
            h.driver.press_named(quadraui::NamedKey::Enter);
            h.driver.render();
            assert!(!h.engine.borrow().settings.acp_follow_agent);
            let screen = h.driver.screen();
            assert!(
                screen.contains("follow-the-agent mode: off"),
                "toggling :AiFollow off again must echo confirmation; \
                 screen:\n{screen}"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // AI panel queues a message typed while the agent is busy.
    // GTK twin: `gtk::testing::issue_1512_queue_message_while_busy`.
    // ─────────────────────────────────────────────────────────────────────
    mod issue_1512_queue_message_while_busy {
        use super::*;
        use std::time::{Duration, Instant};

        /// Same widening + fixture-agent shape as
        /// `issue_1509_ai_chat_submit_on_enter_and_stop_segment::
        /// widened_harness_with_agent` — a fresh copy per module (each
        /// `mod issue_*` block in this file defines its own; see that
        /// module's doc) so this scenario can pick its own `extra_env`.
        /// `ACP_FAKE_TOOL_CALL_HANGS` keeps a turn busy indefinitely (the
        /// same fixture behaviour `issue_1508_ai_panel_busy_status`
        /// depends on) — deterministic, no race against a real completion.
        fn widened_harness_with_hanging_agent() -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec!["ACP_FAKE_TOOL_CALL_HANGS=1".to_string()],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            h.driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            h
        }

        fn ctrl_key(
            driver: &mut quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            c: char,
        ) {
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char(c),
                modifiers: quadraui::Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
                repeat: false,
            });
        }

        /// A message submitted while the agent is busy paints as a
        /// dimmed `"(queued) ..."` transcript turn plus a `"queued (1)"`
        /// status-strip segment. Ctrl+R discards it (relabels it `"(discarded)"`,
        /// drops the segment). A queued message sent with Ctrl+G cancels
        /// the busy turn and flips the queued turn to a plain undimmed line.
        #[cfg(unix)]
        #[test]
        fn queue_discard_and_send_now_via_shell_app() {
            let mut h = widened_harness_with_hanging_agent();
            {
                let driver = &mut h.driver;
                driver.type_char(' ');
                driver.type_char('a');
                driver.type_char('i');
                driver.render();
            }
            assert!(
                h.engine.borrow().ai_has_focus,
                "setup: <leader>ai must focus the AI panel"
            );

            for c in "first".chars() {
                h.driver.type_char(c);
            }
            h.driver.press_named(quadraui::NamedKey::Enter);

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = h.driver.screen();
            while !screen.contains("execute: Run the tests") && Instant::now() < deadline {
                h.driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = h.driver.screen();
            }
            assert!(
                screen.contains("execute: Run the tests"),
                "setup: the first turn must be genuinely busy on the wire \
                 before submitting a second message; screen:\n{screen}"
            );

            // ── Queue a second message while busy ───────────────────────
            for c in "second".chars() {
                h.driver.type_char(c);
            }
            h.driver.press_named(quadraui::NamedKey::Enter);
            h.driver.render();
            screen = h.driver.screen();
            assert!(
                screen.contains("(queued) second"),
                "a message submitted while busy must paint as a dimmed \
                 queued turn, not vanish silently; screen:\n{screen}"
            );
            // The status strip has a separate busy Spinner icon overlaid at the
            // right end, which may cover the trailing paren; the substring still
            // only matches when the segment's text is present.
            assert!(
                screen.contains("queued (1"),
                "the status strip must show the queued count; screen:\n{screen}"
            );

            // ── Ctrl+R discards it ───────────────────────────────────────
            ctrl_key(&mut h.driver, 'r');
            h.driver.render();
            screen = h.driver.screen();
            assert!(
                screen.contains("(queued) second (discarded)"),
                "Ctrl+R must relabel the queued turn as discarded, not \
                 remove it outright; screen:\n{screen}"
            );
            assert!(
                !screen.contains("queued (1)"),
                "the queued-count segment must clear once nothing is \
                 queued; screen:\n{screen}"
            );

            // ── Queue again, then Ctrl+G ("send now") ───────────────────
            for c in "third".chars() {
                h.driver.type_char(c);
            }
            h.driver.press_named(quadraui::NamedKey::Enter);
            h.driver.render();
            assert!(
                h.driver.screen().contains("(queued) third"),
                "sanity: the second queue must also paint before Ctrl+G; \
                 screen:\n{}",
                h.driver.screen()
            );

            ctrl_key(&mut h.driver, 'g');
            h.driver.render();
            screen = h.driver.screen();
            assert!(
                screen.contains("[cancelled by user]"),
                "Ctrl+G must cancel the current turn first, same as Ctrl+C \
                 while streaming; screen:\n{screen}"
            );
            assert!(
                !screen.contains("(queued) third"),
                "the sent turn must no longer paint as queued/dimmed; \
                 screen:\n{screen}"
            );
            assert!(
                screen.contains("third"),
                "the message itself must still be on screen, just no \
                 longer marked queued; screen:\n{screen}"
            );
            assert!(
                !screen.contains("queued (1)"),
                "nothing should still be queued after send-now consumed it; \
                 screen:\n{screen}"
            );
        }
    }

    mod issue_1513_ai_panel_plan_block {
        use super::*;

        /// Wide (220x30, `Alt+Right` x40 — same widening
        /// `issue_1511_ai_panel_tool_call_cards`'s harness uses, so the
        /// plan entries' text never word-wraps and confuses a
        /// `screen.contains` check) harness with the AI panel open and no
        /// ACP agent configured — `AcpSession::plan` is set directly, the
        /// same "source-agnostic model, no transport in play" contract
        /// `ai_panel_renders_plan_built_without_any_acp_transport` (GTK's
        /// twin of this test, `src/gtk/testing.rs`) already covers.
        fn widened_plan_harness() -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            engine.acp_mut().plan = vec![
                crate::core::acp::AcpPlanEntry {
                    content: "PLAN_STEP_DONE".to_string(),
                    status: crate::core::acp::AcpPlanEntryStatus::Completed,
                },
                crate::core::acp::AcpPlanEntry {
                    content: "PLAN_STEP_ACTIVE".to_string(),
                    status: crate::core::acp::AcpPlanEntryStatus::InProgress,
                },
            ];
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            h.driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            h
        }

        /// The plan checklist pins as its own collapsible block above the
        /// transcript instead of appending as a trailing turn. Collapsed
        /// (the default) shows only the in-progress entry plus `n/m` progress;
        /// clicking the block's header expands it to the full checklist.
        #[test]
        fn plan_block_collapses_to_in_progress_entry_and_expands_on_header_click_via_shell_app() {
            let mut h = widened_plan_harness();
            h.driver.render();

            let screen = h.driver.screen();
            assert!(
                screen.contains("Plan 1/2"),
                "the status strip must show n/m progress (1 of the 2 \
                 seeded entries is Completed); screen:\n{screen}"
            );
            assert!(
                screen.contains("PLAN_STEP_ACTIVE"),
                "the collapsed plan block must show the in-progress entry; \
                 screen:\n{screen}"
            );
            assert!(
                !screen.contains("PLAN_STEP_DONE"),
                "collapsed (the default) must not show the completed \
                 entry; screen:\n{screen}"
            );

            let plan_rect = h.engine.borrow().ai_plan_rect.get();
            assert!(
                plan_rect.width > 0.0 && plan_rect.height > 0.0,
                "the plan band must have painted a non-empty rect; got \
                 {plan_rect:?}"
            );
            h.driver.click(plan_rect.x + 1.0, plan_rect.y + 0.1);
            h.driver.render();

            let screen = h.driver.screen();
            assert!(
                screen.contains("PLAN_STEP_DONE"),
                "clicking the plan header must expand it to the full \
                 checklist; screen:\n{screen}"
            );
            assert!(
                screen.contains("PLAN_STEP_ACTIVE"),
                "the in-progress entry must still be visible expanded; \
                 screen:\n{screen}"
            );
        }
    }

    mod issue_1513_at_dir_and_at_symbol_mentions {
        use super::*;

        /// Wide (220x30, `Alt+Right` x40 — same widening every other
        /// AI-panel test in this file uses) harness with the AI panel open
        /// and `engine.cwd`/`workspace_root` pointed at a real temp
        /// directory, so `Engine::ai_mention_completions`' ignore-aware
        /// filesystem walk has something real to find.
        fn widened_ai_panel_harness(
            workspace: &std::path::Path,
        ) -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.cwd = workspace.to_path_buf();
            engine.workspace_root = Some(workspace.to_path_buf());
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            h.driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            // Real keyboard-focus path into the panel's own input box, not
            // `engine.ai_has_focus = true` directly — `<leader>ai` (default
            // leader Space), same production gesture
            // `leader_ai_toggles_focus_back_to_editor_via_shell_app` uses.
            h.driver.type_char(' ');
            h.driver.type_char('a');
            h.driver.type_char('i');
            h.driver.render();
            assert!(
                h.engine.borrow().ai_has_focus,
                "setup: <leader>ai must focus the AI panel input"
            );
            h
        }

        /// `@dir/` — typing `@` plus a matching directory prefix shows a
        /// trailing-slash directory candidate in the completion popup, and
        /// accepting it splices the literal `@subdir/ ` text into the input.
        #[test]
        fn at_dir_completion_popup_paints_and_accepts_via_shell_app() {
            let workspace = std::env::temp_dir()
                .join(format!("vimcode_test_1513_tui_dir_{}", std::process::id()));
            std::fs::create_dir_all(workspace.join("subdir")).expect("create test dir");
            std::fs::write(workspace.join("subdir/a.rs"), "").expect("write file");
            let mut h = widened_ai_panel_harness(&workspace);

            for c in "@subd".chars() {
                h.driver.type_char(c);
            }
            h.driver.render();
            let screen = h.driver.screen();
            assert!(
                screen.contains("@subdir/"),
                "the completion popup must show a trailing-slash directory \
                 candidate; screen:\n{screen}"
            );

            h.driver.press_named(quadraui::NamedKey::Enter);
            h.driver.render();
            assert_eq!(
                h.engine.borrow().ai_chat.borrow().input_text(),
                "@subdir/ ",
                "accepting the directory candidate must splice the literal \
                 text verbatim plus a trailing space"
            );

            let _ = std::fs::remove_dir_all(&workspace);
        }

        /// `@symbol` — with a cached `workspace/symbol` result, typing `@`
        /// plus a matching symbol name shows a `@path#Name` candidate in the
        /// popup. Accepting it splices that text into the input and shows the
        /// symbol's chip on the status line, proving an `AcpSymbolMention`
        /// was actually staged.
        #[test]
        fn at_symbol_completion_popup_paints_accepts_and_shows_a_chip_via_shell_app() {
            let workspace = std::env::temp_dir().join(format!(
                "vimcode_test_1513_tui_symbol_{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&workspace).expect("create test dir");
            let mut h = widened_ai_panel_harness(&workspace);
            h.engine.borrow_mut().ai_mention_symbol_cache = vec![crate::core::lsp::SymbolInfo {
                name: "MyStruct".to_string(),
                kind: crate::core::lsp::SymbolKind::Struct,
                detail: Some("struct MyStruct".to_string()),
                container: None,
                path: Some(workspace.join("src/lib.rs")),
                line: 41,
                character: 0,
                children: Vec::new(),
            }];

            for c in "@MyStr".chars() {
                h.driver.type_char(c);
            }
            h.driver.render();
            let screen = h.driver.screen();
            assert!(
                screen.contains("@src/lib.rs#MyStruct"),
                "the completion popup must show a @path#Name symbol \
                 candidate; screen:\n{screen}"
            );

            h.driver.press_named(quadraui::NamedKey::Enter);
            h.driver.render();
            assert_eq!(
                h.engine.borrow().ai_chat.borrow().input_text(),
                "@src/lib.rs#MyStruct ",
                "accepting the symbol candidate must splice the literal \
                 @path#Name text verbatim plus a trailing space"
            );
            assert_eq!(
                h.engine.borrow().acp_pending_symbol_mentions.len(),
                1,
                "accepting a symbol candidate must stage an AcpSymbolMention"
            );
            let screen = h.driver.screen();
            assert!(
                screen.contains("MyStruct"),
                "the staged symbol mention's chip must paint on the \
                 always-repainted status line; screen:\n{screen}"
            );

            let _ = std::fs::remove_dir_all(&workspace);
        }
    }

    mod issue_1522_acp_terminal_cards {
        use super::*;
        use std::time::{Duration, Instant};

        /// Wide (220x30, `Alt+Right` x40 — same widening every other
        /// AI-panel test in this file uses, so the card's `$ echo …`/`cwd:
        /// …`/exit-status lines never word-wrap) harness with a fixture
        /// agent configured to reply via `$ACP_FAKE_TERMINAL` — see that
        /// env var's doc in `tests/fixtures/fake_acp_agent.sh` for the
        /// exact `terminal/create` -> `terminal/wait_for_exit` ->
        /// `terminal/release` sequence it drives.
        fn widened_acp_terminal_harness() -> crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        > {
            let mut engine = plain_engine();
            engine.check_settings_reload();
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_AI,
            ));
            let fixture = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            );
            engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
                name: "alpha".to_string(),
                command: format!("sh \"{fixture}\""),
                cwd: String::new(),
                env: vec!["ACP_FAKE_TERMINAL=1".to_string()],
                mcp_servers: Vec::new(),
            }];
            engine.settings.acp_active_agent = "alpha".to_string();
            let mut h = crate::tui_main::testing::conformance_harness(engine, 220, 30);
            h.driver.press_named(quadraui::NamedKey::Escape);
            for _ in 0..40 {
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Right),
                    modifiers: quadraui::Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                    repeat: false,
                });
            }
            h
        }

        /// `{type: "terminal"}` tool content expands into a live card showing
        /// the command line, its `cwd`, and its exit status/output — and
        /// that card keeps rendering after the agent calls `terminal/release`,
        /// instead of the card going blank or the terminal's resources leaving
        /// no trace at all.
        #[cfg(unix)]
        #[test]
        fn terminal_tool_content_renders_a_live_card_that_persists_after_release_via_shell_app() {
            let mut h = widened_acp_terminal_harness();
            let driver = &mut h.driver;
            driver.type_char(':');
            for c in "AI hi".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);

            // Wait for the fixture's scripted round trip to fully finish —
            // `terminal/create` -> `terminal/wait_for_exit` ->
            // `terminal/release`, each parked and answered out of band by
            // the real `Engine::acp_handle_terminal_*` handlers this issue
            // adds — not just the tool call reaching "completed" (which
            // happens *before* `terminal/release` is even sent; see the
            // fixture's own doc). Polling engine state here only decides
            // *when* to stop driving the event loop; every actual
            // assertion below reads painted screen text, never this state,
            // per this repo's "rendered output, not state" rule.
            let deadline = Instant::now() + Duration::from_secs(5);
            let released = loop {
                driver.tick();
                let released = h
                    .engine
                    .borrow()
                    .acp()
                    .acp_terminals
                    .values()
                    .next()
                    .map(|r| r.released)
                    .unwrap_or(false);
                if released || Instant::now() >= deadline {
                    break released;
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            assert!(
                released,
                "setup: the fixture's terminal/release round trip must \
                 complete within 5s"
            );

            let screen = driver.screen();
            assert!(
                screen.contains("[x] execute: Run a command"),
                "the tool call must reach its completed collapsed summary \
                 line; screen:\n{screen}"
            );

            driver.click_text("execute: Run a command");
            driver.tick();
            let screen = driver.screen();
            assert!(
                screen.contains("$ echo acp-terminal-output"),
                "expanding the card must show the terminal's command line; \
                 screen:\n{screen}"
            );
            assert!(
                screen.contains("cwd: /tmp"),
                "expanding the card must show the terminal's cwd; \
                 screen:\n{screen}"
            );
            assert!(
                screen.contains("exit code 0"),
                "expanding the card must show the terminal's exit status, \
                 read after terminal/release dropped the live PTY — proving \
                 the card's snapshot survives release; screen:\n{screen}"
            );
            assert!(
                screen.contains("acp-terminal-output"),
                "expanding the card must show the command's own captured \
                 output; screen:\n{screen}"
            );
        }
    }
    // ─────────────────────────────────────────────────────────────────────────
    // Immediate plugin API — the live engine seam, painted
    // ─────────────────────────────────────────────────────────────────────────
    mod live_plugin_api {
        use super::*;

        /// An engine with one Lua plugin loaded from a temp dir, named so
        /// concurrent runs cannot collide.
        fn engine_with_plugin(unique: &str, code: &str) -> crate::core::Engine {
            let dir = std::env::temp_dir().join(format!(
                "vc_app_on_tui_live_api_{unique}_{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{unique}.lua")), code).unwrap();
            let mut engine = plain_engine();
            let mut mgr =
                crate::core::plugin::PluginManager::new().expect("PluginManager::new must succeed");
            mgr.load_plugins_dir(&dir, &[]);
            assert!(
                mgr.plugins[0].error.is_none(),
                "plugin must load cleanly: {:?}",
                mgr.plugins[0].error
            );
            engine.set_plugin_manager(mgr);
            let _ = std::fs::remove_dir_all(&dir);
            engine
        }

        /// Acceptance scenario: a `:`-command whose Lua callback
        /// creates a scratch buffer, writes it with the immediate API, reads it
        /// back in the same callback, and shows it with `vimcode.window.set_buf`.
        ///
        /// Three separate things have to work for the screen to be right, and
        /// each shows up as different painted text:
        ///
        /// * `ZQ_LIVE_ONE`/`ZQ_LIVE_TWO` paint only if `buffer.create` returned
        ///   a usable handle, the immediate `set_lines` landed in that buffer,
        ///   and `window.set_buf` put it on screen.
        /// * `ZQ_READ=ZQ_LIVE_TWO/11` paints only if the read-after-write inside
        ///   the same callback saw the write and stripped the trailing
        ///   newline: `#("ZQ_LIVE_TWO")` is 11, a terminator would make it 12.
        /// * `[ZQSCRATCH]` on the tab row proves the created buffer is the one
        ///   being displayed, not a coincidentally-similar edit of the original.
        #[test]
        fn immediate_api_scratch_buffer_paints_after_plugin_command_via_shell_app() {
            let engine = engine_with_plugin(
                "live_scratch",
                r#"
                vimcode.command("ZqLive", function(_)
                    local b = vimcode.buffer.create({ scratch = true, name = "ZQSCRATCH" })
                    vimcode.buffer.set_lines(b, 0, -1, { "ZQ_LIVE_ONE", "ZQ_LIVE_TWO" })
                    local back = vimcode.buffer.get_lines(b, 0, 2)
                    local n = vimcode.buffer.line_count(b)
                    vimcode.buffer.set_lines(b, n, n, { "ZQ_READ=" .. back[2] .. "/" .. #back[2] })
                    vimcode.window.set_buf(0, b)
                end)
                "#,
            );
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            let before = driver.screen();
            assert!(
                !before.contains("ZQ_LIVE_ONE"),
                "precondition: nothing is painted before the command runs; \
                 screen:\n{before}"
            );

            driver.type_char(':');
            for c in "ZqLive".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();

            let screen = driver.screen();
            assert!(
                screen.contains("ZQ_LIVE_ONE") && screen.contains("ZQ_LIVE_TWO"),
                "the immediately-written scratch buffer must paint after \
                 window.set_buf; screen:\n{screen}"
            );
            assert!(
                screen.contains("ZQ_READ=ZQ_LIVE_TWO/11"),
                "the in-callback read-after-write must have returned the \
                 just-written line without its newline terminator (length 11, \
                 not 12); screen:\n{screen}"
            );
            assert!(
                screen.contains("[ZQSCRATCH]"),
                "the created scratch buffer must be the one on display, named \
                 as the plugin asked; screen:\n{screen}"
            );
        }

        /// Acceptance scenario: a `vimcode.completion.register` source's item
        /// appears in the real completion popup — by its `label`, not its
        /// `insert_text`, since those differ here — the moment its trigger
        /// prefix is typed, and accepting it inserts `insert_text` into the
        /// buffer. Also exercises the start-column fix: the buffer has no word
        /// anywhere starting with `zq`, so the popup is plugin-only, and
        /// only a single keystroke is typed.
        #[test]
        fn completion_plugin_source_label_paints_in_popup_and_accept_inserts_text_via_shell_app() {
            let engine = engine_with_plugin(
                "completion_paint_1805",
                r#"
                vimcode.completion.register({
                    name = "static",
                    source = function(ctx, request_id)
                        return {
                            {
                                label = "zq_label(..)",
                                insert_text = "zq_insert_text",
                            },
                        }
                    end,
                })
                "#,
            );
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            driver.type_char('i'); // Normal -> Insert
            driver.type_char('z'); // single keystroke — see doc comment above

            let screen = driver.screen();
            assert!(
                screen.contains("zq_label"),
                "the plugin source's label must paint in the completion \
                 popup as soon as its trigger prefix is typed; screen:\n{screen}"
            );
            assert!(
                !screen.contains("zq_insert_text"),
                "the popup must show the item's label, not its insert_text \
                 (they deliberately differ in this fixture); screen:\n{screen}"
            );

            // Accept — Vim mode's default accept key is `<C-y>`.
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char('y'),
                modifiers: quadraui::Modifiers {
                    ctrl: true,
                    ..Default::default()
                },
                repeat: false,
            });

            let screen = driver.screen();
            assert!(
                screen.contains("zq_insert_text"),
                "accepting the plugin candidate must paint its insert_text \
                 in the buffer; screen:\n{screen}"
            );
            assert!(
                !screen.contains("zzq_insert_text") && !screen.contains("zq_insert_textz"),
                "accepting must replace the typed prefix exactly, not \
                 leave a stray leftover character glued to the inserted \
                 text (the start-column review finding); screen:\n{screen}"
            );
        }

        // ─────────────────────────────────────────────────────────────────
        // Native Extension API — Phase 2: a Lua `ys{motion}`
        // operator, built entirely from `vimcode.*` (keymap.set +
        // set_operatorfunc + the '[/'] marks + the immediate buffer API) —
        // the same shape a real nvim-surround-style extension uses.
        // ─────────────────────────────────────────────────────────────────

        /// Acceptance scenario: a Lua `n`-mode map on `ys` registers an
        /// operatorfunc and feeds `g@` (entering operator-pending, same as
        /// vim's own `g@`), so the *next* keystroke supplies the motion —
        /// exactly nvim-surround's `ys{motion}` shape. The operatorfunc reads
        /// the motion's span via `'[`/`']` (set by `g@`) and wraps it
        /// in quotes through the immediate `vimcode.buffer.*` API. Asserted
        /// on the painted screen, not engine state.
        #[test]
        fn lua_ys_operator_wraps_motion_text_via_shell_app() {
            let mut engine = engine_with_plugin(
                "ys_surround",
                r#"
                vimcode.keymap.set("n", "ys", function()
                    vimcode.set_operatorfunc(function(_)
                        local a = vimcode.state.mark("[")
                        local b = vimcode.state.mark("]")
                        local buf = vimcode.buffer.current()
                        local line = vimcode.buffer.get_lines(buf, a.line - 1, a.line)[1]
                        local before = line:sub(1, a.col - 1)
                        local middle = line:sub(a.col, b.col)
                        local after = line:sub(b.col + 1)
                        vimcode.buffer.set_lines(buf, a.line - 1, a.line, {
                            before .. "\"" .. middle .. "\"" .. after,
                        })
                    end)
                    vimcode.feedkeys("g@")
                end)
                "#,
            );
            engine.buffer_mut().insert(0, "hello world\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            let before = driver.screen();
            assert!(
                before.contains("hello world"),
                "precondition: the unwrapped word is on screen; screen:\n{before}"
            );

            driver.type_char('y');
            driver.type_char('s');
            driver.type_char('e'); // motion: to the end of "hello"
            driver.render();

            let screen = driver.screen();
            assert!(
                screen.contains("\"hello\" world"),
                "ys + a motion must wrap that motion's text in quotes, via a \
                 Lua operatorfunc reading '[/'] and vimcode.buffer.set_lines; \
                 screen:\n{screen}"
            );
        }

        // ─────────────────────────────────────────────────────────────────
        // Native Extension API — Phase 3: `vimcode.loop.spawn`'s
        // streamed output, driven through the real event loop and asserted
        // on **painted** output — the issue's own acceptance bar ("A
        // `TuiDriver` black-box test: a plugin spawns a process that prints
        // over time, appends each chunk to a scratch buffer, and the
        // painted buffer shows the streamed lines").
        // ─────────────────────────────────────────────────────────────────

        /// Acceptance scenario: a `:`-command spawns `/bin/sh` printing two
        /// markers with a real delay between them, appending each streamed
        /// `on_stdout` chunk to a scratch buffer via the immediate
        /// `vimcode.buffer` API and displaying it with `window.set_buf`.
        /// Both markers must reach the painted screen — not just engine
        /// state — proving the whole path (background reader thread ->
        /// `Engine::poll_plugin_spawns` -> `with_plugin_dispatch` ->
        /// `vimcode.buffer.set_lines` -> repaint) actually shows up on
        /// screen, and that `on_stdout` really fires per chunk over time
        /// rather than only once, in bulk, at exit.
        #[test]
        #[cfg(unix)]
        fn loop_spawn_streams_output_into_scratch_buffer_paints_via_shell_app() {
            use std::time::{Duration, Instant};

            let engine = engine_with_plugin(
                "spawn_stream_1624",
                r#"
                vimcode.command("ZqSpawnStream", function(_)
                    local b = vimcode.buffer.create({ scratch = true, name = "ZQSPAWNSTREAM" })
                    vimcode.buffer.set_lines(b, 0, -1, {})
                    vimcode.window.set_buf(0, b)
                    vimcode.loop.spawn(
                        "/bin/sh",
                        { "-c", "echo ZQ_STREAM_ONE; sleep 0.05; echo ZQ_STREAM_TWO" },
                        {
                            on_stdout = function(chunk)
                                for line in chunk:gmatch("[^\r\n]+") do
                                    local n = vimcode.buffer.line_count(b)
                                    vimcode.buffer.set_lines(b, n, n, { line })
                                end
                            end,
                        }
                    )
                end)
                "#,
            );
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            driver.type_char(':');
            for c in "ZqSpawnStream".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();

            // The child prints its second marker only after a real 50ms
            // sleep, so this must poll the real event loop rather than
            // asserting once right after `Enter` — `driver.tick()` is what
            // drives `App::tick_dispatch` -> `Engine::poll_idle` ->
            // `poll_plugin_spawns`, same as the ACP terminal tests above.
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !(screen.contains("ZQ_STREAM_ONE") && screen.contains("ZQ_STREAM_TWO"))
                && Instant::now() < deadline
            {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }

            assert!(
                screen.contains("ZQSPAWNSTREAM"),
                "the streamed-into scratch buffer must be the one on \
                 display; screen:\n{screen}"
            );
            assert!(
                screen.contains("ZQ_STREAM_ONE"),
                "the first streamed chunk must land in the painted buffer \
                 within 5s; screen:\n{screen}"
            );
            assert!(
                screen.contains("ZQ_STREAM_TWO"),
                "the second, later-arriving streamed chunk must also land \
                 in the painted buffer, proving on_stdout fires per chunk \
                 over time rather than only once at exit; screen:\n{screen}"
            );
        }

        // ─────────────────────────────────────────────────────────────────
        // Native Extension API — Phase 4: `vimcode.picker.open` fed
        // by a `vimcode.loop.spawn`'s streamed stdout, driven through the
        // real event loop and asserted on **painted** output — the issue's
        // own acceptance bar ("a Lua plugin opens a picker fed by a spawned
        // process that prints lines over time. The painted picker shows the
        // streamed items, and typing filters them").
        // ─────────────────────────────────────────────────────────────────

        /// Acceptance scenario: a `:`-command opens an empty `vimcode.picker`
        /// and spawns `/bin/sh` printing two markers with a real delay
        /// between them, `:append`-ing each streamed `on_stdout` line as a
        /// new item. Both markers must reach the painted screen (not just
        /// engine state), proving the whole path (background reader thread
        /// -> `Engine::poll_plugin_spawns` -> `with_plugin_dispatch` ->
        /// `vimcode.picker.open(...):append` -> repaint) actually shows up
        /// on screen — then typing a query that matches only the second
        /// marker must filter the first one out of the painted list.
        #[test]
        #[cfg(unix)]
        fn picker_streamed_by_spawn_paints_and_filters_via_shell_app() {
            use std::time::{Duration, Instant};

            let engine = engine_with_plugin(
                "picker_spawn_stream_1630",
                r#"
                vimcode.command("ZqPickerSpawn", function(_)
                    local h = vimcode.picker.open({ title = "Streamed" })
                    vimcode.loop.spawn(
                        "/bin/sh",
                        { "-c", "echo ZQ_PICK_ONE; sleep 0.05; echo ZQ_PICK_TWO" },
                        {
                            on_stdout = function(chunk)
                                for line in chunk:gmatch("[^\r\n]+") do
                                    h:append({ { display = line } })
                                end
                            end,
                        }
                    )
                end)
                "#,
            );
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            driver.type_char(':');
            for c in "ZqPickerSpawn".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();

            // The child prints its second marker only after a real 50ms
            // sleep, so this must poll the real event loop rather than
            // asserting once right after `Enter`.
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !(screen.contains("ZQ_PICK_ONE") && screen.contains("ZQ_PICK_TWO"))
                && Instant::now() < deadline
            {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("ZQ_PICK_ONE"),
                "the first streamed item must land in the painted picker \
                 within 5s; screen:\n{screen}"
            );
            assert!(
                screen.contains("ZQ_PICK_TWO"),
                "the second, later-arriving streamed item must also land \
                 in the painted picker, proving `:append` fires per chunk \
                 over time rather than only once at exit; screen:\n{screen}"
            );

            // Typing must reach `Engine::handle_picker_key` and fuzzy-filter
            // the picker's items, same as any other picker source.
            for c in "TWO".chars() {
                driver.type_char(c);
            }
            let screen = driver.screen();
            assert!(
                screen.contains("ZQ_PICK_TWO"),
                "typing a query that matches the second item must keep it \
                 visible; screen:\n{screen}"
            );
            assert!(
                !screen.contains("ZQ_PICK_ONE"),
                "typing a query that only matches the second item must \
                 filter the first one out of the painted list; \
                 screen:\n{screen}"
            );
        }

        // ─────────────────────────────────────────────────────────────────
        // Async requests via `vimcode.http`, delivered via the callback
        // registry, driving a plugin view's painted output.
        // ─────────────────────────────────────────────────────────────────

        /// A minimal one-shot loopback HTTP fixture: binds an ephemeral port,
        /// answers exactly one request with a fixed 200 response and `body`,
        /// then exits. Mirrors `tests/extensions.rs`'s
        /// `spawn_http_fixture_server` — this file compiles into the library
        /// crate, not the `tests/` integration binary, so it can't reuse that
        /// helper directly — trimmed to what this single scenario needs: no
        /// method/path/header/body echoing, since the plugin here only ever
        /// issues one GET.
        fn spawn_one_shot_http_fixture(body: &'static str) -> String {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")
                .expect("bind an ephemeral loopback port");
            let addr = listener.local_addr().expect("resolve bound local_addr");
            let base_url = format!("http://{addr}");
            std::thread::spawn(move || {
                use std::io::{Read as _, Write as _};
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                // Drain the request up to the blank line ending its headers
                // before responding — curl doesn't start reading a response
                // until it has finished writing the request.
                let mut chunk = [0u8; 4096];
                let mut seen = Vec::new();
                loop {
                    let Ok(n) = stream.read(&mut chunk) else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    seen.extend_from_slice(&chunk[..n]);
                    if seen.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                    if seen.len() > 65_536 {
                        return;
                    }
                }
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            });
            base_url
        }

        /// Acceptance scenario: a plugin view with a button that performs an
        /// HTTP GET against a local test server and paints the response body
        /// into the view. Driven through the real event loop end to end: a
        /// `:`-command opens the view as an editor tab (`vimcode.ui.open_view`),
        /// `Enter` on the (only, already-focused) button field fires
        /// `vimcode.http.request` against the fixture server on a background
        /// thread, and the response callback's `vimcode.ui.refresh()` re-renders
        /// the view once `Engine::poll_plugin_http` (drained by `driver.tick()`).
        /// delivers the result.
        #[test]
        fn plugin_view_button_http_get_paints_response_body_via_shell_app() {
            use std::time::{Duration, Instant};

            let base_url = spawn_one_shot_http_fixture("ZQ_HTTP_1632_PROOF_BODY");

            let engine = engine_with_plugin(
                "http_view_1632",
                &format!(
                    r#"
                    _G.status_text = "idle"
                    vimcode.ui.register_view("http_demo_1632", {{
                        title = "HTTP Demo",
                        render = function()
                            return {{
                                fields = {{
                                    {{ type = "label", id = "status", label = _G.status_text }},
                                    {{ type = "button", id = "go", label = "Go" }},
                                }},
                            }}
                        end,
                        on_event = function(_ctx, ev)
                            if ev.widget_id == "go" then
                                vimcode.http.request(
                                    {{ method = "GET", url = "{base_url}/proof" }},
                                    function(resp)
                                        _G.status_text = resp.body
                                            or ("ERR:" .. tostring(resp.error))
                                        vimcode.ui.refresh("http_demo_1632")
                                    end
                                )
                            end
                        end,
                    }})
                    vimcode.command("ZqOpenHttpView", function(_)
                        vimcode.ui.open_view("http_demo_1632")
                    end)
                    "#
                ),
            );
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            driver.type_char(':');
            for c in "ZqOpenHttpView".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();

            let before = driver.screen();
            assert!(
                before.contains("Go") && before.contains("idle"),
                "the view must paint its button and initial label before \
                 the request fires; screen:\n{before}"
            );

            // The button is the only focusable field — Enter activates it,
            // dispatching `ButtonClicked` to the Lua `on_event` handler,
            // which fires the real HTTP request.
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();

            let deadline = Instant::now() + Duration::from_secs(10);
            let mut screen = driver.screen();
            while !screen.contains("ZQ_HTTP_1632_PROOF_BODY") && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            assert!(
                screen.contains("ZQ_HTTP_1632_PROOF_BODY"),
                "the fixture server's response body must be painted into \
                 the view within 10s of the button click; screen:\n{screen}"
            );
        }

        /// `vimcode.ui.open_view(name, { location = "vsplit" })` must place
        /// the view as a sibling window split of the active window, not as
        /// a new tab: the original buffer's own text and the view's own
        /// painted label must both be visible on screen at once, and the
        /// tab bar must still show exactly one tab.
        #[test]
        fn open_view_vsplit_places_view_as_sibling_split_not_new_tab_via_shell_app() {
            let mut engine = engine_with_plugin(
                "open_view_vsplit_2009",
                r#"
                vimcode.ui.register_view("split_demo_2009", {
                    title = "Split Demo",
                    render = function()
                        return {
                            fields = {
                                { type = "label", id = "lbl", label = "ZQ_SPLIT_VIEW_LABEL" },
                            },
                        }
                    end,
                })
                vimcode.command("ZqOpenSplitView", function(_)
                    vimcode.ui.open_view("split_demo_2009", { location = "vsplit" })
                end)
                "#,
            );
            engine.buffer_mut().insert(0, "ZQ_ORIGINAL_FILE_TEXT\n");
            engine.settings.breadcrumbs = false; // one-row tab bar, simplest geometry
            let active_group = engine.active_group;
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            driver.type_char(':');
            for c in "ZqOpenSplitView".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();
            driver.mouse_up(1.0, 1.0); // settle the layout, as Ctrl-W v's own test does

            let screen = driver.screen();
            // Two tabs would only ever paint *one* tab's content at a
            // time (the active one); both markers visible together is
            // only possible if the view shares the window group as a
            // split, not a swapped-in tab.
            assert!(
                screen.contains("ZQ_ORIGINAL_FILE_TEXT") && screen.contains("ZQ_SPLIT_VIEW_LABEL"),
                "a vsplit-placed view must paint alongside the original \
                 window's own buffer text, not replace it with a new tab; \
                 screen:\n{screen}"
            );

            // Locate the tab bar via its own layout-registered bounds
            // (`active_group_tab_bar_row`), not by assuming it's the
            // screen's first row or by content: the view window becomes
            // the active window, so the tab label itself reflects the
            // view's title, not the original buffer's "[No Name]". Each
            // open tab paints its own close button ('×'), so counting them
            // directly rules out a second tab having opened alongside the
            // split.
            let tab_bar_row = super::tab_bar::active_group_tab_bar_row(&h, active_group);
            assert_eq!(
                tab_bar_row.matches('×').count(),
                1,
                "a vsplit-placed view must share the active window's tab, \
                 not open a second tab; tab bar row:\n{tab_bar_row}"
            );

            let content_row = screen
                .lines()
                .find(|line| {
                    line.contains("ZQ_ORIGINAL_FILE_TEXT") && line.contains("ZQ_SPLIT_VIEW_LABEL")
                })
                .unwrap_or_else(|| {
                    panic!("expected a row with both panes' content painted; screen:\n{screen}")
                });
            // `splitright` defaults to false, so the new (view) window
            // lands first/left of the original window — don't assume
            // which marker comes first in the row.
            let original_at = content_row.find("ZQ_ORIGINAL_FILE_TEXT").unwrap();
            let view_at = content_row.find("ZQ_SPLIT_VIEW_LABEL").unwrap();
            let (first_end, second_start) = if original_at < view_at {
                (original_at + "ZQ_ORIGINAL_FILE_TEXT".len(), view_at)
            } else {
                (view_at + "ZQ_SPLIT_VIEW_LABEL".len(), original_at)
            };
            let between = &content_row[first_end..second_start];
            assert_eq!(
                between.matches('\u{2502}').count(),
                1,
                "a vsplit must reserve exactly one column for the divider \
                 between the two panes; between-text: {between:?}; \
                 row:\n{content_row}"
            );
        }

        /// `vimcode.ui.open_view(name, { location = "split" })` must place
        /// the view as a horizontal (stacked) sibling split of the active
        /// window rather than a vertical (side-by-side) one or a new tab.
        ///
        /// Reproduces a pre-existing, feature-independent rendering bug that
        /// is not specific to plugin views: `screen.window_dividers` is
        /// painted unconditionally by `draw_dividers_as_splits` at
        /// `src/app/paint.rs`'s `EditorOp::Windows` rung, with no
        /// `Horizontal`-direction filter analogous to
        /// `painted_group_dividers` (`src/render/frame_editor_band.rs`). A
        /// stacked window divider's `position` sits exactly on the bottom
        /// window's own first content row (see
        /// `WindowLayout::layout_snapped`'s `Horizontal` arm in
        /// `src/core/window.rs`, and `calculate_window_dividers` in
        /// `src/core/engine/windows.rs`), so that row's painted text is
        /// overwritten. Fixing this requires editing `src/render/*` and
        /// `src/app/paint.rs`, which are out of scope for this change.
        #[ignore = "reproduces a pre-existing window-divider paint-order bug \
                    in src/render/frame_editor_band.rs + src/app/paint.rs; \
                    tracked for a follow-up fix outside this change's scope"]
        #[test]
        fn open_view_split_stacks_view_below_active_window_via_shell_app() {
            let mut engine = engine_with_plugin(
                "open_view_split_2009",
                r#"
                vimcode.ui.register_view("stack_demo_2009", {
                    title = "Stack Demo",
                    render = function()
                        return {
                            fields = {
                                { type = "label", id = "lbl", label = "ZQ_STACK_VIEW_LABEL" },
                            },
                        }
                    end,
                })
                vimcode.command("ZqOpenStackView", function(_)
                    vimcode.ui.open_view("stack_demo_2009", { location = "split" })
                end)
                "#,
            );
            engine.buffer_mut().insert(0, "ZQ_ORIGINAL_FILE_TEXT\n");
            engine.settings.breadcrumbs = false; // one-row tab bar, simplest geometry
            let active_group = engine.active_group;
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            driver.type_char(':');
            for c in "ZqOpenStackView".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();
            driver.mouse_up(1.0, 1.0); // settle the layout

            let screen = driver.screen();
            // Two tabs would only ever paint *one* tab's content at a
            // time; both markers visible together is only possible if
            // the view shares the window group as a split.
            assert!(
                screen.contains("ZQ_ORIGINAL_FILE_TEXT") && screen.contains("ZQ_STACK_VIEW_LABEL"),
                "a split-placed view must paint alongside the original \
                 window's own buffer text, not replace it with a new tab; \
                 screen:\n{screen}"
            );

            // Locate the tab bar via its own layout-registered bounds
            // (`active_group_tab_bar_row`), not by assuming it's the
            // screen's first row or by content: the view window becomes
            // the active window, so the tab label itself reflects the
            // view's title, not the original buffer's "[No Name]". Each
            // open tab paints its own close button ('×'), so counting them
            // directly rules out a second tab having opened alongside the
            // split.
            let tab_bar_row = super::tab_bar::active_group_tab_bar_row(&h, active_group);
            assert_eq!(
                tab_bar_row.matches('×').count(),
                1,
                "a split-placed view must share the active window's tab, \
                 not open a second tab; tab bar row:\n{tab_bar_row}"
            );

            // A horizontal (stacked) split means the two panes occupy
            // disjoint rows, unlike a vsplit where both markers share a
            // row — so no single row may contain both markers.
            let original_row = screen
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains("ZQ_ORIGINAL_FILE_TEXT"))
                .unwrap_or_else(|| {
                    panic!("expected a row painting \"ZQ_ORIGINAL_FILE_TEXT\"; screen:\n{screen}")
                });
            let view_row = screen
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains("ZQ_STACK_VIEW_LABEL"))
                .unwrap_or_else(|| {
                    panic!("expected a row painting \"ZQ_STACK_VIEW_LABEL\"; screen:\n{screen}")
                });
            assert_ne!(
                original_row.0, view_row.0,
                "'location = \"split\"' must stack the panes on separate \
                 rows (horizontal split), not place them side by side on \
                 the same row; screen:\n{screen}"
            );

            // The horizontal divider between the two stacked panes paints
            // a row of '─' strictly between their two content rows; a
            // vsplit or a missing split would not produce one there.
            let (top_row, bottom_row) = if original_row.0 < view_row.0 {
                (original_row.0, view_row.0)
            } else {
                (view_row.0, original_row.0)
            };
            let lines: Vec<&str> = screen.lines().collect();
            assert!(
                lines[(top_row + 1)..bottom_row]
                    .iter()
                    .any(|line| line.contains('\u{2500}')),
                "'location = \"split\"' must paint a horizontal divider \
                 row between the two stacked panes' content rows ({top_row} \
                 and {bottom_row}); screen:\n{screen}"
            );
        }

        /// `vimcode.ui.open_view(name, { location = "vsplit", side = "right"
        /// })` must pin the view window on the right, overriding
        /// `splitbelow`/`splitright`'s own default placement (which would
        /// otherwise put it on the left — see the sibling vsplit test's
        /// comment). This is the one assertion that distinguishes `side`
        /// from the default path.
        #[test]
        fn open_view_vsplit_side_right_overrides_default_placement_via_shell_app() {
            let mut engine = engine_with_plugin(
                "open_view_vsplit_side_2009",
                r#"
                vimcode.ui.register_view("side_demo_2009", {
                    title = "Side Demo",
                    render = function()
                        return {
                            fields = {
                                { type = "label", id = "lbl", label = "ZQ_SIDE_VIEW_LABEL" },
                            },
                        }
                    end,
                })
                vimcode.command("ZqOpenSideView", function(_)
                    vimcode.ui.open_view("side_demo_2009", { location = "vsplit", side = "right" })
                end)
                "#,
            );
            engine.buffer_mut().insert(0, "ZQ_ORIGINAL_FILE_TEXT\n");
            // `splitright` stays at its default `false`; `side = "right"`
            // must still land the view window on the right.
            assert!(!engine.settings.splitright);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            driver.type_char(':');
            for c in "ZqOpenSideView".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();
            driver.mouse_up(1.0, 1.0); // settle the layout, as Ctrl-W v's own test does

            let screen = driver.screen();
            let content_row = screen
                .lines()
                .find(|line| {
                    line.contains("ZQ_ORIGINAL_FILE_TEXT") && line.contains("ZQ_SIDE_VIEW_LABEL")
                })
                .unwrap_or_else(|| {
                    panic!("expected a row with both panes' content painted; screen:\n{screen}")
                });
            let original_at = content_row.find("ZQ_ORIGINAL_FILE_TEXT").unwrap();
            let view_at = content_row.find("ZQ_SIDE_VIEW_LABEL").unwrap();
            assert!(
                original_at < view_at,
                "'side = \"right\"' must place the view window to the \
                 right of the original window regardless of 'splitright''s \
                 own default; row:\n{content_row}"
            );
        }

        /// Driver-tier test: a `:`-command's Lua callback `async_shell`s `pwd`
        /// with no `cwd` option, and the completion handler writes the result
        /// into a scratch buffer (`vimcode.buffer.*` + `vimcode.window.set_buf`),
        /// so the assertion is on `driver.screen()` containing a marker
        /// derived from the engine's `cwd`, not on any engine field.
        ///
        /// The assertion uses the temp dir's basename, not its full path:
        /// this harness is a fixed 80x24 grid and `TuiDriver::screen()`
        /// emits one `\n` per 80-cell row, so no string longer than 80
        /// columns can ever be a contiguous substring of `screen()` — and
        /// the line-number gutter eats a few more. On macOS the canonical
        /// temp path is very long (~97 chars), making a full-path assertion
        /// unreliable. The directory name is unique to this test and process,
        /// so the child could only have printed it by actually being spawned
        /// in the engine's `cwd`.
        #[test]
        #[cfg(unix)]
        fn async_shell_cwd_default_paints_into_buffer_via_shell_app_1751() {
            use std::time::{Duration, Instant};

            // Keep the directory name short enough that `MARKER + name`
            // still fits in one 80-column row after the gutter, while
            // staying unique per process.
            let dir_name = format!("vc_aot_cwd_1751_{}", std::process::id());
            let tmp = std::env::temp_dir().join(&dir_name);
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp).unwrap();
            // Canonicalize so this matches what the child's `pwd -P`
            // reports (resolves any symlink in the temp root, e.g. macOS's
            // `/tmp` -> `/private/tmp`).
            let tmp = tmp.canonicalize().unwrap();

            // Absolute path + `-P`, agreeing with the companion test in
            // `tests/extensions.rs`: never let the shell resolve a utility
            // via `PATH`, and `-P` asks `pwd` to resolve via `getcwd()`
            // rather than print whatever logical `$PWD` it inherited from
            // this test binary's environment (a shell-builtin `pwd` can
            // echo the inherited value on some `sh` implementations).
            let pwd = ["/usr/bin", "/bin", "/usr/local/bin", "/opt/homebrew/bin"]
                .iter()
                .map(|dir| std::path::Path::new(dir).join("pwd"))
                .find(|candidate| candidate.is_file())
                .map(|candidate| candidate.to_string_lossy().into_owned())
                .expect("this test needs the POSIX utility `pwd`");

            let mut engine = engine_with_plugin(
                "async_shell_cwd_1751",
                &format!(
                    r#"
                vimcode.command("RunShellCwd1751", function(_)
                    vimcode.async_shell("{pwd} -P", "shell_cwd_done_1751")
                end)
                vimcode.on("shell_cwd_done_1751", function(output)
                    local trimmed = output:gsub("%s+$", "")
                    -- Only the trailing path component: the full path can
                    -- exceed this harness's 80-column row width (macOS's
                    -- temp root is ~56 chars on its own) and would then be
                    -- unfindable in `screen()` no matter how it painted.
                    local base = trimmed:match("([^/]+)$") or "NOMATCH"
                    local b = vimcode.buffer.create({{ scratch = true, name = "ZQCWD1751" }})
                    vimcode.buffer.set_lines(b, 0, -1, {{ "ZQCWD1751:" .. base }})
                    vimcode.window.set_buf(0, b)
                end)
                "#
                ),
            );
            // Point the engine's workspace at `tmp` — deliberately different
            // from this test binary's real OS process directory, which this
            // test never moves (it overrides the *child's* cwd explicitly
            // via the production fallback under test, not the process's own).
            engine.cwd = tmp.clone();
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            let needle = format!("ZQCWD1751:{dir_name}");
            assert!(
                needle.len() < 70,
                "the proof marker must fit in one 80-column row with room \
                 for the gutter, else `screen()` can never contain it: \
                 {needle:?} is {} chars",
                needle.len()
            );
            let before = driver.screen();
            assert!(
                !before.contains(&needle),
                "precondition: the proof marker isn't painted before the \
                 command runs; screen:\n{before}"
            );

            driver.type_char(':');
            for c in "RunShellCwd1751".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut screen = driver.screen();
            while !screen.contains(&needle) && Instant::now() < deadline {
                driver.tick();
                std::thread::sleep(Duration::from_millis(10));
                screen = driver.screen();
            }
            let _ = std::fs::remove_dir_all(&tmp);
            assert!(
                screen.contains(&needle),
                "vimcode.async_shell with no `cwd` option must default to \
                 the engine's own `cwd` ({:?}, whose basename the child's \
                 `pwd -P` reports back), painted into the scratch buffer \
                 within 5s as {needle:?}; screen:\n{screen}",
                tmp.display()
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Native API P5: `vimcode.decor.*` painted output.
    //
    // Every scenario here calls `engine.decor`'s public methods directly
    // (`DecorState::namespace`/`set_hl`/`set_mark` are all `pub`, same as
    // `vimcode.decor.*`'s own `engine/plugins.rs` wrappers call) rather than
    // round-tripping through a loaded Lua plugin — the thing under test is
    // the paint path in `render.rs`, not the Lua binding (already covered
    // by `tests/extensions.rs`'s namespace/shift/clear suite), so there's
    // no need to stand up a `PluginManager` for these.
    // ─────────────────────────────────────────────────────────────────────────
    mod issue_1653_decor_api {
        use super::*;
        use crate::core::buffer::{DecorOpts, HlGroupDef, VirtTextChunk, VirtTextPos};

        /// A highlighted range (`hl_group`) must paint in the resolved
        /// group's colour, not the default foreground.
        ///
        /// RED against unfixed `develop`: `Engine` has no `decor` field at
        /// all there, so this doesn't compile — confirmed instead by
        /// temporarily making `resolve_decor_style` ignore `hl_group` and
        /// always return the theme default: the painted cell's `fg` then
        /// reads back as the theme foreground, not `Rgb(255, 0, 255)`.
        #[test]
        fn decor_highlight_range_paints_in_group_color_via_app_on_tui() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQHIGHLIGHTME\n");
            engine.decor.set_hl(
                "ZqTestHl",
                HlGroupDef {
                    fg: Some("#ff00ff".to_string()),
                    ..Default::default()
                },
            );
            let ns = engine.decor.namespace("zq_hl_test");
            let buf_id = engine.active_buffer_id();
            engine.decor.set_mark(
                buf_id,
                ns,
                0,
                0,
                None,
                Some("ZQHIGHLIGHTME".chars().count()),
                DecorOpts {
                    hl_group: Some("ZqTestHl".to_string()),
                    ..Default::default()
                },
            );

            let h = harness_no_sidebar(engine);
            let driver = &h.driver;
            // Search for a substring starting two columns into the match
            // (skipping "ZQ"), not the full "ZQHIGHLIGHTME" — the mark
            // starts at column 0, same cell the Normal-mode block cursor
            // sits on by default, which paints its own reverse-video style
            // over whatever the span underneath says (see
            // `quadraui::tui::editor`'s cursor-paint match).
            let (x, y) = driver
                .find("HIGHLIGHTME")
                .expect("the marked line must paint");
            let style = driver
                .style_at(x as u16, y as u16)
                .expect("the matched cell must exist");
            assert_eq!(
                style.fg,
                quadraui::tui::testing::Color::Rgb(255, 0, 255),
                "a cell inside the highlighted range must paint in the \
                 resolved group colour"
            );
        }

        /// A plugin group that `link`s to a name which isn't itself a
        /// registered plugin group must be resolved against the active
        /// `Theme`'s matching role instead of silently falling through to
        /// the default foreground — e.g. `link = "Comment"` tracks
        /// `theme.comment` of whatever theme `Settings::default()` resolves
        /// to (`plain_engine()` never overrides `colorscheme`).
        #[test]
        fn decor_set_hl_link_resolves_against_theme_role_via_app_on_tui() {
            let mut engine = plain_engine();
            let comment_fg = {
                let theme = crate::render::Theme::from_name(&engine.settings.colorscheme);
                let t = theme.comment;
                quadraui::tui::testing::Color::Rgb(t.r, t.g, t.b)
            };
            engine.buffer_mut().insert(0, "ZQLINKEDHL\n");
            engine.decor.set_hl(
                "ZqLinksToComment",
                HlGroupDef {
                    link: Some("Comment".to_string()),
                    ..Default::default()
                },
            );
            let ns = engine.decor.namespace("zq_link_test");
            let buf_id = engine.active_buffer_id();
            engine.decor.set_mark(
                buf_id,
                ns,
                0,
                0,
                None,
                Some("ZQLINKEDHL".chars().count()),
                DecorOpts {
                    hl_group: Some("ZqLinksToComment".to_string()),
                    ..Default::default()
                },
            );

            let h = harness_no_sidebar(engine);
            let driver = &h.driver;
            let (x, y) = driver.find("LINKEDHL").expect("the marked line must paint");
            let style = driver
                .style_at(x as u16, y as u16)
                .expect("the matched cell must exist");
            assert_eq!(
                style.fg, comment_fg,
                "a group whose `link` isn't a registered plugin group must \
                 resolve against the matching `Theme` role (the active \
                 theme's `comment`), not fall through to the default \
                 foreground"
            );
        }

        /// Overlay virtual text replaces the glyphs already at its column,
        /// same width — the original text underneath must not still show.
        #[test]
        fn decor_overlay_virt_text_paints_over_existing_text_via_app_on_tui() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQBEFOREXXXAFTERZQ\n");
            let ns = engine.decor.namespace("zq_overlay");
            let buf_id = engine.active_buffer_id();
            engine.decor.set_mark(
                buf_id,
                ns,
                0,
                8, // the first "X" of "XXX"
                None,
                None,
                DecorOpts {
                    virt_text: vec![VirtTextChunk {
                        text: "JJJ".to_string(),
                        hl_group: None,
                    }],
                    virt_text_pos: Some(VirtTextPos::Overlay),
                    ..Default::default()
                },
            );

            let h = harness_no_sidebar(engine);
            let driver = &h.driver;
            let screen = driver.screen();
            assert!(
                screen.contains("ZQBEFOREJJJAFTERZQ"),
                "overlay virt text must replace the XXX span with JJJ; \
                 screen:\n{screen}"
            );
            assert!(
                !screen.contains("XXX"),
                "the original text under an overlay must not still show; \
                 screen:\n{screen}"
            );
        }

        /// Inline virtual text inserts at its column, shifting later text on
        /// the same line right — and the painted cursor column must follow,
        /// even though the engine's own (buffer-coordinate) cursor column
        /// never changes.
        #[test]
        fn decor_inline_virt_text_shifts_text_and_cursor_col_via_app_on_tui() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQHEADZQTAIL\n");
            let ns = engine.decor.namespace("zq_inline");
            let buf_id = engine.active_buffer_id();
            engine.decor.set_mark(
                buf_id,
                ns,
                0,
                6, // right after "ZQHEAD", right before "ZQTAIL"
                None,
                None,
                DecorOpts {
                    virt_text: vec![VirtTextChunk {
                        text: ">>".to_string(),
                        hl_group: None,
                    }],
                    virt_text_pos: Some(VirtTextPos::Inline),
                    ..Default::default()
                },
            );
            // Cursor sits in buffer coordinates at column 6 — the "Z" of
            // "ZQTAIL" before any inline text exists.
            engine.view_mut().cursor.col = 6;

            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;
            let screen = driver.screen();
            assert!(
                screen.contains("ZQHEAD>>ZQTAIL"),
                "inline virt text must be inserted, shifting the following \
                 text right; screen:\n{screen}"
            );

            let (tail_x, tail_y) = driver
                .find("ZQTAIL")
                .expect("the shifted tail text must paint");
            // `terminal_cursor_position()` only reflects a Bar/Underline
            // cursor (`Frame::set_cursor_position`) — Normal mode's Block
            // cursor paints as a plain reverse-video cell instead (see
            // `quadraui::tui::editor`'s cursor-paint match), so switch to
            // Insert mode first. This doesn't move `view.cursor.col` (`i`
            // inserts *before* the cursor), so the column this test cares
            // about is unchanged.
            driver.type_char('i');
            let cursor_pos = driver
                .terminal_cursor_position()
                .expect("the editor cursor must have painted");
            assert_eq!(
                cursor_pos,
                (tail_x as u16, tail_y as u16),
                "the painted cursor column must shift right by the inline \
                 text's length, landing back on the (now-shifted) \"Z\" of \
                 ZQTAIL rather than on the \">>\" that pushed it there"
            );
        }

        /// A `sign_text` mark paints its glyph in the gutter, left of the
        /// line's own text.
        #[test]
        fn decor_sign_appears_in_gutter_via_app_on_tui() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQLINEMARKER\n");
            let ns = engine.decor.namespace("zq_sign");
            let buf_id = engine.active_buffer_id();
            engine.decor.set_mark(
                buf_id,
                ns,
                0,
                0,
                None,
                None,
                DecorOpts {
                    sign_text: Some("S".to_string()),
                    ..Default::default()
                },
            );

            let h = harness_no_sidebar(engine);
            let driver = &h.driver;
            let (line_x, line_y) = driver
                .find("ZQLINEMARKER")
                .expect("the signed line's text must paint");
            let row = driver.styled_row(line_y as u16);
            let gutter_chars: String = row[..(line_x as usize).min(row.len())]
                .iter()
                .map(|(ch, _)| *ch)
                .collect();
            assert!(
                gutter_chars.contains('S'),
                "the decor sign must paint somewhere in the gutter, left of \
                 the line text; gutter cells: {gutter_chars:?}"
            );
        }

        /// A mark-anchored highlight follows an insert *above* it: once a
        /// new line is spliced in before the marked line (same `O` + Escape
        /// sequence the plain vim-mark shift tests use), the highlight must
        /// paint on the mark's *new* row, not its original one.
        #[test]
        fn decor_mark_anchored_highlight_follows_insert_above_via_app_on_tui() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQKEEPCOLOR\nZQOTHERLINE\n");
            engine.decor.set_hl(
                "ZqFollowHl",
                HlGroupDef {
                    fg: Some("#00ffff".to_string()),
                    ..Default::default()
                },
            );
            let ns = engine.decor.namespace("zq_follow");
            let buf_id = engine.active_buffer_id();
            engine.decor.set_mark(
                buf_id,
                ns,
                0,
                0,
                None,
                Some("ZQKEEPCOLOR".chars().count()),
                DecorOpts {
                    hl_group: Some("ZqFollowHl".to_string()),
                    ..Default::default()
                },
            );

            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // "KEEPCOLOR", not "ZQKEEPCOLOR" — the mark starts at column 0,
            // under the Normal-mode block cursor's default position, which
            // paints its own reverse-video style (see the sibling highlight
            // test's comment for why).
            let (x0, y0) = driver
                .find("KEEPCOLOR")
                .expect("precondition: the marked line must paint before the edit");
            let style0 = driver
                .style_at(x0 as u16, y0 as u16)
                .expect("precondition: the matched cell must exist");
            assert_eq!(
                style0.fg,
                quadraui::tui::testing::Color::Rgb(0, 255, 255),
                "precondition: the highlight must paint before the edit"
            );

            // `O` + Escape splices a new line in above row 0 — the exact
            // sequence `test_mark_shifts_after_line_inserted_above`
            // (`new_vim_features.rs`) uses to pin the equivalent vim-mark
            // behaviour.
            driver.type_char('O');
            for c in "NEWTOPLINE".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Escape);

            let screen = driver.screen();
            assert!(
                screen.contains("NEWTOPLINE") && screen.contains("ZQKEEPCOLOR"),
                "precondition: the insert must have landed; screen:\n{screen}"
            );

            let (x1, y1) = driver
                .find("ZQKEEPCOLOR")
                .expect("the marked line must still paint after the edit");
            assert!(
                y1 > y0,
                "precondition: ZQKEEPCOLOR must have moved down a row"
            );
            let style1 = driver
                .style_at(x1 as u16, y1 as u16)
                .expect("the matched cell must exist after the edit");
            assert_eq!(
                style1.fg,
                quadraui::tui::testing::Color::Rgb(0, 255, 255),
                "the highlight must have followed the mark to its new row, \
                 not stayed pinned to the row it started on"
            );
        }

        /// End-of-line virtual text (`virt_text_pos = "eol"`, or no
        /// `virt_text_pos` at all) paints after the line's content via the
        /// same path `vimcode.buf.annotate_line` uses
        /// (`RenderedLine::annotation`). Also covers two marks on one line
        /// drawing in creation order (oldest `set_mark` first — `DecorOpts`
        /// has no `priority` field) separated by a space, and the combined
        /// text following a line inserted above it (same `O` + Escape
        /// sequence the sibling highlight-follows-insert test above uses).
        #[test]
        fn decor_eol_virt_text_paints_after_line_and_follows_insert_above_via_app_on_tui() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQBEFORE\nZQOTHERLINE\n");
            let ns = engine.decor.namespace("zq_eol");
            let buf_id = engine.active_buffer_id();
            engine.decor.set_mark(
                buf_id,
                ns,
                0,
                0,
                None,
                None,
                DecorOpts {
                    virt_text: vec![VirtTextChunk {
                        text: "ZQEOLA".to_string(),
                        hl_group: None,
                    }],
                    virt_text_pos: Some(VirtTextPos::Eol),
                    ..Default::default()
                },
            );
            // A second eol mark on the same line, created after the first,
            // must draw after it (creation order — `DecorOpts` has no
            // `priority` field), separated by a space — not glued to the
            // first mark's text.
            engine.decor.set_mark(
                buf_id,
                ns,
                0,
                0,
                None,
                None,
                DecorOpts {
                    virt_text: vec![VirtTextChunk {
                        text: "ZQEOLB".to_string(),
                        hl_group: None,
                    }],
                    virt_text_pos: Some(VirtTextPos::Eol),
                    ..Default::default()
                },
            );

            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;
            let (before_x, before_y) = driver
                .find("ZQBEFORE")
                .expect("the marked line's own text must paint");
            let screen = driver.screen();
            assert!(
                screen.contains("ZQEOLA ZQEOLB"),
                "two eol marks on one line must draw in creation order, \
                 separated by a space; screen:\n{screen}"
            );
            let (eol_x, eol_y) = driver
                .find("ZQEOLA")
                .expect("the eol virtual text must paint");
            assert_eq!(
                eol_y, before_y,
                "the eol text must paint on the same row as the line it's \
                 anchored to"
            );
            assert!(
                eol_x > before_x,
                "the eol text must paint after the line's own content, not \
                 before/over it"
            );

            // `O` + Escape splices a new line in above row 0 — the eol
            // marks must follow it down.
            driver.type_char('O');
            for c in "NEWTOPLINE".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Escape);

            let screen = driver.screen();
            assert!(
                screen.contains("NEWTOPLINE") && screen.contains("ZQBEFORE"),
                "precondition: the insert must have landed; screen:\n{screen}"
            );
            let (before_x2, before_y2) = driver
                .find("ZQBEFORE")
                .expect("the marked line must still paint after the edit");
            assert!(
                before_y2 > before_y,
                "precondition: ZQBEFORE must have moved down a row"
            );
            let (eol_x2, eol_y2) = driver
                .find("ZQEOLA")
                .expect("the eol text must still paint after the edit");
            assert_eq!(
                eol_y2, before_y2,
                "the eol text must have followed the mark down to its new \
                 row, not stayed pinned to the row it started on"
            );
            assert!(
                eol_x2 > before_x2,
                "the eol text must still paint after the (now-shifted) \
                 line's content"
            );
        }

        /// Decor eol virtual text shares `RenderedLine::annotation`
        /// with `vimcode.buf.annotate_line`'s blame text, but must NOT
        /// inherit that field's Insert-mode mute. The mute exists so the
        /// ambient blame annotation stops flickering beside the caret while
        /// the user types; plugin-owned eol text (inlay hints, lint
        /// messages) is the opposite — it is most useful exactly while
        /// editing, and Neovim keeps it painted in insert mode too.
        #[test]
        fn decor_eol_virt_text_survives_insert_mode_but_blame_still_mutes_via_app_on_tui() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQLINEONE\n");
            // The blame-sourced annotation this field normally carries —
            // the control for the assertion that eol text behaves
            // differently from it.
            engine.line_annotations.insert(0, "ZQBLAMETEXT".to_string());
            let ns = engine.decor.namespace("zq_eol_insert");
            let buf_id = engine.active_buffer_id();
            engine.decor.set_mark(
                buf_id,
                ns,
                0,
                0,
                None,
                None,
                DecorOpts {
                    virt_text: vec![VirtTextChunk {
                        text: "ZQEOLINS".to_string(),
                        hl_group: None,
                    }],
                    virt_text_pos: Some(VirtTextPos::Eol),
                    ..Default::default()
                },
            );

            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;
            let screen = driver.screen();
            assert!(
                screen.contains("ZQBLAMETEXT") && screen.contains("ZQEOLINS"),
                "precondition: in Normal mode both the blame annotation and \
                 the decor eol text must paint; screen:\n{screen}"
            );

            driver.type_char('i');
            let screen = driver.screen();
            assert!(
                screen.contains("ZQEOLINS"),
                "decor eol virtual text must stay painted in Insert mode — \
                 it is plugin-owned inlay/lint text, not the ambient blame \
                 annotation the Insert-mode mute is for; screen:\n{screen}"
            );
            assert!(
                !screen.contains("ZQBLAMETEXT"),
                "the blame annotation must keep its pre-#1810 Insert-mode \
                 mute; screen:\n{screen}"
            );
        }

        /// On a *wrapped* line, "end of line" means after all of the
        /// line's content — the eol text must ride the line's LAST visual
        /// row, not the first one (where the blame annotation sits, and
        /// where it would read as text spliced into the middle of the
        /// line).
        #[test]
        fn decor_eol_virt_text_rides_last_visual_row_of_a_wrapped_line_via_app_on_tui() {
            let mut engine = plain_engine();
            engine.settings.wrap = true;
            // Wide enough to wrap at the harness's 80-column terminal, with
            // two distinctive anchors: one in the first visual row, one in
            // the last. `repeat(30)` is deliberately generous: the line is
            // ~133 chars total, wrapping into a ~57-char final segment
            // ("... zzz ZQTAIL") against the editor's content width (80
            // columns minus the gutter), leaving comfortable double-digit
            // columns of headroom for the 9-char "ZQEOLWRAP" annotation to
            // fit on that same tail row without truncating. If a future
            // gutter-width change narrows the content area enough to eat
            // that headroom, this test should start failing as an obvious
            // truncation (`ZQEOLWRAP` missing/cut short), not a confusing
            // one — if it ever does, widen the repeat count rather than
            // shrink the terminal width assumption.
            let long = format!("ZQHEAD{} ZQTAIL\n", " zzz".repeat(30));
            engine.buffer_mut().insert(0, &long);
            let ns = engine.decor.namespace("zq_eol_wrap");
            let buf_id = engine.active_buffer_id();
            engine.decor.set_mark(
                buf_id,
                ns,
                0,
                0,
                None,
                None,
                DecorOpts {
                    virt_text: vec![VirtTextChunk {
                        text: "ZQEOLWRAP".to_string(),
                        hl_group: None,
                    }],
                    virt_text_pos: Some(VirtTextPos::Eol),
                    ..Default::default()
                },
            );

            let h = harness_no_sidebar(engine);
            let driver = &h.driver;
            let (_, head_y) = driver
                .find("ZQHEAD")
                .expect("the wrapped line's first visual row must paint");
            let (tail_x, tail_y) = driver
                .find("ZQTAIL")
                .expect("the wrapped line's last visual row must paint");
            assert!(
                tail_y > head_y,
                "precondition: the line must actually wrap onto a later \
                 visual row; screen:\n{}",
                driver.screen()
            );
            let (eol_x, eol_y) = driver
                .find("ZQEOLWRAP")
                .expect("the eol virtual text must paint");
            assert_eq!(
                eol_y,
                tail_y,
                "the eol text must paint on the wrapped line's LAST visual \
                 row (after all its content), not on the first; screen:\n{}",
                driver.screen()
            );
            assert!(
                eol_x > tail_x,
                "the eol text must paint after the tail of the line's own \
                 content"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Native API P6: `vimcode.syntax.*` read access, driven by a real
    // Lua plugin keymap and asserted on **painted output**: a plugin maps
    // a key that selects the enclosing function via syntax.query, and the
    // painted selection covers it. "Selects" here means painting a
    // `vimcode.decor` highlight over the enclosing node's range rather
    // than entering real Visual mode — P6 is a read-only API, and
    // `vimcode.decor.*` is the existing tool for turning a resolved range
    // into painted output.
    // ─────────────────────────────────────────────────────────────────────────
    mod issue_1654_syntax_api {
        use super::*;
        use crate::core::buffer::HlGroupDef;
        use crate::core::syntax::{Syntax, SyntaxLanguage};

        /// An engine with one Lua plugin loaded from a temp dir. Mirrors
        /// `live_plugin_api::engine_with_plugin` (private to that sibling
        /// module, so not reusable directly here).
        fn engine_with_plugin(unique: &str, code: &str) -> crate::core::Engine {
            let dir = std::env::temp_dir().join(format!(
                "vc_app_on_tui_syntax_api_{unique}_{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{unique}.lua")), code).unwrap();
            let mut engine = plain_engine();
            let mut mgr =
                crate::core::plugin::PluginManager::new().expect("PluginManager::new must succeed");
            mgr.load_plugins_dir(&dir, &[]);
            assert!(
                mgr.plugins[0].error.is_none(),
                "plugin must load cleanly: {:?}",
                mgr.plugins[0].error
            );
            engine.set_plugin_manager(mgr);
            let _ = std::fs::remove_dir_all(&dir);
            engine
        }

        /// RED-verified against unfixed `develop`: there is no
        /// `vimcode.syntax` table there, so `<leader>f`'s callback errors on
        /// its first line and no `ZqSelectFn`-coloured highlight ever gets
        /// painted — confirmed by temporarily reverting `Syntax::node_at`
        /// to always return `Err`, which leaves the keymap's `pcall`-free
        /// call erroring the same way and the function body unhighlighted.
        #[test]
        fn syntax_node_at_selects_enclosing_function_paints_highlight_via_shell_app() {
            let mut engine = engine_with_plugin(
                "syntax_select_fn",
                r#"
                vimcode.keymap.set("n", "<leader>f", function()
                    local buf = vimcode.buffer.current()
                    local pos = vimcode.window.get_cursor(0)
                    local node = vimcode.syntax.node_at(buf, pos.line - 1, pos.col - 1)
                    if node.parent ~= nil then
                        local ns = vimcode.decor.namespace("zq_select_fn")
                        vimcode.decor.set_mark(buf, ns, {
                            row = node.parent.range.start_row,
                            col = node.parent.range.start_col,
                            end_row = node.parent.range.end_row,
                            end_col = node.parent.range.end_col,
                            hl_group = "ZqSelectFn",
                        })
                    end
                end)
                "#,
            );
            engine.decor.set_hl(
                "ZqSelectFn",
                HlGroupDef {
                    fg: Some("#ff00ff".to_string()),
                    ..Default::default()
                },
            );
            engine
                .buffer_mut()
                .insert(0, "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n");
            engine.active_buffer_state_mut().syntax =
                Some(Syntax::new_for_language(SyntaxLanguage::Rust));
            engine.active_buffer_state_mut().update_syntax();
            // Cursor on the function name `add` (row 0, byte col 3 — "fn ").
            engine.view_mut().cursor = crate::core::cursor::Cursor { line: 0, col: 3 };

            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            let before = driver.screen();
            assert!(
                before.contains("a + b"),
                "precondition: the function body paints before the keymap fires; \
                 screen:\n{before}"
            );
            let (bx, by) = driver
                .find("a + b")
                .expect("precondition: the function body must paint");
            let before_style = driver
                .style_at(bx as u16, by as u16)
                .expect("precondition: the matched cell must exist");
            assert_ne!(
                before_style.fg,
                quadraui::tui::testing::Color::Rgb(255, 0, 255),
                "precondition: the body isn't highlighted before the keymap fires"
            );

            // Leader (default Space) + 'f'.
            driver.type_char(' ');
            driver.type_char('f');
            driver.render();

            // The `function_item` node spans every line of the function —
            // including its body, two rows below the cursor's own line — so
            // asserting the highlight reaches "a + b" proves the *whole*
            // enclosing-function range painted, not just the name's own
            // single-line range `node_at` would report without `.parent`.
            let (x, y) = driver
                .find("a + b")
                .expect("the function body must still paint after the keymap fires");
            let style = driver
                .style_at(x as u16, y as u16)
                .expect("the matched cell must exist");
            assert_eq!(
                style.fg,
                quadraui::tui::testing::Color::Rgb(255, 0, 255),
                "the enclosing function's full range — found via \
                 vimcode.syntax.node_at's `.parent` — must paint in the \
                 resolved highlight colour, proving the selection covers the \
                 whole function body"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // `:w!` (force-write) must be recognised as the same ex command as
    // plain `:w` — Vim accepts a bang on `:write` with no behaviour change
    // (vimcode has no readonly/overwrite distinction for the bang to force
    // past). Before the fix, `normalize_ex_command`'s abbreviation table
    // still expanded "w!" to "write!" (preserving the bang through the
    // rewrite), but `Engine::execute_command`'s match arm only listed the
    // bare "write" string, so "write!" fell through to the `_ =>` catch-all
    // and printed "Not an editor command: write!" instead of saving.
    // ─────────────────────────────────────────────────────────────────────────
    mod issue_1790_force_write_bang {
        use super::*;

        /// RED-verified against unfixed `develop`: with the `"write"` match
        /// arm reverted to not also match `"write!"`, this fails on both
        /// assertions — the command-line paints "Not an editor command:
        /// write!" and the file on disk is left unchanged ("one\ntwo\n").
        #[test]
        fn force_write_bang_saves_and_is_recognised_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1790_force_write_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("a.txt");
            std::fs::write(&path, "one\ntwo\n").unwrap();

            let mut engine = plain_engine();
            let old_id = engine.active_buffer_id();
            let _ = engine.buffer_manager.delete(old_id, true);
            let buf_id = engine.buffer_manager.open_file(&path).unwrap();
            if let Some(window) = engine.windows.get_mut(&engine.active_window_id()) {
                window.buffer_id = buf_id;
            }

            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // Change the buffer so the write below has an observable effect
            // on disk, matching Vim's own `:w!` semantics (same as `:w`).
            driver.type_char('A');
            for c in " CHANGED".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Escape);

            for c in ":w!".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();

            let screen = driver.screen();
            assert!(
                !screen.contains("Not an editor command"),
                "`:w!` must be recognised as the force-write ex command, \
                 same as plain `:w`; screen:\n{screen}"
            );

            let on_disk = std::fs::read_to_string(&path).unwrap();
            assert_eq!(
                on_disk, "one CHANGED\ntwo\n",
                "`:w!` must actually save the buffer to disk, same as `:w`"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Extensions marketplace `i` (install) keystroke, driven through
    // the real focus-routed key path, not by calling engine functions
    // directly. Fixes for two root causes and a regression:
    //
    // 1. `quadraui::SidebarSystem::move_selection_by` only ever moves the
    //    selection *within* the already-active section. A fresh install
    //    starts with an empty INSTALLED section, so navigation was stuck
    //    at section 0 and could never reach AVAILABLE.
    //
    // 2. `dispatch_ext_sidebar_action_key`'s `"i"` arm blocked on blocking
    //    `curl` subprocesses for README fetch and script download directly
    //    on the key-dispatch path. Fixed by moving both off the UI thread.
    //
    // 3. Round 2's backgrounded script download raced `plugin_init()`,
    //    which runs synchronously a few lines later in the same handler.
    //    A freshly installed Lua-scripted extension would register none
    //    of its commands until the next launch. Fixed by giving the
    //    download thread a completion channel that re-runs `plugin_init()`
    //    once the files are actually on disk.
    // ─────────────────────────────────────────────────────────────────────────
    mod issue_1739_ext_install_key {
        use super::*;
        use crate::core::engine::sidebar::PANEL_EXTENSIONS;
        use crate::core::extensions::ExtensionManifest;

        fn down_key() -> quadraui::UiEvent {
            quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Named(quadraui::NamedKey::Down),
                modifiers: quadraui::Modifiers::default(),
                repeat: false,
            }
        }

        fn i_key() -> quadraui::UiEvent {
            quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Char('i'),
                modifiers: quadraui::Modifiers::default(),
                repeat: false,
            }
        }

        /// Build an engine with an empty INSTALLED section and one
        /// *available* extension. No scripts, no registry base URL: install
        /// must be a pure local no-op (mark installed + status message),
        /// no network involved to confound the result.
        fn harness_with_one_available_extension(
            unique: &str,
        ) -> (
            crate::harness::ConformanceHarness<
                quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            >,
            String,
            crate::core::paths::TestHomeGuard,
        ) {
            let home = std::env::temp_dir().join(format!(
                "vimcode_test_1739_ext_install_{unique}_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&home);
            std::fs::create_dir_all(&home).unwrap();
            // Returned (not `forget`ten): `set_test_home` overrides a thread-local,
            // which under `cargo test -- --test-threads=1` survives into every
            // subsequent test on the same thread until dropped. Every other direct-call
            // fixture in this file returns the guard in its tuple instead so it drops
            // with the test — this fixture now matches that, rather than leaking the
            // override for the rest of the process.
            let guard = crate::core::paths::set_test_home(&home);

            let available_name = format!("vc-tui-1739-available-{unique}");

            // Short display name: the sidebar body column is narrow
            // enough (even at this module's usual 80-column harness) to
            // truncate anything much longer than this before the
            // identifying text ever reaches the screen (same column-width
            // constraint `harness_with_far_future_requirement` above works
            // around with a 220-column terminal + repeated Alt+Right
            // resizes — overkill for this fixture's short name).
            let mut engine = plain_engine();
            // Without this, `plain_engine()`'s own startup still fires a real,
            // backgrounded fetch against the *default* public registry, which
            // can land mid-test and overwrite `ext_registry` below with real
            // extensions, clobbering this fixture's single-entry list. Clearing
            // `extension_registries` first makes that startup fetch a same-tick
            // no-op instead of a live network race.
            engine.settings.extension_registries = Vec::new();
            engine.ext_registry = Some(vec![ExtensionManifest {
                name: available_name.clone(),
                display_name: "Zavail".to_string(),
                ..Default::default()
            }]);

            engine.focus_sidebar_panel(PANEL_EXTENSIONS);

            let h = harness(engine);
            (h, available_name, guard)
        }

        /// Verifies cross-section navigation fix: with it removed, the screen
        /// after pressing `Down` then `i` is unchanged from before either
        /// keypress — no "installed" message, extension never marked installed.
        #[test]
        fn down_then_i_installs_the_only_available_extension_via_shell_app() {
            let (mut h, available_name, _home_guard) =
                harness_with_one_available_extension("keyboard");
            let driver = &mut h.driver;

            driver.render();
            let before = driver.screen();
            assert!(
                before.contains("AVAILABLE") && before.contains("Zavail"),
                "precondition: the Extensions sidebar must list the \
                 available extension; screen:\n{before}"
            );

            // The keyboard path: Enter-into-panel already happened via
            // `focus_sidebar_panel` above, so this is just the "arrow-key Down",
            // selecting the sidebar's one and only row (INSTALLED is empty).
            driver.dispatch(down_key());
            driver.dispatch(i_key());
            driver.render();

            // `"' installed"` rather than a bare `"installed"` needle: the
            // sibling "already installed" message also contains the word
            // "installed", so a regression into that branch could otherwise
            // still pass this assertion.
            let after = driver.screen();
            assert!(
                after.contains("' installed"),
                "#1739: pressing Down then `i` must select the available \
                 extension and paint an 'installed' status message via \
                 the real key dispatch path; screen:\n{after}"
            );

            let engine = h.engine.borrow();
            assert!(
                engine.message.contains(&available_name) && engine.message.contains("' installed"),
                "#1739: `Engine::message` must name the installed extension; \
                 message: {:?}",
                engine.message
            );
            assert!(
                engine.extension_state.is_installed(&available_name),
                "#1739: pressing `i` must actually mark the extension \
                 installed, not just paint a message"
            );
        }

        /// The repro's mouse variant: clicking directly on the available
        /// row must select it (and, critically, must not regress now that
        /// `ext_sidebar_navigate` grew cross-section logic) so that `i`
        /// still installs it with no keyboard navigation at all.
        #[test]
        fn click_then_i_installs_the_clicked_available_extension_via_shell_app() {
            let (mut h, available_name, _home_guard) =
                harness_with_one_available_extension("mouse");
            let driver = &mut h.driver;

            driver.render();
            let (zx, zy) = driver
                .find("Zavail")
                .expect("the available extension's row must paint its text");
            driver.click(zx, zy);
            driver.dispatch(i_key());
            driver.render();

            // `"' installed"` rather than a bare `"installed"` needle —
            // see the keyboard twin above for why.
            let after = driver.screen();
            assert!(
                after.contains("' installed"),
                "#1739: clicking the available row then pressing `i` must \
                 install it; screen:\n{after}"
            );
            assert!(
                h.engine
                    .borrow()
                    .extension_state
                    .is_installed(&available_name),
                "#1739: clicking then pressing `i` must actually mark the \
                 extension installed"
            );
        }

        /// Acceptance scenario: verifies that pressing `i` against an
        /// unresponsive registry does not block the dispatch path. A local TCP
        /// listener that accepts the connection and never responds represents
        /// the worst case — `curl --max-time 10/30` will sit on it for the full
        /// timeout. If the fix is reverted so that `dispatch_ext_sidebar_action_key`'s
        /// `"i"` arm still calls registry functions inline, `driver.dispatch(i_key())`
        /// would block for several seconds. With the fix, both calls run on
        /// background threads, so `dispatch` returns as soon as the local,
        /// synchronous half of the handler finishes.
        #[test]
        fn pressing_i_against_an_unresponsive_registry_does_not_block_dispatch_1739() {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")
                .expect("binding a local TCP listener for the stuck-registry fixture");
            let addr = listener.local_addr().unwrap();
            // Accept connections and then just hold them open without ever
            // writing a response or closing — the worst case for a
            // blocking HTTP client (`curl` will sit there until its own
            // `--max-time` elapses, not fail fast).
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let _held = stream;
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            });

            let (mut h, available_name, _home_guard) =
                harness_with_one_available_extension("slowreg");
            {
                let mut engine = h.engine.borrow_mut();
                if let Some(reg) = engine.ext_registry.as_mut() {
                    for m in reg.iter_mut() {
                        if m.name == available_name {
                            // Non-empty `scripts` + non-empty `registry_base_url`
                            // is exactly the gate `ext_install_from_registry_
                            // with_runtime_check`'s download loop and the `"i"`
                            // arm's README fetch both check before touching the
                            // network at all (`lsp_ops.rs`/`ext_panel.rs`) — the
                            // other two tests in this module skip both legs
                            // entirely by leaving these at their `Default` (empty)
                            // values.
                            m.scripts = vec!["init.lua".to_string()];
                            m.registry_base_url = format!("http://{addr}");
                        }
                    }
                }
            }

            let driver = &mut h.driver;
            driver.render();
            driver.dispatch(down_key());

            let start = std::time::Instant::now();
            driver.dispatch(i_key());
            let elapsed = start.elapsed();

            assert!(
                elapsed < std::time::Duration::from_secs(2),
                "#1739: pressing `i` against an unresponsive registry must \
                 not block the key-dispatch path on a `curl --max-time \
                 10`/`30` subprocess; took {elapsed:?}"
            );

            // The install's own `self.message` assignment is synchronous
            // (`ext_install_from_registry_with_runtime_check`'s final
            // lines never depend on the backgrounded script download or
            // README fetch), so it must already be visible without
            // waiting on either background thread.
            driver.render();
            assert!(
                driver.screen().contains("' installed"),
                "#1739: the install message must still repaint \
                 immediately even though the registry never responds; \
                 screen:\n{}",
                driver.screen()
            );
        }

        /// A minimal loopback HTTP fixture: binds an ephemeral port and
        /// answers *every* request it receives with a fixed 200 response
        /// and `body`, until the test process exits. Unlike
        /// `live_plugin_api`'s `spawn_one_shot_http_fixture` (a few
        /// thousand lines up in this file — that one's `mod`-private, so
        /// this module gets its own), this one must survive more than one
        /// connection: pressing `i` fires off *two* requests against the
        /// same `registry_base_url` — the script download
        /// (`ext_install_from_registry_with_runtime_check`) and the
        /// post-install README fetch (`ext_show_readme_or_fetch_async`,
        /// `ext_panel.rs`'s `"i"` arm) — racing each other to connect. A
        /// one-shot listener serves whichever wins and refuses the other
        /// outright (the listening socket is gone once that single
        /// `accept()` returns), which silently drops the script download
        /// on a coin flip and made this fixture's own test flaky under
        /// parallel `cargo test` load.
        fn spawn_multi_shot_http_fixture(body: &'static str) -> String {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")
                .expect("bind an ephemeral loopback port");
            let addr = listener.local_addr().expect("resolve bound local_addr");
            let base_url = format!("http://{addr}");
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    // Each connection gets its own thread so the script
                    // download and README fetch can both be served
                    // concurrently rather than queueing behind each other.
                    std::thread::spawn(move || {
                        use std::io::{Read as _, Write as _};
                        let mut stream = stream;
                        // Drain the request up to the blank line ending its
                        // headers before responding — curl doesn't start
                        // reading a response until it has finished writing
                        // the request.
                        let mut chunk = [0u8; 4096];
                        let mut seen = Vec::new();
                        loop {
                            let Ok(n) = stream.read(&mut chunk) else {
                                return;
                            };
                            if n == 0 {
                                return;
                            }
                            seen.extend_from_slice(&chunk[..n]);
                            if seen.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                            if seen.len() > 65_536 {
                                return;
                            }
                        }
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes());
                        let _ = stream.flush();
                    });
                }
            });
            base_url
        }

        /// Acceptance scenario: verifies that a freshly installed Lua-scripted
        /// extension's commands become callable in the same session without
        /// restart. Drives a real registry that serves a real `init.lua`
        /// defining `vimcode.command("Hello1739Ext", ...)`, installs it via
        /// the real key-dispatch path, then proves the command becomes
        /// callable — through `:Hello1739Ext`, typed and run via the real
        /// command line, with its message painted to the status line — without
        /// any explicit `:Plugin reload` or restart.
        #[test]
        fn installed_extensions_script_becomes_callable_without_restart_1739() {
            use std::time::{Duration, Instant};

            let init_lua = r#"
                vimcode.command("Hello1739Ext", function(args)
                    vimcode.message("Hello1739Ext says: " .. args)
                end)
            "#;
            let base_url = spawn_multi_shot_http_fixture(init_lua);

            let (mut h, available_name, _home_guard) = harness_with_one_available_extension("scr");
            {
                let mut engine = h.engine.borrow_mut();
                if let Some(reg) = engine.ext_registry.as_mut() {
                    for m in reg.iter_mut() {
                        if m.name == available_name {
                            m.scripts = vec!["init.lua".to_string()];
                            m.registry_base_url = base_url.clone();
                        }
                    }
                }
            }

            let driver = &mut h.driver;
            driver.render();
            driver.dispatch(down_key());
            driver.dispatch(i_key());
            driver.render();
            assert!(
                driver.screen().contains("' installed"),
                "precondition: the install itself must still succeed \
                 synchronously; screen:\n{}",
                driver.screen()
            );

            // Escape first: the Extensions sidebar still has focus after
            // the install, and single letters in the command about to be
            // typed below (`world` contains `r`) would otherwise route
            // through `dispatch_ext_sidebar_action_key` instead of
            // reaching the command line at all (`r` there re-triggers
            // `ext_refresh()` — exactly what an earlier version of this
            // test tripped over, surfacing as a spurious "Extension
            // registry updated" message rather than the plugin's own).
            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Named(quadraui::NamedKey::Escape),
                modifiers: quadraui::Modifiers::default(),
                repeat: false,
            });

            // Poll the real idle hook (same `tick` both backends call every
            // frame) until the background download lands and
            // `poll_ext_scripts` reloads plugins — bounded so a
            // regression back to fire-and-forget hangs this loop instead
            // of silently passing. State inspection only gates the loop;
            // the actual pass/fail assertion below is on painted output
            // from running the command for real, exactly once. 10s (not
            // this file's usual 5s) because this one shells out to a real
            // `curl` subprocess, which can be slow to schedule under a
            // fully parallel `cargo test` run on a loaded machine.
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut loaded = false;
            while Instant::now() < deadline {
                driver.tick();
                loaded = h
                    .engine
                    .borrow()
                    .plugin_manager
                    .as_ref()
                    .is_some_and(|pm| pm.plugins.iter().any(|p| p.error.is_none()));
                if loaded {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                loaded,
                "#1739 review round 2: the install's background script \
                 download must land and `poll_ext_scripts` must reload \
                 plugins within 5s, so the extension's `init.lua` is \
                 loaded without restarting vimcode"
            );

            driver.type_char(':');
            for c in "Hello1739Ext world".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();
            let screen = driver.screen();

            assert!(
                screen.contains("Hello1739Ext says: world"),
                "#1739 review round 2: an extension's script, downloaded \
                 by the backgrounded install, must become callable \
                 (its command runnable, its message painted) within this \
                 session — no restart, no explicit `:Plugin reload` — \
                 within 5s of the install; screen:\n{screen}"
            );
            assert!(
                h.engine
                    .borrow()
                    .extension_state
                    .is_installed(&available_name),
                "#1739: the extension must still be marked installed"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Ex commands expand a leading `~` in a file-path argument before
    // resolving it, so `:vsplit ~/foo.txt` opens the real file under the
    // home directory rather than an empty buffer for a literal path under a
    // directory named `~`.
    mod tilde_expansion_1847 {
        use super::*;

        /// RAII guard returned by [`fresh_test_home_with_file`]: restores the
        /// previous `$HOME` override (via the inner `TestHomeGuard`) *and*
        /// deletes the temp directory it created, so the test doesn't leave
        /// a populated fake-home tree behind on disk.
        struct FreshHomeGuard {
            _home_guard: crate::core::paths::TestHomeGuard,
            dir: std::path::PathBuf,
        }

        impl Drop for FreshHomeGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.dir);
            }
        }

        /// Point `$HOME` at a fresh, unique temp dir holding one real file
        /// with distinctive content, for the duration of the returned guard.
        fn fresh_test_home_with_file(tag: &str, file_name: &str, content: &str) -> FreshHomeGuard {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let home = std::env::temp_dir().join(format!(
                "vimcode_test_1847_home_{tag}_{}_{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&home);
            std::fs::create_dir_all(&home).unwrap();
            std::fs::write(home.join(file_name), content).unwrap();
            let home_guard = crate::core::paths::set_test_home(&home);
            FreshHomeGuard {
                _home_guard: home_guard,
                dir: home,
            }
        }

        /// `:vsplit ~/<file>` must expand `~` to the test `$HOME` and open
        /// that file's real contents in the new split, not an empty buffer
        /// for a literal path under a directory named `~`.
        #[test]
        fn vsplit_tilde_path_opens_real_file_contents() {
            let _home_guard =
                fresh_test_home_with_file("vsplit", "issue1847.txt", "ISSUE1847_TILDE_OK");

            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            driver.type_char(':');
            for c in "vsplit ~/issue1847.txt".chars() {
                driver.type_char(c);
            }
            driver.press_named(quadraui::NamedKey::Enter);
            driver.render();

            let screen = driver.screen();
            assert!(
                screen.contains("ISSUE1847_TILDE_OK"),
                "`:vsplit ~/issue1847.txt` must expand `~` to the home \
                 directory and open the real file's contents, not an empty \
                 buffer for a literal path under a directory named `~`; \
                 screen:\n{screen}"
            );
        }
    }

    #[test]
    fn settings_colorscheme_cycles_on_return() {
        use crate::core::engine::sidebar::PANEL_SETTINGS;

        let mut engine = plain_engine();
        engine.toggle_sidebar_panel(PANEL_SETTINGS);
        let mut h = harness(engine);
        let driver = &mut h.driver;

        known_bug_gate("app_on_tui::settings_colorscheme_cycles_on_return", || {
            let before = driver.screen();
            assert!(
                before.contains("dark"),
                "precondition: default colorscheme is dark"
            );

            driver.dispatch(quadraui::UiEvent::KeyPressed {
                key: quadraui::Key::Named(quadraui::NamedKey::Enter),
                modifiers: quadraui::Modifiers::default(),
                repeat: false,
            });

            let after = driver.screen();
            assert!(
                after.contains("vscode-dark"),
                "colorscheme must cycle from dark to vscode-dark after Return; screen:\n{after}"
            );
        });
    }

    #[test]
    fn settings_colorscheme_cycles_after_click_open() {
        use crate::core::engine::sidebar::PANEL_SETTINGS;

        let engine = plain_engine();
        let mut h = harness(engine);

        known_bug_gate(
            "app_on_tui::settings_colorscheme_cycles_after_click_open",
            || {
                // Open settings by clicking the settings icon — the real user flow
                let bounds = h
                    .driver
                    .inventory()
                    .zones()
                    .iter()
                    .find(|z| z.id.as_str() == PANEL_SETTINGS)
                    .map(|z| z.bounds)
                    .expect("settings icon must register a chrome zone");
                h.driver.click(
                    bounds.x + bounds.width / 2.0,
                    bounds.y + bounds.height / 2.0,
                );

                let screen = h.driver.screen();
                eprintln!("=== After clicking settings icon ===");
                eprintln!("{}", &screen[..screen.len().min(1200)]);

                {
                    let engine = h.engine.borrow();
                    eprintln!("settings_has_focus: {}", engine.settings_has_focus);
                    eprintln!("settings_modal_open: {}", engine.settings_modal_open);
                    eprintln!("settings_selected: {}", engine.settings_selected);
                    eprintln!("activity_bar_focused: {}", engine.activity_bar_focused);
                    let flat = engine.settings_flat_list();
                    for (i, row) in flat.iter().enumerate().take(5) {
                        eprintln!("  flat[{i}] = {row:?}");
                    }
                }

                // Press Enter to cycle colorscheme
                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Enter),
                    modifiers: quadraui::Modifiers::default(),
                    repeat: false,
                });

                let after = h.driver.screen();
                eprintln!("=== After pressing Enter ===");
                eprintln!("{}", &after[..after.len().min(1200)]);

                {
                    let engine = h.engine.borrow();
                    eprintln!("colorscheme now: {}", engine.settings.colorscheme);
                    eprintln!("settings_selected: {}", engine.settings_selected);
                    eprintln!("settings_has_focus: {}", engine.settings_has_focus);
                    eprintln!("activity_bar_focused: {}", engine.activity_bar_focused);
                }

                assert_eq!(
                    h.engine.borrow().settings.colorscheme,
                    "vscode-dark",
                    "colorscheme must cycle from dark to vscode-dark after Enter"
                );
            },
        );
    }

    /// When the saved colorscheme is not in `available_colorschemes()` (e.g.
    /// a deprecated alias like "vscode-dark-modern"), Enter must still cycle
    /// to the first available option instead of silently doing nothing.
    #[test]
    fn settings_colorscheme_cycles_even_when_current_value_unlisted() {
        use crate::core::engine::sidebar::PANEL_SETTINGS;

        let engine = plain_engine();
        let mut h = harness(engine);

        known_bug_gate(
            "app_on_tui::settings_colorscheme_cycles_even_when_current_value_unlisted",
            || {
                h.engine.borrow_mut().settings.colorscheme = "vscode-dark-modern".to_string();

                let bounds = h
                    .driver
                    .inventory()
                    .zones()
                    .iter()
                    .find(|z| z.id.as_str() == PANEL_SETTINGS)
                    .map(|z| z.bounds)
                    .expect("settings icon zone");
                h.driver.click(
                    bounds.x + bounds.width / 2.0,
                    bounds.y + bounds.height / 2.0,
                );

                h.driver.dispatch(quadraui::UiEvent::KeyPressed {
                    key: quadraui::Key::Named(quadraui::NamedKey::Enter),
                    modifiers: quadraui::Modifiers::default(),
                    repeat: false,
                });
                assert_ne!(
                    h.engine.borrow().settings.colorscheme,
                    "vscode-dark-modern",
                    "colorscheme must cycle away from an unlisted value"
                );
            },
        );
    }
}
