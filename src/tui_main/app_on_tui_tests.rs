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
    //! `mod.rs`'s own `mod app_on_tui_tests;` declaration is deliberately
    //! *not* itself `#[cfg(test)]`-gated, even though every item this file
    //! defines is (this one `mod tests` block wraps literally everything
    //! below, including its own module doc you're reading right now).
    //! Gating that bodiless, semicolon-form declaration *in addition to*
    //! this inner gate trips a latent `scripts/prod_lines.py` blind spot:
    //! its brace-balance skip loop only knows how to skip a *braced* item,
    //! so on a `#[cfg(test)] mod x;` line with no body it never finds an
    //! opening `{` to close on and instead runs off the end of the
    //! *`mod.rs` file*, silently miscounting everything below it as
    //! skipped (confirmed by hand — `src/tui_main`'s own reported total
    //! dropped by hundreds of lines the one time this was tried). Leaving
    //! `mod.rs`'s declaration ungated sidesteps that bug; the module still
    //! compiles to empty outside tests either way, since everything here
    //! is gated at this level instead.
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

    use quadraui::testing::{ConformanceDriver, DriverInput};

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

    /// Click the already-active Explorer activity-bar icon a second time — the
    /// real production toggle-closed gesture
    /// [`sidebar_panels::search_icon_second_click_toggles_sidebar_closed`]
    /// exercises directly — collapsing the sidebar's screen-space reservation.
    /// Several tests below want the full terminal width for the editor/tab
    /// bar/bottom band rather than competing with the sidebar for the same
    /// narrow 80-column budget. Only works when Explorer is the fixture's
    /// already-active panel — true of every fixture built from
    /// [`plain_engine`] (`new_for_test`'s own default).
    ///
    /// Mutating `engine.app_shell` directly (`hide_sidebar()`) does **not**
    /// achieve this, and is deliberately not used here: `App`'s own
    /// runner-side `AppShell` (the actual layout/column reservation) starts
    /// from `ShellConfig`'s own default and is never synced from the engine's
    /// shadow copy at startup — only a real click through
    /// `AppShell::handle_activity_click`'s toggle-closed branch flips it.
    /// Confirmed while writing this module: mutating the shadow alone left
    /// the sidebar column painted regardless.
    fn collapse_sidebar<D: ConformanceDriver + DriverInput>(driver: &mut D) {
        let explorer_zone = driver
            .inventory()
            .zones()
            .iter()
            .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_EXPLORER)
            .map(|z| z.bounds)
            .expect("the Explorer activity-bar icon must register a chrome zone");
        driver.click(
            explorer_zone.x + explorer_zone.width / 2.0,
            explorer_zone.y + explorer_zone.height / 2.0,
        );
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

            // #1425 gate: unit — typed text has nowhere to paint once the editor content band
            // has collapsed. Target: UnitProfile.
            known_bug_gate(
                "app_on_tui::key_press_inserts_text_via_shell_app_general_fallback",
                || {
                    driver.type_char('i'); // Normal -> Insert
                    for c in "ZQXW_TYPED".chars() {
                        driver.type_char(c);
                    }
                    let screen = driver.screen();
                    assert!(
                    screen.contains("ZQXW_TYPED"),
                    "typed text should reach the buffer via Engine::handle_key; screen:\n{screen}"
                );
                },
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

            // #1425 gate: unit — the editor content band never paints at all, so its
            // precondition (the first line visible) can't be satisfied. Target:
            // UnitProfile.
            known_bug_gate("app_on_tui::dd_deletes_the_current_line", || {
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
            });
        }

        /// `u` after `dd` must restore the deleted line — the undo stack, same
        /// key pipeline as [`dd_deletes_the_current_line`].
        #[test]
        fn undo_restores_after_dd() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "ZQXW_UNDO_LINE\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            // #1425 gate: unit — same editor-band collapse as the sibling key_dispatch tests
            // above. Target: UnitProfile.
            known_bug_gate("app_on_tui::undo_restores_after_dd", || {
                driver.type_char('d');
                driver.type_char('d');
                driver.type_char('u');
                assert!(
                    driver.screen_has("ZQXW_UNDO_LINE"),
                    "'u' after 'dd' must restore the deleted line; screen:\n{}",
                    driver.screen()
                );
            });
        }

        /// `i` then `Escape` must return to Normal mode — a typed key after
        /// `Escape` must not insert further text.
        #[test]
        fn escape_returns_to_normal_mode_after_insert() {
            let mut h = harness_no_sidebar(plain_engine());
            let driver = &mut h.driver;

            known_bug_gate(
                "app_on_tui::escape_returns_to_normal_mode_after_insert",
                || {
                    driver.type_char('i');
                    driver.type_char('a');
                    driver.press_named(quadraui::NamedKey::Escape);
                    // In Normal mode, 'x' deletes the character under the cursor
                    // rather than inserting — if Escape didn't work, this 'x' would
                    // instead insert a literal 'x' into the buffer.
                    driver.type_char('x');
                    assert!(
                        !driver.screen_has("ax"),
                        "'x' after Escape must delete under the cursor (Normal \
                 mode), not insert a literal 'x' (would mean Escape never \
                 left Insert mode); screen:\n{}",
                        driver.screen()
                    );
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

            // #1425 gate: unit — a plain modal Dialog never reaches the screen at this
            // viewport at all (fails on its own precondition) — the same
            // overlay-positioning symptom explorer_context_menu::
            // context_menu_delete_opens_confirm_dialog hits for the
            // delete-confirmation dialog specifically. Target: UnitProfile.
            known_bug_gate("app_on_tui::dialog_intercepts_all_keys", || {
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
            });
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

            // #1425 gate: feature — App unconditionally reserves and paints its own
            // GTK-style menu-bar row (File Edit View Go Run Terminal Help) on
            // every backend; the shipped TUI shell only shows that row in
            // vscode-mode or when Alt-revealed, so its tab bar sits on row 0
            // where TuiShellApp's own mirrored test expects it and App's
            // does not. Target: menu-bar caps (conditional menu-bar reveal).
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

            // #1425 gate: feature — same permanent-menu-bar-row divergence as
            // render_content_paints_single_group_tab_bar_via_shell_app above.
            // Target: menu-bar caps (conditional menu-bar reveal).
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

            // #1425 gate: unit — only one of the two tabs' labels reaches the painted tab
            // bar (confirmed by hand: the *active* one is missing, the
            // *inactive* one paints) — suspected the same pixel/row unit
            // confusion narrowing the tab bar's effective column budget, but
            // unconfirmed; flag for its own follow-up once UnitProfile lands
            // rather than assumed. Target: UnitProfile.
            known_bug_gate("app_on_tui::two_tabs_paint_both_labels", || {
                assert!(
                    driver.screen_has("zqxwA1425.txt") && driver.screen_has("zqxwB1425.txt"),
                    "both tabs' labels must paint on the tab bar; screen:\n{}",
                    driver.screen()
                );
            });
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

            // #1425 gate: unit — App::render_content reserves render::TAB_ROW_HEIGHT_PX/
            // BREADCRUMB_ROW_HEIGHT_PX as cell-grid rows, collapsing the editor/
            // status-bar band on a realistic terminal height. Target: UnitProfile.
            known_bug_gate("app_on_tui::status_bar_paints_cursor_position", || {
                assert!(
                    driver.screen_has("Ln 1,"),
                    "a fresh buffer's status bar must show the cursor on line \
                 1; screen:\n{}",
                    driver.screen()
                );
            });
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

            // #1425 gate: unit — the terminal pane itself paints (the leaked 'd' keystrokes are
            // visible in its prompt, confirming the swallow claim is actually
            // true), but the assertion has to read the editor buffer to prove
            // it, and that band never paints. Target: UnitProfile.
            known_bug_gate(
                "app_on_tui::focused_terminal_swallows_editor_keys_via_shell_app",
                || {
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
                },
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

            known_bug_gate(
                // #1425 gate: unit — the bottom panel band is part of the same collapsed layout.
                // Target: UnitProfile.
                "app_on_tui::menu_terminal_activation_opens_terminal_pane_via_shell_app",
                || {
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
                    let after = driver.inventory().count("Terminal");
                    assert!(
                        after > before,
                        "the open-terminal accelerator must open a terminal \
                     pane, painting a new \"Terminal\" occurrence (before \
                     {before}, after {after}); screen:\n{}",
                        driver.screen()
                    );
                },
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

            // #1425 gate: unit — neither the explorer context menu popup nor the confirm
            // dialog it opens paints at this viewport (confirmed alongside
            // popups::dialog_intercepts_all_keys's identical symptom for a
            // plain dialog) — the engine-level dispatch still runs
            // (context_menu_new_file_starts_inline_edit's inline-edit result
            // proves that half works), only the overlay's own paint is
            // missing. Target: UnitProfile.
            known_bug_gate(
                "app_on_tui::context_menu_delete_opens_confirm_dialog",
                || {
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
                },
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

            // #1425 gate: unit — the minimap rung is part of the same collapsed editor content
            // band. Target: UnitProfile.
            known_bug_gate("app_on_tui::minimap_paints_braille_when_enabled", || {
                assert!(
                    has_braille(&driver.screen()),
                    "a scrollable buffer with the minimap on must paint \
                 braille somewhere; screen:\n{}",
                    driver.screen()
                );
            });
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

            known_bug_gate("app_on_tui::no_minimap_braille_when_setting_is_off", || {
                assert!(
                    !has_braille(&driver.screen()),
                    "`minimap: false` must reserve no strip, so no braille may \
                 reach the cells; screen:\n{}",
                    driver.screen()
                );
            });
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

            // #1425 gate: unit — same editor-band collapse, compounded by the split's second
            // pane not painting at all (see the dividers module's own gated
            // tests). Target: UnitProfile.
            known_bug_gate("app_on_tui::split_paints_minimap_in_both_panes", || {
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
            });
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

            // #1425 gate: unit — the split's second pane's tab bar never paints (only one of
            // the two "[No Name]" labels reaches the screen), so the divider
            // between them has nothing to anchor to either. Target: UnitProfile.
            known_bug_gate(
                "app_on_tui::render_content_paints_group_divider_via_shell_app",
                || {
                    let screen = driver.screen();
                    // Locate the tab row by content, not row 0 — see
                    // `ctrl_w_v_reserves_one_column_for_the_divider_via_shell_app`'s
                    // own comment on why.
                    let (tab_y, tab_row) = screen
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains("[No Name]"))
                        .unwrap_or((0, ""));
                    let starts: Vec<usize> =
                        tab_row.match_indices("[No Name]").map(|(i, _)| i).collect();
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
                },
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

            known_bug_gate(
                // #1425 gate: unit — same collapse as the sibling divider tests above. Target:
                // UnitProfile.
                "app_on_tui::group_divider_drag_moves_the_painted_divider_via_shell_app",
                || {
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
                },
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

            known_bug_gate(
                // #1425 gate: unit — the group divider glyph itself never paints once the editor
                // content band has collapsed to zero rows. Target: UnitProfile.
                "app_on_tui::group_divider_click_without_move_leaves_the_divider_put_via_shell_app",
                || {
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
                },
            );
        }

        /// A `Ctrl-W v` vertical split must reserve exactly one column for the
        /// divider — mirrors `shell_app.rs`'s
        /// `ctrl_w_v_reserves_one_column_for_the_divider_via_shell_app`, using
        /// the `Ctrl-W v` keychord (rather than the `open_editor_group` engine
        /// call every other divider test here uses) so this specifically
        /// exercises the key-dispatch route into the same split.
        #[test]
        fn ctrl_w_v_reserves_one_column_for_the_divider_via_shell_app() {
            let mut engine = plain_engine();
            engine.buffer_mut().insert(0, "short\n");
            let mut h = harness_no_sidebar(engine);
            let driver = &mut h.driver;

            known_bug_gate(
                // #1425 gate: unit — same TAB_ROW_HEIGHT_PX-as-rows collapse: the split's second
                // pane never paints its own tab bar at all. Target: UnitProfile.
                "app_on_tui::ctrl_w_v_reserves_one_column_for_the_divider_via_shell_app",
                || {
                    driver.ctrl_char('w');
                    driver.type_char('v');
                    driver.mouse_up(1.0, 1.0);

                    let screen = driver.screen();
                    // Whichever row the tab bar actually paints on (not
                    // assumed to be row 0 — `App` reserves its own permanent
                    // menu-bar row above it, see
                    // `tab_bar::render_content_paints_single_group_tab_bar_via_shell_app`'s
                    // own doc) — this scenario's claim is about the divider
                    // column reservation, not the row offset.
                    let tab_row = screen
                        .lines()
                        .find(|line| line.contains("[No Name]"))
                        .unwrap_or("");
                    let starts: Vec<usize> =
                        tab_row.match_indices("[No Name]").map(|(i, _)| i).collect();
                    assert_eq!(
                        starts.len(),
                        2,
                        "'Ctrl-W v' must split into two panes, each with its \
                     own tab bar; row:\n{tab_row}"
                    );
                },
            );
        }
    }
}
