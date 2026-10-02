//! Driver-tier black-box tests for the bundled `contrib/extensions/rest-client`
//! extension (#147).
//!
//! # Why this loads the real, shipped Lua
//!
//! Every other fixture under `src/harness/` writes synthetic Lua inline for
//! the test ([`crate::harness::plugin_view`]'s own module doc explains why:
//! a real [`crate::core::plugin::PluginManager`] load keeps the parse/paint/
//! dispatch wiring in the measured path). This module goes one step
//! further and loads the actual file the extension ships —
//! `contrib/extensions/rest-client/rest_client.lua` — via
//! [`crate::core::plugin::PluginManager::load_plugins_dir`] pointed straight
//! at the on-disk directory, exactly as #147's acceptance criterion asks
//! ("load `contrib/extensions/rest-client` from disk") and exactly the path
//! `Engine::plugin_init` takes for a real installed extension (one `.lua`
//! file per top-level entry in the extension's own directory, no `init.lua`
//! wrapper — see that function's own doc). A regression in the shipped
//! extension is caught here the way it would hit a user, not via a
//! hand-rolled parallel fixture that could quietly drift from what ships.
//!
//! # Zero extension-specific Rust (epic #1403 / GOALS.md's Platform-
//! Neutrality Rule)
//!
//! `rest_client.lua` is pure `vimcode.*` Lua — `vimcode.ui.register_view`,
//! `vimcode.http.request`, `vimcode.json.{encode,decode}`,
//! `vimcode.storage.{get,set}`, `vimcode.command`. Nothing in this module
//! (or anywhere else in this PR) adds extension-specific code to
//! `src/core/`; every API the extension calls already shipped (#146, #1627,
//! #1631, #1632).
//!
//! # Six views, one hub
//!
//! `rest_client.lua`'s own module doc explains the view split in detail;
//! in short: the Request view (`rest_client_request`) is a field-stack hub
//! carrying URL/method/body/status plus navigation buttons, and
//! Headers/Params (`Table`-kind), Response (`TextView`-kind), History
//! (`List`-kind), Collections (`Tree`-kind) and Environment (another field
//! stack) are each their own registered view/tab — `PluginView`'s own doc
//! (`src/core/plugin_ui.rs`) is explicit that a `Table`/`TextView` body
//! cannot share a view with field-stack rows.
//!
//! # Driving navigation via `Engine::plugin_run_command`, not typed `:`
//!
//! Every scenario below opens a view's tab via [`open_tab`], which calls
//! [`Engine::plugin_run_command`] directly on the harness's exposed
//! `Rc<RefCell<Engine>>` rather than typing `:RestClient<Enter>` through the
//! driver. That is deliberate, not a shortcut around #1627's in-panel text
//! entry: several scenarios are still focused on a `text`/`text_area` field
//! (URL, the Environment buffer) at the moment they need to switch views,
//! and a literal `:` keystroke there would be inserted into the focused
//! field's value instead of opening the command line — exactly as it
//! should, since #1627 gives a focused field first claim on every printable
//! key. `plugin_run_command` is the identical dispatch a real
//! `:RestClient<Enter>` reaches once the ex-command parser has resolved the
//! name (`PluginManager::call_command`), so the command registration and
//! the view-opening wiring are still exercised end to end — only the literal
//! keystrokes of the ex-command parser itself are skipped, and that parser
//! is not part of what #147 adds. `ConformanceHarness::engine` is exposed
//! for precisely this kind of "reach past painted text" need (its own doc
//! comment says so).
//!
//! Every *behaviour* under test — typing into fields, clicking buttons,
//! editing table cells, the async HTTP round trip, cross-reload persistence
//! — is still driven through real [`quadraui::testing::ConformanceDriver`]
//! calls and asserted only on painted output (plus, for "did the server
//! receive this header", on what a real loopback `TcpListener` captured —
//! painted output cannot show that at all, so it is the only correctness
//! signal available for that one assertion; every other assertion in this
//! module reads [`ConformanceDriver::screen_has`]).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use quadraui::testing::ConformanceDriver;
use quadraui::NamedKey;

use crate::core::Engine;

/// Absolute path to the real, shipped extension this module drives —
/// `env!("CARGO_MANIFEST_DIR")` rather than a relative path so the test
/// doesn't depend on the test binary's current directory.
fn extension_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("contrib/extensions/rest-client")
}

