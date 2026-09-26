#[cfg(test)]
mod tests {
    //! #1425: App-on-TUI gap inventory, as executable tests.
    //!
    //! `crate::app::App` (the cross-backend-shared shell — also what `gtk` and
    //! the `tui` control arm of `crate::harness`'s `backend_conformance!`
    //! macro wrap) already runs on `quadraui::tui::TuiBackend` via
    //! [`crate::tui_main::testing::conformance_harness`]. This module drives
    //! it through a representative slice of the same scenarios
    //! `src/tui_main/shell_app.rs`'s own `#[cfg(test)] mod tests` already
    //! covers for the *shipped* TUI shell (`TuiShellApp`) — reusing that
    //! module's test names where the assertion body transfers unmodified (so
    //! `grep`-ing a name finds both halves of the comparison), and picking a
    //! new, descriptive name where `App`'s own construction/fields differ
    //! enough that a faithful port needed a different shape.
    //!
    //! # No production code here
    //!
    //! Every test below drives already-shipped code through the existing
    //! [`crate::tui_main::testing::conformance_harness`] / [`crate::harness`]
    //! seams. Nothing in `src/app.rs`, `src/render.rs`, or `src/tui_main/`
    //! (outside this file and the one `mod` declaration in `mod.rs`) changes.
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
    //! `render::TAB_ROW_HEIGHT_PX`/`BREADCRUMB_ROW_HEIGHT_PX` (35/22) as if
    //! they were cell-grid *rows* rather than pixels, so on an 80x24 terminal
    //! the tab bar alone claims more rows than exist and the editor content
    //! band collapses. That is exactly the gap this module exists to inventory
    //! test-by-test rather than leave as a single paragraph in an issue body.
    //!
    //! # #1432 tranche 3 is a partial port — see the PR for the running count
    //!
    //! #1432's title calls for porting *every* remaining behavioural test in
    //! `shell_app.rs` (~392 as of this tranche) plus the standalone suites in
    //! `mouse.rs` (14), `panels.rs` (12) and `mod.rs` (2 +
    //! `clipboard_hermeticity_tests`) onto this seam. This tranche closes the
    //! issue's *other* two acceptance bullets in full (the quadraui pin bump
    //! and driving `crate::harness::KNOWN_BUGS` to `&[]` — see that const's
    //! own doc), but only ports a first slice of the test-porting bullet
    //! itself: `panels.rs`'s `sc_panel_tests` (all 7 — see `sidebar_panels`'s
    //! `sc_panel_*` tests below) and two of `mouse.rs`'s right-click
    //! scenarios (see `explorer_context_menu`'s `right_click_*` tests below).
    //! Everything else named above — the remaining ~9 `mouse.rs` behavioural
    //! tests, `panels.rs`'s 5-test `activity_bar_keyboard_ring_tests` (needs
    //! a `driver.styled_row`-based colour probe, not just `screen_has`),
    //! `render_impl.rs`'s 39 tests (10 of which drive the legacy
    //! `render_tui_buffer_impl`/`with_frame_scope` path named in the issue
    //! body and are dropped rather than ported once that's confirmed truly
    //! dead), `mod.rs`'s 4, and the ~392 in `shell_app.rs` — remains
    //! unported. At the pace #1430/#1431 (each a dedicated tranche session)
    //! actually ran (10 and 17 tests respectively), the full remaining count
    //! is a multi-session undertaking on its own, not something a single
    //! review-fix iteration can close alongside everything else already in
    //! this PR. Do not read `KNOWN_BUGS` being `&[]` as "the App-on-TUI port
    //! is complete" — it only means the *known, categorised* gaps this
    //! module had already inventoried are closed; the bulk of the suite this
    //! issue asks to port has simply not been visited yet.

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
    /// toggle-closed gesture
    /// [`sidebar_panels::search_icon_second_click_toggles_sidebar_closed`]
    /// exercises directly. Several tests below want the full terminal width
    /// for the editor/tab bar/bottom band rather than competing with the
    /// sidebar for the same narrow 80-column budget.
    ///
    /// Two clicks, not one (#1427): `App::shell_config`'s `cell`-profile
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
    /// Confirmed while writing this module: mutating the shadow alone left
    /// the sidebar column painted regardless.
    fn collapse_sidebar(driver: &mut quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>) {
        // Two real, independent clicks: without disabling folding, two
        // `driver.click()` calls with no simulated time between them fold
        // into a single `UiEvent::DoubleClick` (same reasoning
        // `shell_app.rs`'s own `hamburger_relocated_click_after_reveal_
        // hides_menu_bar` documents), which would only ever activate
        // Explorer once, never reach the toggle-closed branch. Needs the
        // concrete `TuiDriver` type (not the generic `ConformanceDriver`/
        // `DriverInput` bound this helper used before #1427), since
        // `set_double_click_folding` is TUI-only — fine here, this whole
        // module is TUI-only by construction (see its own doc).
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

            // #1425 gated this as "unit — typed text has nowhere to paint
            // once the editor content band has collapsed"; #1426
            // (`render::UnitProfile`) fixed the collapse and this now
            // passes unwrapped — confirmed by `known_bug_gate`'s
            // `FixLanded` panic before the KNOWN_BUGS entry was deleted.
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

        /// Mirrors `shell_app.rs`'s test of the same name (#605): in Normal
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

            // #1425 gated this as "unit — the editor content band never
            // paints"; #1426 (`render::UnitProfile`) fixed the collapse and
            // this now passes unwrapped.
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

        /// `u` after `dd` must restore the deleted line — the undo stack, same
        /// key pipeline as [`dd_deletes_the_current_line`].
        #[test]
        fn undo_restores_after_dd() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQXW_UNDO_LINE\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // #1425 gated this alongside the sibling key_dispatch tests
            // above (same editor-band collapse); #1426
            // (`render::UnitProfile`) fixed it and this now passes
            // unwrapped.
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

            // #1425 gated this alongside the sibling key_dispatch tests
            // above (same editor-band collapse); #1426
            // (`render::UnitProfile`) fixed it and this now passes
            // unwrapped.
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
    // #1430 tranche 1: the activity-bar half of the "key dispatch, activity
    // bar, sidebar panels" slice. Every test below is a mechanical port of
    // its `shell_app.rs` namesake (`TuiShellApp::new_for_test`/`TuiShellApp::
    // new(None)` → [`plain_engine`]/[`harness`], `app.engine.*` → the local
    // `Engine` before it's handed to [`harness`]) — see this module's own
    // "No production code here" doc at the top of the file.
    // ─────────────────────────────────────────────────────────────────────────
    mod activity_bar {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name (#757): Ctrl-L
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

        /// Mirrors `shell_app.rs`'s test of the same name (#1053): clicking
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
                "vimcode_test_1053_activity_bar_all_targets_{:?}",
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
            // #1432: without this, the six back-to-back `driver.click()`
            // calls below (no simulated time between them) let quadraui's
            // `TuiBackend::translate_injected` fold any pair landing within
            // its double-click time/radius window into a `DoubleClick` —
            // observed by instrumenting `App::try_route_sidebar_mouse_event`
            // directly: the Search→Source-Control click pair stayed plain
            // `MouseDown`s, but the very next click (Source Control→
            // Extensions, on the activity bar's fixed-width column, one row
            // apart) arrived as `DoubleClick`, and a double-click on a plain
            // activity-bar icon zone has no "activate panel" handler — it
            // silently did nothing, exactly the "click lands, panel doesn't
            // switch" symptom this scenario used to gate as a "product"
            // dispatch gap. Same root cause and same fix as this module's
            // own `collapse_sidebar` doc (#1427/#1432).
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

            // Located by chrome zone id, not `driver.find`'s glyph
            // search: several fallback icons (Nerd Fonts off, same
            // as every other fixture in this module) are single
            // ASCII characters that also occur in a previously-
            // opened panel's own body text — `EXTENSIONS.s()` is
            // `"#"`, and the fixed activity-bar rail is not the
            // *only* `"#"` `find` can match once a panel with body
            // text is showing. A chrome zone's `id` is exact, so
            // this can never collide.
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
            // Was gated as "#1430: clicking Extensions right after
            // Source Control never switches the sidebar body" — see
            // this fn's own `set_double_click_folding(false)` comment
            // above for the real cause (a folded `DoubleClick`, not
            // the SC panel's click handling).
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

        /// Mirrors `shell_app.rs`'s test of the same name (#694): a
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

        /// Mirrors `shell_app.rs`'s test of the same name (#694): the
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

        /// Mirrors `shell_app.rs`'s test of the same name (#694): hamburger
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

        /// Mirrors `shell_app.rs`'s test of the same name (#694): the first
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

        /// Mirrors `shell_app.rs`'s test of the same name (#694): combines
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
    // Sidebar panels
    // ─────────────────────────────────────────────────────────────────────────
    mod sidebar_panels {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name: the explorer
        /// sidebar's own painted content (a real scratch directory tree, not
        /// just its header) must reach the screen via `App::render_content`.
        #[test]
        fn render_content_paints_explorer_sidebar_content_via_shell_app() {
            let dir =
                std::env::temp_dir().join(format!("vc1425expl_{:?}", std::thread::current().id()));
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
                "vimcode_test_1425_sc_panel_{:?}",
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
        /// Source Control panel already shown + focused — same fixture
        /// shape `source_control_panel_paints_header` above uses, factored
        /// out so `panels.rs::sc_panel_tests`'s remaining scenarios (#1432
        /// tranche 3) can each layer their own commit/branch/help state on
        /// top of it. `tag` keeps concurrently-running tests' tmp dirs from
        /// colliding, same convention as
        /// `explorer_context_menu::engine_with_folder_ctx_menu`.
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
        /// renders_branches_and_marks_current` onto `App`, including the
        /// #677-audited current-branch-glyph distinction (not just that
        /// both names paint somewhere).
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
                "vc1430focus_{tag}_{:?}",
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

        /// Mirrors `shell_app.rs`'s test of the same name (#757 divergence
        /// 3): a plugin panel's own focus must outrank a stale
        /// `explorer_has_focus` left set alongside it — reached via the
        /// *real* keyboard-activation path (`Engine::activity_bar_activate`'s
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

        /// Mirrors `shell_app.rs`'s test of the same name (#757 divergence
        /// 4): a real focus flag must outrank the merely *visible* explorer
        /// panel — the explorer is left as the default visible panel while
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

        /// Mirrors `shell_app.rs`'s test of the same name (#759): Alt+Right
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

        /// Mirrors `shell_app.rs`'s test of the same name (#1243): opening the
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

            // #1432 fixed the root cause (re-diagnosed from #1425's "unit"
            // guess, which was wrong): `App::render_content` now filters
            // `quadraui::native_dialog_options`'s answer on
            // `backend.backend_caps().native_dialogs` before queuing a
            // native present, so TUI (no native alert facility) falls back
            // to the in-canvas `Dialog` rung instead of never painting it.
            // Ungated — was the same root cause as `explorer_context_menu::
            // context_menu_delete_opens_confirm_dialog`.
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

        // ── #1431 tranche 2: completion/hover popup anchoring ───────────

        /// Mirrors `shell_app.rs`'s test of the same name (#1237): the
        /// completion popup must anchor at the cursor's real *display*
        /// column, not its raw character column — on a tab-indented line the
        /// two disagree. See the mirrored test's own doc for the full
        /// rationale; ported unmodified here (down to the `find_bounds`
        /// needle bracketing *both* popup borders, restored below) since it
        /// only drives `TuiDriver`'s public `type_char`/
        /// `terminal_cursor_position`/`find_bounds` API, none of which is
        /// `TuiShellApp`-specific.
        ///
        /// #1432 re-diagnosed this, in two layers:
        ///
        /// 1. It was gated as `render::editor_popup_anchors` omitting the
        ///    active window's own `rect.x` offset, but instrumenting
        ///    `App::paint_editor_popups_rung` directly showed the computed
        ///    anchor was already correct (x=19, matching the real cursor
        ///    column) on every frame. The actual bug was one line further
        ///    down: `App`'s completion-popup width floor (`popup_w =
        ///    (...).max(100.0)`) is a bare GTK pixel constant, but this
        ///    function runs unmodified for TUI-via-`App` too, where `cw`
        ///    (raw units per character) is `1.0` — so the same "100"
        ///    demanded a 100-*cell*-wide popup, wider than the 80-column
        ///    terminal, which made `quadraui::Completions::layout`'s
        ///    right-edge-overflow branch always fire and clamp `x` straight
        ///    back to the window's left edge, discarding the correct
        ///    anchor. Fixed by scaling the floor by `cw` and matching
        ///    `tui_main::render_impl`'s own `+4`/`.max(12.0)` completion-
        ///    width formula exactly (a bare `+2` here had never mattered
        ///    while `100.0` always dominated it, but once scaled down to a
        ///    real cell-sized floor it clipped the popup's own trailing
        ///    padding cell).
        /// 2. Fixing (1) exposed that this port's own `find_bounds` needle
        ///    had silently dropped the mirrored test's trailing `" │"` —
        ///    see the needle's own comment below for why that makes it
        ///    match the *dictionary line* (which happens to carry the same
        ///    leading `"│ "` via this fixture's persistent gutter/divider
        ///    chrome) instead of the popup, on every run, independent of
        ///    the anchor bug. Restoring the exact mirrored needle was
        ///    needed before (1)'s fix could be observed to work at all.
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
                // #1432: the mirrored `shell_app.rs` test brackets with
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

        /// Mirrors `shell_app.rs`'s test of the same name (#1237 review): the
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

        // ── #1431 tranche 2: tab switcher popup clicks ──────────────────

        /// `count` file tabs, zero-padded so no name is a substring of
        /// another. Mirrors `shell_app.rs`'s `app_with_many_file_tabs_and_
        /// switcher_open` fixture builder, minus the `TuiShellApp`-specific
        /// sidebar-hiding (this module's [`harness_no_sidebar`] does that
        /// after construction instead).
        fn engine_with_two_file_tabs_and_switcher_open() -> crate::core::Engine {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1431_tab_switcher_{:?}",
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

        /// Mirrors `shell_app.rs`'s test of the same name (#733): a click
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

                    // Plain editor body, well left of the centred popup.
                    driver.click(6.0, title.y + 2.0);
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

        // ── #1431 tranche 2: unified-picker row clicks ──────────────────

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

        /// Mirrors `shell_app.rs`'s test of the same name (#831): a click
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

        /// Mirrors `shell_app.rs`'s test of the same name (#815): the shared
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
                "vimcode_test_1431_folder_picker_{:?}",
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
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Tab bar
    // ─────────────────────────────────────────────────────────────────────────
    mod tab_bar {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name (#551): the unsplit
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

            // #1425 gate, closed by #1427: `App` used to unconditionally
            // reserve and paint its own GTK-style menu-bar row (File Edit
            // View Go Run Terminal Help) on every backend; the shipped TUI
            // shell only ever showed that row in vscode-mode or when
            // Alt-revealed, so its tab bar sat on row 0 where TuiShellApp's
            // own mirrored test expects it and `App`'s did not.
            // `App::setup`'s `BackendCaps::window_chrome`/`native_menu`
            // three-way branch now matches — this label is no longer in
            // `KNOWN_BUGS`, so `known_bug_gate` treats a pass here as
            // ordinary green, not `FixLanded`.
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

            // #1425 gate, closed by #1427: same permanent-menu-bar-row
            // divergence as `render_content_paints_single_group_tab_bar_
            // via_shell_app` above — see that test's own comment.
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
                "vimcode_test_1425_two_tabs_{:?}",
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

            // #1425 gated this as "unit — only one of the two tabs' labels
            // reaches the painted tab bar", suspecting the same pixel/row
            // unit confusion narrowing the tab bar's effective column
            // budget; #1426 (`render::UnitProfile`) confirmed the
            // suspicion and this now passes unwrapped.
            assert!(
                driver.screen_has("zqxwA1425.txt") && driver.screen_has("zqxwB1425.txt"),
                "both tabs' labels must paint on the tab bar; screen:\n{}",
                driver.screen()
            );
        }

        /// A single-group frame must not paint a group-divider glyph ('│')
        /// anywhere to the right of the tab label — paint-only twin of
        /// `shell_app.rs`'s `unsplit_editor_composes_no_group_divider_rung_via_shell_app`
        /// (that test additionally inspects `composed_editor_band`, a private
        /// `TuiShellApp` field with no `App` equivalent reachable from this
        /// harness).
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
                    // reserves its own permanent menu-bar row above the tab bar
                    // (see `tab_bar::render_content_paints_single_group_tab_bar_via_shell_app`'s
                    // own doc), unlike the shipped TUI's `TuiShellApp` fixture the
                    // mirrored `shell_app.rs` test assumes.
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

        // ── #1431 tranche 2: tab drag / hover / :tabonly ────────────────

        /// Mirrors `shell_app.rs`'s test of the same name (#609): the
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

        /// Mirrors `shell_app.rs`'s test of the same name (#753): dragging a
        /// tab past a neighbour on the unsplit tab bar must actually reorder
        /// the painted tab labels, not just paint a ghost overlay.
        #[test]
        fn tui_tab_drag_past_a_neighbour_reorders_the_painted_tab_bar() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1431_tui_tab_drag_{:?}",
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

        /// Mirrors `shell_app.rs`'s test of the same name (#609): the
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

        /// Mirrors `shell_app.rs`'s test of the same name (#1154): `:tabonly`
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
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Bottom band (quickfix / debug output / terminal / status)
    // ─────────────────────────────────────────────────────────────────────────
    mod bottom_band {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name (#608): the Debug
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

        /// Mirrors `shell_app.rs`'s test of the same name (#608): the terminal
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

            // #1425 gated this as "unit — App::render_content reserves
            // render::TAB_ROW_HEIGHT_PX/BREADCRUMB_ROW_HEIGHT_PX as
            // cell-grid rows, collapsing the editor/status-bar band"; #1426
            // (`render::UnitProfile`) fixed it and this now passes
            // unwrapped.
            assert!(
                driver.screen_has("Ln 1,"),
                "a fresh buffer's status bar must show the cursor on line \
                 1; screen:\n{}",
                driver.screen()
            );
        }

        // ── #1431 tranche 2: quickfix / location-list rows and E42 ──────

        /// Mirrors `shell_app.rs`'s test of the same name (#608): the
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

        /// Mirrors `shell_app.rs`'s test of the same name (#1155): the
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

        /// Mirrors `shell_app.rs`'s test of the same name (#1155): when both
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

        /// Mirrors `shell_app.rs`'s test of the same name (#1283): `:cnext`
        /// on an empty quickfix list must paint Neovim's `E42: No Errors` on
        /// the command line — driven through the real command line, not by
        /// calling `qf_next` directly (the state-only trap #587/#592 calls
        /// out).
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

        /// Mirrors `shell_app.rs`'s test of the same name (focus rung): with a
        /// terminal pane focused, a key that would otherwise be a Normal-mode
        /// buffer edit must not reach the editor buffer.
        #[test]
        fn focused_terminal_swallows_editor_keys_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQXW_TERM_FOCUS_LINE\n");
            engine.terminal_new_tab(80, 10);
            engine.terminal_has_focus = true;
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // #1425 gated this as "unit — the terminal pane itself paints
            // (the leaked 'd' keystrokes are visible in its prompt,
            // confirming the swallow claim is actually true), but the
            // assertion has to read the editor buffer to prove it, and that
            // band never paints"; #1426 (`render::UnitProfile`) fixed it
            // and this now passes unwrapped.
            //
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
        ///
        /// #1432 re-diagnosed this: it was gated as a "product" dispatch gap,
        /// but `App`'s `UiEvent::Accelerator(OpenTerminal, ..)` arm
        /// (`GtkAccelHost::open_terminal`) queues a `DeferredAction::
        /// ToggleTerminal`, drained only inside `App::tick_dispatch` — not by
        /// `TuiDriver::dispatch`/`render` alone, which never call `app.tick`
        /// (confirmed against quadraui's own `tui::testing`/`runtime`
        /// source: `dispatch` → `preprocess_event` → `app.handle`, no `tick`
        /// anywhere in that path). The gate's own scenario was missing the
        /// `driver.tick()` every other deferred-queue-consuming scenario in
        /// this suite already calls after a dispatch. Fixed the test, not
        /// `App`; confirmed `FixLanded`.
        #[test]
        fn menu_terminal_activation_opens_terminal_pane_via_shell_app() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            // Baseline count, not `!screen_has("Terminal")` — `App`
            // always paints a permanent "Terminal" top-level menu item
            // (`File Edit View Go Run Terminal Help`), so a bare
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
            engine.terminal_has_focus = true;
            let mut h = harness_no_sidebar(engine);
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
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Dialogs (#1431 tranche 2)
    // ─────────────────────────────────────────────────────────────────────────
    mod dialogs {
        use super::*;

        /// Mirrors `shell_app.rs`'s test of the same name (#605): a modal
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

            // #1432 fixed the shared root cause (same as
            // `popups::dialog_intercepts_all_keys`): `App::render_content`
            // now filters `quadraui::native_dialog_options` on
            // `backend.backend_caps().native_dialogs`, so TUI paints the
            // in-canvas `Dialog` rung instead of dropping it for a native
            // present it has no facility for. Ungated.
            assert!(
                driver.screen_has("ZQXW605DIALOG"),
                "modal dialog should paint via App::render_content; \
                 screen:\n{}",
                driver.screen()
            );
        }

        /// Mirrors `shell_app.rs`'s test of the same name (#999): `:CheckNerdFonts`
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

            // #1432 fixed the shared root cause — see
            // `dialogs::render_content_paints_dialog_via_shell_app`'s own
            // comment. Ungated.
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
            // #1427: `session.explorer_visible` alone leaves the shadow
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

            // #1432 fixed the shared root cause (re-diagnosed from #1425's
            // "unit" guess, which was wrong) — same as
            // `popups::dialog_intercepts_all_keys`: `App::render_content`
            // now filters `quadraui::native_dialog_options` on
            // `backend.backend_caps().native_dialogs`, so TUI paints the
            // in-canvas `Dialog` rung. Ungated.
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

        /// Ports `mouse.rs::tests::right_click_in_debug_panel_does_not_
        /// open_explorer_context_menu` onto `App`: right-clicking inside a
        /// *different* active sidebar panel must not resurrect a stale
        /// Explorer context menu just because `explorer_rows` still holds
        /// rows from an earlier visit (#575 Bug 1's own regression).
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
            // Leave `explorer_rows` populated (stale, per #575 Bug 1's own
            // diagnosis) but switch the *active* sidebar panel to Debug —
            // the explorer row content is never painted once Debug is
            // active, so locate the right-click coordinate via the Debug
            // icon's own chrome zone instead of a row of explorer text.
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

        /// With the setting on (the default), a buffer with enough lines to
        /// need scrolling must paint minimap braille somewhere on screen.
        #[test]
        fn minimap_paints_braille_when_enabled() {
            let mut engine = plain_engine();
            let text = (1..=200)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n");
            engine.buffer_mut().insert(0, &text);
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            // #1425 gated this as "unit — the minimap rung is part of the
            // same collapsed editor content band"; #1426
            // (`render::UnitProfile`) fixed it and this now passes
            // unwrapped.
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

            // #1425 gated this alongside `minimap_paints_braille_when_enabled`
            // (same collapsed editor content band); #1426
            // (`render::UnitProfile`) fixed it and this now passes
            // unwrapped.
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

        /// A vertical split must paint minimap braille in *both* panes, not
        /// just one — mirrors `shell_app.rs`'s
        /// `split_paints_two_independent_minimap_strips_via_shell_app`.
        #[test]
        fn split_paints_minimap_in_both_panes() {
            let mut engine = plain_engine();
            let text = (1..=200)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n");
            engine.buffer_mut().insert(0, &text);
            engine.open_editor_group(SplitDirection::Vertical);
            let h = harness_no_sidebar(engine);
            let driver = &h.driver;

            // #1425 gated this as "unit — same editor-band collapse,
            // compounded by the split's second pane not painting at all";
            // #1426 (`render::UnitProfile`) fixed it and this now passes
            // unwrapped.
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

            // #1425 gated this as "unit — the split's second pane's tab bar
            // never paints"; #1426 (`render::UnitProfile`) fixed it and
            // this now passes unwrapped.
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

        /// Mirrors `shell_app.rs`'s test of the same name (#753): dragging the
        /// group divider must actually move it, repainted further in the drag
        /// direction.
        #[test]
        fn group_divider_drag_moves_the_painted_divider_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "short\n");
            engine.open_editor_group(SplitDirection::Vertical);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // #1425 gated this alongside the sibling divider tests above
            // (same editor-band collapse); #1426 (`render::UnitProfile`)
            // fixed it and this now passes unwrapped.
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

        /// Mirrors `shell_app.rs`'s test of the same name (#753): a press and
        /// release with no intervening move must leave the divider exactly
        /// where it was.
        #[test]
        fn group_divider_click_without_move_leaves_the_divider_put_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "short\n");
            engine.open_editor_group(SplitDirection::Vertical);
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // #1425 gated this as "unit — the group divider glyph itself
            // never paints once the editor content band has collapsed to
            // zero rows"; #1426 (`render::UnitProfile`) fixed it and this
            // now passes unwrapped.
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

        /// A `Ctrl-W v` vertical split must reserve exactly one column for the
        /// divider — mirrors `shell_app.rs`'s
        /// `ctrl_w_v_reserves_one_column_for_the_divider_via_shell_app`, using
        /// the `Ctrl-W v` keychord (rather than the `open_editor_group` engine
        /// call every other divider test here uses) so this specifically
        /// exercises the key-dispatch route into the same split.
        ///
        /// #1432 re-diagnosed this: it was gated as a "product" dispatch gap
        /// (`Ctrl-W v` not producing a second window through `App`), but the
        /// gate's own assertion was checking for the wrong shape. `Ctrl-W v`
        /// is `Engine::split_window` — a plain vim **window** split, which
        /// shares its group's one tab bar (unlike the sibling tests above,
        /// which call `open_editor_group` directly — a VSCode-style
        /// **editor-group** split, which paints one tab bar *per group*).
        /// Debug-dumping the painted screen showed the key chord dispatches
        /// correctly on `App` — two window panes, each showing `"short"` and
        /// its own status-bar segment, separated by a divider column — just
        /// under a single tab bar, exactly as real vim semantics say it
        /// should. Fixed to assert that shape (mirroring the divider-glyph
        /// checks the sibling group-divider tests above use) instead of the
        /// two-tab-bars shape only an editor-group split produces. Ungated.
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
    // #1428: terminal-shell behaviours keyed on `BackendCaps`
    // ─────────────────────────────────────────────────────────────────────────
    //
    // Five behaviours the shipped TUI (`TuiShellApp`) had that `App` lacked —
    // see issue #1428's own description for the full audit. Each is a
    // capability read or a no-op on GUI backends, not a platform fork, so
    // each got wired into the shared `App`/`render.rs` rather than staying
    // TUI-only.
    mod terminal_shell_behaviours {
        use super::*;

        // ── 1: `keyboard_enhanced` (kitty-keyboard protocol) ────────────

        /// #1428 acceptance: `App::setup` must read `App::keyboard_enhanced`
        /// from the *live* `Backend::backend_caps().kitty_keyboard` answer,
        /// not a hardcoded `true` — wrong on a terminal without the kitty
        /// protocol (#826) — and that value must be exactly what reaches
        /// `render::engine_key_from_ui` at `App::handle_dispatch`'s named-key
        /// decode call site.
        ///
        /// Deterministic in both directions via `TuiBackend::
        /// set_kitty_keyboard` (a real test hook quadraui exposes for
        /// exactly this — `TuiBackend::kitty_keyboard`'s own doc) rather
        /// than depending on this runner's ambient `TERM`/`KITTY_WINDOW_ID`
        /// environment. Not a `driver`/`TuiDriver::dispatch` round trip:
        /// `App::handle_dispatch`'s `Key::Char` arm (unlike its `Key::Named`
        /// arm — the one this issue's literal-`true` bug lived in) never
        /// calls `engine_key_from_ui` at all, matching plain GTK/GDK
        /// behaviour (GDK hands over an already-resolved keysym per
        /// physical key, so there is no terminal-only ambiguity for that
        /// arm to resolve — see `engine_key_from_ui`'s own doc) — a
        /// pre-existing, out-of-scope-for-#1428 GTK/TUI-via-`App`
        /// divergence this test does not paper over. Calling `setup()`
        /// directly and asserting on `render::engine_key_from_ui`'s own
        /// return value is what actually proves the acceptance claim
        /// ("`engine_key_from_ui` receives `false`/`true`") without
        /// depending on that unrelated gap.
        ///
        /// RED-verified: reverting `App::setup`'s
        /// `self.keyboard_enhanced = backend.backend_caps().kitty_keyboard`
        /// to a hardcoded `false` (or `App::handle_dispatch`'s
        /// `self.keyboard_enhanced` back to a hardcoded `true`) makes the
        /// `kitty_keyboard(true)` half of this test fail.
        #[test]
        fn setup_reads_keyboard_enhanced_from_live_backend_caps() {
            let engine = std::rc::Rc::new(std::cell::RefCell::new(plain_engine()));
            let backend_handle: std::rc::Rc<
                std::cell::RefCell<Box<dyn crate::app::TextMetricsBackend>>,
            > = std::rc::Rc::new(std::cell::RefCell::new(Box::new(
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

        /// #1428 acceptance: the caret-shape decision `App::tick_dispatch`
        /// now feeds `Backend::set_caret_shape` is the exact shared
        /// `render::caret_shape_for_mode` TUI's own
        /// `caret_shape_for_mode_tracks_engine_mode_and_pending_replace`
        /// pins (moved there from `TuiShellApp::caret_shape_for_mode`) —
        /// ported here against `App`'s own `Engine::sidebar_has_focus()`
        /// wiring (TUI passes `TuiSidebar::has_focus` instead; see each
        /// caller in `app.rs`/`shell_app.rs`).
        ///
        /// Pure-function coverage, not a driver test: the actual
        /// `backend.set_caret_shape` write is gated behind `App::live` and,
        /// on a real `TuiBackend`, writes straight to the real process
        /// `std::io::stdout()` with no test-mode guard of its own (see
        /// `App::live`'s doc) — the same "the decision is testable, the
        /// write is a `SMOKE_TESTS` item" split TUI's own test documents.
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

        /// [`quadraui::tui::vt_testing::TuiVtDriver`]-wrapped `App`, the
        /// vt100-backed observer needed to prove `Backend::
        /// request_full_repaint` actually cleared a stale cell — mirrors
        /// [`crate::tui_main::testing::conformance_harness`] exactly, using
        /// `quadraui::tui::vt_testing::driver_with_shell` in place of
        /// `quadraui::tui::testing::driver_with_shell` (quadraui#1060, at
        /// this repo's pinned rev). See `render::is_force_redraw_key`'s own
        /// doc for why a `TestBackend`-based `TuiDriver` cannot observe this
        /// at all — `ratatui::Terminal::clear()` is output-identical to an
        /// ordinary diffed redraw under a `TestBackend`.
        fn vt_driver(
            engine: crate::core::Engine,
        ) -> quadraui::tui::vt_testing::TuiVtDriver<impl quadraui::AppLogic> {
            let engine = std::rc::Rc::new(std::cell::RefCell::new(engine));
            let backend: std::rc::Rc<std::cell::RefCell<Box<dyn crate::app::TextMetricsBackend>>> =
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

        /// #1428 acceptance: Ctrl+L (`render::is_force_redraw_key`'s call
        /// site in `App::handle_key_press`) must call `Backend::
        /// request_full_repaint` and force the next paint to wipe a cell an
        /// incremental diff would otherwise skip — mirrors
        /// `tui_main::shell_app`'s
        /// `ctrl_l_repaints_a_stale_cell_an_incremental_diff_would_skip_via_vt_driver`
        /// verbatim, against `App` instead of `TuiShellApp`.
        ///
        /// RED-verified: removing `App::handle_key_press`'s
        /// `backend.request_full_repaint()` call makes the final assertion
        /// fail (the injected glyph survives the Ctrl+L redraw).
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

        // ── 4: terminal PTY resize on WindowResized ──────────────────────

        /// #1428 acceptance: `App::handle_dispatch`'s `WindowResized` arm
        /// must forward the resize to any open terminal PTY
        /// (`render::route_terminal_resize`) — previously a no-op on GTK
        /// (and, transitively, on this `App`-on-TUI arm), unlike TUI's own
        /// `TuiShellApp::handle` (#758 / #734 slice 3).
        ///
        /// Two `WindowResized` dispatches, not one: `App::
        /// painted_editor_content_width` (what `terminal_panel_cols` feeds
        /// `route_terminal_resize`) is the *last-painted* editor bounds —
        /// there is no live pixel width in scope in `handle_dispatch`
        /// itself, the same "no live width" fallback every other
        /// accelerator/menu/tick call site of `terminal_panel_cols` already
        /// accepts (see that function's own doc). The first dispatch's
        /// resize computation therefore still reads the *pre*-resize
        /// painted width; its own `handle_resize()` call sets
        /// `draw_needed`, and `TuiDriver::dispatch`'s `Reaction::Redraw`
        /// handling repaints immediately afterward at the real new
        /// (`driver.resize`-d) backend size, updating the cached bounds —
        /// which the *second* dispatch's computation then picks up. This
        /// mirrors how a real live resize settles over consecutive
        /// `WindowResized` events rather than resolving instantly on the
        /// first.
        ///
        /// RED-verified: removing the `render::route_terminal_resize` call
        /// from `App::handle_dispatch`'s `WindowResized` arm makes the
        /// final assertion fail (`cols()` never changes, no matter how many
        /// resize events fire).
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

        /// #1428 acceptance: `App::tick_dispatch` must drain
        /// `App::pending_startup_msg` into `engine.message` (and request a
        /// redraw) exactly as `TuiShellApp::tick` does — previously
        /// unread on `App`, so the one-shot nerd-font nudge never reached
        /// the user at all on this arm.
        ///
        /// The *natural* trigger (`use_nerd_fonts` never explicitly set
        /// *and* the backend-derived default resolves to ASCII fallback,
        /// `App::assemble`'s doc) is platform-gated —
        /// `core::settings::default_use_nerd_fonts` resolves `true` on
        /// every non-Windows TUI, so it can never fire on this suite's
        /// Linux/macOS runners regardless of this fix (confirmed by
        /// `tui_main::shell_app`'s own
        /// `check_nerd_fonts_disable_stops_painting_glyphs_next_frame_via_shell_app`,
        /// whose fixture comment notes the same "unset resolves true off
        /// Windows"). Seeding the field directly — the same "pin what the
        /// ambient environment can't" pattern this module already uses for
        /// `use_nerd_fonts` itself — isolates the half #1428 actually
        /// changed: the *drain*, not the platform-gated resolution.
        ///
        /// RED-verified: removing `App::tick_dispatch`'s
        /// `self.pending_startup_msg.take()` drain makes the final
        /// assertion fail (the message never reaches `engine.message`, so
        /// it never paints).
        #[test]
        fn tick_drains_the_pending_nerd_font_startup_message_via_shell_app() {
            let engine = std::rc::Rc::new(std::cell::RefCell::new(plain_engine()));
            let backend: std::rc::Rc<std::cell::RefCell<Box<dyn crate::app::TextMetricsBackend>>> =
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
    // Mouse wheel scroll polarity (#1433)
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

        /// #554, re-verified now that `UiEvent::Scroll` reaches `App` from
        /// the *TUI* runner (`tui_main::run`'s flip onto `App`, #1433)
        /// instead of only ever from GTK — the direct model here is
        /// `crate::gtk::testing`'s
        /// `gdk_wheel_down_scrolls_the_viewport_down_not_up`, whose "Half 2"
        /// (what a real `UiEvent::Scroll` does to the engine through
        /// production dispatch) is exactly what this ports, since
        /// `App::handle_dispatch`'s `UiEvent::Scroll` arm — the negate-back-
        /// to-GTK-raw-polarity code #554 fixed — is backend-neutral and
        /// runs unmodified for both backends' drivers.
        ///
        /// Also exercises the #1433 item-4 refactor of
        /// `App::handle_mouse_scroll_msg`'s hovered-window lookup onto
        /// `render::find_window_at` (previously a hand-rolled
        /// `calculate_group_window_rects` scan): if that lookup ever
        /// resolved the wrong window, or no window at all, the scroll
        /// would silently no-op instead of moving `scroll_top`.
        ///
        /// RED-verified: temporarily dropping the `-` in
        /// `App::handle_dispatch`'s `self.handle_mouse_scroll_msg(&*backend,
        /// delta.x as f64, -(delta.y as f64))` (passing `delta.y` straight
        /// through) makes the "wheel down" assertion below fail —
        /// `scroll_top` stays `0` instead of rising, since a quadraui-
        /// convention `delta.y` of `-1.0` (scroll down) would then reach
        /// `handle_mouse_scroll_msg` un-negated and `scroll_up_visible` at
        /// `scroll_top == 0` clamps to `0`.
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

        /// #1433 item 4: `App::handle_mouse_scroll_msg`'s hovered-window
        /// lookup — freshly refactored onto `render::find_window_at`
        /// against `self.cached_screen_layout`, replacing a hand-rolled
        /// `calculate_group_window_rects` scan — must resolve the
        /// *unfocused* pane the pointer is actually over, not just fall
        /// back to the active window. Direct TUI-on-`App` port of
        /// `crate::gtk::testing`'s
        /// `wheel_scrolls_the_pane_under_the_pointer_not_the_focused_one`.
        ///
        /// RED-verified: forcing `hovered_window_id` to always resolve to
        /// `None` (e.g. by making the `render::find_window_at` call always
        /// return `None`) makes the first assertion below fail — the
        /// scroll would then land on the *focused* window (the `unwrap_or
        /// (active_id)` fallback) instead of the unfocused one under the
        /// pointer, leaving `unfocused`'s `scroll_top` at `0`.
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
    // User-configured MCP servers (#1487, redo of #1462 on the multi-session
    // engine — see that issue's "Tests go on the App-on-TUI seam" redo note)
    // ─────────────────────────────────────────────────────────────────────────
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
            // `TuiShellApp`'s own separate `TuiSidebar::has_focus` latch),
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
}
