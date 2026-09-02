// Reachability slice for **issue #657** — "Put vimcode on the oracle loop".
//
// This is NOT a real Gate-A acceptance slice. It is the throwaway `ms-example`
// the issue asks for: it exists only to prove that the #657 Stage-1 lib
// promotion + Stage-2 `test-support` feature genuinely make **both** backends'
// black-box harnesses reachable from an *external* integration-test crate.
// Real slices are JIT-authored per issue by the `test-author` agent from a
// Gate-A contract, under `tests/acceptance/ms-NN/`.
//
// Every assertion below is on **rendered output** — the TUI character grid and
// the GTK backend's recorded paint — never on a state field being populated.
// That rule is in CLAUDE.md for a reason: `ScreenLayout.picker` was populated
// on GTK for months while nothing painted it (#587), and a test asserting the
// field is `Some` passes against that bug.
//
// This file is `include!`d at crate root by `tests/acceptance.rs`.

mod seam_657 {
    use vimcode_core::quadraui::tui::testing::driver_with_shell;
    use vimcode_core::tui_main::testing::TuiShellApp;
    use vimcode_core::Engine;

    /// A marker string that cannot occur in a restored session, a settings
    /// file, or any chrome vimcode paints — so finding it on screen can only
    /// mean the frame rendered the buffer we seeded.
    const MARKER: &str = "SEAM657MARKER";

    // ───────────────────────── TUI half ──────────────────────────────────

    /// The TUI backend is driveable from this crate.
    ///
    /// Before #657 this could not compile at all: `tui_main` lived inside the
    /// `vimcode` binary, so `vimcode_core::tui_main` did not exist.
    ///
    /// The assertion is that a full frame of the requested size came back out
    /// of the *real* `ShellApp::render_content` path — `driver_with_shell`
    /// routes through quadraui's shell runner exactly as `tui_main::run` does
    /// — and that it is not blank.
    #[test]
    fn tui_backend_paints_a_full_frame_from_an_integration_test() {
        let mut driver = driver_with_shell(
            TuiShellApp::new(None),
            TuiShellApp::shell_config(false),
            80,
            24,
        );
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

    /// Buffer text seeded through the public `TuiShellApp::engine` seam shows
    /// up on the painted character grid.
    ///
    /// `TuiShellApp::new` runs the real `Engine::startup`, which reads the
    /// developer's `~/.config/vimcode` — so sidebar visibility and scroll
    /// offsets are *ambient*, not fixed (the #634 trap: five tests silently
    /// inherited "sidebar open" from a dev box and failed on CI). Scroll is
    /// pinned to the top here rather than inherited, so the marker is on the
    /// first painted row on any machine.
    #[test]
    fn tui_backend_paints_seeded_buffer_text() {
        let mut app = TuiShellApp::new(None);
        app.engine.buffer_mut().insert(0, &format!("{MARKER}\n"));
        for window in app.engine.windows.values_mut() {
            window.view.scroll_top = 0;
        }

        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 120, 24);
        driver.render();

        assert!(
            driver.screen_contains(MARKER),
            "seeded buffer text never reached the character grid:\n{}",
            driver.screen()
        );
    }

    // ───────────────────────── GTK half ──────────────────────────────────

    /// The GTK backend and the #646 headless `GtkDriver` harness are driveable
    /// from this crate.
    ///
    /// Before #657 this was the harder half: `gtk` lived in the `vimcode`
    /// binary *and* `gtk::testing::{Harness, harness}` were both `pub(super)`,
    /// so even a same-crate sibling module could not reach them.
    ///
    /// `painted_texts` is the GTK backend's record of what `render_content`
    /// actually drew, so a non-empty result means a real paint pass ran — this
    /// is deliberately not an assertion that some state field is `Some`.
    #[test]
    fn gtk_backend_paints_from_an_integration_test() {
        let mut h = vimcode_core::gtk::testing::harness(Engine::new(), 1400, 900);
        h.driver.render();

        assert!(
            !h.driver.painted_texts().is_empty(),
            "GTK render_content recorded no painted text at all"
        );
    }

    /// The GTK half of the seeded-text assertion — the cross-backend twin of
    /// `tui_backend_paints_seeded_buffer_text`.
    ///
    /// `harness()` takes the `Engine` from the caller precisely so a test
    /// states its own preconditions instead of depending on the developer's
    /// real session (no `Engine::startup` runs here), which is why no scroll
    /// pinning is needed on this side.
    ///
    /// The GTK backend does not `record_painted_text` for *editor* text, so
    /// `find`/`screen_contains` cannot see buffer content — `Harness
    /// ::window_center` is the documented substitute: it reports the pixel
    /// rect the last frame actually painted the editor pane into. A `Some`
    /// here means the renderer laid the window out and reported it, which is
    /// the finest-grained rendered-output signal this harness exposes for a
    /// pane.
    #[test]
    fn gtk_backend_reports_a_painted_editor_pane_rect() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, &format!("{MARKER}\n"));
        let focused = engine.active_window_id();

        let mut h = vimcode_core::gtk::testing::harness(engine, 1400, 900);
        h.driver.render();

        let center = h.window_center(focused);
        assert!(
            center.is_some(),
            "no painted rect for the focused editor window — render_content \
             produced no ScreenLayout entry for it"
        );
        let (x, y) = center.unwrap();
        assert!(
            x > 0.0 && y > 0.0 && x < 1400.0 && y < 900.0,
            "painted editor pane centre {center:?} is outside the 1400x900 surface"
        );
    }
}