/// Every view the extension registers — used both to sanity-check the load
/// and to build the panel-registration harvest `load_extension` mirrors
/// from `Engine::plugin_init`.
const VIEWS: &[&str] = &[
    "rest_client_request",
    "rest_client_headers",
    "rest_client_params",
    "rest_client_response",
    "rest_client_history",
    "rest_client_collections",
    "rest_client_env",
];

/// Load the real on-disk extension into `engine`, mirroring the harvest
/// `Engine::plugin_init` performs for a real installed extension (panel
/// registrations into `ext_panels`, then the manager itself). Shared by
/// every fixture builder in this module, same rationale as
/// `crate::harness::plugin_view::load_fixture_plugin`.
fn load_extension(engine: &mut Engine) {
    engine.settings.use_nerd_fonts = Some(false);
    crate::icons::set_nerd_fonts(false);
    engine.settings.plugins_enabled = true;
    // A machine with real plugins installed would otherwise contribute its
    // own registrations here (see `crate::harness::plugin_panel`'s same
    // guard).
    engine.ext_panels.clear();
    engine.plugin_views.clear();

    let mut pm = crate::core::plugin::PluginManager::new().expect("lua state");
    pm.load_plugins_dir(&extension_dir(), &[]);
    assert!(
        pm.plugins.iter().all(|p| p.error.is_none()),
        "contrib/extensions/rest-client must load cleanly; errors: {:?}",
        pm.plugins
            .iter()
            .filter_map(|p| p.error.clone())
            .collect::<Vec<_>>()
    );
    for (name, panel) in &pm.panels {
        engine
            .ext_panel_sections_expanded
            .insert(name.clone(), vec![true; panel.sections.len()]);
        engine.ext_panels.insert(name.clone(), panel.clone());
    }
    engine.set_plugin_manager(pm);
    for view in VIEWS {
        assert!(
            engine.is_plugin_view(view),
            "contrib/extensions/rest-client must register {view:?}"
        );
    }
}

/// Build a fresh engine with the extension loaded and the sidebar parked on
/// a non-explorer panel (mirrors `crate::harness::plugin_view`'s fixtures —
/// see that module's own doc for why: the Explorer panel's `TreeController`
/// would otherwise intercept clicks via its stale cached rect).
pub fn engine_with_extension() -> Engine {
    let mut engine = Engine::new_for_test();
    load_extension(&mut engine);
    engine.app_shell.show_panel(&quadraui::WidgetId::new(
        crate::core::engine::sidebar::PANEL_SETTINGS,
    ));
    engine
}

// ── Cross-backend idle/repaint seam ─────────────────────────────────────
//
// `TuiDriver` and `GtkDriver` have no shared *trait* method for "advance
// one tick of async/idle work and repaint": `TuiDriver::tick()` is a
// concrete method with no `GtkDriver` equivalent at all —
// `src/gtk/testing.rs`'s own `busy_status_shows_running_tool_call_and_
// elapsed_time_via_gtk_driver` test documents exactly this gap ("`GtkDriver`
// has no `tick()`") and instead drives `Engine::poll_idle()` directly
// followed by an explicit `render()`. A local trait implemented for both
// (Rust's orphan rule allows implementing a local trait for a foreign type)
// lets every scenario body below stay genuinely backend-neutral rather than
// being duplicated once per backend for this one seam.

pub trait DrivesIdle {
    /// Advance one tick of whatever makes async work progress, and repaint.
    fn drive_idle_once(&mut self, engine: &RefCell<Engine>);
    /// Force an immediate repaint with no dispatch/poll — used after a
    /// direct [`Engine::plugin_run_command`] call, which (like any other
    /// engine mutation made without going through `dispatch`) does not
    /// repaint on its own.
    fn repaint(&mut self);
}

impl<A: quadraui::AppLogic> DrivesIdle for quadraui::tui::testing::TuiDriver<A> {
    fn drive_idle_once(&mut self, _engine: &RefCell<Engine>) {
        self.tick();
    }
    fn repaint(&mut self) {
        self.render();
    }
}

#[cfg(feature = "gui")]
impl<A: quadraui::AppLogic> DrivesIdle for quadraui::gtk::testing::GtkDriver<A> {
    fn drive_idle_once(&mut self, engine: &RefCell<Engine>) {
        engine.borrow_mut().poll_idle();
        self.render();
    }
    fn repaint(&mut self) {
        self.render();
    }
}

