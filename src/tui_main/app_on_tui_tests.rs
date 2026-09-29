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
    //! covers for the *shipped* TUI shell (the pre-#1434 TUI shell) — reusing that
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
    // its `shell_app.rs` namesake (the pre-#1434 TUI shell's `new_for_test`/
    // `new(None)` → [`plain_engine`]/[`harness`], `app.engine.*` → the local
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

        /// #1574: the Explorer's selected row, when the tree does **not**
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
            let dir = std::env::temp_dir()
                .join(format!("vc1574explinact_{:?}", std::thread::current().id()));
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

            let expected_bg = quadraui::tui::ratatui_color(
                crate::render::Theme::vscode_light().sidebar_sel_bg_inactive,
            );
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

        /// #1576: the sidebar header row (the `" EXPLORER "` strip above the
        /// tree, painted by quadraui's `AppShell::render`) must come from
        /// vimcode's own theme, not quadraui's hard-coded VS-Code-dark
        /// literal `Color::rgb(37, 37, 38)` (`#252526`) paired with
        /// `Color::rgb(220, 220, 220)` foreground. `to_quadraui_theme_chrome`
        /// (`src/render.rs`) has mapped `header_bg`/`header_fg` from
        /// `theme.status_bg`/`theme.status_fg` since #1574, but until
        /// quadraui#1180 (picked up by this issue's pin bump) `AppShell::
        /// render` never read those fields at all — it painted the literal
        /// directly, so the mapping had no effect on what actually reached
        /// the screen. Confirmed red against the pre-#1576 pin: with the
        /// old rev, this assertion fails because the painted header bg/fg
        /// are quadraui's literal, not `vscode_light`'s `#007acc`/`#ffffff`.
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

            let theme = crate::render::Theme::vscode_light();
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

        /// #1574: the search sidebar's `"  NN: "` line-number prefix must
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
                quadraui::tui::ratatui_color(crate::render::Theme::vscode_light().line_number_fg);
            assert_eq!(
                style.fg,
                expected_fg,
                "the search sidebar's line-number prefix must paint with \
                 `theme.line_number_fg`, not a hard-coded grey; screen:\n{}",
                driver.screen()
            );
        }

        /// #1545: dotfiles show in the explorer by default (VS Code-style)
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
            let dir =
                std::env::temp_dir().join(format!("vc1545dot_{:?}", std::thread::current().id()));
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

        // #1574: no TUI driver test covers the `PANEL_SETTINGS` arm's
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
        /// the pre-#1434 TUI shell-specific.
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
        /// another. Mirrored the deleted `shell_app.rs`'s (#1434)
        /// `app_with_many_file_tabs_and_switcher_open` fixture builder,
        /// minus the pre-#1434 TUI shell-specific sidebar-hiding (this
        /// module's [`harness_no_sidebar`] does that after construction
        /// instead).
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

                    // Plain editor body, well left of the centred popup —
                    // #1543: line numbers now default on, so the gutter is
                    // wider than the fixed column 6 this used to click;
                    // resolve a real text pixel instead of a hardcoded one.
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

        /// #1545: flipping `show_hidden_files` on by default must not leak
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
            let dir =
                std::env::temp_dir().join(format!("vc1545qo_{:?}", std::thread::current().id()));
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
            // Alt-revealed, so its tab bar sat on row 0 where the pre-#1434 TUI shell's
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

        /// #1586: a *stacked* split (`Ctrl+W s` / `open_editor_group
        /// (Horizontal)`) must paint the lower group's own tab row with its
        /// active tab's label — not overwritten by the group-boundary
        /// divider line, which #1586 traced to landing on exactly that row
        /// (`GroupLayout::calculate_group_rects`/`dividers` reserve zero
        /// divider thickness for a stacked split, so the divider's
        /// `position` coincides exactly with the lower group's tab-row
        /// band; the fix, `render::painted_group_dividers`, simply never
        /// paints a line for that direction — see its own doc for why
        /// reserving extra geometric space instead cannot be made to work
        /// uniformly across backends).
        ///
        /// Deliberately reads the *painted* row the current frame's own
        /// `ScreenLayout::group_tab_bars` says the bottom group's tab bar
        /// occupies (`bounds.y - tab_bar_height`, one cell row with
        /// breadcrumbs off), rather than a hardcoded row number or a bare
        /// `screen_has` — a bare `screen_has(label)` would pass even on the
        /// unfixed bug, since the breadcrumb row directly below the (missing)
        /// tab row already shows the same filename as its last path segment.
        ///
        /// RED-verified against unfixed `develop` (53894ed): before #1586,
        /// [`crate::render::EditorOp::GroupDividers`]'s body painted every
        /// entry in `screen.group_dividers` unconditionally, including a
        /// stacked one landing on this exact row — this row then paints the
        /// divider line (`'─'` repeated across the group's width) and
        /// contains neither tab label.
        ///
        /// Uses a `(80, 25)` driver, not this module's usual `(80, 24)`
        /// (see [`harness`]): at `24` rows the pre-fix boundary happens to
        /// land on an exact `.5` row (e.g. `11.5`), and the tab-bar rung
        /// *rounds* that to place the label row while the divider rung
        /// *truncates* it to place its own — two different conventions that,
        /// on a `.5` tie, land one row apart and accidentally dodge the
        /// collision this test exists to catch. `25` rows lands the boundary
        /// on a whole row, where both conventions agree and the bug
        /// reproduces (confirmed empirically against unfixed `develop` before
        /// picking this size).
        #[test]
        fn stacked_groups_bottom_tab_row_shows_its_label_1586() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1586_stacked_tabs_{:?}",
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
        /// anywhere to the right of the tab label — paint-only twin of
        /// `shell_app.rs`'s `unsplit_editor_composes_no_group_divider_rung_via_shell_app`
        /// (that test additionally inspects `composed_editor_band`, a private
        /// the pre-#1434 TUI shell field with no `App` equivalent reachable from this
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
                    // own doc), unlike the shipped TUI's the pre-#1434 TUI shell fixture the
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

        /// #1580 acceptance: right-clicking in the Explorer opens the
        /// painted menu, clicking an item runs its command, and Escape
        /// dismisses it — driven entirely through mouse/keyboard events
        /// (`driver.right_click`/`driver.click`/`driver.press_named`), not
        /// `Engine::open_explorer_context_menu` called directly the way
        /// `engine_with_folder_ctx_menu`'s fixture above does.
        ///
        /// Parametrized over every `MenuStyle` value: `quadraui::TuiBackend`
        /// always declares `BackendCaps::native_menu: false`, so
        /// `quadraui::MenuStyle::resolve` (via `Backend::
        /// effective_menu_style`) resolves `Custom` here regardless of the
        /// setting — this loop is the black-box proof that holds for every
        /// value, not just the default.
        ///
        /// "Copy Path" is the item under test: unlike `new_file`/
        /// `open_terminal`/`rename`/`delete` (UI-backend-plumbed, only via
        /// the *keyboard* confirm path — see `dispatch_context_menu_key`'s
        /// doc), `copy_path` runs entirely inside `Engine::
        /// context_menu_confirm` itself, so it exercises exactly the mouse-
        /// click route (`apply_context_menu_route`) this test drives
        /// through, with an effect (`engine.message`, painted in the status
        /// bar) that is unmistakably observable and impossible to confuse
        /// with "the click was silently swallowed".
        ///
        /// RED against a body that regressed the paint/click/dismiss round
        /// trip for any one `MenuStyle` value: the loop runs the full
        /// scenario fresh per variant, so a regression scoped to one value
        /// still fails this test.
        ///
        /// **RED-verified against unfixed `develop`.** Flipping the
        /// `FrameOp::ContextMenu` rung's guard in `App::render_content`
        /// from `effective_menu_style() == Custom` to `== Native` (the
        /// exact shape of the special-case bug described above — it still
        /// resolves `Custom` on `TuiBackend`, since `native_menu` is always
        /// `false`, so this is testing the rung's own guard, not
        /// `effective_menu_style` itself) makes this test fail: the menu
        /// never paints, so `driver.find("Copy Path")` after the first
        /// `right_click` panics. Restored before committing.
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
    // Five behaviours the shipped TUI (the pre-#1434 TUI shell) had that `App` lacked —
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

        /// #1428 acceptance: the caret-shape decision `App::tick_dispatch`
        /// now feeds `Backend::set_caret_shape` is the exact shared
        /// `render::caret_shape_for_mode` TUI's own
        /// `caret_shape_for_mode_tracks_engine_mode_and_pending_replace`
        /// pins (moved there from the pre-#1434 TUI shell's `caret_shape_for_mode`) —
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

        /// #1428 acceptance: Ctrl+L (`render::is_force_redraw_key`'s call
        /// site in `App::handle_key_press`) must call `Backend::
        /// request_full_repaint` and force the next paint to wipe a cell an
        /// incremental diff would otherwise skip — mirrors
        /// `tui_main::shell_app`'s
        /// `ctrl_l_repaints_a_stale_cell_an_incremental_diff_would_skip_via_vt_driver`
        /// verbatim, against `App` instead of the pre-#1434 TUI shell.
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
        /// the pre-#1434 TUI shell's `handle` (#758 / #734 slice 3).
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
        /// redraw) exactly as the pre-#1434 TUI shell's `tick` does — previously
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
    // Editor scrollbars (#1493)
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
    // `:set wrap linebreak` scroll drift (#1496)
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
        /// for, drifting `G` off the bottom of the viewport (#1496).
        ///
        /// RED verified: reverting `render.rs`'s `wrap_text_len` trim makes
        /// the next line paint two rows below the wrapped one, not one.
        #[test]
        fn exactly_full_wrapped_line_paints_no_trailing_blank_row() {
            let _settings_guard = crate::core::settings::TestSettingsPathGuard::install(
                std::env::temp_dir().join(format!(
                    "vimcode_test_1496_no_settings_{:?}.json",
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
                    "vimcode_test_1496_no_settings_{:?}.json",
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
    // #1543: line numbers default to absolute (VS Code/Neovim hybrid default)
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

    // ─────────────────────────────────────────────────────────────────────────
    // Fold gutter controls (#1544, VS Code's `editor.showFoldingControls`)
    // ─────────────────────────────────────────────────────────────────────────
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

        /// Non-blocking review follow-up (#1544): the settings round-trip
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

        /// Non-blocking review follow-up (#1544): same gap as above for
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
    // Sticky scroll (#1546): pin enclosing-scope header lines at the top of
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

            // #1511: expand the card — collapsed cards show only
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
        /// behaviour), the loop below observes "Tool kind: edit" paint and
        /// fails immediately instead of waiting out the full 5s with it
        /// absent.
        #[cfg(unix)]
        #[test]
        fn acp_permission_default_allow_all_skips_the_dialog_via_shell_app() {
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

            let deadline = Instant::now() + Duration::from_secs(5);
            while h.engine.borrow().acp().ai_streaming && Instant::now() < deadline {
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
                 completion within 5s, not hang waiting for a human who is \
                 never asked"
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

            // #1520 review: the default 20-cell sidebar width clips the
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

    // ─────────────────────────────────────────────────────────────────────
    // #1500: public App-on-TUI test seam
    //
    // Mirrors `tests/acceptance/ms-example/seam_657.rs`'s TUI half exactly
    // — same marker, same two assertions — but built with only the new
    // `crate::tui_main::testing::tui_driver`/`tui_driver_with` functions,
    // not `crate::`-private items (`conformance_harness`,
    // `ConformanceHarness`, `App` itself). That restriction is the point:
    // it proves an *external* crate could write these same two tests once
    // `seam_657.rs` is re-pointed at this seam instead of the pre-#1434 TUI shell
    // (which #1434 is about to delete).
    // ─────────────────────────────────────────────────────────────────────
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
    }

    // ─────────────────────────────────────────────────────────────────────
    // #1560 review fix: black-box coverage for the BOM decode fix
    //
    // CLAUDE.md's "Testing (CRITICAL)" section requires a driver test
    // asserting on rendered output for any user-visible behaviour change —
    // the unit tests added to `src/core/buffer.rs` cover the decode logic
    // in isolation, but not that a UTF-16/UTF-8-BOM file opened through the
    // real production path (`Engine::open` → `App` → `TuiDriver`) actually
    // paints its decoded content instead of the "stream did not contain
    // valid UTF-8" error (and the stuck-looking `[No Name]` buffer) the
    // issue reported.
    // ─────────────────────────────────────────────────────────────────────
    mod file_encoding {
        use super::*;

        use crate::core::Engine;

        /// #1560: opening a UTF-16LE-with-BOM file (Notepad's/PowerShell's
        /// "Unicode" default on Windows) through `Engine::open` — the exact
        /// fixed read path (`Buffer::from_file` → `decode_file_bytes`) —
        /// must paint the decoded text on screen, with no trace of the raw
        /// decode error.
        #[test]
        fn opening_utf16le_bom_file_shows_decoded_content_via_shell_app() {
            let path = std::env::temp_dir().join(format!(
                "vimcode_test_1560_utf16le_{:?}.txt",
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

        /// #1560: mirror of the above for a UTF-8-with-BOM file.
        #[test]
        fn opening_utf8_bom_file_shows_decoded_content_via_shell_app() {
            let path = std::env::temp_dir().join(format!(
                "vimcode_test_1560_utf8bom_{:?}.txt",
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
    // #1583: idle-stability — comment text must not flash, and an idle
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
                "vimcode_test_1583_home_{:?}",
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&home);
            std::fs::create_dir_all(&home).unwrap();
            let home_guard = crate::core::paths::set_test_home(&home);

            let path = std::env::temp_dir().join(format!(
                "vimcode_test_1583_{:?}.rs",
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

    /// #1397/#1577: the recommended-extension install offer, TUI half of
    /// the black-box coverage — GTK's twin is
    /// `crate::gtk::testing::issue_1577_ext_install_offer_toast`, which its
    /// own module doc explains the shared rationale for (title/action-button
    /// wording, widget-id scheme, why click targets come from the cached
    /// `toast_layout` rather than text search). `ConformanceHarness::engine`
    /// stays reachable here too, so this module reads engine state directly
    /// exactly like the GTK twin, rather than needing the `find`/style-probe
    /// workarounds this module's own header doc says older TUI-only
    /// harnesses used to require.
    mod issue_1577_ext_install_offer_toast {
        use super::*;

        /// Build a [`harness`] whose first frame already shows the
        /// #1397/#1577 install-offer toast for a synthetic extension — same
        /// fixture shape as the GTK twin's `harness_with_ext_install_offer`,
        /// including the two-"hello"-line buffer the `N`-search regression
        /// test below needs.
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

        /// #1397/#1577 core acceptance (TUI half): opening a file whose
        /// recommended extension isn't installed must paint a toast offer
        /// with separate, non-overlapping "Install" and "Don't ask again"
        /// buttons — not a missable status-line hint, and not shortcuts
        /// printed as body text.
        ///
        /// **Verified RED against unfixed `develop`:** before #1577 there
        /// was only one action button (`action_rects.get(1)` would panic)
        /// and "Don't ask again" was body text, not a button — reverting
        /// `Engine::push_extension_recommendation_toast` to the pre-#1577
        /// single-action `push_sticky_action_toast` call reproduces both
        /// failures (checked against this test directly).
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

        /// #1577 core regression: `N` must NOT be hijacked by the toast.
        /// With the pre-#1577 code, pressing `N` in Normal mode — vim's own
        /// "previous search match" — while this (sticky, never-expiring)
        /// toast happened to be up would silently and permanently dismiss
        /// the extension instead of searching. Drives a real backward
        /// search (`*` then `N`) and asserts the cursor actually moves like
        /// vim `N`, AND that the toast and dismissal state are completely
        /// unaffected.
        ///
        /// **Verified RED against unfixed `develop`:** with the old `N`
        /// interception restored ahead of normal key dispatch, this test's
        /// cursor-line assertion fails (`N` dismisses instead of searching)
        /// and `is_dismissed` comes back `true`.
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

        /// #1577: clicking "Install" starts the install
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

        /// #1577: clicking "Don't ask again" persists the dismissal.
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

        /// #1577: clicking × hides the toast without persisting anything.
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

        /// #1577: keyboard access via `Engine::focus_toast_stack`
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

        /// #1577: `:Notifications` (the ex-command spelling of
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

    /// #1346 review: driver-tier black-box coverage for the "package-manager
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

        /// #1346 acceptance: a manifest's `[lsp.acquire]` naming a
        /// package-manager kind (`npm`) whose runtime isn't on PATH must
        /// paint a "needs npm" status line — the visible fallback, not a
        /// silent no-op and not a native-acquisition attempt that would
        /// just fail. `runtime_present` is stubbed to always return `false`
        /// via the `pub(crate)` `ext_install_from_registry_with_runtime_
        /// check` seam, exactly like the engine-internal-state sibling test
        /// in `core::engine::lsp_ops`.
        ///
        /// Verified RED two ways: (1) against the pre-#1346-review-fix
        /// build, where `ext_install_from_registry_with_runtime_check` was
        /// still private — this test could not even compile, i.e. the
        /// black-box coverage gap the review flagged; and (2) against a
        /// deliberately reintroduced logic regression (commenting out the
        /// `status_parts.push(format!("LSP: needs {runtime} — {hint}"))`
        /// line in `lsp_ops.rs`), confirming this test also catches a real
        /// behavioural break, not just a visibility change — see this
        /// module's own doc comment above.
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

        /// #1346 acceptance: the same missing-runtime fallback must also
        /// queue the terminal-tier command carrying the dependency hint —
        /// the install path the user actually runs once they've read the
        /// status line above, exercised here through the extension's DAP
        /// half (`[dap.acquire]`) so both `resolve_acquire_action` call
        /// sites in `lsp_ops.rs` get driver-tier coverage, not just the LSP
        /// one.
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

    // ─────────────────────────────────────────────────────────────────────
    // #1507: AI panel persistent send/stop/leave hint + `<leader>ai` focus
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

        /// #1507 acceptance: the send/stop/leave hint
        /// `render::populate_ai_chat_controller` pins to
        /// `ChatController::set_hint` must (a) actually reach the painted
        /// screen once the panel has keyboard focus and (b) stay on screen
        /// once the user starts typing a message — unlike the `TextInput`
        /// placeholder it sits above, which the panel already painted
        /// before #1507 and which vanishes the moment the input buffer is
        /// non-empty (`ChatController`'s own *Persistent hint line* doc).
        /// Focus is driven in through the real `<leader>ai` key sequence
        /// (default leader: Space), not `engine.ai_has_focus = true`
        /// directly, so the test exercises the same production key-dispatch
        /// path a user's keystrokes take.
        ///
        /// RED verified: with the `chat.set_hint(...)` call this issue adds
        /// to `populate_ai_chat_controller` removed, this fails — the
        /// screen after `<leader>ai` shows none of "send"/"stop"/"editor",
        /// and typing "hi" doesn't change that (there was never anything to
        /// lose).
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

            // Unlike the pre-#1507 empty-input placeholder, the hint must
            // not vanish once the user starts composing a message.
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

        /// #1507 acceptance: pressing `<leader>ai` again while the AI panel
        /// already has keyboard focus and its input is empty toggles focus
        /// back to the editor — `Engine::ai_leader_toggle_key`, intercepted
        /// by `render::route_ai_chat_event` ahead of `ChatController::
        /// handle`'s ordinary text-insertion path.
        ///
        /// Proves focus genuinely returned to the editor, not merely that
        /// `ai_has_focus` flipped in isolation (CLAUDE.md's "rendered
        /// output, not state" rule): a subsequent `i` keystroke must enter
        /// the editor's own Insert mode (painted in the window status
        /// line's mode segment, `Engine::mode_str`) — if focus were still on
        /// the chat panel, that same `i` would instead be typed as a
        /// literal character into the now-empty message box, and Insert
        /// mode would never show.
        ///
        /// RED verified: with `render::route_ai_chat_event`'s
        /// `ai_leader_toggle_key` intercept removed (so `<leader>ai`'s three
        /// characters fall through to `ChatController::handle`'s plain
        /// text-insertion path instead), this fails two ways: `ai_has_focus`
        /// stays `true`, and the follow-up `i` reads back as literal text
        /// (`ChatController::input_text() == "i"`), never as an editor mode
        /// change — the screen never shows "INSERT" at all.
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

        /// #1507: typing the literal characters ` ai` into an in-progress
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

        /// #1507 review: a partial `<leader>ai` match interrupted by a key
        /// `Engine::ai_leader_toggle_key` never sees at all (any
        /// `quadraui::Key::Named`, not just a plain mismatched `Char`) must
        /// still be replayed into the chat input rather than silently
        /// dropped. Concrete repro from the review: type the leader (Space)
        /// then `a` — both buffered, nothing visible yet — then press Enter
        /// to start a second line before typing the rest of the message.
        /// `ai_chat_submit_on_enter` is forced `false` here (#1509 flipped
        /// its default to `true`, Zed parity — see
        /// `issue_1509_ai_chat_submit_on_enter_and_stop`) so plain Enter
        /// still inserts a newline rather than submitting: this test is
        /// about the leader-prefix replay, not about which key sends, and
        /// keeping it pinned to the newline-on-Enter mode is what makes the
        /// two-line " a\nrest" assertion below meaningful regardless of the
        /// setting's default.
        ///
        /// RED verified against this fix removed (i.e. `route_ai_chat_event`
        /// only ever consulting `ai_leader_toggle_key` for a plain,
        /// unmodified `Char`, with no fallback for `Named` keys): the
        /// buffered " a" is discarded when Enter arrives, and the input
        /// reads back as "\nrest" instead of " a\nrest".
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

        /// #1507 review, "related" point: a partial match abandoned via
        /// Escape (leaving the panel) must not survive to wrongly complete
        /// against an unrelated later message once the panel regains focus.
        /// Guarded by two independent layers that each discard the buffer
        /// on the way out — `dispatch_ai_chat_event`'s `Cancelled` arm, and
        /// `route_ai_chat_event`'s own `Escape`-clears-pending branch ahead
        /// of it — either one alone is enough to pass this test; see
        /// `ai_leader_toggle_key_tests::cancelled_discards_a_buffered_partial_match`
        /// for a unit test that isolates the `Cancelled`-arm layer
        /// specifically by calling `dispatch_ai_chat_event` directly.
        ///
        /// RED verified against the pre-fix code (neither layer present,
        /// i.e. `develop` before this fix): the second `<leader>a` + `i`
        /// below (typed as an ordinary message opener, with the panel
        /// re-entered via the palette-equivalent direct `ai_has_focus =
        /// true` this harness's re-open step performs) wrongly completes
        /// the stale match left over from the first, abandoned attempt and
        /// toggles focus off instead of leaving "i" as literal input text.
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

        /// #1508 acceptance: the AI panel's status strip must show the live
        /// in-progress tool call's title plus an elapsed-time reading while
        /// busy, replacing the old frozen `"(thinking\u{2026})"` literal —
        /// and the busy `Spinner` icon (`ChatController::set_spinner_frame`,
        /// previously never called at all — see the issue) must actually
        /// animate. The fixture's `$ACP_FAKE_TOOL_CALL_HANGS` announces one
        /// `in_progress` "execute: Run the tests" call and then never
        /// answers `session/prompt`, so the scenario stays busy
        /// indefinitely — long enough to observe both the tool-call text
        /// and a spinner-frame advance without racing a real completion.
        ///
        /// Distinguishing the new status-strip text from the pre-existing
        /// transcript line: `AcpToolCallStatus::glyph()` prefixes the
        /// transcript's own summary with a bracketed glyph (`"[~] execute:
        /// Run the tests"`, no elapsed reading), while the status strip's
        /// text has no brackets and is suffixed with `"\u{b7} <N>s"` — so
        /// searching for the title immediately followed by that separator
        /// only matches the new text this issue adds.
        ///
        /// RED verified: reverting `render::populate_ai_chat_controller`'s
        /// header back to the static `"(thinking\u{2026})"` literal (this
        /// issue's starting point, with no `chat.set_spinner_frame` call
        /// either) makes this fail on both counts — the screen never shows
        /// "execute: Run the tests \u{b7}" anywhere, and the glyph painted
        /// immediately before "execute: Run the tests" on screen never
        /// changes no matter how many ticks are driven.
        ///
        /// `#[cfg(unix)]`: shells out to the `.sh` fixture via `sh
        /// "$fixture"`, same as every other ACP-agent test in this file
        /// (e.g. `ai_agent_status_line_shows_active_mcp_server_via_shell_app`
        /// above).
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
    // #1509: AI panel adopts wrapping/auto-grow input and `Enter`-sends
    // (`submit_on_enter`) with a Send/Stop segment (quadraui#1136/#1137).
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

        /// #1509 acceptance: `ai_chat_submit_on_enter` defaults to `true`
        /// (Zed parity, quadraui#1137) — typing a message into the focused
        /// AI panel and pressing plain `Enter` (no modifiers) must actually
        /// send it, not insert a newline. Proven by the fake agent's own
        /// echoed "Hello world" reply reaching the screen: that text can
        /// only paint once `session/prompt` was actually dispatched, which
        /// only happens if `Enter` triggered `ChatControllerEvent::Submit`
        /// (`quadraui::ChatController::handle`, driven by
        /// `Settings::ai_chat_submit_on_enter` via
        /// `render::populate_ai_chat_controller`'s
        /// `chat.set_submit_on_enter` call) rather than `ChatController`'s
        /// own newline-insertion path.
        ///
        /// RED verified: with `populate_ai_chat_controller`'s
        /// `chat.set_submit_on_enter(...)` call removed (leaving
        /// `ChatController`'s own hardcoded `submit_on_enter: false`
        /// default in effect, the pre-#1509 behaviour this issue changes),
        /// this fails — plain `Enter` only inserts a newline, the message
        /// is never sent, and "Hello world" never reaches the screen within
        /// the deadline.
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

        /// #1509 acceptance: `ai_chat_submit_on_enter = false` must keep the
        /// pre-#1509 behaviour — plain `Enter` inserts a newline instead of
        /// sending — while the always-available `Ctrl+S` chord still sends.
        /// Proves the setting genuinely gates the behaviour rather than the
        /// default simply being read once at startup: the message must
        /// *not* reach the fake agent (no "Hello world" reply, even after
        /// several driven ticks) until `Ctrl+S` is pressed.
        ///
        /// RED verified: with `chat.set_submit_on_enter` hardcoded to
        /// `true` regardless of the setting (ignoring
        /// `engine.settings.ai_chat_submit_on_enter`), this fails — plain
        /// `Enter` sends immediately and "Hello world" appears well before
        /// `Ctrl+S` is ever pressed, at the first wait loop's very first
        /// iteration.
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

        /// #1509 acceptance (quadraui#1137): clicking the Send/Stop segment
        /// while a turn is streaming (it reads "Stop" then) must abort the
        /// turn via the same `session/cancel` path as `Ctrl+C`
        /// (`Engine::acp_cancel_turn`) — `Engine::dispatch_ai_chat_event`'s
        /// `ChatControllerEvent::StopRequested` arm. The fixture's
        /// `$ACP_FAKE_TOOL_CALL_HANGS` announces one `in_progress` tool
        /// call and then never answers `session/prompt`, so the scenario
        /// stays busy (and the segment keeps reading "Stop") until this
        /// click cancels it.
        ///
        /// Asserts on rendered output only: the transcript's
        /// `"[cancelled by user]"` line (`acp_cancel_turn`'s own message)
        /// must paint, and the busy tool-call status line must be gone —
        /// never an internal `ai_streaming` flag read in isolation.
        ///
        /// RED verified: with `Engine::dispatch_ai_chat_event`'s
        /// `Ev::StopRequested => { self.acp_cancel_turn(); true }` arm
        /// removed (falling through to the catch-all `_ => true`, a no-op),
        /// this fails — the click is consumed but nothing happens: the
        /// screen still shows the busy "execute: Run the tests" status
        /// line and never shows "[cancelled by user]", even after waiting
        /// out the full deadline.
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

    /// #1515: turn-end review, badge instead of auto-opening the
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
                "vimcode_test_1515_tui_badge_{:?}",
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

        /// GTK twin: `gtk::testing::issue_1515_acp_review_badge::
        /// badge_mode_paints_gutter_markers_on_agent_changed_lines_via_gtk_driver`
        /// — see that test's doc comment for the full rationale. Proved in
        /// a plain (non-git) temp directory, so any marker painted can
        /// only have come from the ACP-turn overlay, never `crate::core::
        /// git::compute_file_diff`.
        ///
        /// RED verified: with `render::build_render_window`'s `acp_turn_
        /// status` overlay removed (reverting `has_git`/`git_status` to
        /// read only `buffer_state.git_diff`, the pre-#1515 code), this
        /// buffer never gets a gutter column at all (it isn't a git repo,
        /// and `git_diff` stays empty) — the "▌" assertion below fails.
        #[test]
        fn badge_mode_paints_gutter_markers_on_agent_changed_lines_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1515_tui_gutter_{:?}",
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

        /// Review finding (#1515 fix iteration 1): `off` mode's contract
        /// is "neither the modal nor the badge/gutter nudge — only an
        /// explicit `:AiReview` shows anything"
        /// (`AcpReviewOnTurnEnd::Off`'s own doc). GTK twin:
        /// `gtk::testing::issue_1515_acp_review_badge::
        /// off_mode_paints_no_gutter_markers_via_gtk_driver`.
        ///
        /// RED verified against the pre-fix code (gated on `!= Auto`
        /// instead of `== Badge`): the gutter marker glyph assertion
        /// below failed, since `off` painted the same "▌" marker `badge`
        /// does.
        #[test]
        fn off_mode_paints_no_gutter_markers_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1515_tui_off_gutter_{:?}",
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

    /// #1517: in-buffer review of agent hunks — a virtual `[a] keep  [r]
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

        /// The virtual action row paints right in the normal buffer view
        /// — no modal, no full-viewport "Change 1/" surface — and the
        /// status message left by `Engine::acp_end_turn` invites `:AiReview`
        /// / `ga` as the alternative full-review path.
        ///
        /// RED verified: with the `render.rs` "#1517: in-buffer inline
        /// review" action-row block removed, neither "keep hunk" nor
        /// "reject hunk" appears anywhere on screen.
        #[test]
        fn virtual_action_row_paints_in_the_normal_buffer_view_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1517_tui_row_{:?}",
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
                "vimcode_test_1517_tui_ar_{:?}",
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
                "vimcode_test_1517_tui_ak_{:?}",
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

        /// "Edit before accept" (#1517's own words): a human edit made to
        /// the hunk's range before deciding is what `<leader>ak` keeps —
        /// the action row's label switches to "(edited)" and disk ends up
        /// with the human's text, not the agent's original write.
        ///
        /// RED verified (nit fix, iteration 1): the on-disk assertion
        /// alone can't fail here — `keep` never writes to disk (it's a
        /// pure decision; the human's own `:w` a few lines up already put
        /// "TWO CHANGED BY HUMAN" on disk), so it would pass unchanged
        /// even with `<leader>ak` completely disabled. The "action row
        /// disappears" assertion below closes that gap: with the `"ak"`
        /// leader-sequence arm removed from `keys.rs`, this test's final
        /// `screen().contains("keep hunk")` check fails (the row never
        /// disappears — nothing resolved it).
        #[test]
        fn editing_a_hunk_before_keep_labels_it_edited_and_keeps_the_edit_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1517_tui_edit_{:?}",
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
                "vimcode_test_1517_tui_git_precedence_{:?}",
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

            // Turn 1: agent changes line 2, turn ends, human keeps the
            // hunk (resolved — no longer outstanding — but still
            // uncommitted, so `git diff HEAD` still reports it).
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

    /// #1516: hunk-level Keep/Reject on the ACP turn-review surface — `a`/
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
        /// leaves a genuinely mixed result on disk and the new footer
        /// legend visible — the core new capability #1516's title names.
        ///
        /// RED against unfixed `develop` (pre-#1516 whole-file `a`/`r`):
        /// pressing `a` once would have kept (accepted) the *entire* file
        /// and auto-closed the review immediately — the "still open, hunk
        /// 1 still pending" assertion below would fail, and disk would
        /// show both `AGENT-A` and `AGENT-B` rather than a mix.
        #[test]
        fn keep_one_hunk_reject_the_other_via_shell_app() {
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_1516_tui_hunks_{:?}",
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

    /// #1510: AI panel assistant/thought turns render as markdown
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

        /// #1510 acceptance: the assistant's markdown reply
        /// (`"# Heading\n**bold** text"`) must paint as rendered markdown —
        /// "Heading" and "bold" visible, with no literal `#`/`**` syntax
        /// characters on screen — proving the transcript now goes through
        /// `quadraui::render_markdown_to_styled` rather than
        /// `StyledText::colored` on the raw source.
        ///
        /// RED verified: with `populate_ai_chat_controller`'s markdown path
        /// reverted to `quadraui::StyledText::colored(m.content.clone(),
        /// fg)` (this issue's starting point), this fails — the screen
        /// shows the literal source `"# Heading"` and `"**bold** text"`
        /// verbatim, so the `!screen.contains("# Heading")` and
        /// `!screen.contains("**bold**")` assertions below both fail.
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

        /// #1510 acceptance: a thought turn's markdown content
        /// (`"# Pondering\n**deeply**"`) must never reach the screen at
        /// all — thought turns collapse by default to a fixed one-line
        /// "Thinking..." summary (full expansion is out of scope here).
        ///
        /// RED verified: with the `chat.set_turn_collapsed`/
        /// `set_turn_summary` calls this issue adds removed, this fails —
        /// the thought's own text ("Pondering"/"deeply") paints on screen
        /// instead of the "Thinking..." summary.
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

        /// #1510 regression: routing transcript turns through the markdown
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

        /// Review regression (#1510): `is_genuine_thought_chunk`'s original
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

        /// A wide (220x30, `Alt+Right` x40 — same widening
        /// `issue_1510_ai_panel_markdown_rendering`'s harnesses use, so a
        /// card's title/location text never word-wraps and confuses a
        /// `screen.contains`/ordering check) harness with a fixture agent
        /// configured to reply via `$ACP_FAKE_TOOL_CALL_CARD` — see that
        /// env var's doc in `tests/fixtures/fake_acp_agent.sh` for the exact
        /// sequence (a `tool_call` carrying `rawInput`, a `tool_call_update`
        /// adding `rawOutput`, then a SECOND thought+message pair emitted
        /// afterward).
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

        /// #1511 acceptance: a tool-call card renders collapsed by default
        /// (just the `{glyph} {kind}: {title}` line — no `rawInput`/
        /// `rawOutput`, no locations), and a click on it expands the card to
        /// show both, then a second click collapses it again.
        ///
        /// RED verified: with `populate_ai_chat_controller` reverted to
        /// pre-#1511 (`tool_call_summary_line` painted unconditionally, no
        /// `set_turn_collapsed`/`is_turn_collapsed` for tool calls), this
        /// fails at the very first assertion — `bytesWritten` (the
        /// `rawOutput` value) is on screen immediately, before any click.
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

        /// #1511 acceptance: the tool-call card renders **between** the two
        /// replies it chronologically belongs between, not after the whole
        /// conversation — the issue's core complaint
        /// (`populate_ai_chat_controller` used to append every tool call
        /// after all of `ai_messages`, "at the deliberate cost of it not
        /// being in strict chronological order").
        ///
        /// RED verified: with the interleave reverted to the pre-#1511
        /// "build every message turn, then push every tool call after the
        /// loop" shape, this fails — `"Goodbye"` (the second reply, which
        /// streams in AFTER the tool call over the wire) paints *above* the
        /// card instead of below it, so `pos("Goodbye") < pos(card)` and
        /// the ordering assertion fails.
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

        /// #1511 acceptance: `Tab` moves keyboard focus onto the tool-call
        /// card, and plain `Enter` toggles it open — the "keyboard, not just
        /// mouse" half of "Toggle via click / `Tab`/`Enter` on the focused
        /// card".
        ///
        /// RED verified: with `render::route_ai_chat_event`'s focused-turn
        /// `Enter` intercept removed, `ChatController::handle` still
        /// consumes the key (toggling its own *internal* collapsed map) but
        /// the very next frame's `populate_ai_chat_controller` call
        /// overwrites that from `AcpSession::tool_call_expanded`, which
        /// never got the toggle — so `bytesWritten` never appears and this
        /// fails.
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

        /// #1511 acceptance: clicking inside an *expanded* card's body (as
        /// opposed to its row-0/row-1 header, which toggles it — see
        /// `tool_call_card_collapses_by_default_and_toggles_via_click_via_
        /// shell_app`) jumps to the tool call's location — "→ path:line
        /// location can't be followed" from the issue's Problem section.
        ///
        /// RED verified: with `Engine::ai_chat_turn_clicked`'s `else if`
        /// branch (the `Self::ai_open_tool_call_location` call) deleted,
        /// this fails — clicking `"bytesWritten"` does nothing, no new tab
        /// opens, and the status bar never shows `"Ln 3"`.
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
    // #1514: "follow the agent" — `acp_follow_agent` + `:AiFollow`, and the
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
        /// jumped, not just that some engine field got set (#587/#592).
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
    // #1512: AI panel queues a message typed while the agent is busy.
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

        /// #1512 acceptance, full flow through the real `App`/`TuiDriver`
        /// stack: a message submitted while the agent is busy paints as a
        /// dimmed `"(queued) ..."` transcript turn plus a `"queued (1)"`
        /// status-strip segment (never a silent no-op — the pre-#1512
        /// behaviour); Ctrl+R discards it (relabels it `"(discarded)"`,
        /// drops the status segment); a message queued again and sent with
        /// Ctrl+G ("send now") cancels the busy turn (`"[cancelled by
        /// user]"` painted, same as Ctrl+C) and flips the queued turn to a
        /// plain, undimmed line — all read from the *painted* screen, not
        /// engine state.
        ///
        /// RED verified: reverting `ai_send_message` to the pre-#1512 `if
        /// text.is_empty() || self.acp_mut().ai_streaming { return; }`
        /// early return (dropping the Ctrl+R/Ctrl+G arms along with it)
        /// makes this fail at the very first assertion — "second" never
        /// reaches the input at all as a queued turn; the screen shows no
        /// `"(queued)"` text and no `"queued (1)"` segment anywhere.
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
            // Not the trailing `)`: `ChatController::render` overlays a
            // separate busy `Spinner` icon at a fixed position at the
            // right end of the status strip (quadraui's own doc: "2.
            // Spinner (overlaid at the right end of the status strip)"),
            // painted on top of whatever status text lands under it — at
            // this harness width, that's this segment's own closing
            // paren. A pre-existing widget behaviour, unrelated to #1512;
            // the substring below still only matches once this segment's
            // text is actually present.
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

        /// #1513 acceptance: the plan checklist pins as its own
        /// collapsible block above the transcript instead of appending as
        /// a synthetic trailing turn that streaming text pushes around and
        /// scrolls away (the issue's core complaint). Collapsed (the
        /// default) shows only the in-progress entry plus `n/m` progress
        /// in the status strip; clicking the block's header expands it to
        /// the full checklist.
        ///
        /// RED verified: with `render::ai_plan_multi_section_view`
        /// reverted to always report `collapsed: false` (i.e. #1513's
        /// default-collapsed behaviour undone), the second assertion below
        /// — that the completed entry is *not* painted before the header
        /// click — fails.
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

        /// #1513 acceptance: `@dir/` — typing `@` plus a matching directory
        /// prefix shows a trailing-slash directory candidate in the
        /// completion popup, and accepting it (Enter, same key `ai_
        /// mention_accept_selected`'s existing `@file` behaviour uses)
        /// splices the literal `@subdir/ ` text into the input.
        ///
        /// RED verified: with `ai_mention_completions`' directory-walk
        /// change reverted (only `entry.file_type().is_file()` offered, no
        /// `is_dir` arm), the first `screen.contains("@subdir/")` assertion
        /// fails — no directory candidate ever appears in the popup.
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

        /// #1513 acceptance: `@symbol` — with a `workspace/symbol` result
        /// already cached (`Engine::ai_mention_symbol_cache`, standing in
        /// for a real LSP round trip the same way this file's other
        /// LSP-adjacent tests seed `picker_all_items`/`SymbolInfo`
        /// directly rather than driving a real language server — see
        /// `Engine::ai_mention_tick`'s own doc for why the fetch itself is
        /// unit-tested separately, not through a driver), typing `@` plus
        /// a matching symbol name shows a `@path#Name` candidate in the
        /// popup, and accepting it (a) splices that literal text into the
        /// input and (b) shows the symbol's chip on the always-repainted
        /// status line — proving the accept path actually staged an
        /// `AcpSymbolMention`, not just updated the input text.
        ///
        /// RED verified: with the `chosen.strip_prefix('@')`/
        /// `split_once('#')` staging block removed from `Engine::
        /// ai_mention_accept_selected`, the chip assertion fails (no
        /// `⚑ MyStruct` on screen) even though the input-text assertion
        /// above it still passes.
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

        /// #1522 acceptance: `{type: "terminal"}` tool content, previously
        /// parsed but never rendered (a one-line "(terminal output
        /// omitted)" placeholder), now expands into a live card showing
        /// the command line, its `cwd`, and its exit status/output — and
        /// that card keeps rendering after the agent calls
        /// `terminal/release` (this issue's "persists after release"
        /// acceptance bar), instead of the card going blank or the
        /// terminal's resources leaving no trace at all.
        ///
        /// RED verified: with `AcpToolCallContentBlock::Terminal`'s render
        /// arm in `tool_call_expanded_text` reverted to the pre-#1522
        /// `"(terminal output omitted)"` placeholder, this fails — none of
        /// `cwd:`/`exit code 0`/the captured `acp-terminal-output` text
        /// ever appear on screen, expanded or not.
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
}