/// Open `command`'s view as a new editor-area tab. See this module's own
/// doc ("Driving navigation via `Engine::plugin_run_command`") for why this
/// calls the engine directly rather than typing `:command<Enter>`.
fn open_tab<D: ConformanceDriver + DrivesIdle>(
    driver: &mut D,
    engine: &Rc<RefCell<Engine>>,
    command: &str,
) {
    let opened = engine.borrow_mut().plugin_run_command(command, "");
    assert!(
        opened,
        "{command:?} must be a command the rest-client extension registers"
    );
    driver.repaint();
}

/// Poll (via [`DrivesIdle::drive_idle_once`]) until `needle` is painted or
/// `timeout` elapses.
fn wait_for<D: ConformanceDriver + DrivesIdle>(
    driver: &mut D,
    engine: &Rc<RefCell<Engine>>,
    needle: &str,
    timeout: Duration,
) {
    let deadline = std::time::Instant::now() + timeout;
    while !driver.screen_has(needle) && std::time::Instant::now() < deadline {
        driver.drive_idle_once(engine);
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// How long a scenario waits for the fixture server's response to land —
/// generous because the request goes through a real `curl` child process
/// (`Engine::spawn_http_request`), not a mock.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// The status line's method/elapsed/size separator
/// (`string.format("%d · %dms · %dB", ...)` in `rest_client.lua`) — waiting
/// for it (rather than for a literal status code) proves the callback
/// actually fired, independent of which scenario's status text follows it.
const STATUS_SEPARATOR: &str = "\u{b7}";

fn painted<D: ConformanceDriver>(driver: &D) -> Vec<String> {
    driver
        .inventory()
        .text_runs()
        .iter()
        .map(|r| r.text.clone())
        .collect()
}

/// `Tab` to the Request view's "Add Header" button and activate it with
/// `Enter`, from the `url` field (the view's `first_focusable` row, where
/// every scenario below starts). Keyboard activation rather than
/// `ConformanceDriver::click_text("Add Header")` deliberately: a quadraui
/// `Form` row paints a `Button`-kind field's declared `label` **twice** —
/// once as the row's own label (left column) and once as the clickable
/// widget's caption (right column, bracketed: `"< Add Header >"`) — so
/// "Add Header" is not a unique needle on GTK's two-column layout (TUI's
/// single-column layout happens not to show the ambiguity, which is exactly
/// why this was missed until the GTK arm of this test failed first:
/// `click_text` resolves to whichever occurrence `find_bounds` returns
/// first, and that one is not reliably the real widget). Tab-to-index
/// sidesteps the ambiguity entirely, on both backends, the same way
/// `crate::harness::plugin_view::keyboard_activation_reaches_the_plugin_
/// handler` already does for its own single button.
///
/// Field order (`rest_client.lua`'s `rest_client_request` view, 0-based):
/// `0 hdr(label) 1 url 2 method 3 body 4 add_header 5 add_param 6 send
/// 7 save 8 status(read_only) 9.. nav buttons`. `first_focusable` skips the
/// two non-interactive rows (0, 8), so three `Tab`s from `url` reaches
/// `add_header`, five reaches `send`.
fn activate_add_header<D: ConformanceDriver>(driver: &mut D) {
    for _ in 0..3 {
        driver.press_named(NamedKey::Tab);
    }
    driver.press_named(NamedKey::Enter);
}

/// See [`activate_add_header`] — same reasoning, five `Tab`s from `url` to
/// `send`.
fn activate_send<D: ConformanceDriver>(driver: &mut D) {
    for _ in 0..5 {
        driver.press_named(NamedKey::Tab);
    }
    driver.press_named(NamedKey::Enter);
}

// ── Shared scenario bodies ──────────────────────────────────────────────

/// #147 acceptance 1 — "fill the URL, send, and assert the painted status
/// line and the pretty-printed JSON body."
///
/// The Request view's `url` field is already focused when its tab opens
/// (`first_focusable` skips the non-interactive header label), and every
/// keystroke reaches the plugin as `TextChanged` (#1627), so no explicit
/// commit is needed before clicking Send.
pub fn fill_url_send_and_paint_response<D: ConformanceDriver + DrivesIdle>(
    driver: &mut D,
    engine: &Rc<RefCell<Engine>>,
    get_url: &str,
    response_needle: &str,
) {
    open_tab(driver, engine, "RestClient");
    assert!(
        driver.screen_has("REST Client") && driver.screen_has("idle"),
        "the Request view's initial paint must show its header and idle \
         status; painted runs were {:?}",
        painted(driver)
    );
    driver.type_text(get_url);
    activate_send(driver);
    wait_for(driver, engine, STATUS_SEPARATOR, HTTP_TIMEOUT);
    assert!(
        driver.screen_has("200"),
        "the status line must show the fixture server's 200 response \
         within {HTTP_TIMEOUT:?}; painted runs were {:?}",
        painted(driver)
    );
    open_tab(driver, engine, "RestClientResponse");
    assert!(
        driver.screen_has(response_needle),
        "the Response view must paint the pretty-printed JSON body; \
         painted runs were {:?}",
        painted(driver)
    );
}

/// #147 acceptance 2 — "add a header row and assert the server received
/// it." Only the *painted* half (typing the key/value into the Headers
/// table and confirming they're committed) lives here; the caller checks
/// what the fixture server actually saw, since that is not something
/// painted output can show at all.
pub fn add_header_row_then_send<D: ConformanceDriver + DrivesIdle>(
    driver: &mut D,
    engine: &Rc<RefCell<Engine>>,
    get_url: &str,
    header_name: &str,
    header_value: &str,
) {
    open_tab(driver, engine, "RestClient");
    driver.type_text(get_url);
    activate_add_header(driver);
    open_tab(driver, engine, "RestClientHeaders");
    assert!(
        driver.screen_has("Key") && driver.screen_has("Value"),
        "the Headers view must paint its table columns; painted runs \
         were {:?}",
        painted(driver)
    );
    // Row 0 / column 0 ("Key"): Enter starts editing (cursor at the end of
    // the empty value), type the header name, Enter commits.
    driver.press_named(NamedKey::Enter);
    driver.type_text(header_name);
    driver.press_named(NamedKey::Enter);
    // Right moves the column selection to the second editable column
    // ("Value" — `handle_plugin_view_table_key`'s review fix, #1631).
    driver.press_named(NamedKey::Right);
    driver.press_named(NamedKey::Enter);
    driver.type_text(header_value);
    driver.press_named(NamedKey::Enter);
    assert!(
        driver.screen_has(header_name) && driver.screen_has(header_value),
        "the edited header row must paint its committed key and value; \
         painted runs were {:?}",
        painted(driver)
    );
    open_tab(driver, engine, "RestClient");
    activate_send(driver);
    wait_for(driver, engine, STATUS_SEPARATOR, HTTP_TIMEOUT);
    assert!(
        driver.screen_has("200"),
        "the status line must show the fixture server's 200 response \
         within {HTTP_TIMEOUT:?}; painted runs were {:?}",
        painted(driver)
    );
}

/// #147 acceptance 3, first half — send one request, then confirm it shows
/// up in the History view.
pub fn send_then_open_history<D: ConformanceDriver + DrivesIdle>(
    driver: &mut D,
    engine: &Rc<RefCell<Engine>>,
    get_url: &str,
) {
    open_tab(driver, engine, "RestClient");
    driver.type_text(get_url);
    activate_send(driver);
    wait_for(driver, engine, STATUS_SEPARATOR, HTTP_TIMEOUT);
    open_tab(driver, engine, "RestClientHistory");
    assert!(
        driver.screen_has("GET") && driver.screen_has(get_url),
        "the History view must list the just-sent request; painted runs \
         were {:?}",
        painted(driver)
    );
}

/// #147 acceptance 3, second half — "survives a plugin reload": on a
/// **freshly loaded** extension (a new `PluginManager`/`Lua` VM, built by
/// the caller against the same `VIMCODE_TEST_DATA_HOME`), the History view
/// must still show the entry the previous load persisted, with no request
/// sent on this engine at all. Proves `vimcode.storage` round-trips through
/// a real reload, not just "stayed in a Lua local that never got dropped."
pub fn open_history_without_sending<D: ConformanceDriver + DrivesIdle>(
    driver: &mut D,
    engine: &Rc<RefCell<Engine>>,
    get_url: &str,
) {
    open_tab(driver, engine, "RestClientHistory");
    assert!(
        driver.screen_has("GET") && driver.screen_has(get_url),
        "a freshly reloaded extension's History view must still show the \
         entry persisted by the previous load; painted runs were {:?}",
        painted(driver)
    );
}

/// #147 acceptance 4 — "`{{base}}` substitution": a variable typed into the
/// Environment view resolves inside the Request view's URL before the
/// request is sent. Asserted indirectly but unambiguously: `{{base}}` is
/// not a resolvable host by itself, so the request only reaches (and gets
/// a `200` from) the fixture server if the substitution actually ran.
pub fn env_substitution_then_send<D: ConformanceDriver + DrivesIdle>(
    driver: &mut D,
    engine: &Rc<RefCell<Engine>>,
    base_url: &str,
    path: &str,
) {
    open_tab(driver, engine, "RestClientEnv");
    driver.type_text(&format!("base={base_url}"));
    open_tab(driver, engine, "RestClient");
    driver.type_text(&format!("{}{path}", "{{base}}"));
    activate_send(driver);
    wait_for(driver, engine, STATUS_SEPARATOR, HTTP_TIMEOUT);
    assert!(
        driver.screen_has("200") && !driver.screen_has("ERROR"),
        "the {{{{base}}}} placeholder must resolve to the fixture \
         server's real URL and the request must succeed; painted runs \
         were {:?}",
        painted(driver)
    );
}

/// Not one of #147's four acceptance bullets, but cheap insurance for the
/// three views none of them happen to open: a Lua error inside a `render()`
/// callback is swallowed silently (`Engine::refresh_plugin_view_inner`'s
/// `Err` arm leaves the view's last-good content in place rather than
/// panicking), so a typo in the Params/Collections/Env views' `render()`
/// would otherwise ship invisibly — the tab would just paint blank, and
/// nothing above would ever open it to notice. Opens each and asserts its
/// own declared title paints.
pub fn other_views_render_without_error<D: ConformanceDriver + DrivesIdle>(
    driver: &mut D,
    engine: &Rc<RefCell<Engine>>,
) {
    open_tab(driver, engine, "RestClientParams");
    assert!(
        driver.screen_has("Key") && driver.screen_has("Value"),
        "the Params view must paint its table columns; painted runs \
         were {:?}",
        painted(driver)
    );
    open_tab(driver, engine, "RestClientCollections");
    assert!(
        driver.screen_has("Saved Requests"),
        "the Collections view must paint its root tree node; painted runs \
         were {:?}",
        painted(driver)
    );
    open_tab(driver, engine, "RestClientEnv");
    assert!(
        driver.screen_has("Environment Variables"),
        "the Environment view must paint its header; painted runs \
         were {:?}",
        painted(driver)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// Pixel/cell viewports — same sizes `crate::harness::plugin_view`'s
    /// tests use for an editor-tab-hosted view (the Request view's ~15
    /// fields need real vertical room, unlike that module's narrower
    /// sidebar-only fixtures).
    const TUI_W: u16 = 120;
    const TUI_H: u16 = 40;
    #[cfg(feature = "gui")]
    const W: i32 = 1400;
    #[cfg(feature = "gui")]
    const H: i32 = 1000;

    // ── `VIMCODE_TEST_DATA_HOME` isolation (#147's "survives a plugin
    // reload" scenario needs `vimcode.storage` to resolve to a throwaway
    // directory, not a developer's real one) ───────────────────────────
    //
    // Mirrors `src/gtk/testing.rs`'s `acquire_status_paint_1345::EnvVarGuard`
    // exactly — same lock (`VIMCODE_TEST_DATA_HOME_LOCK`'s own doc explains
    // why every mutator of this one process-global env var shares it rather
    // than each guarding separately), same restore-on-drop shape. Not
    // reused directly: that struct is private to its own test module.

    struct EnvVarGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &std::ffi::OsStr) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.old.take() {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn data_home_guard(tag: &str) -> (EnvVarGuard, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_147_rest_client_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", dir.as_os_str());
        (guard, dir)
    }

    // ── Loopback HTTP fixture server ────────────────────────────────────

    /// What the fixture server observed about the one request it answered.
    struct CapturedRequest {
        method: String,
        path: String,
        headers: HashMap<String, String>,
    }

    /// Spawn a one-shot loopback HTTP server: binds an ephemeral port,
    /// answers exactly one request with a fixed `200`/`body`, capturing the
    /// request's method/path/headers along the way, then exits. Mirrors
    /// `src/tui_main/app_on_tui_tests.rs`'s `spawn_one_shot_http_fixture` /
    /// `tests/extensions.rs`'s `spawn_http_fixture_server`, extended with
    /// header capture — #147's "assert the server received it" scenario is
    /// the one consumer of that extra half.
    fn spawn_capturing_server(body: &'static str) -> (String, Arc<Mutex<Option<CapturedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral loopback port");
        let addr = listener.local_addr().expect("resolve bound local_addr");
        let base_url = format!("http://{addr}");
        let captured = Arc::new(Mutex::new(None));
        let captured_for_thread = Arc::clone(&captured);
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut seen = Vec::new();
            let mut chunk = [0u8; 4096];
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
            let text = String::from_utf8_lossy(&seen);
            let mut lines = text.split("\r\n");
            let request_line = lines.next().unwrap_or_default();
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_string();
            let path = parts.next().unwrap_or_default().to_string();
            let mut headers = HashMap::new();
            for line in lines {
                if line.is_empty() {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    headers.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
            *captured_for_thread.lock().unwrap() = Some(CapturedRequest {
                method,
                path,
                headers,
            });
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        });
        (base_url, captured)
    }

    // ── TUI harness ──────────────────────────────────────────────────────

    fn tui_harness(
        engine: Engine,
    ) -> crate::harness::ConformanceHarness<
        quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
    > {
        let mut h = crate::tui_main::testing::conformance_harness(engine, TUI_W, TUI_H);
        h.driver.render();
        h
    }

    #[test]
    fn rest_client_fill_url_send_and_paints_response_on_tui() {
        let (_data_home, _dir) = data_home_guard("fill_send_tui");
        let (base_url, _captured) = spawn_capturing_server(r#"{"status":"zqxw147ok"}"#);
        let get_url = format!("{base_url}/get147a");
        let mut h = tui_harness(engine_with_extension());
        let engine = Rc::clone(&h.engine);
        fill_url_send_and_paint_response(&mut h.driver, &engine, &get_url, "zqxw147ok");
    }

    #[test]
    fn rest_client_add_header_row_server_receives_it_on_tui() {
        let (_data_home, _dir) = data_home_guard("add_header_tui");
        let (base_url, captured) = spawn_capturing_server(r#"{"status":"zqxw147ok"}"#);
        let get_url = format!("{base_url}/get147b");
        let mut h = tui_harness(engine_with_extension());
        let engine = Rc::clone(&h.engine);
        add_header_row_then_send(&mut h.driver, &engine, &get_url, "X-Zqxw147", "zqxw147val");
        let got = captured
            .lock()
            .unwrap()
            .take()
            .expect("the fixture server must have received exactly one request");
        assert_eq!(got.method, "GET");
        assert_eq!(got.path, "/get147b");
        assert_eq!(
            got.headers.get("X-Zqxw147").map(String::as_str),
            Some("zqxw147val"),
            "the server must have received the header added through the \
             Headers table; captured headers were {:?}",
            got.headers
        );
    }

    #[test]
    fn rest_client_history_survives_reload_on_tui() {
        let (_data_home, _dir) = data_home_guard("history_reload_tui");
        let (base_url, _captured) = spawn_capturing_server(r#"{"status":"zqxw147ok"}"#);
        let get_url = format!("{base_url}/get147hist");

        {
            let mut h = tui_harness(engine_with_extension());
            let engine = Rc::clone(&h.engine);
            send_then_open_history(&mut h.driver, &engine, &get_url);
        }

        // A brand new `Engine`/`PluginManager`/`Lua` VM against the same
        // `VIMCODE_TEST_DATA_HOME` — simulates `:Plugin reload`'s "drop the
        // old manager, build a new one" exactly (`Engine::plugin_init`'s own
        // `"reload"` arm), without needing to wire up the real
        // `~/.config/vimcode/plugins` scan path this fixture doesn't use.
        let mut h2 = tui_harness(engine_with_extension());
        let engine2 = Rc::clone(&h2.engine);
        open_history_without_sending(&mut h2.driver, &engine2, &get_url);
    }

    #[test]
    fn rest_client_env_var_substitution_on_tui() {
        let (_data_home, _dir) = data_home_guard("env_sub_tui");
        let (base_url, _captured) = spawn_capturing_server(r#"{"status":"zqxw147ok"}"#);
        let mut h = tui_harness(engine_with_extension());
        let engine = Rc::clone(&h.engine);
        env_substitution_then_send(&mut h.driver, &engine, &base_url, "/get147env");
    }

    #[test]
    fn rest_client_other_views_render_without_error_on_tui() {
        let (_data_home, _dir) = data_home_guard("other_views_tui");
        let mut h = tui_harness(engine_with_extension());
        let engine = Rc::clone(&h.engine);
        other_views_render_without_error(&mut h.driver, &engine);
    }

    // ── GTK harness ──────────────────────────────────────────────────────

    #[cfg(feature = "gui")]
    fn gtk_harness(
        engine: Engine,
    ) -> crate::harness::ConformanceHarness<
        quadraui::gtk::testing::GtkDriver<impl quadraui::AppLogic>,
    > {
        let mut h = crate::gtk::testing::conformance_harness(engine, W, H);
        h.driver.render();
        h
    }

    #[cfg(feature = "gui")]
    #[test]
    fn rest_client_fill_url_send_and_paints_response_on_gtk() {
        let (_data_home, _dir) = data_home_guard("fill_send_gtk");
        let (base_url, _captured) = spawn_capturing_server(r#"{"status":"zqxw147ok"}"#);
        let get_url = format!("{base_url}/get147a");
        let mut h = gtk_harness(engine_with_extension());
        let engine = Rc::clone(&h.engine);
        fill_url_send_and_paint_response(&mut h.driver, &engine, &get_url, "zqxw147ok");
    }

    #[cfg(feature = "gui")]
    #[test]
    fn rest_client_add_header_row_server_receives_it_on_gtk() {
        let (_data_home, _dir) = data_home_guard("add_header_gtk");
        let (base_url, captured) = spawn_capturing_server(r#"{"status":"zqxw147ok"}"#);
        let get_url = format!("{base_url}/get147b");
        let mut h = gtk_harness(engine_with_extension());
        let engine = Rc::clone(&h.engine);
        add_header_row_then_send(&mut h.driver, &engine, &get_url, "X-Zqxw147", "zqxw147val");
        let got = captured
            .lock()
            .unwrap()
            .take()
            .expect("the fixture server must have received exactly one request");
        assert_eq!(got.method, "GET");
        assert_eq!(got.path, "/get147b");
        assert_eq!(
            got.headers.get("X-Zqxw147").map(String::as_str),
            Some("zqxw147val"),
            "the server must have received the header added through the \
             Headers table; captured headers were {:?}",
            got.headers
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn rest_client_history_survives_reload_on_gtk() {
        let (_data_home, _dir) = data_home_guard("history_reload_gtk");
        let (base_url, _captured) = spawn_capturing_server(r#"{"status":"zqxw147ok"}"#);
        let get_url = format!("{base_url}/get147hist");

        {
            let mut h = gtk_harness(engine_with_extension());
            let engine = Rc::clone(&h.engine);
            send_then_open_history(&mut h.driver, &engine, &get_url);
        }

        let mut h2 = gtk_harness(engine_with_extension());
        let engine2 = Rc::clone(&h2.engine);
        open_history_without_sending(&mut h2.driver, &engine2, &get_url);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn rest_client_env_var_substitution_on_gtk() {
        let (_data_home, _dir) = data_home_guard("env_sub_gtk");
        let (base_url, _captured) = spawn_capturing_server(r#"{"status":"zqxw147ok"}"#);
        let mut h = gtk_harness(engine_with_extension());
        let engine = Rc::clone(&h.engine);
        env_substitution_then_send(&mut h.driver, &engine, &base_url, "/get147env");
    }

    #[cfg(feature = "gui")]
    #[test]
    fn rest_client_other_views_render_without_error_on_gtk() {
        let (_data_home, _dir) = data_home_guard("other_views_gtk");
        let mut h = gtk_harness(engine_with_extension());
        let engine = Rc::clone(&h.engine);
        other_views_render_without_error(&mut h.driver, &engine);
    }
}
