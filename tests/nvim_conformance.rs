//! Neovim conformance tests — the repo's only *oracle-backed* Vim-behaviour suite.
//!
//! Each case defines an initial buffer, a cursor position (1-indexed line and
//! col — Neovim convention), an optional per-case `setup` snippet of Lua, and a
//! key sequence.  The same scenario is run through a real Neovim and through
//! `Engine`, and the resulting buffer + cursor are compared.  **Nothing here is
//! hand-authored**: Neovim is the oracle, so a case cannot encode the author's
//! misconception about what Vim does the way an expectation-based test can.
//!
//! ## Adding cases
//!
//! Append to the relevant per-area `CASES_*` array via the `c(..)` constructor
//! (or `cs(..)` when the case needs Lua `setup` to pin a Vim-vs-Neovim option
//! default such as `startofline`, `joinspaces`, `nrformats` or `smarttab` —
//! without that you cannot distinguish "vimcode differs from Vim" from "Neovim
//! differs from Vim").  A `cs(..)` case's `setup` is applied to **both** sides
//! — spliced into the oracle's Lua by `run_in_neovim` and mapped onto
//! `Settings` by `apply_setup` for `run_in_vimcode` — so naming an option
//! `apply_setup` has no mapping for fails loudly rather than quietly running
//! vimcode at its defaults (#1002).  The arrays are per-area purely for
//! editability; the runner flattens them.
//!
//! ## `KNOWN_DEVIATIONS` (#799)
//!
//! VimCode does not yet match Neovim on every case, so the suite ships a list of
//! labels that are *currently* expected to differ.  The gate is **bidirectional**:
//!
//!   * an unlisted label that fails → **regression**, test fails;
//!   * a listed label that starts passing → **fix landed**, test fails until the
//!     entry is deleted.
//!
//! That is what lets the full corpus land green today and shrink monotonically —
//! the list can only ever get shorter, and each Vim-compat fix is forced to prove
//! itself by deleting entries.
//!
//! Regenerate the list after an intentional behaviour change with:
//!
//! ```sh
//! CONFORMANCE_DUMP_DEVIATIONS=/tmp/dev.txt \
//!   cargo test --no-default-features --test nvim_conformance -- --nocapture
//! ```
//!
//! which writes the current failing set (already formatted as Rust string
//! literals) instead of asserting.  Never regenerate to paper over a regression:
//! the point of the list is that it does not grow.
//!
//! ## The oracle transport: an attached-UI RPC session (#1008)
//!
//! The oracle is `nvim --headless --embed` driven over msgpack-RPC with a UI
//! attached (`nvim_ui_attach`, 80x24 — see [`NvimRpc`]), and the case's keys
//! are typed **one at a time** through `nvim_input`, with the window's scroll
//! bookkeeping re-validated between each.
//!
//! It used to be `nvim --headless -l script.lua` handed the whole key
//! sequence in one `nvim_feedkeys()` burst. That oracle attaches no UI, so no
//! redraw ever runs, so `w_topline` / `w_botline` / `w_empty_rows` are never
//! re-validated between keystrokes — and `nvim_feedkeys(.., "x")` executes
//! inside `exec_normal()`, which never returns to the main loop where that
//! re-validation lives. Window-relative reads after a scroll therefore
//! answered against stale state, which is why the `scroll:` group carried a
//! long list of excuses for three issues (#805, #875, #867). Swapping the
//! transport deleted all of them; what it also did, less comfortably, is show
//! that the last surviving excuse ("scroll:2<C-b>") had been covering a real
//! vimcode bug — see `HARNESS_LIMITED`.
//!
//! Typing keys for real brings Neovim's *interactive* behaviour with it, and
//! three pieces of that are deliberately turned back off in the fixture
//! preamble (`oracle_probe`) so the corpus keeps measuring vimcode against
//! **Vim**, not against Neovim's UI defaults:
//!
//! | Turned off | Why |
//! |---|---|
//! | default mappings (`mapclear`, `mapclear!`) | Neovim maps `Y` to `y$` and `&` to `:&&<CR>`; `feedkeys(.., "n..")` used to bypass mappings, real typing does not |
//! | `'inccommand'` | a live `:s` preview, typed a character at a time, runs real substitutions that clobber the flags `:s/a/c/&` then asks for |
//! | swap files (`-n`) | every case modifies the unnamed buffer, and a few hundred concurrent swap files exhaust the suffix space; `E326` under an attached UI is a `hit-enter` prompt that blocks every deferred API call |
//!
//! ## `HARNESS_LIMITED` (#875)
//!
//! A separate, smaller list next to `KNOWN_DEVIATIONS` for cases this *test*
//! cannot faithfully probe — a harness gap or a broken oracle, not a vimcode
//! bug. These are reported but never enter the bidirectional gate: they can
//! fail forever without being a regression, and cannot force an entry
//! deletion by passing. It is **empty** as of #1008; the array's own doc
//! comment explains why both entries it ever held are gone and what the bar
//! is for adding another.
//!
//! ## Debugging a single area
//!
//! `PROBE_FILTER=<label-substring>` restricts the run; `PROBE_VERBOSE=1` prints
//! passes too.  Failures are tagged `BUF`, `CUR` or `BUF+CUR` so you can tell a
//! wrong edit from a wrong final cursor at a glance.
//!
//! ## Harness fidelity — do not "simplify" these away
//!
//! Each of the following was silencing real failures before it was added:
//!
//! | Detail | Why |
//! |---|---|
//! | `vim.o.undolevels = -1` around the fixture write, restored to `1000` | `nvim_buf_set_lines` is itself an undo step, so `u` undid the *fixture* and the buffer became `""` (41 spurious undo failures) |
//! | keys typed one at a time via `nvim_input`, never `nvim_feedkeys` | `feedkeys(.., "x")` runs `exec_normal()`, which force-`<Esc>`s an unfinished command and never redraws between keys — the #1008 transport swap |
//! | capture `nvim_win_get_height(0)`, mirror via `engine.set_viewport_lines(rows)` | `H`/`M`/`L`/`<C-d>`/`zt` are meaningless with mismatched window heights |
//! | `engine.ensure_cursor_visible()` after placing the start cursor | `nvim_win_set_cursor` scrolls the window; a raw engine cursor write does not (12 spurious scroll failures) |
//! | pump `macro_playback_queue` after every key | the UI normally pumps it, so the harness must too, or `@a` never executes on the VimCode side |
//!
//! ## Requires `nvim` >= 0.12 on PATH — failing is the default (#865)
//!
//! A missing oracle is **not** a pass: it is 1,436 cases that did not run.  The
//! runner therefore *fails* on every lane — CI, a coordinator Test leg, a
//! developer laptop — when `nvim` is absent, unparseable, or older than
//! [`MIN_NVIM_VERSION`].  Skipping is still possible, but must be deliberate and
//! visible on the command line:
//!
//! ```sh
//! NVIM_CONFORMANCE_ALLOW_SKIP=1 cargo test
//! ```
//!
//! This replaced a `CI`-env-var-only guard (#795).  That guard was right about
//! the danger and wrong about the lane: `CI` is set by GitHub Actions but **not**
//! by the coordinator's Test stage, so on a fleet Test leg a host with no `nvim`
//! on its service-unit PATH was indistinguishable from 1,436 passing cases —
//! measured across three agent hosts, one of which could not see its own
//! Homebrew `nvim` at all.  Before #795, CI never installed nvim, so this suite's
//! "SKIP" was reported as `ok` on every PR — a regression in `d}`, `ciw`, `da"`,
//! etc. would have sailed through with a green check.  Do not reintroduce an
//! implicit skip, and do not remove the CI install steps
//! (`.github/workflows/ci.yml`, both jobs).
//!
//! The version floor exists for the same reason the `cs(..)` Lua `setup` hook
//! does: Neovim's own option defaults and behaviour move between
//! releases, so a verdict from 0.9.x is not comparable with one from 0.12.x.  The
//! fleet standard — every agent host and both CI jobs — is upstream stable
//! **v0.12.5**, which CI installs from a pinned release tarball rather than apt
//! (`ubuntu-24.04` apt ships 0.9.5, below the floor).  Every run prints the
//! resolved binary path and its version, so "which nvim produced this verdict" is
//! answerable from a log.
//!
//! ## Oracle version skew (#868, #865, #872)
//!
//! `KNOWN_DEVIATIONS` was captured against Neovim [`DEVIATIONS_ORACLE`], which
//! sits next to the list itself precisely because the two are one fact.  A
//! markedly different Neovim can legitimately disagree on a handful of labels;
//! that is oracle-version skew, not a regression, and is **not** a reason to
//! edit the list.
//!
//! That policy used to be advice the runner then contradicted: the "a listed
//! label now passes" direction panicked unconditionally, so a dev on a newer
//! Neovim was *forced* to delete entries that CI would immediately re-report as
//! regressions.  Concretely, going from the 0.9.x baseline to 0.12 thirty-seven
//! entries "pass" — all but one of them a `scroll:` label excused by the
//! headless-topline bug documented in Group A below, which upstream has since
//! fixed.  Measured with the same 60-line/22-row probe as that comment:
//!
//! ```text
//!     keys    0.9.x headless w0    0.12 headless w0    interactive w0
//!     22j            23 (== cursor)              2                  2
//!     G              60 (== cursor)             39                 39
//!     50%            30 (== cursor)              9                  9
//! ```
//!
//! So the runner applies the documented policy itself: the *fixed* direction is
//! enforcing when the running Neovim's major.minor matches [`DEVIATIONS_ORACLE`]
//! (or cannot be determined at all — failing closed), and is otherwise
//! downgraded to a printed advisory (see [`fixes_are_enforced`]).  The
//! **regression** direction and the stale-entry check stay fatal everywhere —
//! they are the ones that catch real bugs.
//!
//! #865 raised the floor to 0.12 and moved CI onto the pinned fleet oracle,
//! which left a gap: no lane ran [`DEVIATIONS_ORACLE`] (still 0.9 at that
//! point), so the fixed direction was advisory everywhere.  #867 then deleted
//! the 37 entries measured to pass under 0.12.5 (114 -> 77), and #872 closed
//! the remaining gap: regenerating the 77-entry list against 0.12.5 changed
//! nothing — same 77 labels, byte-for-byte, confirming #867's manual deletion
//! had already found everything 0.12 fixed — and bumped [`DEVIATIONS_ORACLE`]
//! to `(0, 12)` in that same commit.  The fixed direction is enforcing again on
//! every standard host and both CI jobs.

mod common;

use common::engine_with;
use rmpv::Value;
use serde::Deserialize;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use vimcode_core::core::OpenMode;
use vimcode_core::{Engine, EngineAction, Settings};

#[derive(Deserialize)]
struct NvimResult {
    buf: Vec<String>,
    line: usize,
    col: usize,
    rows: usize,
    /// `line('w0')` sampled after **every** key (#1008), never deserialized —
    /// [`run_in_neovim`] fills it in as it feeds the sequence. It is the
    /// evidence that the redraw between keystrokes actually happened, and it
    /// is printed on a failure so "which keystroke moved the window wrongly"
    /// is answerable without re-running by hand.
    #[serde(default)]
    toplines: Vec<i64>,
}

/// Unique suffix for this probe's temp files, so probes can run concurrently.
fn probe_id() -> String {
    static N: AtomicUsize = AtomicUsize::new(0);
    format!(
        "{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

// ---------------------------------------------------------------------------
// The oracle transport (#1008): `nvim --embed` over msgpack-RPC, UI attached.
//
// This replaced `nvim --headless -l script.lua`. See the module docs for the
// measurements; the short version is that `-l` attaches no UI, so no redraw
// ever runs, so `w_topline`/`w_botline`/`w_empty_rows` are never re-validated
// and any window-relative read afterwards answers against stale state.
// ---------------------------------------------------------------------------

/// The terminal geometry the oracle attaches.
///
/// 80x24 is the screen a `nvim --headless` process already assumed, so the
/// **window** height the corpus mirrors — `nvim_win_get_height`, 22 once the
/// status line and the command line are taken off — is unchanged by the
/// transport swap. Every case reads `rows` back out of nvim rather than
/// assuming it (and [`run_in_vimcode`] mirrors that value via
/// `set_viewport_lines`), so the two sides cannot drift even if a future
/// Neovim changes what it subtracts.
const UI_WIDTH: u64 = 80;
const UI_HEIGHT: u64 = 24;

/// How long the oracle will wait for one request to be answered before the
/// case is declared broken.
///
/// This is a stall detector, not a performance budget: every answer is
/// produced by a local process in microseconds, so anything near this bound
/// means nvim is wedged. Without it a wedged oracle is an indefinite hang in
/// CI; with it, the case reports as `NvimBroke`, which is already a hard
/// failure on a CI lane.
const RPC_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// How long a keystroke's post-redraw barrier waits before concluding that
/// nvim is part-way through a command and cannot answer (see
/// [`NvimRpc::type_key`]), and equally the slice length between prompt checks
/// in [`NvimRpc::request_pumped`].
///
/// Generous against the thing it races: nvim is a local process answering a
/// one-line `nvim_eval` in tens of microseconds, and a machine loaded enough
/// to miss 250ms would fail the surrounding test suite on its own.
const RPC_KEY_BARRIER: std::time::Duration = std::time::Duration::from_millis(50);

/// A minimal synchronous msgpack-RPC client for one `nvim --embed` process.
///
/// msgpack-RPC has exactly three frame shapes and this speaks all three:
/// request `[0, id, method, params]`, response `[1, id, error, result]`,
/// notification `[2, method, params]`. With a UI attached nvim streams
/// `redraw` notifications continuously; draining them is not incidental
/// bookkeeping, it is what keeps nvim's stdout from filling and deadlocking
/// the case.
///
/// Frames are read on a dedicated thread into a channel rather than straight
/// off the pipe so that every wait can carry a deadline. A blocking `read` on
/// a pipe has no portable timeout, and "the suite hangs forever" is a far
/// worse failure mode on a CI lane than "this case reports broken".
struct NvimRpc {
    child: Child,
    stdin: ChildStdin,
    frames: std::sync::mpsc::Receiver<Result<Value, String>>,
    next_id: u64,
    /// Responses that arrived while a *different* request was being awaited.
    /// Needed because [`NvimRpc::request_pumped`] interleaves prompt probes
    /// with the wait for a long-running request; without this, the probe's
    /// read loop would throw away the very answer it is waiting for.
    responses: std::collections::HashMap<u64, Result<Value, String>>,
    /// 1-indexed `w_topline`, straight from the UI protocol's own
    /// `win_viewport` event — the value nvim computed *during a redraw*, which
    /// is a number a `-l` script could never observe.
    topline: i64,
}

impl NvimRpc {
    /// Spawn nvim, attach a UI, and wait until it will answer API calls.
    ///
    /// `--headless` alongside `--embed` is deliberate. On its own, `--embed`
    /// makes nvim pause startup until a UI attaches, which sounds like the
    /// stronger guarantee but in practice races: the fixture lands mid-startup
    /// on roughly one spawn in five under a 16-way parallel run. `--headless
    /// --embed` starts immediately, `nvim_ui_attach` below gives the window a
    /// real screen anyway (this is the same pairing Neovim's own UI test
    /// harness uses), and the one thing it costs — the intro screen arriving
    /// as scrolled message output, so the first buffer modification hits a
    /// `hit-enter` prompt — is deterministic rather than intermittent, and is
    /// handled by [`NvimRpc::wait_until_ready`].
    fn spawn() -> Option<Self> {
        let mut child = Command::new("nvim")
            .arg("--headless")
            .arg("--embed")
            .arg("-u")
            .arg("NONE")
            .arg("-i")
            .arg("NONE")
            // `-n`: no swap file. Not hygiene — load-bearing. Every case
            // modifies the unnamed buffer, which makes nvim write a swap file
            // named after the *working directory*; a few hundred concurrent
            // cases exhaust the `.saa`..`.svz` suffix space and the next one
            // stops on `E326: Too many swap files found`, an error message,
            // which under an attached UI is a `hit-enter` prompt that blocks
            // every deferred API call until it is answered. The `-l` oracle
            // never hit this because it never redrew.
            .arg("-n")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Deliberately discarded rather than piped-and-ignored: an unread
            // pipe fills and blocks nvim. Anything that matters comes back as
            // an RPC error instead.
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdin = child.stdin.take()?;
        let mut stdout = BufReader::new(child.stdout.take()?);
        let (tx, frames) = std::sync::mpsc::channel();
        std::thread::spawn(move || loop {
            match rmpv::decode::read_value(&mut stdout) {
                Ok(value) => {
                    if tx.send(Ok(value)).is_err() {
                        return; // the case finished; nobody is listening
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    return;
                }
            }
        });
        let mut rpc = NvimRpc {
            child,
            stdin,
            frames,
            next_id: 0,
            responses: std::collections::HashMap::new(),
            topline: 1,
        };
        rpc.request(
            "nvim_ui_attach",
            vec![
                Value::from(UI_WIDTH),
                Value::from(UI_HEIGHT),
                Value::Map(vec![(Value::from("ext_linegrid"), Value::Boolean(true))]),
            ],
        )
        .ok()?;
        rpc.wait_until_ready().ok()?;
        Some(rpc)
    }

    /// Next frame, or `Ok(None)` if `deadline` passes first. An error here is
    /// always fatal for the case: the reader thread only reports one when the
    /// pipe breaks.
    fn next_frame_by(&mut self, deadline: std::time::Instant) -> Result<Option<Value>, String> {
        let window = deadline.saturating_duration_since(std::time::Instant::now());
        match self.frames.recv_timeout(window) {
            Ok(Ok(value)) => Ok(Some(value)),
            Ok(Err(e)) => Err(format!("oracle stdout: {e}")),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err("oracle stdout closed".to_string())
            }
        }
    }

    /// Fold a `redraw` notification into the one piece of UI state this
    /// harness keeps. Everything else about the screen is discarded.
    fn absorb_notification(&mut self, frame: &[Value]) {
        if frame.get(1).and_then(Value::as_str) != Some("redraw") {
            return;
        }
        let Some(Value::Array(events)) = frame.get(2) else {
            return;
        };
        for event in events {
            let Value::Array(parts) = event else { continue };
            // `["win_viewport", [grid, win, topline, botline, ...], ...]` —
            // one trailing array per batched call, so take the last.
            if parts.first().and_then(Value::as_str) == Some("win_viewport") {
                if let Some(Value::Array(args)) = parts.iter().skip(1).next_back() {
                    if let Some(top) = args.get(2).and_then(Value::as_i64) {
                        self.topline = top + 1; // the event is 0-indexed
                    }
                }
            }
        }
    }

    /// Write a request and return its id, without waiting for the answer.
    fn send(&mut self, method: &str, params: Vec<Value>) -> Result<u64, String> {
        self.next_id += 1;
        let id = self.next_id;
        let msg = Value::Array(vec![
            Value::from(0u64),
            Value::from(id),
            Value::from(method),
            Value::Array(params),
        ]);
        rmpv::encode::write_value(&mut self.stdin, &msg).map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Read frames until `id` is answered or `deadline` passes, absorbing
    /// redraws and stashing any other request's answer on the way.
    fn await_response(
        &mut self,
        method: &str,
        id: u64,
        deadline: std::time::Instant,
    ) -> Result<Option<Value>, String> {
        if let Some(done) = self.responses.remove(&id) {
            return done.map(Some).map_err(|e| format!("{method}: {e}"));
        }
        loop {
            let frame = match self.next_frame_by(deadline)? {
                Some(frame) => frame,
                None => return Ok(None),
            };
            let arr = match frame {
                Value::Array(a) => a,
                other => return Err(format!("msgpack-rpc frame is not an array: {other}")),
            };
            match arr.first().and_then(Value::as_u64) {
                Some(2) => self.absorb_notification(&arr),
                Some(1) => {
                    let Some(other_id) = arr.get(1).and_then(Value::as_u64) else {
                        return Err("msgpack-rpc response with no id".to_string());
                    };
                    let err = arr.get(2).cloned().unwrap_or(Value::Nil);
                    let answer = if err.is_nil() {
                        Ok(arr.into_iter().nth(3).unwrap_or(Value::Nil))
                    } else {
                        Err(err.to_string())
                    };
                    if other_id == id {
                        return answer.map(Some).map_err(|e| format!("{method}: {e}"));
                    }
                    self.responses.insert(other_id, answer);
                }
                // nvim calling *us*. Nothing we attach asks for this, but an
                // unanswered request would block nvim forever, so answer nil.
                Some(0) => {
                    let their_id = arr.get(1).cloned().unwrap_or(Value::Nil);
                    let reply =
                        Value::Array(vec![Value::from(1u64), their_id, Value::Nil, Value::Nil]);
                    rmpv::encode::write_value(&mut self.stdin, &reply)
                        .map_err(|e| e.to_string())?;
                    self.stdin.flush().map_err(|e| e.to_string())?;
                }
                _ => return Err("malformed msgpack-rpc frame".to_string()),
            }
        }
    }

    /// Send a request and wait at most `window` for its response. `Ok(None)`
    /// means the deadline passed.
    ///
    /// Abandoning a request is safe: responses carry their id and land in
    /// `responses` rather than being discarded. That matters because the
    /// caller *deliberately* abandons one on every mid-command keystroke —
    /// see [`NvimRpc::type_key`].
    fn request_bounded(
        &mut self,
        method: &str,
        params: Vec<Value>,
        window: std::time::Duration,
    ) -> Result<Option<Value>, String> {
        let id = self.send(method, params)?;
        self.await_response(method, id, std::time::Instant::now() + window)
    }

    fn request(&mut self, method: &str, params: Vec<Value>) -> Result<Value, String> {
        self.request_bounded(method, params, RPC_STALL_TIMEOUT)?
            .ok_or_else(|| format!("{method}: no response within the stall timeout"))
    }

    /// Send a request, and keep answering `hit-enter` / `-- More --` prompts
    /// until it comes back.
    ///
    /// Neovim serves **no** deferred API call while one of those prompts is
    /// up, so without this a single over-long message turns into a 30-second
    /// stall and a broken case. That is not hypothetical: with a UI attached,
    /// the intro screen alone puts nvim there as soon as the buffer is first
    /// modified, so the fixture install needs it every single time.
    ///
    /// Only `r` (hit-enter) and `rm` (more) are answered — never `r?`, the
    /// `:confirm` query, whose answer is a real editing decision that belongs
    /// to the case's own keystrokes. Pressing `<CR>` at the two that *are*
    /// answered changes no buffer, cursor or register: it dismisses output.
    fn request_pumped(&mut self, method: &str, params: Vec<Value>) -> Result<Value, String> {
        let id = self.send(method, params)?;
        let give_up = std::time::Instant::now() + RPC_STALL_TIMEOUT;
        // Start with a short slice and back off. The common case — the
        // fixture install, which hits the startup prompt every single time —
        // is then ~1ms of waiting rather than a fixed quarter-second per case,
        // which is worth roughly half this suite's wall clock.
        let mut slice = std::time::Duration::from_millis(1);
        while std::time::Instant::now() < give_up {
            let until = std::time::Instant::now() + slice;
            if let Some(answer) = self.await_response(method, id, until)? {
                return Ok(answer);
            }
            self.dismiss_message_prompt()?;
            slice = (slice * 2).min(RPC_KEY_BARRIER);
        }
        Err(format!("{method}: no response within the stall timeout"))
    }

    /// If nvim is parked on a message prompt, press `<CR>`. Returns whether it
    /// did.
    ///
    /// `nvim_get_mode` and `nvim_input` are two of the few API calls Neovim
    /// marks "fast", i.e. served even while it is blocked — which is the only
    /// reason this is observable and fixable from the client side at all.
    fn dismiss_message_prompt(&mut self) -> Result<bool, String> {
        let Some(mode) = self.request_bounded("nvim_get_mode", Vec::new(), RPC_KEY_BARRIER)? else {
            return Ok(false);
        };
        let Value::Map(entries) = mode else {
            return Ok(false);
        };
        let get = |key: &str| {
            entries
                .iter()
                .find(|(k, _)| k.as_str() == Some(key))
                .map(|(_, v)| v.clone())
        };
        let blocking = get("blocking").and_then(|v| v.as_bool()).unwrap_or(false);
        let mode = get("mode")
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        if blocking && (mode == "r" || mode == "rm") {
            self.request_bounded("nvim_input", vec![Value::from("<CR>")], RPC_KEY_BARRIER)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Block until nvim will actually answer an API request, dismissing the
    /// startup prompt described on [`NvimRpc::spawn`] if one is in the way.
    fn wait_until_ready(&mut self) -> Result<(), String> {
        self.request_pumped("nvim_eval", vec![Value::from("1")])?;
        Ok(())
    }

    /// Type one key, then wait for nvim to come back to a state where it has
    /// re-validated the window. Returns that window's top line.
    ///
    /// Three choices here are load-bearing and none of them is the obvious one:
    ///
    /// * **`nvim_input`, not `nvim_feedkeys`.** `feedkeys(.., "x")` runs
    ///   nvim's `exec_normal` machinery, which force-`<Esc>`s an incomplete
    ///   command the moment typeahead drains — so feeding `dw` one key at a
    ///   time that way would cancel the `d` before its motion ever arrived.
    ///   `nvim_input` goes through the real input path, where a half-finished
    ///   command simply waits, exactly as it does for a human. It is also what
    ///   puts the keys through the **main loop**, whose `normal_check()` runs
    ///   `update_topline()` / `validate_cursor()` / `update_screen()` between
    ///   commands. `exec_normal` never returns to that loop, which is the
    ///   whole reason the old `-l` oracle could not see a chained `<C-d>`
    ///   correctly.
    ///
    /// * **The barrier is a deferred request, not a redraw notification.**
    ///   `nvim_eval` is served only from a safe point, so its answer is proof
    ///   that the key was consumed *and* that the redraw on the way back to
    ///   idle ran; `line('w0')` is then read from the freshly-validated
    ///   `w_topline`. Waiting on the UI's `flush` event instead looks tempting
    ///   and is wrong twice over: `remote_ui_flush()` emits nothing when a
    ///   keystroke changed nothing on screen (so the wait can hang forever),
    ///   and the flush that *does* arrive is the one `display_showcmd()`
    ///   emits when the key is read, i.e. **before** the command it starts has
    ///   run. Both were measured the hard way.
    ///
    /// * **The wait is bounded, and timing out is not an error.** nvim cannot
    ///   serve a deferred request while it is part-way through a Normal-mode
    ///   command, so every key that leaves a count or an operator pending —
    ///   the `3` of `3<C-e>`, the `2` of `25jH`, the `d` of `dw` — would
    ///   otherwise hang the case forever. A timeout here *is* the answer "nvim
    ///   is waiting for the rest of this command", and a command that has not
    ///   run yet has no window state to re-validate; the previous key's
    ///   topline (kept current from the UI's own `win_viewport` events)
    ///   stands.
    fn type_key(&mut self, key: &str) -> Result<i64, String> {
        let mut rest = key;
        while !rest.is_empty() {
            let written = self
                .request("nvim_input", vec![Value::from(rest)])?
                .as_u64()
                .ok_or_else(|| "nvim_input did not return a byte count".to_string())?
                as usize;
            if written == 0 {
                return Err(format!("nvim_input refused to consume {rest:?}"));
            }
            rest = rest
                .get(written..)
                .ok_or_else(|| "nvim_input split a multi-byte key".to_string())?;
        }
        if let Some(w0) = self.request_bounded(
            "nvim_eval",
            vec![Value::from("line('w0')")],
            RPC_KEY_BARRIER,
        )? {
            self.topline = w0
                .as_i64()
                .ok_or_else(|| format!("line('w0') was not an integer: {w0}"))?;
        }
        Ok(self.topline)
    }
}

impl Drop for NvimRpc {
    fn drop(&mut self) {
        // Not `:qa!` over RPC: a case can leave nvim mid-command, mid-prompt
        // or in Insert mode, where that would need its own escaping dance for
        // no benefit. The process is disposable — and killing it closes the
        // pipe, which is what retires the reader thread.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Run one case through the oracle, reporting *why* it failed rather than
/// collapsing every failure into `None`.
///
/// The old `-l` oracle printed nvim's stderr when a probe produced no
/// parseable result; keeping an equivalent is not optional now that the
/// transport has more ways to go wrong (a stalled redraw, an RPC error, a
/// killed process), and `Outcome::NvimBroke` on its own says none of them.
fn oracle_probe(
    lines: &[&str],
    cursor_line_1: usize,
    cursor_col_1: usize,
    keys: &str,
    setup: &str,
) -> Result<NvimResult, String> {
    let mut nvim = NvimRpc::spawn().ok_or_else(|| "could not spawn `nvim --embed`".to_string())?;
    let mut lua = String::new();
    // Neovim ships *default mappings* (`:h default-mappings`) that redefine
    // keys this corpus probes — `Y` is `y$`, `&` is `:&&<CR>`. The `-l` oracle
    // never saw them because `nvim_feedkeys(.., "ntx")` carries `n`, "do not
    // remap"; real typing through `nvim_input` does. Clearing them keeps this
    // a Vim-conformance suite rather than a Neovim-defaults one, and keeps
    // every case meaning what it meant when it was captured (#1008).
    lua.push_str("vim.cmd('mapclear')\nvim.cmd('mapclear!')\n");
    // `'inccommand'` defaults to "nosplit", i.e. Neovim live-previews a `:s`
    // *as it is typed*. Harmless when the command arrives as one feedkeys
    // burst; not harmless when it is typed a character at a time at an
    // attached UI, where the preview runs a real substitution that clobbers
    // the remembered flags `:s/a/c/&` then asks for. Off, like the `-l` oracle
    // effectively had it — vimcode has no live preview to compare against
    // either (#1008).
    lua.push_str("vim.o.inccommand = ''\n");
    lua.push_str("vim.o.compatible = false\n");
    lua.push_str("vim.o.shiftwidth = 4\n");
    lua.push_str("vim.o.expandtab = true\n");
    lua.push_str("vim.o.tabstop = 4\n");
    lua.push_str(setup);
    lua.push('\n');
    // `nvim_buf_set_lines` is itself an undo step, so without this the `undo:`
    // cases undo the *fixture* and compare against an empty buffer.
    lua.push_str("vim.o.undolevels = -1\n");
    lua.push_str("vim.api.nvim_buf_set_lines(0, 0, -1, false, {");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            lua.push_str(", ");
        }
        let escaped = line
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\t', "\\t");
        lua.push('"');
        lua.push_str(&escaped);
        lua.push('"');
    }
    lua.push_str("})\n");
    lua.push_str("vim.o.undolevels = 1000\n");
    lua.push_str(&format!(
        "vim.api.nvim_win_set_cursor(0, {{{}, {}}})\n",
        cursor_line_1,
        cursor_col_1.saturating_sub(1)
    ));
    nvim.request_pumped(
        "nvim_exec_lua",
        vec![Value::from(lua.as_str()), Value::Array(Vec::new())],
    )?;

    // One key at a time, each waiting for the redraw it caused (#1008). A
    // single `nvim_feedkeys` burst — even under an attached UI — reproduces
    // the very bug this transport exists to fix: the whole sequence executes
    // inside `exec_normal`, which never returns to the main loop, so no redraw
    // separates the keys and the second `<C-d>` of a chain still inherits an
    // un-revalidated `w_botline`/`w_empty_rows` from the first.
    let mut toplines = Vec::new();
    for key in nvim_key_tokens(keys) {
        toplines.push(nvim.type_key(&key)?);
    }

    // Read the verdict out over the same channel, from whatever state the
    // sequence ended in — Insert mode, an unfinished operator, a `:s///c`
    // confirm prompt.
    let dump = "local buf = vim.api.nvim_buf_get_lines(0, 0, -1, false)\n\
                local pos = vim.api.nvim_win_get_cursor(0)\n\
                local rows = vim.api.nvim_win_get_height(0)\n\
                return vim.fn.json_encode({buf = buf, line = pos[1], col = pos[2] + 1, rows = rows})";
    let json = nvim.request_pumped(
        "nvim_exec_lua",
        vec![Value::from(dump), Value::Array(Vec::new())],
    )?;
    let json = json
        .as_str()
        .ok_or_else(|| format!("oracle dump was not a string: {json}"))?;
    let mut parsed: NvimResult =
        serde_json::from_str(json).map_err(|e| format!("oracle dump {json:?}: {e}"))?;
    parsed.toplines = toplines;
    Ok(parsed)
}

fn run_in_neovim(
    lines: &[&str],
    cursor_line_1: usize,
    cursor_col_1: usize,
    keys: &str,
    setup: &str,
) -> Option<NvimResult> {
    match oracle_probe(lines, cursor_line_1, cursor_col_1, keys, setup) {
        Ok(result) => Some(result),
        Err(why) => {
            eprintln!("oracle failed for keys={keys:?}: {why}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// VimCode runner
// ---------------------------------------------------------------------------

/// Drain the macro playback queue — the UI pumps this every frame, so a harness
/// that doesn't will see `@a` do nothing at all.
fn pump(engine: &mut Engine) {
    let mut n = 0;
    while !engine.macro_playback_queue.is_empty() && n < 100_000 {
        engine.advance_macro_playback();
        n += 1;
    }
}

fn press_char(engine: &mut Engine, ch: char) {
    engine.handle_key(&ch.to_string(), Some(ch), false);
    pump(engine);
}

fn press_special(engine: &mut Engine, name: &str) {
    engine.handle_key(name, None, false);
    pump(engine);
}

fn press_ctrl(engine: &mut Engine, ch: char) {
    engine.handle_key(&ch.to_string(), Some(ch), true);
    pump(engine);
}

/// One tokenized unit of a Vim-style key sequence.
enum KeyUnit {
    Char(char),
    Special(String),
    Ctrl(char),
}

/// One token of a key sequence, still in **Neovim's** notation.
///
/// Deliberately distinct from [`KeyUnit`], which has already been translated
/// into vimcode's key names (`<Esc>` → `"Escape"`) and so cannot be turned
/// back into something `nvim_input` understands.
#[derive(Debug, PartialEq, Eq)]
enum KeyToken {
    /// A literal character.
    Char(char),
    /// An angle-bracket key, *without* its brackets: `Esc`, `C-d`, `CR`, …
    Angle(String),
}

/// Split a Vim-style key sequence into tokens, without sending them anywhere
/// or renaming anything.
///
/// This is the single place the corpus's `keys` strings are given meaning:
/// [`parse_keys`] maps these onto vimcode key names for the engine side, and
/// [`nvim_key_tokens`] renders them straight back out for the oracle side, so
/// the two sides cannot disagree about where one keystroke ends and the next
/// begins. That matters much more since #1008 than it did before — the oracle
/// now feeds keys **individually**, so a tokenizer split that differs from
/// vimcode's would silently compare two different key sequences.
fn tokenize_keys(keys: &str) -> Vec<KeyToken> {
    let mut tokens = Vec::new();
    let mut chars = keys.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let rest: String = chars.clone().collect();
            let has_closing = rest.contains('>');
            let starts_special = chars
                .peek()
                .map(|&c| c.is_ascii_uppercase() || c == 'C')
                .unwrap_or(false);
            if has_closing && starts_special {
                let name: String = chars.by_ref().take_while(|&c| c != '>').collect();
                tokens.push(KeyToken::Angle(name));
            } else {
                tokens.push(KeyToken::Char('<'));
            }
        } else {
            tokens.push(KeyToken::Char(ch));
        }
    }
    tokens
}

/// Render each token the way `nvim_input` wants to receive it, one keystroke
/// per string.
///
/// A literal `<` becomes `<lt>`: fed on its own it would otherwise look to
/// nvim like the start of an unfinished key name and hang the case waiting for
/// the rest of it. The corpus really does contain bare `<` — `<<` (shift left)
/// is two of them.
fn nvim_key_tokens(keys: &str) -> Vec<String> {
    tokenize_keys(keys)
        .into_iter()
        .map(|t| match t {
            KeyToken::Char('<') => "<lt>".to_string(),
            KeyToken::Char(c) => c.to_string(),
            KeyToken::Angle(name) => format!("<{name}>"),
        })
        .collect()
}

/// Tokenize a key sequence (`<Esc>`, `<CR>`, `<C-x>`, named keys, and literal
/// characters) into discrete units, without sending them anywhere. Shared by
/// `send_keys` (single-buffer harness) and `send_keys_multi` (#985 multi-file
/// harness) so the two harnesses can never drift on what a given `keys`
/// string means — extracted from the pre-#985 `send_keys` body verbatim, and
/// since #1008 sharing its scanner with the oracle via [`tokenize_keys`].
fn parse_keys(keys: &str) -> Vec<KeyUnit> {
    tokenize_keys(keys)
        .into_iter()
        .map(|token| match token {
            KeyToken::Char(c) => KeyUnit::Char(c),
            KeyToken::Angle(name) => match name.as_str() {
                "Esc" => KeyUnit::Special("Escape".to_string()),
                "CR" | "Enter" => KeyUnit::Special("Return".to_string()),
                "BS" => KeyUnit::Special("BackSpace".to_string()),
                "Tab" => KeyUnit::Special("Tab".to_string()),
                "Del" | "Delete" => KeyUnit::Special("Delete".to_string()),
                "Up" => KeyUnit::Special("Up".to_string()),
                "Down" => KeyUnit::Special("Down".to_string()),
                "Left" => KeyUnit::Special("Left".to_string()),
                "Right" => KeyUnit::Special("Right".to_string()),
                "Home" => KeyUnit::Special("Home".to_string()),
                "End" => KeyUnit::Special("End".to_string()),
                n if n.starts_with("C-") => KeyUnit::Ctrl(n.chars().nth(2).unwrap()),
                other => KeyUnit::Special(other.to_string()),
            },
        })
        .collect()
}

/// Parse and send a key sequence to the engine.
/// Supports `<Esc>`, `<CR>`, `<C-x>`, named keys, and literal characters.
fn send_keys(engine: &mut Engine, keys: &str) {
    for unit in parse_keys(keys) {
        match unit {
            KeyUnit::Char(c) => press_char(engine, c),
            KeyUnit::Special(name) => press_special(engine, &name),
            KeyUnit::Ctrl(c) => press_ctrl(engine, c),
        }
    }
}

/// Strip one layer of matching Lua string quotes, if present.
fn unquote_lua(value: &str) -> Option<&str> {
    for q in ['"', '\''] {
        if let Some(inner) = value.strip_prefix(q) {
            return inner.strip_suffix(q);
        }
    }
    Some(value)
}

fn parse_lua_bool(name: &str, value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!(
            "'{name}' expects a Lua boolean, got {other:?} — extend apply_setup if a \
             non-boolean form is now needed"
        )),
    }
}

/// Apply a conformance case's Lua `setup` snippet to vimcode's [`Settings`]
/// (#1002).
///
/// Before #1002 `run_in_vimcode` took no `setup` at all, so every `cs(..)` case
/// drove a *configured* Neovim against a *default* vimcode — not a Vim-compat
/// comparison but a comparison of two differently-configured editors. That one
/// gap accounted for nine excused labels.
///
/// **No Lua interpreter is involved, and none must be introduced.** The corpus
/// uses exactly one narrow statement form — `vim.o.<name>=<value>`, one per
/// line — so this parses that form directly and maps each option onto its
/// `Settings` field.
///
/// Anything unrecognised or unparseable is an `Err`, never a silent fallback to
/// defaults: that silent fallback *is* the bug #1002 fixed, and re-introducing
/// it in the error path would hide the next instance. Adding a `cs(..)` case
/// that names an option not handled here is therefore a loud failure telling
/// you to extend this function.
fn apply_setup(settings: &mut Settings, setup: &str) -> Result<(), String> {
    for raw in setup.lines() {
        let stmt = raw.trim();
        if stmt.is_empty() {
            continue;
        }
        // #1151: the `:map` family's oracle cases need two statement shapes
        // `vim.o.<name>=<value>` can't express — a mapping definition, and
        // (for `<leader>` cases) the leader itself. Both are real Lua Neovim
        // needs no help with; only vimcode's side needs a translator, since
        // it has no Lua interpreter.
        if let Some(value) = stmt.strip_prefix("vim.g.mapleader") {
            let value = value.trim().strip_prefix('=').ok_or_else(|| {
                format!("unparseable setup statement {stmt:?} — expected `vim.g.mapleader = value`")
            })?;
            let value = unquote_lua(value.trim())
                .ok_or_else(|| format!("unterminated string in setup statement {stmt:?}"))?;
            let ch = value.chars().next().ok_or_else(|| {
                format!(
                    "'vim.g.mapleader' must be a single character, got {value:?} (from {stmt:?})"
                )
            })?;
            settings.leader = ch;
            continue;
        }
        if let Some(inner) = stmt
            .strip_prefix("vim.keymap.set(")
            .and_then(|s| s.strip_suffix(')'))
        {
            apply_keymap_set(settings, stmt, inner)?;
            continue;
        }
        let body = stmt.strip_prefix("vim.o.").ok_or_else(|| {
            format!("unsupported setup statement {stmt:?} — only `vim.o.<name>=<value>` is parsed")
        })?;
        let (name, value) = body.split_once('=').ok_or_else(|| {
            format!("unparseable setup statement {stmt:?} — expected `name=value`")
        })?;
        let name = name.trim();
        let raw_value = value.trim();
        let value = unquote_lua(raw_value)
            .ok_or_else(|| format!("unterminated string in setup statement {stmt:?}"))?;
        match name {
            "autoindent" | "ai" => settings.auto_indent = parse_lua_bool(name, value)?,
            "startofline" | "sol" => settings.startofline = parse_lua_bool(name, value)?,
            "joinspaces" | "js" => settings.joinspaces = parse_lua_bool(name, value)?,
            "smarttab" | "sta" => settings.smarttab = parse_lua_bool(name, value)?,
            "nrformats" | "nf" => {
                settings.nrformats = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            // 'completeopt' has no vimcode counterpart and deliberately maps to
            // nothing: vimcode's `<C-n>`/`<C-p>` insert the match inline with no
            // popup menu, which is exactly what `completeopt=""` makes Neovim do.
            // The empty value is therefore a *recognised* no-op, not a skip —
            // and only the empty value, since any non-empty `completeopt`
            // (`menuone`, `noselect`, …) genuinely changes Neovim's behaviour
            // and would need real handling here.
            "completeopt" | "cot" if value.is_empty() => {}
            "completeopt" | "cot" => {
                return Err(format!(
                    "'completeopt' is only handled as the empty string (vimcode has no popup \
                     menu); {raw_value:?} would change the oracle's behaviour and needs real \
                     handling in apply_setup"
                ));
            }
            "foldmethod" | "fdm" => {
                if !matches!(value, "manual" | "indent" | "marker") {
                    return Err(format!(
                        "'foldmethod' only 'manual'/'indent'/'marker' are modeled in \
                         apply_setup; {raw_value:?} needs real handling there"
                    ));
                }
                settings.foldmethod = value.to_string();
            }
            "foldlevel" | "fdl" => {
                settings.foldlevel = value.parse::<usize>().map_err(|_| {
                    format!("'foldlevel' expects a non-negative integer, got {raw_value:?}")
                })?;
            }
            // #1159
            "foldmarker" | "fmr" => {
                settings.foldmarker = value.to_string();
            }
            "foldnestmax" | "fdn" => {
                settings.foldnestmax = value.parse::<usize>().map_err(|_| {
                    format!("'foldnestmax' expects a positive integer, got {raw_value:?}")
                })?;
            }
            // #1153
            "wrapscan" | "ws" => settings.wrapscan = parse_lua_bool(name, value)?,
            "shiftround" | "sr" => settings.shiftround = parse_lua_bool(name, value)?,
            "gdefault" | "gd" => settings.gdefault = parse_lua_bool(name, value)?,
            "softtabstop" | "sts" => {
                settings.softtabstop = value
                    .parse::<i32>()
                    .map_err(|_| format!("'softtabstop' expects an integer, got {raw_value:?}"))?;
            }
            "virtualedit" | "ve" => {
                settings.virtualedit = value.to_string();
            }
            // #1191
            "iskeyword" | "isk" => {
                settings.iskeyword = value.to_string();
            }
            // #1190
            "hidden" | "hid" => settings.hidden = parse_lua_bool(name, value)?,
            other => {
                return Err(format!(
                    "no vimcode Settings mapping for option '{other}' (from {stmt:?}) — add one \
                     to apply_setup rather than letting the case run with vimcode defaults"
                ));
            }
        }
    }
    Ok(())
}

/// Parse and apply one `vim.keymap.set('mode', 'lhs', 'rhs' [, {remap = true}])`
/// setup statement onto vimcode's [`Settings::keymaps`] (#1151).
///
/// Real Neovim's `vim.keymap.set` defaults to **non-recursive** — `opts.remap`
/// defaults to `false`, the opposite of legacy `:map`'s default — so a call
/// with no options table is stored `noremap`; only an explicit
/// `remap = true` makes it recursive (vimcode's `noremap = false`). Getting
/// this default backwards would make every case using the plain 3-argument
/// form compare vimcode's `noremap` against Neovim's `map`, silently.
/// Scan a Lua options-table fragment (e.g. `{ remap = true }` or
/// `{ noremap = true, silent = true }`) for an exact `key = true` /
/// `key=true` assignment. A raw substring test (`inner.contains("remap =
/// true")`) also matches inside `noremap = true` — `"noremap = true"[2..]`
/// is literally `"remap = true"` — which would flip `remap` on for the
/// opposite, and historically real, `nvim_set_keymap` option name (#1151
/// review). Scanning comma-separated key=value segments and matching `key`
/// as a whole token (not a substring) avoids that.
fn has_true_opt(inner: &str, key: &str) -> bool {
    inner.split(',').any(|part| {
        let part = part
            .trim()
            .trim_start_matches('{')
            .trim_end_matches('}')
            .trim();
        match part.strip_prefix(key) {
            Some(rest) => {
                let rest = rest.trim_start();
                rest.strip_prefix('=')
                    .map(|v| v.trim_start().starts_with("true"))
                    .unwrap_or(false)
            }
            None => false,
        }
    })
}

fn apply_keymap_set(settings: &mut Settings, stmt: &str, inner: &str) -> Result<(), String> {
    let remap = has_true_opt(inner, "remap");
    // "'i', 'jk', '<Esc>'".split('\'') → ["", "i", ", ", "jk", ", ", "<Esc>", ""],
    // so the three string arguments sit at indices 1, 3, 5.
    let quoted: Vec<&str> = inner.split('\'').collect();
    if quoted.len() < 6 {
        return Err(format!(
            "unparseable vim.keymap.set(...) args in {stmt:?} — expected \
             vim.keymap.set('mode', 'lhs', 'rhs') with single-quoted string args"
        ));
    }
    let (mode, lhs, rhs) = (quoted[1], quoted[3], quoted[5]);
    if mode.chars().count() != 1 {
        return Err(format!(
            "vim.keymap.set mode {mode:?} must be a single letter (from {stmt:?}) — \
             a table of modes is not supported here"
        ));
    }
    let bang = if remap { "" } else { "!" };
    settings.keymaps.push(format!("{mode}{bang} {lhs} {rhs}"));
    Ok(())
}

fn run_in_vimcode(
    label: &str,
    lines: &[&str],
    cursor_line_1: usize,
    cursor_col_1: usize,
    keys: &str,
    rows: usize,
    setup: &str,
) -> (String, usize, usize) {
    let text = lines.join("\n");
    let mut engine = engine_with(&text);
    engine.settings.shift_width = 4;
    engine.settings.expand_tab = true;
    engine.settings.tabstop = 4;
    // The case's own `setup` goes on last so it overrides those three shared
    // defaults, mirroring `run_in_neovim`, which likewise splices `setup` in
    // after its `shiftwidth`/`expandtab`/`tabstop` preamble.
    if let Err(why) = apply_setup(&mut engine.settings, setup) {
        panic!("conformance case {label:?}: bad `setup` — {why}");
    }
    // `apply_setup` may have pushed onto `settings.keymaps` (a `vim.g.mapleader`
    // or `vim.keymap.set` statement, #1151) or changed `settings.leader`, and
    // `engine_with` only rebuilt `user_keymaps` from *its* defaults before any
    // of that ran — cheap regardless, since most cases have neither.
    engine.rebuild_user_keymaps();
    // Screen-relative motions (H/M/L, <C-d>, zt) are meaningless unless both
    // sides agree on the window height, so mirror nvim's.
    engine.set_viewport_lines(rows);
    // Neovim computes the whole 'foldmethod'=indent/marker fold hierarchy
    // (down to 'foldlevel') as soon as the buffer is loaded, with no
    // explicit `zf` — mirror that here rather than leaving it for the key
    // sequence to trigger, since a case may probe fold state without ever
    // pressing a z-command (e.g. plain `j`/`G` motions across an
    // already-closed fold). #1159 extended this from "indent" to also cover
    // "marker".
    if matches!(engine.settings.foldmethod.as_str(), "indent" | "marker") {
        engine.apply_foldlevel(engine.settings.foldlevel);
    }
    engine.view_mut().cursor.line = cursor_line_1.saturating_sub(1);
    engine.view_mut().cursor.col = cursor_col_1.saturating_sub(1);
    // nvim_win_set_cursor scrolls the window to show the cursor; a raw engine
    // cursor write does not. Mirror that too.
    engine.ensure_cursor_visible();
    send_keys(&mut engine, keys);
    let buf = engine.buffer().to_string();
    let line = engine.view().cursor.line + 1;
    let col = engine.view().cursor.col + 1;
    (buf, line, col)
}

// ---------------------------------------------------------------------------
// Multi-file harness (#985) — the single-buffer harness above compares only
// buffer text + cursor within *one* buffer, so it structurally cannot express
// "which file is current". The reported bug this issue tracks is specifically
// cross-buffer/cross-tab `<C-o>`/`<C-i>`, which needs real files on disk
// opened via real `:e`/`:tabnew`/`:split` on both the Neovim and Engine side.
//
// `:e`/`:edit` returns `EngineAction::OpenFile` rather than opening the file
// itself — normally the UI layer (`handle_action` in `src/tui_main/mod.rs`)
// does the actual `open_file_with_mode` call after `handle_key` returns. This
// harness has no UI layer, so `press_*_multi`/`send_keys_multi` below do that
// interception themselves; `send_keys` above deliberately does not, since no
// single-buffer case ever changes files.
// ---------------------------------------------------------------------------

fn handle_multi_action(engine: &mut Engine, action: EngineAction) {
    if let EngineAction::OpenFile(path) = action {
        let _ = engine.open_file_with_mode(&path, OpenMode::Permanent);
    }
}

fn press_char_multi(engine: &mut Engine, ch: char) {
    let action = engine.handle_key(&ch.to_string(), Some(ch), false);
    handle_multi_action(engine, action);
    pump(engine);
}

fn press_special_multi(engine: &mut Engine, name: &str) {
    let action = engine.handle_key(name, None, false);
    handle_multi_action(engine, action);
    pump(engine);
}

fn press_ctrl_multi(engine: &mut Engine, ch: char) {
    let action = engine.handle_key(&ch.to_string(), Some(ch), true);
    handle_multi_action(engine, action);
    pump(engine);
}

/// Same key-sequence grammar as `send_keys` (built on the same `parse_keys`
/// tokenizer), but intercepting `EngineAction::OpenFile` the way the UI layer
/// normally would — see the module doc above.
fn send_keys_multi(engine: &mut Engine, keys: &str) {
    for unit in parse_keys(keys) {
        match unit {
            KeyUnit::Char(c) => press_char_multi(engine, c),
            KeyUnit::Special(name) => press_special_multi(engine, &name),
            KeyUnit::Ctrl(c) => press_ctrl_multi(engine, c),
        }
    }
}

/// One scenario for the multi-file jumplist harness: `files` are written to a
/// fresh temp dir before the case runs, `files[start_file]` is opened first,
/// and `{F0}`, `{F1}`, ... in `keys` are substituted with each file's absolute
/// path (see `resolve_multi_keys`) before the keys are sent to either side.
struct MultiFileCase {
    label: &'static str,
    files: &'static [(&'static str, &'static [&'static str])],
    start_file: usize,
    start_line: usize,
    start_col: usize,
    keys: &'static str,
}

const fn mfc(
    label: &'static str,
    files: &'static [(&'static str, &'static [&'static str])],
    start_file: usize,
    start_line: usize,
    start_col: usize,
    keys: &'static str,
) -> MultiFileCase {
    MultiFileCase {
        label,
        files,
        start_file,
        start_line,
        start_col,
        keys,
    }
}

/// Write `case.files` to a fresh temp dir; returns the dir (caller must clean
/// it up) and each file's absolute path in `files` order.
///
/// The directory is **canonicalized before any fixture path is derived from
/// it**, and that is load-bearing rather than tidy-up: on macOS
/// `std::env::temp_dir()` is `/var/folders/...`, and `/var` is a symlink to
/// `/private/var`. Handing the un-resolved form to both oracles produces a
/// spurious file mismatch whenever one side resolves symlinks and the other
/// echoes back what it was given — nvim's `expand('%:p')` does not resolve
/// them, vimcode's open path does, so the two disagree on a file they both
/// actually have open. `canon_opt` below cannot repair that after the fact
/// because every caller deletes `dir` *before* comparing, at which point
/// `canonicalize` fails and falls back to the raw (still-divergent) strings.
/// Resolving once here means both sides are fed the already-final
/// `/private/var/...` form and there is nothing left to normalize. Linux,
/// where `/tmp` is usually not a symlink, never saw this — hence a failure
/// that reproduced only on macOS.
fn write_multi_fixture(case: &MultiFileCase) -> (PathBuf, Vec<PathBuf>) {
    let dir = std::env::temp_dir().join(format!("vimcode_multi_probe_{}", probe_id()));
    std::fs::create_dir_all(&dir).expect("create temp dir for multi-file probe");
    let dir = dir.canonicalize().unwrap_or(dir);
    let paths = case
        .files
        .iter()
        .map(|(name, lines)| {
            let path = dir.join(name);
            std::fs::write(&path, lines.join("\n")).expect("write multi-file fixture");
            path
        })
        .collect();
    (dir, paths)
}

/// Substitute `{F0}`, `{F1}`, ... in `keys` with the absolute path of the
/// correspondingly-indexed file.
fn resolve_multi_keys(keys: &str, paths: &[PathBuf]) -> String {
    let mut out = keys.to_string();
    for (i, path) in paths.iter().enumerate() {
        out = out.replace(&format!("{{F{i}}}"), &path.to_string_lossy());
    }
    out
}

/// Best-effort path normalization so a symlinked temp dir (or trailing-slash
/// difference) doesn't register as a file mismatch. Falls back to the raw
/// path when the file no longer exists (already cleaned up).
fn canon_opt(p: &Option<PathBuf>) -> Option<PathBuf> {
    p.as_ref()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
}

/// Regression guard for the `write_multi_fixture` canonicalization above —
/// the macOS-only failure that took `nvim_conformance_jumplist_multi_file`
/// red with two "path mismatch" regressions whose two paths named the *same*
/// file (`/var/folders/...` vs `/private/var/folders/...`).
///
/// Two assertions, in the order they would fail:
///
/// 1. **The `canon_opt`-can't-save-us demonstration.** Two paths reaching one
///    file, one of them through a symlinked parent, normalize equal *while
///    the file exists* and diverge the moment it is deleted — which is
///    exactly the state `run_multi_case`/`run_jumps_case` compare in, since
///    both `remove_dir_all` the scratch dir before calling `canon_opt`. Built
///    on an explicit symlink rather than on whatever `std::env::temp_dir()`
///    happens to be, so it carries the same meaning on Linux (where `/tmp` is
///    usually already canonical and this bug was invisible) as on macOS.
/// 2. **The guard on the real call site.** The dir `write_multi_fixture`
///    hands back is its own canonicalization, so no fixture path derived from
///    it can carry an unresolved symlink into the comparison in the first
///    place. This is the assertion that fails on unfixed macOS and passes
///    after the fix.
#[cfg(unix)]
#[test]
fn multi_fixture_paths_are_canonical_so_a_symlinked_tempdir_is_not_a_file_mismatch() {
    let base = std::env::temp_dir().join(format!("vimcode_canon_guard_{}", probe_id()));
    let _ = std::fs::remove_dir_all(&base);
    let real = base.join("real");
    std::fs::create_dir_all(&real).expect("create real scratch dir");
    let link = base.join("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink the scratch dir");
    std::fs::write(real.join("f.txt"), b"x").expect("write fixture file");

    let via_link = Some(link.join("f.txt"));
    let via_real = Some(real.join("f.txt"));
    assert_ne!(
        via_link, via_real,
        "precondition: the two spellings must differ textually, or this proves nothing"
    );
    assert_eq!(
        canon_opt(&via_link),
        canon_opt(&via_real),
        "while the file exists, canon_opt resolves both spellings to one path"
    );

    let _ = std::fs::remove_dir_all(&base);
    assert_ne!(
        canon_opt(&via_link),
        canon_opt(&via_real),
        "once the scratch dir is deleted canon_opt falls back to the raw strings and \
         the two spellings of one file read as a file MISMATCH — so normalizing at \
         comparison time is powerless and the fixture must hand out resolved paths"
    );

    // The actual guard: what `write_multi_fixture` returns is already resolved.
    let case = mfc("canon-guard", &[("a.txt", &["one"])], 0, 1, 1, "");
    let (dir, paths) = write_multi_fixture(&case);
    let resolved = dir.canonicalize().expect("fixture dir exists");
    assert_eq!(
        dir, resolved,
        "write_multi_fixture must canonicalize its scratch dir before deriving \
         fixture paths from it (macOS /var -> /private/var)"
    );
    assert!(
        paths.iter().all(|p| p.starts_with(&resolved)),
        "every fixture path must be rooted at the resolved dir: {paths:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

struct MultiNvimResult {
    file: Option<PathBuf>,
    line: usize,
    col: usize,
}

/// Like `run_in_neovim`, but opens a real file from disk (`:edit`) instead of
/// synthesizing buffer content via `nvim_buf_set_lines`, and reports which
/// file ends up current rather than buffer text.
fn run_multi_in_neovim(
    start_path: &Path,
    start_line: usize,
    start_col: usize,
    resolved_keys: &str,
    cwd: &Path,
) -> Option<MultiNvimResult> {
    #[derive(Deserialize)]
    struct Raw {
        file: String,
        line: usize,
        col: usize,
    }

    let id = probe_id();
    let mut lua = String::new();
    lua.push_str("vim.o.compatible = false\n");
    // Splits/tabs land in :e/:split/:vsplit over an unmodified buffer in
    // every case below, but 'hidden' still matters once a case abandons an
    // unsaved change mid-sequence.
    lua.push_str("vim.o.hidden = true\n");
    let escaped_start = start_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    lua.push_str(&format!(
        "vim.cmd(\"edit \" .. vim.fn.fnameescape(\"{escaped_start}\"))\n"
    ));
    lua.push_str(&format!(
        "vim.api.nvim_win_set_cursor(0, {{{}, {}}})\n",
        start_line,
        start_col.saturating_sub(1)
    ));
    let escaped_keys = resolved_keys.replace('\\', "\\\\").replace('"', "\\\"");
    lua.push_str(&format!(
        "pcall(function() vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes(\"{escaped_keys}\", true, false, true), \"ntx\", false) end)\n"
    ));
    let result_path = std::env::temp_dir().join(format!("vimcode_multi_nvim_probe_{id}.json"));
    let result_path_str = result_path.to_string_lossy().replace('\\', "/");
    lua.push_str(&format!(
        "local name = vim.api.nvim_buf_get_name(0)\n\
         local pos = vim.api.nvim_win_get_cursor(0)\n\
         local result = vim.fn.json_encode({{file = name, line = pos[1], col = pos[2] + 1}})\n\
         local f = io.open(\"{result_path_str}\", \"w\")\n\
         f:write(result)\n\
         f:close()\n\
         vim.cmd(\"qa!\")\n"
    ));
    let script_path = std::env::temp_dir().join(format!("vimcode_multi_nvim_probe_{id}.lua"));
    {
        let mut f = std::fs::File::create(&script_path).ok()?;
        f.write_all(lua.as_bytes()).ok()?;
    }
    let _ = std::fs::remove_file(&result_path);
    let output = std::process::Command::new("nvim")
        .arg("--headless")
        .arg("-u")
        .arg("NONE")
        .arg("-i")
        .arg("NONE")
        .arg("-l")
        .arg(script_path.to_string_lossy().as_ref())
        .current_dir(cwd)
        .output()
        .ok();
    let raw: Option<Raw> = match &output {
        Some(o) => {
            let raw: Option<Raw> = std::fs::read_to_string(&result_path)
                .ok()
                .and_then(|json| serde_json::from_str(&json).ok());
            if raw.is_none() && !o.status.success() {
                eprintln!(
                    "nvim stderr (multi-file probe): {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            raw
        }
        None => None,
    };
    let _ = std::fs::remove_file(&script_path);
    let _ = std::fs::remove_file(&result_path);
    raw.map(|r| MultiNvimResult {
        file: if r.file.is_empty() {
            None
        } else {
            Some(PathBuf::from(r.file))
        },
        line: r.line,
        col: r.col,
    })
}

fn run_multi_in_vimcode(
    start_path: &Path,
    start_line: usize,
    start_col: usize,
    resolved_keys: &str,
) -> (Option<PathBuf>, usize, usize) {
    let mut engine = engine_with("");
    engine
        .open_file_with_mode(start_path, OpenMode::Permanent)
        .expect("open start file for multi-file probe");
    engine.view_mut().cursor.line = start_line.saturating_sub(1);
    engine.view_mut().cursor.col = start_col.saturating_sub(1);
    engine.ensure_cursor_visible();
    send_keys_multi(&mut engine, resolved_keys);
    let file = engine.active_buffer_state().file_path.clone();
    let line = engine.view().cursor.line + 1;
    let col = engine.view().cursor.col + 1;
    (file, line, col)
}

fn run_multi_case(case: &MultiFileCase) -> Outcome {
    let (dir, paths) = write_multi_fixture(case);
    let resolved_keys = resolve_multi_keys(case.keys, &paths);
    let start_path = paths[case.start_file].clone();

    let nvim = match run_multi_in_neovim(
        &start_path,
        case.start_line,
        case.start_col,
        &resolved_keys,
        &dir,
    ) {
        Some(r) => r,
        None => {
            let _ = std::fs::remove_dir_all(&dir);
            return Outcome::NvimBroke;
        }
    };
    let (vc_file, vc_line, vc_col) =
        run_multi_in_vimcode(&start_path, case.start_line, case.start_col, &resolved_keys);
    let _ = std::fs::remove_dir_all(&dir);

    let file_match = canon_opt(&nvim.file) == canon_opt(&vc_file);
    let pos_match = nvim.line == vc_line && nvim.col == vc_col;
    if file_match && pos_match {
        return Outcome::Pass;
    }
    Outcome::Fail(format!(
        "[{}] keys={:?} start={:?}@({},{})\n  file: nvim={:?} vimcode={:?}\n  cursor: nvim=({},{}) vimcode=({},{})",
        case.label,
        case.keys,
        start_path.file_name(),
        case.start_line,
        case.start_col,
        nvim.file.as_ref().map(|p| p.display().to_string()),
        vc_file.as_ref().map(|p| p.display().to_string()),
        nvim.line,
        nvim.col,
        vc_line,
        vc_col
    ))
}

// ---------------------------------------------------------------------------
// `:jumps` list-content harness (#985) — a second multi-file shape: instead
// of "where did the cursor end up", this compares the jump list's *contents*
// (count, ordering, which entry is current, and each entry's file) against
// Neovim's `getjumplist()` oracle. Deliberately does not compare `col`: the
// issue's own acceptance list asks for "count, ordering, the current-position
// marker, and the file column" — not column-number precision — and Neovim's
// jumplist is documented as per-*window*, while vimcode's is deliberately
// global (see `apply_jump_list_entry`'s doc comment) so column-perfect parity
// isn't the property under test here.
// ---------------------------------------------------------------------------

struct MultiJumpsCase {
    label: &'static str,
    files: &'static [(&'static str, &'static [&'static str])],
    start_file: usize,
    start_line: usize,
    start_col: usize,
    keys: &'static str,
}

const fn mjc(
    label: &'static str,
    files: &'static [(&'static str, &'static [&'static str])],
    start_file: usize,
    start_line: usize,
    start_col: usize,
    keys: &'static str,
) -> MultiJumpsCase {
    MultiJumpsCase {
        label,
        files,
        start_file,
        start_line,
        start_col,
        keys,
    }
}

/// `(file, 1-indexed line)` per jumplist entry, plus which index (if any) is
/// "current".
type JumpsSnapshot = (Vec<(Option<PathBuf>, usize)>, Option<usize>);

fn run_jumps_in_neovim(
    start_path: &Path,
    start_line: usize,
    start_col: usize,
    resolved_keys: &str,
    cwd: &Path,
) -> Option<JumpsSnapshot> {
    #[derive(Deserialize)]
    struct RawEntry {
        file: String,
        line: usize,
    }
    #[derive(Deserialize)]
    struct Raw {
        entries: Vec<RawEntry>,
        // getjumplist()'s second return value (baseline-adjusted, see below):
        // index of the *current* position within `entries`, `entries.len()`
        // when "live" past the newest entry (mirrors
        // `Engine::jump_list_position`), or negative when Neovim's raw index
        // still points at or before the baseline.
        current: i64,
    }

    let id = probe_id();
    let mut lua = String::new();
    lua.push_str("vim.o.compatible = false\n");
    lua.push_str("vim.o.hidden = true\n");
    let escaped_start = start_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    lua.push_str(&format!(
        "vim.cmd(\"edit \" .. vim.fn.fnameescape(\"{escaped_start}\"))\n"
    ));
    lua.push_str(&format!(
        "vim.api.nvim_win_set_cursor(0, {{{}, {}}})\n",
        start_line,
        start_col.saturating_sub(1)
    ));
    // Verified empirically (see PR description): even a bare `nvim file -c
    // 'lua print(#vim.fn.getjumplist()[1])'` with no keys sent at all reports
    // one pre-existing entry (the just-opened file's own line 1) -- an
    // artifact of how Neovim's startup opens the first file, not anything
    // `keys` does. `Engine::jump_list_snapshot` starts genuinely empty, so
    // comparing raw counts/indices would forever misreport this harness
    // artifact as a vimcode deviation. Snapshotting this baseline and
    // diffing it out below isolates exactly what `keys` added.
    lua.push_str("local baseline_len = #vim.fn.getjumplist()[1]\n");
    let escaped_keys = resolved_keys.replace('\\', "\\\\").replace('"', "\\\"");
    lua.push_str(&format!(
        "pcall(function() vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes(\"{escaped_keys}\", true, false, true), \"ntx\", false) end)\n"
    ));
    let result_path = std::env::temp_dir().join(format!("vimcode_jumps_nvim_probe_{id}.json"));
    let result_path_str = result_path.to_string_lossy().replace('\\', "/");
    lua.push_str(
        "local list, curidx = unpack(vim.fn.getjumplist())\n\
         local entries = {}\n\
         for i, e in ipairs(list) do\n\
         \x20 if i > baseline_len then\n\
         \x20   table.insert(entries, {file = vim.api.nvim_buf_get_name(e.bufnr), line = e.lnum})\n\
         \x20 end\n\
         end\n\
         local current = curidx - baseline_len\n\
         if current < 0 then current = -1 end\n",
    );
    lua.push_str(&format!(
        "local result = vim.fn.json_encode({{entries = entries, current = current}})\n\
         local f = io.open(\"{result_path_str}\", \"w\")\n\
         f:write(result)\n\
         f:close()\n\
         vim.cmd(\"qa!\")\n"
    ));
    let script_path = std::env::temp_dir().join(format!("vimcode_jumps_nvim_probe_{id}.lua"));
    {
        let mut f = std::fs::File::create(&script_path).ok()?;
        f.write_all(lua.as_bytes()).ok()?;
    }
    let _ = std::fs::remove_file(&result_path);
    let output = std::process::Command::new("nvim")
        .arg("--headless")
        .arg("-u")
        .arg("NONE")
        .arg("-i")
        .arg("NONE")
        .arg("-l")
        .arg(script_path.to_string_lossy().as_ref())
        .current_dir(cwd)
        .output()
        .ok();
    let raw: Option<Raw> = match &output {
        Some(o) => {
            let raw: Option<Raw> = std::fs::read_to_string(&result_path)
                .ok()
                .and_then(|json| serde_json::from_str(&json).ok());
            if raw.is_none() && !o.status.success() {
                eprintln!(
                    "nvim stderr (jumps-list probe): {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            raw
        }
        None => None,
    };
    let _ = std::fs::remove_file(&script_path);
    let _ = std::fs::remove_file(&result_path);
    raw.map(|r| {
        let len = r.entries.len();
        let entries = r
            .entries
            .into_iter()
            .map(|e| {
                (
                    if e.file.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(e.file))
                    },
                    e.line,
                )
            })
            .collect();
        let current = if r.current < 0 || r.current as usize >= len {
            None
        } else {
            Some(r.current as usize)
        };
        (entries, current)
    })
}

fn run_jumps_in_vimcode(
    start_path: &Path,
    start_line: usize,
    start_col: usize,
    resolved_keys: &str,
) -> JumpsSnapshot {
    let mut engine = engine_with("");
    engine
        .open_file_with_mode(start_path, OpenMode::Permanent)
        .expect("open start file for jumps-list probe");
    engine.view_mut().cursor.line = start_line.saturating_sub(1);
    engine.view_mut().cursor.col = start_col.saturating_sub(1);
    engine.ensure_cursor_visible();
    send_keys_multi(&mut engine, resolved_keys);
    let snapshot = engine.jump_list_snapshot();
    let pos = engine.jump_list_position();
    let entries: Vec<(Option<PathBuf>, usize)> = snapshot
        .into_iter()
        .map(|(file, line, _col)| (file, line + 1))
        .collect();
    let current = if pos >= entries.len() {
        None
    } else {
        Some(pos)
    };
    (entries, current)
}

fn run_jumps_case(case: &MultiJumpsCase) -> Outcome {
    let (dir, paths) = write_multi_fixture(&MultiFileCase {
        label: case.label,
        files: case.files,
        start_file: case.start_file,
        start_line: case.start_line,
        start_col: case.start_col,
        keys: case.keys,
    });
    let resolved_keys = resolve_multi_keys(case.keys, &paths);
    let start_path = paths[case.start_file].clone();

    let nvim = match run_jumps_in_neovim(
        &start_path,
        case.start_line,
        case.start_col,
        &resolved_keys,
        &dir,
    ) {
        Some(r) => r,
        None => {
            let _ = std::fs::remove_dir_all(&dir);
            return Outcome::NvimBroke;
        }
    };
    let vc = run_jumps_in_vimcode(&start_path, case.start_line, case.start_col, &resolved_keys);
    let _ = std::fs::remove_dir_all(&dir);

    let (nvim_entries, nvim_current) = nvim;
    let (vc_entries, vc_current) = vc;
    let nvim_norm: Vec<(Option<PathBuf>, usize)> = nvim_entries
        .iter()
        .map(|(f, l)| (canon_opt(f), *l))
        .collect();
    let vc_norm: Vec<(Option<PathBuf>, usize)> =
        vc_entries.iter().map(|(f, l)| (canon_opt(f), *l)).collect();

    if nvim_norm == vc_norm && nvim_current == vc_current {
        return Outcome::Pass;
    }
    let fmt = |entries: &[(Option<PathBuf>, usize)], current: Option<usize>| {
        entries
            .iter()
            .enumerate()
            .map(|(i, (f, l))| {
                let marker = if current == Some(i) { ">" } else { " " };
                format!(
                    "{marker} {l:4}  {}",
                    f.as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Outcome::Fail(format!(
        "[{}] keys={:?}\n  nvim jumps:\n{}\n  vimcode jumps:\n{}",
        case.label,
        case.keys,
        fmt(&nvim_entries, nvim_current),
        fmt(&vc_entries, vc_current)
    ))
}

struct Case {
    label: &'static str,
    lines: &'static [&'static str],
    cursor_line: usize,
    cursor_col: usize,
    keys: &'static str,
    setup: &'static str,
}
const fn c(
    label: &'static str,
    lines: &'static [&'static str],
    cursor_line: usize,
    cursor_col: usize,
    keys: &'static str,
) -> Case {
    Case {
        label,
        lines,
        cursor_line,
        cursor_col,
        keys,
        setup: "",
    }
}
const fn cs(
    label: &'static str,
    lines: &'static [&'static str],
    cursor_line: usize,
    cursor_col: usize,
    keys: &'static str,
    setup: &'static str,
) -> Case {
    Case {
        label,
        lines,
        cursor_line,
        cursor_col,
        keys,
        setup,
    }
}

const LONG: &[&str] = &[
    "L01 a", "L02 b", "L03 c", "L04 d", "L05 e", "L06 f", "L07 g", "L08 h", "L09 i", "L10 j",
    "L11 k", "L12 l", "L13 m", "L14 n", "L15 o", "L16 p", "L17 q", "L18 r", "L19 s", "L20 t",
    "L21 u", "L22 v", "L23 w", "L24 x", "L25 y", "L26 z", "L27 a", "L28 b", "L29 c", "L30 d",
    "L31 e", "L32 f", "L33 g", "L34 h", "L35 i", "L36 j", "L37 k", "L38 l", "L39 m", "L40 n",
    "L41 o", "L42 p", "L43 q", "L44 r", "L45 s", "L46 t", "L47 u", "L48 v", "L49 w", "L50 x",
    "L51 y", "L52 z", "L53 a", "L54 b", "L55 c", "L56 d", "L57 e", "L58 f", "L59 g", "L60 h",
];

// ─────────────────────────── A. operators × motions ───────────────────────────
const CASES_OP: &[Case] = &[
    c(
        "op:dw last word of line does not join",
        &["foo bar", "baz"],
        1,
        5,
        "dw",
    ),
    c("op:dw last word of buffer", &["foo bar"], 1, 5, "dw"),
    c("op:dw on whitespace", &["foo   bar"], 1, 4, "dw"),
    c("op:2dw across line end", &["a b", "c d"], 1, 3, "2dw"),
    c(
        "op:3dw crossing lines",
        &["one two", "three four"],
        1,
        5,
        "3dw",
    ),
    c("op:dw on punctuation", &["foo.bar baz"], 1, 4, "dw"),
    c("op:dw on empty line", &["", "abc"], 1, 1, "dw"),
    c("op:dw on blank-only line", &["   ", "abc"], 1, 1, "dw"),
    c(
        "op:dw last word with trailing spaces",
        &["foo bar   ", "baz"],
        1,
        5,
        "dw",
    ),
    c("op:cw on word", &["foo bar"], 1, 1, "cwX<Esc>"),
    c("op:cw on whitespace", &["foo   bar"], 1, 4, "cwX<Esc>"),
    c("op:cw at end of word", &["foo bar"], 1, 3, "cwX<Esc>"),
    c("op:cw on punctuation", &["foo.bar"], 1, 4, "cwX<Esc>"),
    c("op:2cw", &["one two three"], 1, 1, "2cwX<Esc>"),
    c("op:c2w", &["one two three"], 1, 1, "c2wX<Esc>"),
    c("op:cW", &["foo.bar baz"], 1, 1, "cWX<Esc>"),
    c(
        "op:cw last word of line",
        &["foo bar", "baz"],
        1,
        5,
        "cwX<Esc>",
    ),
    c(
        "op:c3w spans lines",
        &["one two", "three four"],
        1,
        5,
        "c3wX<Esc>",
    ),
    c("op:cw on single char word", &["a b"], 1, 1, "cwX<Esc>"),
    c("op:ce", &["foo bar"], 1, 1, "ceX<Esc>"),
    c("op:cb", &["foo bar"], 1, 5, "cbX<Esc>"),
    c("op:c$", &["foo bar"], 1, 4, "c$X<Esc>"),
    c("op:C", &["foo bar"], 1, 4, "CX<Esc>"),
    c("op:2C", &["foo bar", "baz", "qux"], 1, 4, "2CX<Esc>"),
    c("op:c0", &["foo bar"], 1, 5, "c0X<Esc>"),
    c("op:cc keeps indent", &["    foo", "bar"], 1, 6, "ccX<Esc>"),
    c("op:S keeps indent", &["    foo"], 1, 6, "SX<Esc>"),
    c("op:2cc", &["  a", "  b", "c"], 1, 1, "2ccX<Esc>"),
    c("op:cc on blank", &["", "x"], 1, 1, "ccX<Esc>"),
    cs(
        "op:cc noautoindent",
        &["    foo"],
        1,
        6,
        ":set noai<CR>ccX<Esc>",
        "vim.o.autoindent=false",
    ),
    c("op:s", &["hello"], 1, 2, "sX<Esc>"),
    c("op:3s", &["hello"], 1, 2, "3sX<Esc>"),
    c("op:3s beyond eol", &["hi"], 1, 2, "3sX<Esc>"),
    c("op:de at end of word", &["foo bar baz"], 1, 3, "de"),
    c("op:dge", &["foo bar baz"], 1, 6, "dge"),
    c("op:dgE", &["foo.x bar baz"], 1, 8, "dgE"),
    c("op:dE", &["foo.bar baz"], 1, 1, "dE"),
    c("op:dB", &["foo.bar baz"], 1, 9, "dB"),
    c("op:dW", &["foo.bar baz"], 1, 1, "dW"),
    c(
        "op:d) sentence",
        &["Hello world. Goodbye now. End."],
        1,
        1,
        "d)",
    ),
    c(
        "op:d( sentence",
        &["Hello world. Goodbye now. End."],
        1,
        14,
        "d(",
    ),
    c("op:das", &["Hello world. Goodbye now. End."], 1, 15, "das"),
    c("op:dis", &["Hello world. Goodbye now. End."], 1, 15, "dis"),
    c("op:d% on paren", &["foo(a, b) bar"], 1, 4, "d%"),
    c("op:d% before paren", &["foo(a, b) bar"], 1, 1, "d%"),
    c(
        "op:d% multiline braces",
        &["if (x) {", "  a;", "}", "b"],
        1,
        8,
        "d%",
    ),
    c("op:d/pat", &["foo bar baz"], 1, 1, "d/baz<CR>"),
    c("op:d/pat/e", &["foo bar baz"], 1, 1, "d/bar/e<CR>"),
    c("op:d?pat", &["foo bar baz"], 1, 9, "d?foo<CR>"),
    c("op:dn", &["foo bar foo baz"], 1, 1, "/foo<CR>ggdn"),
    c(
        "op:d/pat multiline",
        &["aaa", "bbb", "ccc"],
        1,
        2,
        "d/ccc<CR>",
    ),
    c(
        "op:d/pat/+1 linewise",
        &["aaa", "bbb", "ccc", "ddd"],
        1,
        1,
        "d/bbb/+1<CR>",
    ),
    c(
        "op:d/pat to col1 exclusive rule",
        &["aaa", "bbb", "ccc"],
        1,
        1,
        "d/ccc<CR>",
    ),
    c("op:d2f,", &["a,b,c,d"], 1, 1, "d2f,"),
    c("op:dt;", &["foo; bar; baz"], 1, 1, "dt;"),
    c("op:t; then ; repeat", &["foo; bar; baz"], 1, 1, "t;;"),
    c("op:t; then ; then ;", &["a;b;c;d"], 1, 1, "t;;;"),
    c("op:dt; then ;", &["foo; bar; baz"], 1, 1, "dt;;"),
    c("op:f, then ;", &["a,b,c,d"], 1, 1, "f,;"),
    c("op:f, then 2;", &["a,b,c,d"], 1, 1, "f,2;"),
    c("op:F, F, then ,", &["a,b,c,d"], 1, 7, "F,F,,"),
    c("op:t, ; ,", &["a,b,c,d"], 1, 1, "t,;,"),
    c("op:T, then ;", &["a,b,c,d"], 1, 7, "T,;"),
    c("op:f fails no move", &["abc"], 1, 1, "fz"),
    c("op:df fails no delete", &["abc def"], 1, 1, "dfz"),
    c("op:3f.", &["a.b.c.d"], 1, 1, "3f."),
    c("op:F at col1", &["abc"], 1, 1, "Fa"),
    c("op:x at eol", &["abc"], 1, 3, "x"),
    c("op:10x beyond eol", &["abcdef"], 1, 4, "10x"),
    c("op:x on empty line", &["", "x"], 1, 1, "x"),
    c("op:X at col1", &["abc"], 1, 1, "X"),
    c("op:3X", &["abcdef"], 1, 5, "3X"),
    c("op:5X beyond start", &["abcdef"], 1, 3, "5X"),
    c("op:dh at col1", &["abc", "def"], 2, 1, "dh"),
    c("op:dl at eol", &["abc"], 1, 3, "dl"),
    c("op:d3l beyond eol", &["abc"], 1, 2, "d3l"),
    c("op:d0 at col1", &["abc"], 1, 1, "d0"),
    c("op:d^", &["   abc def"], 1, 8, "d^"),
    c("op:d^ before first nonblank", &["   abc"], 1, 2, "d^"),
    c("op:dG mid", &["a", "b", "c", "d"], 2, 1, "dG"),
    c("op:dgg mid", &["a", "b", "c", "d"], 3, 1, "dgg"),
    c("op:d3G", &["a", "b", "c", "d"], 1, 1, "d3G"),
    c("op:dj last line", &["a", "b"], 2, 1, "dj"),
    c("op:dk first line", &["a", "b"], 1, 1, "dk"),
    c("op:d2j beyond end", &["a", "b", "c"], 2, 1, "d2j"),
    c("op:d5j beyond end", &["a", "b", "c"], 1, 1, "d5j"),
    c("op:d'a", &["a", "b", "c", "d"], 1, 1, "jjmaggd'a"),
    c("op:d`a", &["abc def", "ghi jkl"], 2, 4, "magg0d`a"),
    c("op:y`a cursor", &["abc def", "ghi jkl"], 2, 4, "magg0y`a"),
    c("op:yiw cursor", &["foo bar"], 1, 6, "yiw"),
    c("op:yb cursor", &["foo bar"], 1, 5, "yb"),
    c("op:yk cursor", &["a", "b"], 2, 1, "yk"),
    c("op:yj cursor", &["a", "b"], 1, 1, "yj"),
    c("op:y$ then P", &["foo bar"], 1, 5, "y$P"),
    c("op:yw at eol then p", &["foo bar", "baz"], 1, 5, "ywjp"),
    c("op:Y is linewise", &["foo bar", "baz"], 1, 5, "Yp"),
    c("op:yy 3p", &["a", "b"], 1, 1, "yy3p"),
    c("op:yw 3p", &["ab cd"], 1, 1, "yw3p"),
    c(
        "op:p linewise cursor first nonblank",
        &["  a", "b"],
        1,
        1,
        "yyjp",
    ),
    c("op:P linewise cursor", &["  a", "b"], 1, 1, "yyjP"),
    c(
        "op:p charwise multiline",
        &["abc", "def", "ghi"],
        1,
        2,
        "vjy$p",
    ),
    c(
        "op:P charwise multiline",
        &["abc", "def", "ghi"],
        1,
        2,
        "vjyP",
    ),
    c("op:gp linewise", &["a", "b"], 1, 1, "yygp"),
    c("op:gP linewise", &["a", "b"], 1, 1, "yygP"),
    c("op:gp charwise", &["abc"], 1, 1, "ylgp"),
    c("op:]p", &["    a", "b"], 1, 1, "yyj]p"),
    c("op:p charwise at eol", &["abc"], 1, 3, "ylp"),
    c("op:xp swap", &["abc"], 1, 1, "xp"),
    c("op:xp at eol", &["abc"], 1, 3, "xp"),
    c("op:ddp swap", &["a", "b", "c"], 1, 1, "ddp"),
    c("op:ddp last line", &["a", "b", "c"], 3, 1, "ddp"),
    c("op:dd last line cursor", &["  a", "  b", "  c"], 3, 1, "dd"),
    c("op:dd only line", &["abc"], 1, 1, "dd"),
    c("op:3dd more than lines", &["a", "b"], 1, 1, "3dd"),
    c("op:5dd from last line", &["a", "b", "c"], 3, 1, "5dd"),
    c("op:D on empty", &["", "a"], 1, 1, "D"),
    c("op:3D", &["abc", "def", "ghi", "jkl"], 1, 2, "3D"),
    c("op:J basic", &["a", "  b"], 1, 1, "J"),
    c(
        "op:J after period (nvim nojoinspaces)",
        &["end.", "next"],
        1,
        1,
        "J",
    ),
    cs(
        "op:J after period (vim joinspaces)",
        &["end.", "next"],
        1,
        1,
        "J",
        "vim.o.joinspaces=true",
    ),
    c("op:J next starts with )", &["foo(", "  )"], 1, 1, "J"),
    c("op:J next blank", &["a", "", "b"], 1, 1, "J"),
    c("op:J current ends with space", &["a ", "b"], 1, 1, "J"),
    c("op:3J", &["a", "b", "c", "d"], 1, 1, "3J"),
    c("op:J last line", &["a", "b"], 2, 1, "J"),
    c("op:5J count too big", &["a", "b"], 1, 1, "5J"),
    c("op:gJ", &["a", "  b"], 1, 1, "gJ"),
    c("op:3gJ", &["a", "b", "c"], 1, 1, "3gJ"),
    c("op:J cursor col", &["abc", "def"], 1, 1, "J"),
    c("op:J with tab indent", &["a", "\tb"], 1, 1, "J"),
    c("op:r", &["abc"], 1, 2, "rx"),
    c("op:3r", &["abcdef"], 1, 2, "3rx"),
    c("op:5r beyond eol", &["abc"], 1, 2, "5rx"),
    c("op:r<CR>", &["abc def"], 1, 4, "r<CR>"),
    c("op:3r<CR>", &["abcdef"], 1, 2, "3r<CR>"),
    c("op:R", &["abcdef"], 1, 2, "Rxy<Esc>"),
    c("op:R past eol", &["abc"], 1, 3, "Rxyz<Esc>"),
    c("op:R BS restores", &["abcdef"], 1, 2, "Rxyz<BS><BS><Esc>"),
    c("op:2R", &["abcdef"], 1, 1, "2Rxy<Esc>"),
    c("op:R <CR>", &["abcdef"], 1, 2, "Rx<CR>y<Esc>"),
    c("op:~", &["abc"], 1, 1, "~"),
    c("op:3~", &["abcdef"], 1, 1, "3~"),
    c("op:5~ past eol", &["abc"], 1, 2, "5~"),
    c("op:~ on non-letter", &["1a"], 1, 1, "~"),
    c("op:g~~ cursor", &["aBc dEf"], 1, 5, "g~~"),
    c("op:gUU", &["  abc"], 1, 3, "gUU"),
    c("op:guu", &["ABC"], 1, 3, "guu"),
    c("op:3guu", &["A", "B", "C", "D"], 1, 1, "3guu"),
    c("op:g~iw", &["aBc dEf"], 1, 6, "g~iw"),
    c("op:gUap", &["abc", "def", "", "ghi"], 1, 2, "gUap"),
    c("op:gu$", &["ABC DEF"], 1, 3, "gu$"),
    c("op:gUw punctuation", &["foo.bar"], 1, 1, "gUw"),
    c("op:3gUw", &["a b c d"], 1, 1, "3gUw"),
    c("op:gUe", &["abc def"], 1, 2, "gUe"),
    c("op:gUiw then w .", &["ab cd"], 1, 1, "gUiww."),
    c("op:g?? rot13", &["hello"], 1, 1, "g??"),
    c("op:g?w", &["hello world"], 1, 1, "g?w"),
    c("op:>>", &["a"], 1, 1, ">>"),
    c("op:3>>", &["a", "b", "c", "d"], 1, 1, "3>>"),
    c("op:3>> skips blank", &["a", "", "b"], 1, 1, "3>>"),
    c("op:>2j", &["a", "b", "c", "d"], 1, 1, ">2j"),
    c("op:>ip", &["a", "b", "", "c"], 1, 1, ">ip"),
    c("op:<< partial indent", &["  a"], 1, 1, "<<"),
    c("op:<< no indent", &["a"], 1, 1, "<<"),
    c("op:>> cursor", &["  abc"], 1, 4, ">>"),
    cs(
        "op:>> cursor sol",
        &["  abc"],
        1,
        4,
        ">>",
        "vim.o.startofline=true",
    ),
    c("op:>> then .", &["a"], 1, 1, ">>."),
    c("op:V2>", &["a", "b"], 1, 1, "V2>"),
    c("op:3>> j .", &["a", "b", "c", "d", "e"], 1, 1, "3>>j."),
    c("op:>> noet ts8", &["a"], 1, 1, ":set ts=8 noet<CR>>>"),
    c("op:>> noet ts4", &["a"], 1, 1, ":set ts=4 noet<CR>>>"),
    c("op:>>>> noet ts4", &["a"], 1, 1, ":set ts=4 noet<CR>>>>>"),
    c(
        "op:>> existing tab noet",
        &["\ta"],
        1,
        1,
        ":set ts=4 noet<CR>>>",
    ),
    c(
        "op:<< mixed tab space",
        &["\t  a"],
        1,
        1,
        ":set ts=4 noet<CR><<",
    ),
    // #1153 'shiftround' — verified against `nvim --headless`.
    c(
        "op:>> shiftround rounds up",
        &["     x"], // 5-space indent
        1,
        1,
        ":set sw=4 et sr<CR>>>",
    ),
    c(
        "op:<< shiftround rounds down",
        &["     x"], // 5-space indent
        1,
        1,
        ":set sw=4 et sr<CR><<",
    ),
    c(
        "op:=G braces",
        &["int f() {", "int x;", "if (x) {", "y();", "}", "}"],
        1,
        1,
        "=G",
    ),
    c("op:=ip flat", &["  a", "      b", "c"], 1, 1, "=ip"),
    c("op:== single", &["      b"], 1, 1, "=="),
    c(
        "op:gqq tw20",
        &["one two three four five six seven eight"],
        1,
        1,
        ":set tw=20<CR>gqq",
    ),
    c(
        "op:gqip tw20",
        &[
            "one two three four five six seven eight",
            "nine ten eleven twelve",
        ],
        1,
        1,
        ":set tw=20<CR>gqip",
    ),
    c(
        "op:gqj joins short",
        &["one two", "three"],
        1,
        1,
        ":set tw=30<CR>gqj",
    ),
    c(
        "op:gwip cursor",
        &["one two three four five six seven eight"],
        1,
        5,
        ":set tw=20<CR>gwip",
    ),
    c(
        "op:gqq cursor",
        &["one two three four five six seven eight"],
        1,
        5,
        ":set tw=20<CR>gqq",
    ),
    c(
        "op:gqq tw0 no wrap",
        &["one two three four five six seven eight"],
        1,
        1,
        "gqq",
    ),
    c(
        "op:gqq indented",
        &["    one two three four five six"],
        1,
        1,
        ":set tw=20<CR>gqq",
    ),
    c(
        "op:Vgq",
        &["one two three four five six"],
        1,
        1,
        ":set tw=10<CR>Vgq",
    ),
    c("op:!Gsort", &["b", "a"], 1, 1, "!Gsort<CR>"),
    c("op:!!tr", &["abc"], 1, 1, "!!tr a-z A-Z<CR>"),
    c("op:o autoindent", &["    foo"], 1, 1, "obar<Esc>"),
    c("op:O autoindent", &["    foo"], 1, 1, "Obar<Esc>"),
    c(
        "op:o esc removes indent",
        &["    foo", "bar"],
        1,
        1,
        "o<Esc>",
    ),
    c(
        "op:o x CR esc no trailing ws",
        &["    foo"],
        1,
        1,
        "ox<CR><Esc>",
    ),
    c("op:O first line", &["a"], 1, 1, "Ob<Esc>"),
    c("op:3o", &["a"], 1, 1, "3ox<Esc>"),
    c("op:2O", &["a"], 1, 1, "2Ox<Esc>"),
    c("op:5i", &["a"], 1, 1, "5ix<Esc>"),
    c("op:3a", &["ab"], 1, 1, "3a-<Esc>"),
    c("op:3A", &["ab"], 1, 1, "3Ax<Esc>"),
    c("op:2I", &["  ab"], 1, 4, "2Ix<Esc>"),
    c("op:I indented", &["  ab"], 1, 4, "Ix<Esc>"),
    c("op:gI", &["  ab"], 1, 4, "gIx<Esc>"),
    c("op:A", &["ab"], 1, 1, "Ax<Esc>"),
    c("op:a at eol", &["ab"], 1, 2, "ax<Esc>"),
    c("op:i at eol esc", &["ab"], 1, 2, "i<Esc>"),
    c("op:i col1 esc", &["ab"], 1, 1, "i<Esc>"),
    c("op:A esc cursor", &["ab"], 1, 1, "A<Esc>"),
    c("op:3iab", &["a"], 1, 1, "3iab<Esc>"),
    c("op:2i with CR", &["a"], 1, 1, "2ix<CR><Esc>"),
    c("op:gi", &["abc", "def"], 1, 2, "ix<Esc>jgiy<Esc>"),
    c("op:cw on empty line", &["", "a"], 1, 1, "cwX<Esc>"),
    cs(
        "op:o noai",
        &["    foo"],
        1,
        1,
        ":set noai<CR>ox<Esc>",
        "vim.o.autoindent=false",
    ),
    c("op:x on tab", &["\ta"], 1, 1, "x"),
    c("op:A Tab noet", &["a"], 1, 1, ":set noet<CR>A<Tab>x<Esc>"),
    c("op:dvj charwise force", &["abc", "def"], 1, 2, "dvj"),
    c("op:dVw linewise force", &["abc def", "ghi"], 1, 1, "dVw"),
    c("op:dve exclusive force", &["abc def"], 1, 1, "dve"),
    c("op:dv$", &["abc def"], 1, 2, "dv$"),
    c(
        "op:d<C-v>j blockwise force",
        &["abcde", "fghij"],
        1,
        2,
        "d<C-v>j",
    ),
    // ─────────────── #1005: first UTF-8 multi-byte conformance slice ───────────────
    //
    // Every case above this point is pure ASCII. These `mb:`-labelled cases are
    // the corpus's first multi-byte coverage (see issue #1005). They are
    // deliberately chosen so the STARTING cursor column never falls after a
    // multi-byte character on the same line: the `Case.cursor_col` field is
    // fed to Neovim as a raw BYTE offset (`nvim_win_set_cursor`) and to
    // VimCode as a raw CHAR offset (`engine.view_mut().cursor.col`) — see
    // `run_in_neovim`/`run_in_vimcode` above — so any starting column with a
    // multi-byte character before it lands the two engines on genuinely
    // different (or, on the Neovim side, invalid mid-byte) positions before a
    // single key is even pressed. Landing *on* a multi-byte character is
    // fine; only characters strictly *before* the start column must be
    // single-byte. The final asserted column is held to the same rule.
    //
    // `dl`/`2dl` on CJK below was additionally hand-verified against a live
    // `nvim --headless -u NONE -i NONE` run (not just this suite's own
    // harness) per the issue's request to confirm the oracle sees what the
    // fixture intends, not two mis-encoded buffers that coincidentally
    // compare equal.
    c("op:mb:x on latin-1 e-acute", &["café bar"], 1, 4, "x"),
    c("op:mb:rX on latin-1 e-acute", &["café"], 1, 4, "rX"),
    c("op:mb:~ on latin-1 e-acute", &["café"], 1, 4, "~"),
    c("op:mb:s on latin-1 e-acute", &["café bar"], 1, 4, "sX<Esc>"),
    c("op:mb:dw over latin-1 word", &["café bar"], 1, 1, "dw"),
    c(
        "op:mb:cw over latin-1 word",
        &["café bar"],
        1,
        1,
        "cwXXX<Esc>",
    ),
    c("op:mb:x on cyrillic first char", &["мир foo"], 1, 1, "x"),
    c("op:mb:dw over cyrillic word", &["мир foo"], 1, 1, "dw"),
    c("op:mb:rX on cyrillic", &["мир"], 1, 1, "rX"),
    c("op:mb:x on CJK first char", &["日本語 end"], 1, 1, "x"),
    // Hand-verified against `nvim --headless`: `dl` on `日本語` deletes
    // exactly the first CJK character (3 bytes, 1 char), leaving `本語`.
    c("op:mb:dl on CJK", &["日本語"], 1, 1, "dl"),
    c("op:mb:2dl on CJK", &["日本語"], 1, 1, "2dl"),
    c("op:mb:rX on CJK", &["日本語"], 1, 1, "rX"),
    c(
        "op:mb:cw over CJK word",
        &["日本語 end"],
        1,
        1,
        "cwXXX<Esc>",
    ),
    c(
        "op:mb:dw ascii word before CJK run",
        &["foo日本語bar"],
        1,
        1,
        "dw",
    ),
    c("op:mb:x on emoji", &["😀 world"], 1, 1, "x"),
    c("op:mb:rX on emoji", &["😀 world"], 1, 1, "rX"),
    c("op:mb:dw over emoji word", &["😀 world"], 1, 1, "dw"),
    c("op:mb:x on astral-plane char", &["𝄞 note"], 1, 1, "x"),
    // Combining marks written as an escape (not literal UTF-8) plus a
    // comment naming the codepoint, per #1005's fixture-encoding rule —
    // a bare U+0301 in source is an invisible byte sequence otherwise.
    c(
        "op:mb:x deletes combining-mark cluster",
        &["e\u{0301} world"], // "e" + U+0301 COMBINING ACUTE ACCENT
        1,
        1,
        "x",
    ),
    c(
        "op:mb:rX replaces combining-mark cluster with one char",
        &["e\u{0301} world"], // "e" + U+0301 COMBINING ACUTE ACCENT
        1,
        1,
        "rX",
    ),
    c(
        "op:mb:x deletes decomposed hangul jamo cluster",
        &["\u{1100}\u{1161} next"], // U+1100 HANGUL CHOSEONG KIYEOK + U+1161 HANGUL JUNGSEONG A
        1,
        1,
        "x",
    ),
    c(
        "op:mb:2x counts cells not codepoints",
        &["e\u{0301}e\u{0301}z"], // two "e" + U+0301 COMBINING ACUTE ACCENT clusters, then "z"
        1,
        1,
        "2x",
    ),
    c("op:mb:f finds latin-1 target", &["xxéyy"], 1, 1, "fé"),
    c(
        "op:mb:t stops before latin-1 target",
        &["xxéyy"],
        1,
        1,
        "té",
    ),
    c("op:mb:f finds CJK target", &["ab日cd"], 1, 1, "f日"),
    c("op:mb:t stops before CJK target", &["ab日cd"], 1, 1, "t日"),
    c("op:mb:0 from mid multi-byte line", &["café bar"], 1, 4, "0"),
    c("op:mb:3| before latin-1 char", &["café"], 1, 1, "3|"),
];

// ─────────────────────────── B. dot repeat ───────────────────────────
const CASES_DOT: &[Case] = &[
    c("dot:dw .", &["a b c d"], 1, 1, "dw."),
    c("dot:cw . next word", &["foo bar baz"], 1, 1, "cwX<Esc>w."),
    c("dot:x...", &["abcdef"], 1, 1, "x..."),
    c("dot:3x .", &["abcdefghij"], 1, 1, "3x."),
    c("dot:3x 2.", &["abcdefghij"], 1, 1, "3x2."),
    c("dot:x 3.", &["abcdefghij"], 1, 1, "x3."),
    c("dot:A; j .", &["a", "b"], 1, 1, "A;<Esc>j."),
    c("dot:ciw w .", &["foo bar"], 1, 1, "ciwX<Esc>w."),
    c("dot:dd .", &["a", "b", "c"], 1, 1, "dd."),
    c("dot:2dd .", &["a", "b", "c", "d", "e"], 1, 1, "2dd."),
    c("dot:2dd 3.", &["a", "b", "c", "d", "e", "f"], 1, 1, "2dd3."),
    c("dot:yyp .", &["a"], 1, 1, "yyp."),
    c("dot:yy3p .", &["a"], 1, 1, "yy3p."),
    c("dot:J .", &["a", "b", "c"], 1, 1, "J."),
    c("dot:~ .", &["abcd"], 1, 1, "~."),
    c("dot:rx l .", &["abcd"], 1, 1, "rxl."),
    c("dot:o .", &["a"], 1, 1, "ob<Esc>."),
    c("dot:O .", &["a"], 1, 1, "Ob<Esc>."),
    c("dot:vlld .", &["abcdefgh"], 1, 1, "vlld."),
    c("dot:Vjd .", &["a", "b", "c", "d", "e"], 1, 1, "Vjd."),
    c("dot:Vj> j .", &["a", "b", "c", "d"], 1, 1, "Vj>j."),
    c("dot:dap .", &["a", "", "b", "", "c"], 1, 1, "dap."),
    c("dot:ifoo .", &["ab"], 1, 1, "ifoo<Esc>."),
    c("dot:3Ax j .", &["a", "b"], 1, 1, "3Ax<Esc>j."),
    c("dot:3Ax j 2.", &["a", "b"], 1, 1, "3Ax<Esc>j2."),
    c("dot:cc j .", &["a", "b"], 1, 1, "ccX<Esc>j."),
    c("dot:s l .", &["abcd"], 1, 1, "sX<Esc>l."),
    c("dot:C j .", &["abc", "def"], 1, 2, "CX<Esc>j."),
    c("dot:ct, .", &["a,b,c"], 1, 1, "ct,X<Esc>ll."),
    c("dot:df. .", &["a.b.c.d"], 1, 1, "df.."),
    c("dot:x u .", &["abc"], 1, 1, "xu."),
    c("dot:R .", &["abcdef"], 1, 1, "Rxy<Esc>ll."),
    c("dot:diw . .", &["foo bar baz"], 1, 1, "diw.."),
    c("dot:& repeat sub", &["a a a", "a a"], 1, 1, ":s/a/b/<CR>j&"),
    c("dot:g&", &["a a", "a a"], 1, 1, ":s/a/b/g<CR>g&"),
    c("dot:@:", &["a", "b", "c"], 1, 1, ":d<CR>@:"),
    c("dot:yank not repeated", &["ab cd"], 1, 1, "xyw."),
    c("dot:\"ayy \"ap .", &["a", "b"], 1, 1, "\"ayyj\"ap."),
    c(
        "dot:\"1p . . increments",
        &["a", "b", "c", "d"],
        1,
        1,
        "dddddd\"1p..",
    ),
    c("dot:>ip .", &["a", "b", "", "c", "d"], 1, 1, ">ip}j."),
    c("dot:I .", &["a", "b"], 1, 1, "Ix<Esc>j."),
    c("dot:i<C-w> .", &["ab cd", "ef gh"], 1, 6, "i<C-w>X<Esc>j$."),
    c("dot:vec .", &["foo bar baz"], 1, 1, "vecX<Esc>w."),
    c(
        "dot:vjd . charwise",
        &["a1", "b2", "c3", "d4", "e5"],
        1,
        1,
        "vjd.",
    ),
    c(
        "dot:<C-v>jIx .",
        &["ab", "ab", "ab", "ab"],
        1,
        1,
        "<C-v>jIx<Esc>jj.",
    ),
    c("dot:cw with count 2.", &["a b c d e"], 1, 1, "cwX<Esc>w2."),
    c("dot:dfx count override", &["a.b.c.d.e"], 1, 1, "df.2."),
    c("dot:gUw .", &["ab cd"], 1, 1, "gUww."),
    c("dot:>> 2.", &["a"], 1, 1, ">>2."),
    c("dot:ofoo<CR>bar .", &["a"], 1, 1, "ofoo<CR>bar<Esc>."),
    c("dot:p charwise .", &["ab"], 1, 1, "ylp."),
    c("dot:xp .", &["abcd"], 1, 1, "xp."),
    c("dot:ciw then . at eol", &["ab cd"], 1, 1, "ciwX<Esc>$."),
];

// ─────────────────────────── C. undo ───────────────────────────
const CASES_UNDO: &[Case] = &[
    c("undo:ifoo u", &["ab"], 1, 1, "ifoo<Esc>u"),
    c("undo:two inserts u", &["ab"], 1, 1, "ifoo<Esc>ibar<Esc>u"),
    c("undo:xxx u", &["abcdef"], 1, 1, "xxxu"),
    c("undo:xxx uu", &["abcdef"], 1, 1, "xxxuu"),
    c("undo:xxx uu C-r", &["abcdef"], 1, 1, "xxxuu<C-r>"),
    c("undo:xxxx 3u", &["abcdef"], 1, 1, "xxxx3u"),
    c("undo:dw u cursor", &["foo bar baz"], 1, 5, "dwu"),
    c("undo:dd u cursor", &["a", "b", "c"], 2, 1, "ddu"),
    c("undo:G dd u cursor", &["a", "b", "c"], 1, 1, "Gddu"),
    c("undo:U", &["abcdef"], 1, 1, "xxxU"),
    c("undo:UU", &["abcdef"], 1, 1, "xxxUU"),
    c(
        "undo:insert with CR is one undo",
        &["ab"],
        1,
        1,
        "ihello<CR>world<Esc>u",
    ),
    c("undo:A xyz u cursor", &["abc"], 1, 1, "A xyz<Esc>u"),
    c("undo:cw u cursor", &["foo bar"], 1, 5, "cwX<Esc>u"),
    c("undo:C-g u splits", &["ab"], 1, 1, "ifoo<C-g>ubar<Esc>u"),
    c(
        "undo:arrow breaks undo",
        &["ab"],
        1,
        1,
        "ifoo<Left>bar<Esc>u",
    ),
    c("undo:u after p", &["a", "b"], 1, 1, "yyjpu"),
    c("undo:u after J", &["a", "b"], 1, 1, "Ju"),
    c("undo:u after >>", &["a"], 1, 1, ">>u"),
    c("undo:u after :s", &["a a"], 1, 1, ":s/a/b/g<CR>u"),
    c(
        "undo:u after :%s cursor",
        &["a", "a", "a"],
        3,
        1,
        ":%s/a/b/<CR>u",
    ),
    c("undo:u after visual d", &["abcdef"], 1, 2, "vlldu"),
    c("undo:u after macro", &["a", "b", "c"], 1, 1, "qaddq@au"),
    c("undo:u after .", &["abcdef"], 1, 1, "x.u"),
    c(
        "undo:C-r after new change noop",
        &["abcdef"],
        1,
        1,
        "xxuux<C-r>",
    ),
    c("undo:u after o cursor", &["a", "b"], 1, 1, "ox<Esc>u"),
    c("undo:u on unchanged", &["a"], 1, 1, "u"),
    c("undo:r u", &["abc"], 1, 2, "rxu"),
    c("undo:3rx u", &["abcdef"], 1, 2, "3rxu"),
    c(
        "undo:u restores cursor after :g",
        &["a", "b", "a"],
        1,
        1,
        ":g/a/d<CR>u",
    ),
    c("undo:cc u cursor", &["  foo", "bar"], 1, 3, "ccX<Esc>u"),
    c("undo:u after R", &["abcdef"], 1, 2, "Rxyz<Esc>u"),
    c("undo:u after ~", &["abc"], 1, 1, "~u"),
    c("undo:u after C-a", &["5"], 1, 1, "<C-a>u"),
    c("undo:u after <C-v>I", &["ab", "ab"], 1, 1, "<C-v>jIx<Esc>u"),
    c(
        "undo:2u after insert ×3",
        &["a"],
        1,
        1,
        "Ax<Esc>Ay<Esc>Az<Esc>2u",
    ),
    c(
        "undo:u after dd on last line cursor",
        &["a", "b", "c"],
        3,
        1,
        "ddu",
    ),
    c("undo:C-r cursor", &["a", "b", "c"], 2, 1, "ddu<C-r>"),
    c(
        "undo:undo insert then cursor col",
        &["hello"],
        1,
        3,
        "ixyz<Esc>u",
    ),
    c("undo:u after :m", &["a", "b", "c"], 1, 1, ":m$<CR>u"),
    c("undo:u after :t", &["a", "b"], 1, 1, ":t$<CR>u"),
    c(
        "undo:u after :normal",
        &["a", "b"],
        1,
        1,
        ":%normal Ax<CR>u",
    ),
];

// ─────────────────────────── D. registers ───────────────────────────
const CASES_REG: &[Case] = &[
    c("reg:\"ayy \"ap", &["a", "b"], 1, 1, "\"ayyj\"ap"),
    c(
        "reg:\"ayw \"Ayw \"ap",
        &["foo bar"],
        1,
        1,
        "\"ayww\"Ayw$\"ap",
    ),
    c(
        "reg:\"Ayy linewise append",
        &["a", "b"],
        1,
        1,
        "\"ayyj\"Ayy\"ap",
    ),
    c(
        "reg:\"Ayy onto charwise becomes linewise",
        &["foo", "bar"],
        1,
        1,
        "\"aywj\"Ayy\"ap",
    ),
    c("reg:dd \"1p", &["a", "b", "c"], 1, 1, "ddj\"1p"),
    c("reg:dd dd \"2p", &["a", "b", "c"], 1, 1, "dddd\"2p"),
    c("reg:dw goes to \"-", &["foo bar"], 1, 1, "dw$\"-p"),
    c(
        "reg:dw does not touch \"1",
        &["foo bar", "x"],
        1,
        1,
        "jddkdw\"1p",
    ),
    c(
        "reg:d/ goes to \"1",
        &["foo bar baz"],
        1,
        1,
        "d/baz<CR>$\"1p",
    ),
    c("reg:d% goes to \"1", &["(ab) cd"], 1, 1, "d%$\"1p"),
    c(
        "reg:dn goes to \"1",
        &["x ab x ab"],
        1,
        1,
        "/ab<CR>ggdn$\"1p",
    ),
    c("reg:yy dd \"0p", &["a", "b", "c"], 1, 1, "yyjdd\"0p"),
    c("reg:\"_dd then p", &["a", "b", "c"], 1, 1, "yyj\"_ddp"),
    c(
        "reg:\"add \"bdd \"ap \"bp",
        &["a", "b", "c"],
        1,
        1,
        "\"add\"bdd\"ap\"bp",
    ),
    c("reg:3\"ap", &["a", "b"], 1, 1, "\"ayy3\"ap"),
    c("reg:\"ayl 3\"ap", &["ab"], 1, 1, "\"ayl3\"ap"),
    c("reg:\". insert register", &["ab"], 1, 1, "ifoo<Esc>\".p"),
    c("reg:\": last cmd", &["a a"], 1, 1, ":s/a/b/<CR>\":p"),
    c("reg:\"/ last search", &["foo bar"], 1, 1, "/bar<CR>\"/P"),
    c("reg:i C-r a", &["foo bar"], 1, 1, "\"aywA<C-r>a<Esc>"),
    c("reg:i C-r \"", &["foo bar"], 1, 1, "ywA<C-r>\"<Esc>"),
    c("reg:i C-r 0", &["foo bar"], 1, 1, "ywA<C-r>0<Esc>"),
    c(
        "reg:i C-r a linewise",
        &["a", "b"],
        1,
        1,
        "\"ayyjA<C-r>a<Esc>",
    ),
    c("reg:\"_x then p", &["abc"], 1, 1, "yl\"_xp"),
    c(
        "reg:dd yy \"1p unchanged by yank",
        &["a", "b", "c"],
        1,
        1,
        "ddyy\"1p",
    ),
    c(
        "reg:\"adw does not set \"-",
        &["foo bar"],
        1,
        1,
        "\"adw\"-p",
    ),
    c("reg:\"ayw x \"ap", &["foo bar"], 1, 1, "\"aywx\"ap"),
    c("reg:\"add then \"1p", &["a", "b", "c"], 1, 1, "\"add\"1p"),
    c(
        "reg:\"Add appends",
        &["a", "b", "c"],
        1,
        1,
        "\"add\"Add\"ap",
    ),
    c(
        "reg:\"ayy \"ap count in visual",
        &["a", "b"],
        1,
        1,
        "\"ayyjV\"ap",
    ),
    c("reg:yiw viwp swaps", &["foo bar"], 1, 1, "yiwwviwp0P"),
    c("reg:viw\"_dP", &["foo bar"], 1, 1, "yiwwviw\"_dP"),
    c("reg:\"0 after visual y", &["a", "b"], 1, 1, "Vyjdd\"0p"),
    c("reg:\"- after x", &["abc"], 1, 1, "x$\"-p"),
    c("reg:\"- after s", &["abc"], 1, 1, "sZ<Esc>$\"-p"),
    c("reg:\"1 after cc", &["a", "b"], 1, 1, "ccX<Esc>j\"1p"),
    c("reg:\"- after cw", &["foo bar"], 1, 1, "cwX<Esc>$\"-p"),
    c(
        "reg:\"1p shifted by dd in visual",
        &["a", "b", "c"],
        1,
        1,
        "Vd\"1p",
    ),
    c(
        "reg:\"a in :normal",
        &["a", "b"],
        1,
        1,
        "\"ayy:normal \"ap<CR>",
    ),
    c("reg:\"= expr", &["a"], 1, 1, "\"=1+1<CR>p"),
    c("reg:C-r = in insert", &["a"], 1, 1, "A<C-r>=2*3<CR><Esc>"),
    c("reg:\"% file name empty", &["a"], 1, 1, "\"%p"),
    c(
        "reg:paste count charwise multiline",
        &["ab", "cd"],
        1,
        1,
        "vjy2p",
    ),
    c(
        "reg:p from \"1 then u then \"2p",
        &["a", "b", "c"],
        1,
        1,
        "dddd\"1pu\"2p",
    ),
];

// ─────────────────────────── E. macros ───────────────────────────
const CASES_MAC: &[Case] = &[
    c("mac:qaxjq @a", &["ab", "cd", "ef"], 1, 1, "qaxjq@a"),
    c(
        "mac:qaA! j q 2@a",
        &["a", "b", "c", "d"],
        1,
        1,
        "qaA!<Esc>jq2@a",
    ),
    c("mac:@a @@", &["a", "b", "c", "d"], 1, 1, "qaA!<Esc>jq@a@@"),
    c(
        "mac:10@a stops at failure",
        &["a,b", "c,d", "e f", "g,h"],
        1,
        1,
        "qa0f,xjq10@a",
    ),
    c("mac:qA append", &["ab", "cd", "ef"], 1, 1, "qaxqqAjq@a"),
    c(
        "mac:macro with insert",
        &["a", "b"],
        1,
        1,
        "qaIfoo <Esc>jq@a",
    ),
    c(
        "mac:macro with :s",
        &["a a", "a a"],
        1,
        1,
        "qa:s/a/b/<CR>jq@a",
    ),
    c(
        "mac:macro with search",
        &["x foo", "y foo", "z foo"],
        1,
        1,
        "qa/foo<CR>rXq@a",
    ),
    c("mac:\"ap shows macro", &["ab"], 1, 1, "qaxq\"ap"),
    c(
        "mac:recursive",
        &["a", "b", "c", "d"],
        1,
        1,
        "qaqqaA!<Esc>j@aq@a",
    ),
    c("mac:@a then .", &["abcd"], 1, 1, "qaxq@a."),
    c("mac:3@a", &["a", "b", "c", "d"], 1, 1, "qaddq3@a"),
    c("mac:count inside", &["a b c d e"], 1, 1, "qa2dwq@a"),
    c(
        "mac:macro with visual",
        &["ab", "cd", "ef"],
        1,
        1,
        "qavlUjq@a",
    ),
    c("mac:macro with dot", &["abc", "def"], 1, 1, "qax.jq@a"),
    c("mac:macro with undo", &["abc"], 1, 1, "qaxuq@a"),
    c(
        "mac:macro ending in insert",
        &["a", "b"],
        1,
        1,
        "qaAx<Esc>q@a",
    ),
    c(
        "mac:@@ after 2@a",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "qaA!<Esc>jq2@a@@",
    ),
    c(
        "mac:macro with ctrl-a",
        &["1", "1", "1"],
        1,
        1,
        "qa<C-a>jq2@a",
    ),
    c("mac:macro yank paste", &["a", "b"], 1, 1, "qayypq@a"),
    c(
        "mac:\"ay then @a executes text",
        &["ix<Esc>", "b"],
        1,
        1,
        "\"ay$j@a",
    ),
    c(
        "mac:macro with ci(",
        &["f(a)", "g(b)"],
        1,
        1,
        "qaci(X<Esc>jq@a",
    ),
    c(
        "mac:q register letter uppercase Q",
        &["ab", "cd"],
        1,
        1,
        "qQxq@Q",
    ),
];

// ─────────────────────────── F. marks & jumps ───────────────────────────
const CASES_MARK: &[Case] = &[
    c("mark:'a first nonblank", &["  a", "b", "c"], 1, 3, "majj'a"),
    c("mark:`a exact", &["abc", "def", "ghi"], 1, 3, "majj`a"),
    c("mark:'' after G", &["a", "b", "c", "d"], 2, 1, "G''"),
    c("mark:`` after gg", &["abc", "def", "ghi"], 2, 2, "gg``"),
    c(
        "mark:'' after search",
        &["a", "b", "c foo"],
        1,
        1,
        "/foo<CR>''",
    ),
    c("mark:'.", &["a", "b", "c"], 2, 1, "xgg'."),
    c("mark:`.", &["abc", "def"], 2, 2, "xgg`."),
    c("mark:`] after yank", &["abc def"], 1, 1, "wyiw0`]"),
    c("mark:`[ after p", &["a", "b"], 1, 1, "yyjp`["),
    c(
        "mark:'> after V",
        &["a", "b", "c", "d"],
        2,
        1,
        "Vj<Esc>gg'>",
    ),
    c("mark:`< after v", &["abc", "def"], 1, 2, "vjl<Esc>gg`<"),
    c(
        "mark:mark shifts after O",
        &["a", "b", "c"],
        2,
        1,
        "maggOx<Esc>'a",
    ),
    c(
        "mark:mark on deleted line",
        &["a", "b", "c"],
        2,
        1,
        "maddgg'a",
    ),
    c("mark:'z unset", &["a", "b"], 2, 1, "'z"),
    c("mark:`^", &["ab", "cd"], 1, 1, "jAx<Esc>gg`^"),
    c("mark:'' toggles", &["a", "b", "c", "d"], 1, 1, "3G''''"),
    c("mark:`` after %", &["(a)", "b"], 1, 1, "%``"),
    c("mark:'' after j only", &["a", "b", "c"], 1, 1, "jj''"),
    c("mark:d'a linewise", &["abc", "def"], 2, 2, "magg0d'a"),
    c("mark:c`a", &["abc def"], 1, 5, "ma0c`aX<Esc>"),
    c("mark:mA global", &["a", "b", "c"], 3, 1, "mAgg'A"),
    c("mark:`a after line join", &["ab", "cd"], 2, 2, "makJ`a"),
    c(
        "mark:'a after text insert above",
        &["a", "b"],
        2,
        1,
        "maggOx<CR>y<Esc>'a",
    ),
    c("mark:`` after ''", &["a", "b", "c"], 1, 1, "G''``"),
    c("mark:y'a cursor", &["a", "b", "c"], 3, 1, "maggy'a"),
    c(
        "mark:'a then '' back",
        &["a", "b", "c", "d"],
        4,
        1,
        "magg'a''",
    ),
    c("mark:`[ `] after :s", &["a a"], 1, 1, ":s/a/xyz/<CR>`]"),
    c("mark:`> after gv", &["abc"], 1, 1, "vl<Esc>0gv<Esc>`>"),
    c("mark:`. after o", &["a", "b"], 1, 1, "ox<Esc>gg`."),
    c("mark:'[ after >>", &["a", "b", "c"], 2, 1, ">jgg'["),
    c("jump:G C-o", &["a", "b", "c", "d"], 2, 1, "G<C-o>"),
    c(
        "jump:gg G C-o C-o C-i",
        &["a", "b", "c", "d"],
        2,
        1,
        "ggG<C-o><C-o><C-i>",
    ),
    c("jump:/foo C-o", &["a", "b", "foo"], 1, 1, "/foo<CR><C-o>"),
    c(
        "jump:3G 5G C-o C-o",
        &["1", "2", "3", "4", "5", "6"],
        1,
        1,
        "3G5G<C-o><C-o>",
    ),
    c(
        "jump:C-o after :5",
        &["1", "2", "3", "4", "5", "6"],
        1,
        1,
        "3G:5<CR><C-o>",
    ),
    c(
        "jump:C-o then new jump then C-i",
        &["1", "2", "3", "4", "5", "6"],
        1,
        1,
        "3G5G<C-o>2G<C-i>",
    ),
    c("jump:g;", &["a", "b", "c"], 1, 1, "xjjxggg;"),
    c("jump:g; g;", &["a", "b", "c"], 1, 1, "xjjxggg;g;"),
    c("jump:g; g; g,", &["a", "b", "c"], 1, 1, "xjjxggg;g;g,"),
    c("jump:n C-o", &["foo", "foo", "foo"], 1, 1, "/foo<CR>n<C-o>"),
    c("jump:* C-o", &["foo bar", "foo"], 1, 1, "*<C-o>"),
    c("jump:% C-o", &["(abc)"], 1, 1, "%<C-o>"),
    c("jump:'a C-o", &["a", "b", "c"], 3, 1, "magg'a<C-o>"),
    c("jump:C-o at start", &["a", "b"], 1, 1, "<C-o>"),
    c("jump:C-i at end", &["a", "b"], 1, 1, "G<C-i>"),
    c("jump:} C-o", &["a", "", "b"], 1, 1, "}<C-o>"),
    c("jump:C-o col", &["abc", "def"], 1, 3, "G<C-o>"),
    c(
        "jump:C-o twice same line dedup",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "3G3G5G<C-o><C-o>",
    ),
    c("jump:C-o after j 20 lines", LONG, 1, 1, "20j<C-o>"),
    c("jump:C-o after 50%", LONG, 1, 1, "50%<C-o>"),
    c("jump:C-o after L", LONG, 1, 1, "L<C-o>"),
    c("jump:C-o after (", &["A b. C d."], 1, 8, "(<C-o>"),
    c(
        "jump:g; after 2 changes same line",
        &["abcdef"],
        1,
        1,
        "x$xgg0g;g;",
    ),
    c(
        "jump:3<C-o>",
        &["1", "2", "3", "4", "5", "6"],
        1,
        1,
        "2G3G4G5G3<C-o>",
    ),
    c(
        "jump:C-o after ''",
        &["a", "b", "c", "d"],
        1,
        1,
        "3G''<C-o>",
    ),
];

// ─────────────────────────── G. search ───────────────────────────
const CASES_SEARCH: &[Case] = &[
    c("search:/ basic", &["foo bar", "baz bar"], 1, 1, "/bar<CR>"),
    c("search:/ n", &["foo bar", "baz bar"], 1, 1, "/bar<CR>n"),
    c(
        "search:/ N wraps",
        &["foo bar", "baz bar"],
        1,
        1,
        "/bar<CR>N",
    ),
    c(
        "search:? then n backward",
        &["bar", "foo", "bar"],
        2,
        1,
        "?bar<CR>n",
    ),
    c(
        "search:? then N forward",
        &["bar", "foo", "bar"],
        2,
        1,
        "?bar<CR>N",
    ),
    c("search:*", &["foo bar foo"], 1, 1, "*"),
    c("search:* whole word", &["foo foobar foo"], 1, 1, "*"),
    c("search:g*", &["foo foobar foo"], 1, 1, "g*"),
    c("search:#", &["foo foobar foo"], 1, 12, "#"),
    c("search:g#", &["foo foobar foo"], 1, 12, "g#"),
    c("search:* cursor not on word", &["  foo bar foo"], 1, 1, "*"),
    c("search:* on punctuation", &["a.b a.b"], 1, 2, "*"),
    c(
        "search:* then n keeps boundaries",
        &["foo foobar foo"],
        1,
        1,
        "*n",
    ),
    c(
        "search:* then :s//",
        &["foo bar foo"],
        1,
        1,
        "*:%s//X/g<CR>",
    ),
    c("search:* with count", &["a foo foo foo"], 1, 3, "2*"),
    c("search:# at start wraps", &["foo bar foo"], 1, 1, "#"),
    c("search:/pat/e", &["foo bar"], 1, 1, "/bar/e<CR>"),
    c("search:/pat/e+1", &["foo bar baz"], 1, 1, "/bar/e+1<CR>"),
    c("search:/pat/e-1", &["foo bar baz"], 1, 1, "/bar/e-1<CR>"),
    c("search:/pat/b+2", &["foo bar baz"], 1, 1, "/bar/b+2<CR>"),
    c("search:/pat/s-1", &["foo bar baz"], 1, 1, "/bar/s-1<CR>"),
    c(
        "search:/pat/+1 linewise",
        &["a", "foo", "b", "c"],
        1,
        1,
        "/foo/+1<CR>",
    ),
    c("search:/pat/-1", &["a", "b", "foo"], 1, 1, "/foo/-1<CR>"),
    c(
        "search:/pat/e then n keeps offset",
        &["foo bar foo bar"],
        1,
        1,
        "/bar/e<CR>n",
    ),
    c("search:/\\v", &["fooo bar"], 1, 3, "/\\vo+<CR>"),
    c("search:/foo\\|bar", &["xx bar foo"], 1, 1, "/foo\\|bar<CR>"),
    c("search:/\\<foo\\>", &["foobar foo"], 1, 1, "/\\<foo\\><CR>"),
    c("search:/^foo", &["a foo", "foo b"], 1, 1, "/^foo<CR>"),
    c("search:/foo$", &["foo a", "a foo"], 1, 1, "/foo$<CR>"),
    c("search:/a\\{2}", &["a aa aaa"], 1, 1, "/a\\{2}<CR>"),
    c("search:/\\d\\+", &["ab 123 cd"], 1, 1, "/\\d\\+<CR>"),
    c("search:/[bc]a", &["xa ba ca"], 1, 1, "/[bc]a<CR>"),
    c("search:ic", &["x FOO foo"], 1, 1, ":set ic<CR>/foo<CR>"),
    c("search:noic", &["x FOO foo"], 1, 1, "/foo<CR>"),
    c(
        "search:scs upper",
        &["x FOO Foo foo"],
        1,
        1,
        ":set ic scs<CR>/Foo<CR>",
    ),
    c(
        "search:scs lower",
        &["x FOO Foo foo"],
        1,
        1,
        ":set ic scs<CR>/foo<CR>",
    ),
    c("search:\\c", &["x FOO foo"], 1, 1, "/\\cfoo<CR>"),
    c(
        "search:\\C with ic",
        &["x foo FOO"],
        1,
        1,
        ":set ic<CR>/\\CFOO<CR>",
    ),
    c(
        "search:* with ic scs",
        &["Foo foo Foo"],
        1,
        1,
        ":set ic scs<CR>*",
    ),
    c(
        "search:// repeat",
        &["foo x foo x foo"],
        1,
        1,
        "/foo<CR>//<CR>",
    ),
    c(
        "search:/<CR> repeat",
        &["foo x foo x foo"],
        1,
        1,
        "/foo<CR>/<CR>",
    ),
    c("search:wrap forward", &["foo", "x"], 1, 1, "/foo<CR>"),
    c("search:wrap backward", &["x", "foo"], 1, 1, "?foo<CR>"),
    // #1153 'wrapscan' off — no match ahead of the cursor, stay put instead
    // of wrapping. Verified against `nvim --headless`.
    c(
        "search:nowrapscan forward stays put",
        &["foo", "x"],
        1,
        1,
        ":set nowrapscan<CR>/foo<CR>",
    ),
    c(
        "search:nowrapscan backward stays put",
        &["x", "foo"],
        2,
        1,
        ":set nowrapscan<CR>?foo<CR>",
    ),
    c("search:no match", &["abc"], 1, 2, "/zzz<CR>"),
    c("search:n after *", &["foo x foo x foo"], 1, 1, "*n"),
    c("search:3/pat", &["a foo foo foo"], 1, 1, "3/foo<CR>"),
    c("search:2n", &["a foo foo foo"], 1, 1, "/foo<CR>2n"),
    c(
        "search:/pat/;/pat2/",
        &["a foo b bar"],
        1,
        1,
        "/foo/;/bar<CR>",
    ),
    c("search:?pat?e", &["foo bar baz"], 1, 11, "?bar?e<CR>"),
    c("search:c/pat", &["foo bar baz"], 1, 1, "c/baz<CR>X<Esc>"),
    c("search:y/pat cursor", &["foo bar baz"], 1, 5, "y/baz<CR>"),
    c("search:/ from mid-match", &["foofoo"], 1, 2, "/foo<CR>"),
    c("search:? from mid", &["foofoo"], 1, 5, "?foo<CR>"),
    c(
        "search:/o\\nb multiline",
        &["foo", "bar"],
        1,
        1,
        "/o\\nb<CR>",
    ),
    c(
        "search:/ then :s//",
        &["foo bar foo"],
        1,
        1,
        "/foo<CR>:s//X/g<CR>",
    ),
    c("search:\\zs", &["foobar"], 1, 1, "/foo\\zsbar<CR>"),
    c("search:\\ze", &["xbar foobar"], 1, 1, "/foo\\zebar<CR>"),
    c("search:/. literal dot", &["abc a.c"], 1, 1, "/a\\.c<CR>"),
    c(
        "search:/ with * quantifier",
        &["ac abc abbc"],
        1,
        1,
        "/ab*c<CR>",
    ),
    c("search:/ with \\s", &["a\tb c"], 1, 1, "/\\s<CR>"),
    c("search:\\V nomagic", &["a.c abc"], 1, 1, "/\\Va.c<CR>"),
    c("search:/\\%V? skip", &["abc"], 1, 1, "l"),
    c(
        "search:? n then N",
        &["bar", "foo", "bar", "bar"],
        4,
        1,
        "?bar<CR>nN",
    ),
    c(
        "search:/ then ? then n",
        &["bar", "foo", "bar", "bar"],
        1,
        1,
        "/bar<CR>?bar<CR>n",
    ),
    c(
        "search:d/pat/+0? linewise",
        &["a", "foo", "b"],
        1,
        1,
        "d/foo/0<CR>",
    ),
    c(
        "search:/\\(foo\\)\\1",
        &["foo foofoo"],
        1,
        1,
        "/\\(foo\\)\\1<CR>",
    ),
    // #1157: `\@=` look-ahead — only the "foo" immediately followed by
    // "bar" qualifies; the lookahead is zero-width, so the cursor lands on
    // that "foo"'s own 'f', not inside/after "bar".
    c(
        "search:/\\@= lookahead",
        &["xx foobar foobaz"],
        1,
        1,
        "/foo\\(bar\\)\\@=<CR>",
    ),
    // #1157: `\_s` — a whitespace class that also accepts end-of-line, so
    // the pattern spans the newline between the two lines. Cursor starts on
    // line 2 so the search has to wrap around to land on line 1.
    c(
        "search:/\\_s spans lines",
        &["foo", "bar"],
        2,
        1,
        "/foo\\_sbar<CR>",
    ),
    c(
        "search:/\\w\\+ from col1",
        &["foo bar"],
        1,
        1,
        "/\\w\\+<CR>",
    ),
    c("search:/$ empty match", &["ab", "cd"], 1, 1, "/$<CR>"),
    c("search:/^ empty match", &["ab", "cd"], 1, 1, "/^<CR>"),
    c("search:/\\n at eol", &["ab", "cd"], 1, 1, "/\\n<CR>"),
    c(
        "search:/ upper V with ic",
        &["abc ABC"],
        1,
        1,
        ":set ic<CR>/ABC<CR>",
    ),
    c("search:* on number", &["12 x 12"], 1, 1, "*"),
    c(
        "search:* on word with underscore",
        &["a_b x a_b"],
        1,
        1,
        "*",
    ),
    c("search:gd", &["int x = 1;", "y = x;"], 2, 5, "gd"),
    c(
        "search:gn selects",
        &["foo bar foo"],
        1,
        1,
        "/foo<CR>ggcgnX<Esc>",
    ),
    c(
        "search:cgn .",
        &["foo bar foo baz foo"],
        1,
        1,
        "/foo<CR>ggcgnX<Esc>..",
    ),
    c("search:dgn", &["foo bar foo"], 1, 1, "/foo<CR>ggdgn"),
    c("search:gN", &["foo bar foo"], 1, 11, "/foo<CR>gNd"),
    // #1153 review: `gn` is documented as "like the `n` command", so it
    // should honour 'wrapscan' the same way `n`/`N` already do. Both cases
    // leave the cursor past the last "alpha" (`*$j`), with no further match
    // ahead of it — verified against `nvim --headless`.
    c(
        "search:gn respects nowrapscan",
        &["alpha xxx alpha", "yyy"],
        1,
        1,
        "*$j:set nowrapscan<CR>gn<Esc>",
    ),
    c(
        "search:gn wraps when wrapscan on",
        &["alpha xxx alpha", "yyy"],
        1,
        1,
        "*$jgn<Esc>",
    ),
    // #1191: `*`/`#` (`word_under_cursor`/`star_word_under_cursor`) are the
    // other documented consumer of 'iskeyword' — with `-` added to the
    // keyword class, "foo-bar" is the whole word `*` searches for, so it
    // finds the *next* "foo-bar" run rather than stopping mid-token.
    cs(
        "search:* with iskeyword+=- includes hyphen in the searched word",
        &["foo-bar foo-bar"],
        1,
        1,
        "*",
        "vim.o.iskeyword='@,48-57,_,192-255,-'",
    ),
];

// ─────────────────────────── H. :s / :g / ex ───────────────────────────
const CASES_EX: &[Case] = &[
    c("sub:basic", &["a a"], 1, 1, ":s/a/b/<CR>"),
    c("sub:g", &["a a"], 1, 1, ":s/a/b/g<CR>"),
    // #1153 'gdefault' — inverts the meaning of the `g` flag. Verified
    // against `nvim --headless`.
    c(
        "sub:gdefault makes plain sub global",
        &["a a a"],
        1,
        1,
        ":set gdefault<CR>:s/a/x/<CR>",
    ),
    c(
        "sub:gdefault g flag toggles back to first-only",
        &["a a a"],
        1,
        1,
        ":set gdefault<CR>:s/a/x/g<CR>",
    ),
    c("sub:%", &["a", "a", "a"], 1, 1, ":%s/a/b/<CR>"),
    c("sub:%g cursor", &["a a", "b", "a a"], 2, 1, ":%s/a/x/g<CR>"),
    c("sub:2,3", &["a", "a", "a", "a"], 1, 1, ":2,3s/a/b/<CR>"),
    c("sub:.,+1", &["a", "a", "a", "a"], 2, 1, ":.,+1s/a/b/<CR>"),
    c("sub:.,$", &["a", "a", "a", "a"], 3, 1, ":.,$s/a/b/<CR>"),
    c(
        "sub:'a,'b",
        &["a", "a", "a", "a"],
        1,
        1,
        "majjmbgg:'a,'bs/a/b/<CR>",
    ),
    c(
        "sub:'<,'> auto",
        &["a", "a", "a", "a"],
        2,
        1,
        "Vj:s/a/b/<CR>",
    ),
    c(
        "sub:'<,'> explicit",
        &["a", "a", "a", "a"],
        2,
        1,
        "Vj<Esc>:'<,'>s/a/b/<CR>",
    ),
    c("sub:i flag", &["A a"], 1, 1, ":s/a/b/gi<CR>"),
    c(
        "sub:I flag with ic",
        &["A a"],
        1,
        1,
        ":set ic<CR>:s/a/b/gI<CR>",
    ),
    c("sub:ic applies", &["A a"], 1, 1, ":set ic<CR>:s/a/b/g<CR>"),
    c("sub:n flag", &["a a"], 1, 1, ":s/a/b/gn<CR>"),
    c("sub:e flag", &["a"], 1, 1, ":s/z/b/e<CR>"),
    c(
        "sub:& flag",
        &["a a", "a a"],
        1,
        1,
        ":s/a/b/g<CR>j:s/a/c/&<CR>",
    ),
    c("sub:&&", &["a a", "a a"], 1, 1, ":s/a/b/g<CR>j:&&<CR>"),
    c("sub:& cmd", &["a a", "a a"], 1, 1, ":s/a/b/g<CR>j:&<CR>"),
    c(
        "sub:backrefs",
        &["ab"],
        1,
        1,
        ":s/\\(a\\)\\(b\\)/\\2\\1/<CR>",
    ),
    c("sub:\\v groups", &["ab"], 1, 1, ":s/\\v(a)(b)/\\2\\1/<CR>"),
    // #1004: `\1` *inside the pattern* (a backreference to an earlier
    // `\(...\)` group in the same pattern) — distinct from "sub:backrefs"
    // above, which is `\1`/`\2` in the *replacement* text referencing groups.
    c(
        "sub:pattern backref",
        &["xx aa yy"],
        1,
        1,
        ":s/\\(a\\)\\1/X/<CR>",
    ),
    c(
        "sub:pattern backref no match",
        &["xx ab yy"],
        1,
        1,
        ":s/\\(a\\)\\1/X/<CR>",
    ),
    c("sub:& in replacement", &["foo"], 1, 1, ":s/foo/[&]/<CR>"),
    c("sub:\\0", &["foo"], 1, 1, ":s/foo/[\\0]/<CR>"),
    c("sub:\\U&", &["foo"], 1, 1, ":s/foo/\\U&/<CR>"),
    c("sub:\\u&", &["foo"], 1, 1, ":s/foo/\\u&/<CR>"),
    c("sub:\\L", &["FOO"], 1, 1, ":s/FOO/\\L&/<CR>"),
    c("sub:\\U..\\E", &["foo bar"], 1, 1, ":s/foo/\\U&\\E-x/<CR>"),
    c("sub:\\r newline", &["a,b"], 1, 1, ":s/,/\\r/<CR>"),
    c("sub:\\t", &["a b"], 1, 1, ":s/ /\\t/<CR>"),
    c("sub:alternation", &["a b c"], 1, 1, ":s/a\\|c/x/g<CR>"),
    c("sub:\\zs", &["foobar"], 1, 1, ":s/foo\\zsbar/X/<CR>"),
    c("sub:\\ze", &["foobar"], 1, 1, ":s/foo\\zebar/X/<CR>"),
    // #1157: `\@=` in a `:s` pattern — the lookahead is zero-width, so only
    // "foo" is replaced and "bar" survives untouched.
    c(
        "sub:\\@= lookahead",
        &["foobar foobaz"],
        1,
        1,
        ":s/foo\\(bar\\)\\@=/X/<CR>",
    ),
    // #1157: `\_s` in a `:s` pattern — matches the newline between the two
    // lines, so the substitution merges them into one.
    c(
        "sub:\\_s spans lines",
        &["foo", "bar"],
        1,
        1,
        ":s/foo\\_sbar/X/<CR>",
    ),
    c(
        "sub:~ prev replacement",
        &["a b"],
        1,
        1,
        ":s/a/x/<CR>:s/b/~y/<CR>",
    ),
    c("sub:# delimiter", &["a/b"], 1, 1, ":s#/#-#<CR>"),
    c("sub:empty replacement", &["abc"], 1, 1, ":s/b//<CR>"),
    c("sub:no trailing slash", &["abc"], 1, 1, ":s/b/X<CR>"),
    c("sub:no replacement", &["abc"], 1, 1, ":s/b<CR>"),
    c(
        "sub:empty pattern last search",
        &["foo bar foo"],
        1,
        1,
        "/foo<CR>:s//X/g<CR>",
    ),
    c("sub:count", &["a", "a", "a", "a"], 1, 1, ":s/a/b/ 2<CR>"),
    c(
        "sub:range + count",
        &["a", "a", "a", "a"],
        1,
        1,
        ":2s/a/b/ 2<CR>",
    ),
    c(
        "sub:trailing ws",
        &["a  ", "b "],
        1,
        1,
        ":%s/\\s\\+$//e<CR>",
    ),
    c("sub:^ anchor", &["ab", "cd"], 1, 1, ":%s/^/> /<CR>"),
    c("sub:$ anchor", &["ab", "cd"], 1, 1, ":%s/$/;/<CR>"),
    c("sub:.*", &["abc"], 1, 1, ":s/.*/[&]/<CR>"),
    c("sub:literal dot", &["a.b.c"], 1, 1, ":s/\\./,/g<CR>"),
    c("sub:bar chain", &["a b"], 1, 1, ":s/a/x/|s/b/y/<CR>"),
    c("sub:cursor after", &["x", "a b a"], 1, 1, ":2s/a/y/<CR>"),
    c("sub:no match keeps buffer", &["abc"], 1, 1, ":s/z/y/<CR>"),
    c("sub:\\n multiline", &["a", "b"], 1, 1, ":%s/a\\nb/X/<CR>"),
    c("sub:\\{2}", &["aaa"], 1, 1, ":s/a\\{2}/X/<CR>"),
    c("sub:[] class", &["abc"], 1, 1, ":s/[ac]/X/g<CR>"),
    c("sub:\\w\\+", &["foo bar"], 1, 1, ":s/\\w\\+/X/g<CR>"),
    c("sub:\\< \\>", &["foo foobar"], 1, 1, ":s/\\<foo\\>/X/g<CR>"),
    c("sub:\\{-}", &["aaa"], 1, 1, ":s/a\\{-1,}/X/<CR>"),
    c(
        "sub:\\u\\1 swap words",
        &["foo bar"],
        1,
        1,
        ":s/\\(\\w\\+\\) \\(\\w\\+\\)/\\u\\2 \\u\\1/<CR>",
    ),
    c(
        "sub:%s cursor at end",
        &["a", "b", "a"],
        1,
        1,
        ":%s/a/x/<CR>",
    ),
    c("sub:s on empty match ^", &["abc"], 1, 1, ":s/^/x/g<CR>"),
    c("sub:\\= not vimscript skip", &["a"], 1, 1, "l"),
    c("sub:$ anchor g", &["ab"], 1, 1, ":s/$/;/g<CR>"),
    c("sub:x* g on empty", &["abc"], 1, 1, ":s/x*/-/g<CR>"),
    c(
        "sub:& with \\n in pattern",
        &["a", "b", "c"],
        1,
        1,
        ":%s/\\n//<CR>",
    ),
    c(
        "sub:\\r in middle then cursor",
        &["abc"],
        1,
        1,
        ":s/b/\\r/<CR>",
    ),
    c("sub:\\= escaped slash", &["a/b"], 1, 1, ":s/\\//-/<CR>"),
    c("sub:\\/ in replacement", &["a-b"], 1, 1, ":s/-/\\//<CR>"),
    c("sub:& literal via \\&", &["foo"], 1, 1, ":s/foo/\\&/<CR>"),
    c("sub:~ literal via \\~", &["a"], 1, 1, ":s/a/\\~/<CR>"),
    c(
        "sub:whole line",
        &["hello world"],
        1,
        1,
        ":s/\\v(\\w+) (\\w+)/\\2 \\1/<CR>",
    ),
    // #1031 (#801 Phase 2): `:s///c` confirm loop. Verified against a real
    // interactive `nvim --headless --listen` + `--remote-send` session
    // (v0.12.5). The non-interactive `-es` batch mode the suite's other cases
    // use silently short-circuits `:s///c` (the confirm prompt never
    // engages), so these were hand-checked outside `cargo test` before being
    // added here.
    c(
        "sub:c y n y y",
        &["a", "a", "a", "a"],
        1,
        1,
        ":%s/a/x/gc<CR>ynyy",
    ),
    c(
        "sub:c decline all",
        &["a", "a", "a", "a"],
        1,
        1,
        ":%s/a/x/gc<CR>nnnn",
    ),
    c(
        "sub:c quit early",
        &["a", "a", "a", "a"],
        1,
        1,
        ":%s/a/x/gc<CR>y<Esc>",
    ),
    c(
        "sub:c 'a' fills remaining",
        &["a", "a", "a", "a"],
        1,
        1,
        ":%s/a/x/gc<CR>na",
    ),
    c(
        "sub:c 'l' replaces then quits",
        &["a", "a", "a", "a"],
        1,
        1,
        ":%s/a/x/gc<CR>yl",
    ),
    c("g:d", &["a", "b", "a", "c"], 1, 1, ":g/a/d<CR>"),
    c("g:s", &["a x", "b x", "a x"], 1, 1, ":g/a/s/x/y/<CR>"),
    c("g:!", &["a", "b", "a", "c"], 1, 1, ":g!/a/d<CR>"),
    c("g:v", &["a", "b", "a", "c"], 1, 1, ":v/a/d<CR>"),
    c("g:normal", &["a", "b", "a"], 1, 1, ":g/a/normal Ax<CR>"),
    c("g:m0 reverse", &["1", "2", "3"], 1, 1, ":g/^/m0<CR>"),
    c("g:^$ d", &["a", "", "b", "", ""], 1, 1, ":g/^$/d<CR>"),
    c("g:t$", &["a", "b"], 1, 1, ":g/a/t$<CR>"),
    c(
        "g:cursor after",
        &["a", "b", "a", "c"],
        1,
        1,
        ":g/a/s/a/x/<CR>",
    ),
    c("g:j", &["a", "b", "a", "b"], 1, 1, ":g/a/j<CR>"),
    c("g:range", &["a", "a", "a"], 1, 1, ":2,3g/a/s/a/b/<CR>"),
    c("g:delimiter", &["a", "b"], 1, 1, ":g#a#d<CR>"),
    c(
        "g:normal dd",
        &["a", "b", "a", "c"],
        1,
        1,
        ":g/a/normal dd<CR>",
    ),
    c("g:+1d", &["a", "x", "a", "y"], 1, 1, ":g/a/+1d<CR>"),
    c(
        "g:s// reuse pattern",
        &["a", "b", "a"],
        1,
        1,
        ":g/a/s//x/<CR>",
    ),
    c(
        "g:normal with count",
        &["ab", "cd"],
        1,
        1,
        ":g/./normal 2x<CR>",
    ),
    c(
        "g:copy to end reversed order",
        &["a", "b"],
        1,
        1,
        ":g/./t.<CR>",
    ),
    c(
        "g:d with count",
        &["a", "1", "2", "b", "3", "4"],
        1,
        1,
        ":g/[ab]/d 2<CR>",
    ),
    c(
        "g:normal @a",
        &["a", "b", "a"],
        1,
        1,
        "qaAx<Esc>qu:g/a/normal @a<CR>",
    ),
    c("g:.,+1j", &["a", "b", "c", "d"], 1, 1, ":g/a\\|c/.,+1j<CR>"),
    c("ex:t.", &["a", "b"], 1, 1, ":t.<CR>"),
    c("ex:t0", &["a", "b"], 2, 1, ":t0<CR>"),
    c("ex:t$", &["a", "b"], 1, 1, ":t$<CR>"),
    c("ex:2t0", &["a", "b", "c"], 1, 1, ":2t0<CR>"),
    c("ex:m0", &["a", "b", "c"], 3, 1, ":m0<CR>"),
    c("ex:m$", &["a", "b", "c"], 1, 1, ":m$<CR>"),
    c("ex:m+1", &["a", "b", "c"], 1, 1, ":m+1<CR>"),
    c("ex:m-2", &["a", "b", "c"], 3, 1, ":m-2<CR>"),
    c("ex:2,3m0", &["a", "b", "c", "d"], 1, 1, ":2,3m0<CR>"),
    c("ex:2,3m$", &["a", "b", "c", "d"], 1, 1, ":2,3m$<CR>"),
    c("ex:1,2t$", &["a", "b", "c"], 1, 1, ":1,2t$<CR>"),
    c("ex:1co$", &["a", "b"], 1, 1, ":1co$<CR>"),
    c("ex:d", &["a", "b", "c"], 2, 1, ":d<CR>"),
    c("ex:2d", &["a", "b", "c"], 1, 1, ":2d<CR>"),
    c("ex:2,3d", &["a", "b", "c"], 1, 1, ":2,3d<CR>"),
    c("ex:d a then \"ap", &["a", "b", "c"], 1, 1, ":2d a<CR>\"ap"),
    c("ex:d 2", &["a", "b", "c"], 1, 1, ":d 2<CR>"),
    c("ex:2,3y p", &["a", "b", "c"], 1, 1, ":2,3y<CR>p"),
    c("ex:y a", &["a", "b"], 1, 1, ":y a<CR>j\"ap"),
    c("ex:pu", &["a", "b"], 1, 1, "yy:pu<CR>"),
    c("ex:pu!", &["a", "b"], 1, 1, "yy:pu!<CR>"),
    c("ex:put a", &["a", "b"], 1, 1, "\"ayy:put a<CR>"),
    c("ex:2put", &["a", "b", "c"], 1, 1, "yy:2put<CR>"),
    c("ex:0put", &["a", "b"], 2, 1, "yy:0put<CR>"),
    c("ex:put charwise reg", &["ab", "c"], 1, 1, "yl:put<CR>"),
    c("ex:j", &["a", "b", "c"], 1, 1, ":j<CR>"),
    c("ex:1,3j", &["a", "b", "c"], 1, 1, ":1,3j<CR>"),
    c("ex:j!", &["a", "  b"], 1, 1, ":j!<CR>"),
    c("ex:j 3", &["a", "b", "c", "d"], 1, 1, ":j 3<CR>"),
    c("ex:>", &["a"], 1, 1, ":><CR>"),
    c("ex:>>", &["a"], 1, 1, ":>><CR>"),
    c("ex:2,3>", &["a", "b", "c"], 1, 1, ":2,3><CR>"),
    c("ex:<", &["        a"], 1, 1, ":<<CR>"),
    c("ex:> 2", &["a", "b", "c"], 1, 1, ":> 2<CR>"),
    c("ex:sort", &["c", "a", "b"], 1, 1, ":sort<CR>"),
    c("ex:sort!", &["c", "a", "b"], 1, 1, ":sort!<CR>"),
    c("ex:sort n", &["10", "9", "100"], 1, 1, ":sort n<CR>"),
    c("ex:sort u", &["b", "a", "b"], 1, 1, ":sort u<CR>"),
    c("ex:sort i", &["b", "A", "a"], 1, 1, ":sort i<CR>"),
    c(
        "ex:sort mixed case",
        &["b", "A", "a", "B"],
        1,
        1,
        ":sort<CR>",
    ),
    c("ex:2,3sort", &["c", "b", "a"], 1, 1, ":2,3sort<CR>"),
    c(
        "ex:sort /pat/",
        &["x2 b", "x1 a"],
        1,
        1,
        ":sort /x\\d /<CR>",
    ),
    c(
        "ex:sort /pat/ r",
        &["b 2", "a 1"],
        1,
        1,
        ":sort /\\d/ r<CR>",
    ),
    c(
        "ex:sort n non-numbers first",
        &["b", "2", "a", "1"],
        1,
        1,
        ":sort n<CR>",
    ),
    c("ex:sort cursor", &["c", "a", "b"], 3, 1, ":sort<CR>"),
    c("ex:retab", &["\ta"], 1, 1, ":set ts=4<CR>:retab<CR>"),
    c(
        "ex:retab!",
        &["    a"],
        1,
        1,
        ":set noet ts=4<CR>:retab!<CR>",
    ),
    c("ex:retab 2", &["\ta"], 1, 1, ":set ts=4<CR>:retab 2<CR>"),
    c("ex:undo", &["ab"], 1, 1, "x:undo<CR>"),
    c("ex:undo redo", &["ab"], 1, 1, "x:undo<CR>:redo<CR>"),
    c("ex:5", &["1", "2", "3", "4", "5", "6"], 1, 1, ":5<CR>"),
    c("ex:$", &["1", "2", "3"], 1, 1, ":$<CR>"),
    c("ex:+2", &["1", "2", "3", "4"], 1, 1, ":+2<CR>"),
    c("ex:-1", &["1", "2", "3"], 3, 1, ":-1<CR>"),
    c("ex:/foo/", &["a", "b", "foo"], 1, 1, ":/foo/<CR>"),
    c("ex:?a?", &["a", "b", "c"], 3, 1, ":?a?<CR>"),
    c("ex:/foo/+1", &["a", "foo", "b"], 1, 1, ":/foo/+1<CR>"),
    c("ex:/foo/d", &["a", "foo", "b"], 1, 1, ":/foo/d<CR>"),
    c(
        "ex:/a/,/b/d",
        &["x", "a", "y", "b", "z"],
        1,
        1,
        ":/a/,/b/d<CR>",
    ),
    c(
        "ex:.,/foo/d",
        &["a", "b", "foo", "c"],
        1,
        1,
        ":.,/foo/d<CR>",
    ),
    c("ex:%d", &["a", "b"], 1, 1, ":%d<CR>"),
    c("ex:%j", &["a", "b", "c"], 1, 1, ":%j<CR>"),
    c("ex:2ka 'a", &["a", "b", "c"], 1, 1, ":2ka<CR>'a"),
    c("ex:2mark a", &["a", "b", "c"], 1, 1, ":2mark a<CR>'a"),
    c("ex:le", &["    a"], 1, 1, ":le<CR>"),
    c("ex:le 4", &["a"], 1, 1, ":le 4<CR>"),
    c("ex:ri 10", &["a"], 1, 1, ":ri 10<CR>"),
    c("ex:ce 10", &["a"], 1, 1, ":ce 10<CR>"),
    c("ex:normal Ax", &["a", "b"], 1, 1, ":normal Ax<CR>"),
    c("ex:%normal Ax", &["a", "b"], 1, 1, ":%normal Ax<CR>"),
    c(
        "ex:2,3normal I-",
        &["a", "b", "c"],
        1,
        1,
        ":2,3normal I-<CR>",
    ),
    c("ex:normal 2x", &["abcd"], 1, 1, ":normal 2x<CR>"),
    c("ex:normal! Ax", &["a"], 1, 1, ":normal! Ax<CR>"),
    c("ex:normal incomplete", &["ab"], 1, 1, ":normal d<CR>"),
    c("ex:normal cursor", &["abc", "def"], 1, 1, ":2normal $<CR>"),
    c("ex:r !echo", &["a"], 1, 1, ":r !echo hi<CR>"),
    c("ex:%!sort", &["b", "a"], 1, 1, ":%!sort<CR>"),
    c("ex:2;+1d", &["a", "b", "c", "d"], 1, 1, ":2;+1d<CR>"),
    c("ex:2,+1d", &["a", "b", "c", "d"], 1, 1, ":2,+1d<CR>"),
    c("ex:$-1d", &["a", "b", "c"], 1, 1, ":$-1d<CR>"),
    c("ex:.+2", &["1", "2", "3", "4"], 1, 1, ":.+2<CR>"),
    c("ex:cursor after :t$", &["a", "b"], 1, 1, ":t$<CR>"),
    c("ex:cursor after :m0", &["a", "b", "c"], 3, 1, ":m0<CR>"),
    c("ex:cursor after :2d", &["a", "b", "c"], 1, 1, ":2d<CR>"),
    c("ex:cursor after :>", &["  a"], 1, 2, ":><CR>"),
    c("ex:cursor after :j", &["a", "b", "c"], 1, 1, ":j<CR>"),
    c(
        "ex:cursor after :%normal",
        &["a", "b", "c"],
        1,
        1,
        ":%normal Ax<CR>",
    ),
    c(
        "ex:cursor after :g/d",
        &["a", "b", "a", "c"],
        1,
        1,
        ":g/a/d<CR>",
    ),
    c("ex:1,2co0", &["a", "b", "c"], 1, 1, ":1,2co0<CR>"),
    c("ex:%y then P", &["a", "b"], 2, 1, ":%y<CR>P"),
    c("ex:d _", &["a", "b"], 1, 1, "yyj:d _<CR>p"),
    c("ex:y A append", &["a", "b"], 1, 1, ":y a<CR>j:y A<CR>\"ap"),
    c("ex:.,.+1d", &["a", "b", "c"], 1, 1, ":.,.+1d<CR>"),
    c(
        "ex:'<,'>d after v",
        &["a", "b", "c"],
        1,
        1,
        "vj<Esc>:'<,'>d<CR>",
    ),
    c(
        "ex:*d after visual",
        &["a", "b", "c"],
        1,
        1,
        "Vj<Esc>:*d<CR>",
    ),
    c("ex:g/pat/normal cgn? skip", &["a"], 1, 1, "l"),
    c("ex:s with c flag skipped", &["a"], 1, 1, "l"),
    c(
        "ex:3 goes col firstnonblank",
        &["a", "b", "  c"],
        1,
        1,
        ":3<CR>",
    ),
    c("ex:0", &["a", "b", "c"], 3, 1, ":0<CR>"),
    c("ex:%s then n", &["a", "b", "a"], 1, 1, ":%s/a/x/<CR>ggn"),
    c(
        "ex:s sets last search for n",
        &["a", "b", "a"],
        1,
        1,
        ":s/a/x/<CR>n",
    ),
    c("ex:s cursor col", &["xx a"], 1, 4, ":s/a/b/<CR>"),
    c(
        "ex:%s/x/y/g with \\r cursor",
        &["a,b,c"],
        1,
        1,
        ":s/,/\\r/g<CR>",
    ),
    c("ex:noh no effect", &["a"], 1, 1, "/a<CR>:noh<CR>"),
    c(
        "ex:2>3? shift count",
        &["a", "b", "c", "d"],
        1,
        1,
        ":2> 2<CR>",
    ),
    c("ex:< 2", &["    a", "    b", "    c"], 1, 1, ":< 2<CR>"),
    c("ex:>>> 3 levels", &["a"], 1, 1, ":>>><CR>"),
    c(
        "ex:j with range and count",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        ":2j 3<CR>",
    ),
    c("ex:d x then \"xP", &["a", "b"], 1, 1, ":d x<CR>\"xP"),
    c("ex:m with 0 on first", &["a", "b"], 1, 1, ":m0<CR>"),
    c("ex:m to self", &["a", "b"], 2, 1, ":m2<CR>"),
    c(
        "ex:t with range dest .",
        &["a", "b", "c"],
        3,
        1,
        ":1,2t.<CR>",
    ),
    c(
        "ex:s on visual block ranges",
        &["a a", "a a", "a a"],
        1,
        1,
        "<C-v>j:s/a/b/<CR>",
    ),
    // ── #986: v0.11.0 bug suite -- `:s///c` confirm-prompt spec (#801
    // Phase 2 never built). Every "sub:c ..." case below is listed in
    // KNOWN_DEVIATIONS: `execute.rs`'s `flags.contains('c')` check always
    // errors loudly instead of entering a confirm loop today, so the `c`
    // flag's follow-up keystrokes (`y`/`n`/`a`/`q`/`l`/`<Esc>`) land on
    // whatever ordinary Normal-mode command they happen to spell on the
    // vimcode side (an operator-pending `y`, a `q` macro-record start,
    // etc.) instead of driving a confirm loop -- never a crash, just a
    // guaranteed buffer/cursor mismatch against the real oracle. See this
    // issue's PR description for the full confirm contract as captured
    // from a real `nvim --headless` v0.12.5 (the exact prompt text, and
    // this table), which every case below exercises:
    //
    //   y        replace this match, continue
    //   n        skip this match, continue
    //   a        replace this and all remaining matches
    //   q        quit substituting (this match left unreplaced)
    //   l        replace this match, then quit ("last")
    //   <Esc>    quit substituting (this match left unreplaced)
    //
    // `^E`/`^Y` (scroll the window while the prompt is up) are not covered
    // here: fed directly at a real headless oracle they don't hang it, but
    // they produce no buffer/cursor difference for *this* harness to
    // observe (window scroll position isn't part of what `run_case`
    // compares) -- confirmed by direct probe, not assumed. An honest gap,
    // same treatment as the #805 headless-scroll exclusions above.
    c(
        "sub:c y accepts each prompted match (g)",
        &["abc abc", "abc", "xyz abc"],
        1,
        1,
        ":%s/abc/def/gc<CR>yyyy",
    ),
    c(
        "sub:c n skips each prompted match (g)",
        &["abc abc", "abc", "xyz abc"],
        1,
        1,
        ":%s/abc/def/gc<CR>nnnn",
    ),
    c(
        "sub:c a accepts this and all remaining (g)",
        &["abc abc", "abc", "xyz abc"],
        1,
        1,
        ":%s/abc/def/gc<CR>a",
    ),
    c(
        "sub:c q quits after partial replace (g)",
        &["abc abc", "abc", "xyz abc"],
        1,
        1,
        ":%s/abc/def/gc<CR>yq",
    ),
    c(
        "sub:c l replaces then quits (g)",
        &["abc abc", "abc", "xyz abc"],
        1,
        1,
        ":%s/abc/def/gc<CR>yl",
    ),
    c(
        "sub:c Esc quits after partial replace (g)",
        &["abc abc", "abc", "xyz abc"],
        1,
        1,
        ":%s/abc/def/gc<CR>y<Esc>",
    ),
    // Deliverable 3 (cursor after quitting mid-substitute): the match is
    // *not* at the cursor's starting column, so these two pin that a real
    // `:%s///gc` moves the cursor onto the first candidate match before
    // ever prompting -- quitting immediately (no replacement made at all)
    // still leaves the cursor there, not at the start position. Confirmed
    // against the oracle: nvim reports cursor (1,5) here (the "abc" inside
    // "xxx abc abc"), not (1,1).
    c(
        "sub:c q quits before any replace, cursor at match (g)",
        &["xxx abc abc", "abc", "xyz abc"],
        1,
        1,
        ":%s/abc/def/gc<CR>q",
    ),
    c(
        "sub:c Esc quits before any replace, cursor at match (g)",
        &["xxx abc abc", "abc", "xyz abc"],
        1,
        1,
        ":%s/abc/def/gc<CR><Esc>",
    ),
    // Without the `g` flag: confirm still prompts once per line (only the
    // line's first match), not once for the whole buffer.
    c(
        "sub:c y accepts first match per line (no g)",
        &["abc abc", "abc", "xyz abc"],
        1,
        1,
        ":%s/abc/def/c<CR>yyy",
    ),
    c(
        "sub:c n then y across lines (no g)",
        &["abc abc", "abc", "xyz abc"],
        1,
        1,
        ":%s/abc/def/c<CR>nyy",
    ),
    // Skip-then-accept across the whole buffer (also drives the
    // report-line-excludes-skipped-matches contract pinned via a rendered
    // `screen_has` check in `src/harness.rs`'s
    // `confirm_report_line_excludes_skipped_matches` -- this case covers
    // the buffer/cursor half, that one covers the "3 substitutions on 3
    // lines" report text specifically).
    c(
        "sub:c skip counted correctly across buffer (g)",
        &["abc", "abc", "abc", "abc"],
        1,
        1,
        ":%s/abc/def/gc<CR>ynyy",
    ),
    // Regression guard (deliverable 4): plain `:s` with no `c` flag must be
    // completely unaffected by this issue -- same fixture as the `y`/`n`
    // cases above, but no `c` in the flags. NOT listed in
    // KNOWN_DEVIATIONS: this already passes today and must keep passing.
    c(
        "sub:c plain :s unaffected by confirm gate (regression guard)",
        &["abc abc", "abc", "xyz abc"],
        1,
        1,
        ":%s/abc/def/g<CR>",
    ),
    // #1154: bare `:cc` (no count) on an empty quickfix list. This is the
    // one `:cc`-family behaviour this single-buffer harness can actually
    // compare: `nvim_buf_set_lines` gives both sides an *unnamed* scratch
    // buffer with no backing file, so there is no way to seed the two sides
    // with an identical *populated* quickfix list (real Neovim needs
    // `:vimgrep`-on-a-real-file or `:cexpr`, and vimcode's expression
    // evaluator is explicitly out of scope for this milestone — see the
    // module docs' "Coverage ratchet" section and #1154's own PR
    // description). But the empty-list error path needs no quickfix state
    // at all: confirmed by hand against `nvim --headless -u NONE` that a
    // bare `:cc` with no quickfix list leaves the buffer and cursor
    // completely untouched (it errors — `E42: No Errors` — before doing
    // anything), which is exactly what vimcode's new bare-`:cc` handler
    // does too (`execute.rs`'s `cmd == "cc"` arm, `self.quickfix.items.is_empty()`
    // branch). Before #1154, bare `:cc` fell through to the unknown-ex-command
    // fallback (`"Not an editor command: cc"`) instead of a real quickfix
    // error, but that fallback is likewise a buffer/cursor no-op — so this
    // case cannot regression-guard the historic bug by buffer/cursor
    // comparison alone (`src/core/engine/tests.rs`'s
    // `test_ex_cc_on_empty_quickfix_list_errors_1154` is what actually pins
    // the message). What it *does* prove, and what retires `"ex::cc"` from
    // `COVERAGE_EXEMPT`, is that the id now has a real, passing oracle case
    // at all — required once a command stops being doc-exempt.
    c(
        "ex:cc on empty quickfix list",
        &["a", "b", "c"],
        2,
        1,
        ":cc<CR>",
    ),
    // #1154: the rest of this PR's backfilled ex commands. Each of these was
    // confirmed by hand against `nvim --headless -u NONE` — a fresh
    // single-tab/single-window/single-buffer session leaves every one of
    // these a documented no-op or refusal, which is exactly what makes them
    // usable here: this harness has no multi-tab/multi-window/multi-buffer
    // real-file plumbing (see the multi-file harness section above, which
    // needs real files on disk and its own `MultiFileCase`/`send_keys_multi`
    // machinery), so a *populated* tab/buffer list identical on both sides
    // isn't reachable from a bare `Case`. The no-op/refusal path is still a
    // real, comparable behaviour, not a vacuous placeholder — a case that
    // failed to recognise the command at all (the pre-#1154 unknown-ex-command
    // fallback) also happens to leave the buffer/cursor untouched, so these
    // do not regression-guard the historic "command didn't exist" bug (the
    // engine-level `test_ex_*_1154` tests in `src/core/engine/tests.rs` do
    // that, asserting on `Engine` state a bare buffer/cursor diff can't see);
    // what they retire is the `COVERAGE_EXEMPT` entry itself, per this
    // repo's "an id just needs one real passing case" bar (`ex:cc on empty
    // quickfix list` above sets the same precedent).
    c(
        "ex:tabonly noop with one tab",
        &["a", "b"],
        1,
        1,
        ":tabonly<CR>",
    ),
    c(
        "ex:tabfirst noop with one tab",
        &["a", "b"],
        1,
        1,
        ":tabfirst<CR>",
    ),
    c(
        "ex:tablast noop with one tab",
        &["a", "b"],
        1,
        1,
        ":tablast<CR>",
    ),
    c(
        "ex:bfirst noop with one buffer",
        &["a", "b"],
        1,
        1,
        ":bfirst<CR>",
    ),
    c(
        "ex:blast noop with one buffer",
        &["a", "b"],
        1,
        1,
        ":blast<CR>",
    ),
    c(
        "ex:hide refuses to close the last window",
        &["a", "b"],
        1,
        1,
        ":hide<CR>",
    ),
    // Leading `x` is a real edit (not just `engine_with`'s raw seed insert,
    // which bypasses dirty-tracking on both sides — an *unmodified* sole
    // buffer can genuinely be wiped out, and vimcode has no fallback empty
    // buffer to swap the window onto afterwards, so exercising THAT path
    // here would crash the harness rather than the oracle's Neovim). With a
    // real dirty flag set identically on both sides, the un-forced wipe must
    // refuse with "No write since last change" and leave buffer/cursor
    // exactly where the `x` left them.
    c(
        "ex:bw refuses on a dirty buffer without a bang",
        &["ab", "c"],
        1,
        1,
        "x:bw<CR>",
    ),
    c(
        "ex:bwipeout refuses on a dirty buffer without a bang",
        &["ab", "c"],
        1,
        1,
        "x:bwipeout<CR>",
    ),
    // #1190: `'hidden'`'s guard on `:enew` — before this fix vimcode had
    // *no* dirty check on `:enew`/`:edit`/`:bnext`/`:bprevious`/`:bfirst`/
    // `:blast`/`:buffer` at all (see `Engine::check_buffer_abandon`'s doc
    // comment), so an un-forced `:enew` always silently wiped the modified
    // buffer. Neovim's *actual* default for `'hidden'` is ON (confirmed by
    // hand: `nvim --headless -u NONE -c 'set hidden?'` reports "hidden",
    // not "nohidden" — unlike historical Vim, whose documented default is
    // off), so the un-configured case below expects the abandon to
    // *succeed*; the refusal only shows up once `'hidden'` is explicitly
    // turned off. Same "real edit via `x`" reasoning as the `:bw`/
    // `:bwipeout` cases above for why this isn't the harness's raw seed
    // insert.
    c(
        "ex:enew abandons a dirty buffer by default ('hidden' is on)",
        &["ab", "c"],
        1,
        1,
        "x:enew<CR>",
    ),
    c(
        "ex:enew! forces past a dirty buffer",
        &["ab", "c"],
        1,
        1,
        "x:enew!<CR>",
    ),
    cs(
        "ex:enew refuses on a dirty buffer with 'nohidden' and no bang",
        &["ab", "c"],
        1,
        1,
        "x:enew<CR>",
        "vim.o.hidden=false",
    ),
    c("ex:delmarks a", &["a", "b", "c"], 2, 1, "ma:delmarks a<CR>"),
    c(
        "ex:delm a (abbreviation)",
        &["a", "b", "c"],
        2,
        1,
        "ma:delm a<CR>",
    ),
    // `:startinsert` differentially matters (unlike the no-ops above): typed
    // text after it only lands as literal insertion if the command actually
    // entered Insert mode. Before #1154 `:startinsert` fell through to the
    // unknown-ex-command fallback, so `mode` stayed Normal and `XY` would
    // have run as two Normal-mode commands instead.
    c(
        "ex:startinsert then type",
        &["ab"],
        1,
        1,
        ":startinsert<CR>XY<Esc>",
    ),
    // `:stopinsert` cannot be *typed* from Insert mode in real Vim (a bare
    // `:` there just inserts a literal colon; reaching it needs `i_CTRL-O`
    // or a script/mapping context this harness's single-keystroke-at-a-time
    // `nvim_input` transport does not model). The comparable, safely-typeable
    // case is the documented already-Normal no-op, which still exercises the
    // real `cmd == "stopinsert"` dispatch arm added by this PR.
    c(
        "ex:stopinsert noop when already Normal",
        &["ab"],
        1,
        1,
        ":stopinsert<CR>x",
    ),
];

// ─────────────────── H2. abbreviations (:abbreviate family, #1152) ───────────
// Each case defines its abbreviation(s) with a real typed `:iabbrev`/
// `:cabbrev`/`:abbreviate` command (both sides run the actual command, not a
// stand-in), then exercises the documented trigger rules.
const CASES_ABBREV: &[Case] = &[
    c(
        "abbrev:full-id expands on trigger char",
        &[""],
        1,
        1,
        ":iabbrev teh the<CR>iteh <Esc>",
    ),
    c(
        "abbrev:end-id expands on trigger char",
        &[""],
        1,
        1,
        ":iabbrev #i #include<CR>i#i <Esc>",
    ),
    c(
        "abbrev:C-v before trigger suppresses expansion",
        &[""],
        1,
        1,
        ":iabbrev teh the<CR>iteh<C-v> <Esc>",
    ),
    c(
        "abbrev:does not fire mid-word",
        &[""],
        1,
        1,
        ":iabbrev teh the<CR>iateh <Esc>",
    ),
    c(
        "abbrev:expands on Esc with nothing typed after",
        &[""],
        1,
        1,
        ":iabbrev teh the<CR>iteh<Esc>",
    ),
    c(
        "abbrev:expands on CR",
        &[""],
        1,
        1,
        ":iabbrev teh the<CR>iteh<CR><Esc>",
    ),
    c(
        "abbrev:cabbrev on the command line runs the expanded command",
        &["foo"],
        1,
        1,
        ":cabbrev X %s/foo/bar/<CR>:X<CR>",
    ),
    c(
        "abbrev:iabbrev does not apply on the command line",
        &["H"],
        1,
        1,
        // `:iabbrev` is Insert-only — ":H<CR>" must NOT expand to ":help"
        // (which would open a help window and leave the buffer untouched
        // for a different reason). Instead ":H" is an unknown command on
        // both sides, so the buffer and cursor stay exactly as they started.
        ":iabbrev H help<CR>:H<CR>x",
    ),
    c(
        // `:h :ia[bbrev]` — 2 chars (`:ia`) is real Vim/Neovim's minimal
        // unambiguous prefix, confirmed against `nvim --headless -u NONE`.
        "abbrev:2-char :ia prefix defines an iabbrev",
        &[""],
        1,
        1,
        ":ia teh the<CR>iteh <Esc>",
    ),
    c(
        // `:h :ca[b]` — 2 chars (`:ca`) is real Vim/Neovim's minimal
        // unambiguous prefix, confirmed against `nvim --headless -u NONE`.
        // Expands into a safe substitution (not `:help`, which would open a
        // real help window and make the two sides' final buffers
        // environment-dependent to compare).
        "abbrev:2-char :ca prefix defines a cabbrev",
        &["foo"],
        1,
        1,
        ":ca X %s/foo/bar/<CR>:X<CR>",
    ),
];

// ─────────────────────────── I. insert mode keys ───────────────────────────
const CASES_INS: &[Case] = &[
    c("ins:C-w", &["ab"], 1, 1, "Afoo bar<C-w><Esc>"),
    c(
        "ins:C-w at line start joins",
        &["ab", "cd"],
        2,
        1,
        "i<C-w><Esc>",
    ),
    c(
        "ins:C-w over existing text",
        &["ab cd"],
        1,
        6,
        "A<C-w><Esc>",
    ),
    c("ins:C-w punctuation", &["foo.bar"], 1, 8, "A<C-w><Esc>"),
    c(
        "ins:C-w trailing ws then word",
        &["foo bar  "],
        1,
        1,
        "A<C-w><Esc>",
    ),
    c("ins:C-w only whitespace", &["foo   "], 1, 1, "A<C-w><Esc>"),
    c("ins:C-u inserted", &["ab"], 1, 1, "Afoo<C-u><Esc>"),
    c("ins:C-u before start", &["ab"], 1, 1, "A<C-u><Esc>"),
    c("ins:C-u twice", &["ab"], 1, 1, "Afoo<C-u><C-u><Esc>"),
    c("ins:C-u with indent", &["    ab"], 1, 1, "A<C-u><Esc>"),
    c("ins:BS at col1 joins", &["ab", "cd"], 2, 1, "i<BS><Esc>"),
    c(
        "ins:BS over indent (nvim smarttab)",
        &["    a"],
        1,
        5,
        "i<BS><Esc>",
    ),
    cs(
        "ins:BS over indent (nosmarttab)",
        &["    a"],
        1,
        5,
        "i<BS><Esc>",
        "vim.o.smarttab=false",
    ),
    c("ins:BS mid indent", &["      a"], 1, 4, "i<BS><Esc>"),
    c(
        "ins:Tab at start (smarttab)",
        &["x"],
        1,
        1,
        ":set ts=8<CR>i<Tab><Esc>",
    ),
    cs(
        "ins:Tab at start (nosmarttab)",
        &["x"],
        1,
        1,
        ":set ts=8<CR>i<Tab><Esc>",
        "vim.o.smarttab=false",
    ),
    c("ins:Tab mid line ts4", &["a"], 1, 1, "A<Tab>x<Esc>"),
    c(
        "ins:Tab mid line ts8",
        &["a"],
        1,
        1,
        ":set ts=8<CR>A<Tab>x<Esc>",
    ),
    c("ins:Tab after 2 chars ts4", &["ab"], 1, 1, "A<Tab>x<Esc>"),
    // #1153 'softtabstop' — verified against `nvim --headless`.
    c(
        "ins:Tab uses softtabstop not tabstop",
        &["ab"],
        1,
        2,
        ":set et ts=8 sts=2 nosmarttab<CR>i<Tab><Esc>",
    ),
    c(
        "ins:BS over softtabstop removes a whole soft-tab",
        &[""],
        1,
        1,
        ":set et ts=8 sts=3 nosmarttab<CR>i<Tab><Tab><BS><Esc>",
    ),
    // Unaligned-run regressions caught in review: BackSpace must round the
    // *absolute column* down to the previous 'softtabstop' stop, not cap the
    // contiguous blank-run length at 'sts'. Both diverge from the aligned
    // cases above only when the run doesn't start on a multiple of 'sts'.
    c(
        "ins:BS over softtabstop unaligned leading indent",
        &["     x"],
        1,
        6,
        ":set et ts=8 sts=2 nosmarttab<CR>i<BS><Esc>",
    ),
    c(
        "ins:BS over softtabstop unaligned mid-line run",
        &["a  b"],
        1,
        4,
        ":set et ts=8 sts=2 nosmarttab<CR>i<BS><Esc>",
    ),
    c("ins:C-t", &["a"], 1, 1, "i<C-t><Esc>"),
    c("ins:C-t mid line", &["ab"], 1, 2, "i<C-t><Esc>"),
    c("ins:C-d", &["    a"], 1, 5, "i<C-d><Esc>"),
    c("ins:C-d partial", &["  a"], 1, 1, "A<C-d><Esc>"),
    c("ins:0 C-d", &["    a"], 1, 1, "A0<C-d><Esc>"),
    // #804 CI fix: `i_0_CTRL-D` keys off Vim's `lastc` — the previous
    // *keystroke* — not off the buffer text before the cursor. These three
    // pin the distinction the oracle actually makes.
    c("ins:0 C-d after text", &["    afoo"], 1, 1, "A0<C-d><Esc>"),
    c(
        "ins:C-d with untyped 0 before cursor",
        &["    a0"],
        1,
        1,
        "A<C-d><Esc>",
    ),
    c("ins:caret C-d", &["    a"], 1, 1, "A^<C-d><Esc>"),
    c("ins:0 C-d twice", &["        a"], 1, 1, "A0<C-d><C-d><Esc>"),
    c("ins:C-o dw", &["foo bar"], 1, 1, "i<C-o>dw<Esc>"),
    c("ins:C-o $ then type", &["foo"], 1, 1, "i<C-o>$x<Esc>"),
    c("ins:A C-o h", &["foo"], 1, 1, "A<C-o>hx<Esc>"),
    c("ins:C-o with count", &["a b c d"], 1, 1, "i<C-o>2wx<Esc>"),
    c("ins:C-o p", &["ab"], 1, 1, "yli<C-o>p<Esc>"),
    c("ins:C-o :s", &["a a"], 1, 1, "A<C-o>:s/a/b/<CR>x<Esc>"),
    c("ins:C-e", &["a", "cd"], 1, 1, "A<C-e><Esc>"),
    c("ins:C-y", &["ab", "c"], 2, 1, "A<C-y><Esc>"),
    c("ins:C-y nothing above", &["ab", "c"], 1, 1, "A<C-y><Esc>"),
    c("ins:C-a reinsert", &["ab"], 1, 1, "ifoo<Esc>A<C-a><Esc>"),
    c("ins:C-v Tab", &["a"], 1, 1, "i<C-v><Tab><Esc>"),
    c("ins:C-v 065", &["a"], 1, 1, "i<C-v>065<Esc>"),
    c("ins:C-v x41", &["a"], 1, 1, "i<C-v>x41<Esc>"),
    c("ins:CR autoindent", &["    foo"], 1, 8, "A<CR>bar<Esc>"),
    c("ins:CR mid-line", &["foo bar"], 1, 4, "i<CR><Esc>"),
    c("ins:CR after space", &["foo bar"], 1, 5, "i<CR><Esc>"),
    c("ins:CR on indented mid", &["  foo bar"], 1, 6, "i<CR><Esc>"),
    c(
        "ins:CR then Esc removes autoindent",
        &["    foo"],
        1,
        8,
        "A<CR><Esc>",
    ),
    c(
        "ins:CR CR keeps prev line empty",
        &["    foo"],
        1,
        8,
        "A<CR><CR>x<Esc>",
    ),
    c("ins:Right Right X", &["abc"], 1, 1, "i<Right><Right>X<Esc>"),
    c("ins:Left at col1", &["abc"], 1, 1, "i<Left>X<Esc>"),
    c("ins:Right at eol", &["abc"], 1, 3, "i<Right><Right>X<Esc>"),
    c(
        "ins:Down Down col memory",
        &["abcdef", "ab", "abcdef"],
        1,
        5,
        "i<Down><Down>X<Esc>",
    ),
    c("ins:End Home", &["abc"], 1, 2, "i<End>X<Home>Y<Esc>"),
    c("ins:Del", &["abc"], 1, 1, "i<Del><Esc>"),
    c("ins:Del at eol joins", &["ab", "cd"], 1, 2, "a<Del><Esc>"),
    c("ins:Esc cursor left", &["abc"], 1, 3, "a<Esc>"),
    c("ins:C-h as BS", &["abc"], 1, 3, "a<C-h><Esc>"),
    c("ins:C-j newline", &["ab"], 1, 2, "a<C-j><Esc>"),
    c("ins:3ix Left y", &["a"], 1, 1, "3ix<Left>y<Esc>"),
    c("ins:( no autopair", &["a"], 1, 1, "i(<Esc>"),
    c("ins:\" no autopair", &["a"], 1, 1, "i\"<Esc>"),
    c("ins:{ CR no autopair", &["a"], 1, 1, "i{<CR><Esc>"),
    c("ins:[ no autopair", &["a"], 1, 1, "A[<Esc>"),
    // `completeopt=""` is not cosmetic: with the default `menu,preview` and two
    // or more candidates, nvim 0.9.x tries to draw the completion popup and
    // *segfaults* under `--headless -l`, so the oracle can never answer. Pinning
    // it off keeps these cases probeable.
    cs(
        "ins:C-n completion",
        &["foo", "f"],
        2,
        1,
        "A<C-n><Esc>",
        "vim.o.completeopt=\"\"",
    ),
    cs(
        "ins:C-p completion",
        &["foo", "fob", "f"],
        3,
        1,
        "A<C-p><Esc>",
        "vim.o.completeopt=\"\"",
    ),
    c(
        "ins:typing prefix then CR no completion",
        &["foo bar"],
        1,
        1,
        "ofo<CR>x<Esc>",
    ),
    c(
        "ins:typing prefix then Tab",
        &["foo bar"],
        1,
        1,
        "ofo<Tab>x<Esc>",
    ),
    c("ins:typing prefix then Esc", &["foo bar"], 1, 1, "ofo<Esc>"),
    c("ins:C-r C-w? skip", &["a"], 1, 1, "l"),
    c("ins:i then Up", &["abc", "def"], 2, 2, "i<Up>X<Esc>"),
    c("ins:o then Up", &["abc"], 1, 1, "o<Up>X<Esc>"),
    c(
        "ins:BS at start of insert over prev text",
        &["abc"],
        1,
        3,
        "i<BS><BS><Esc>",
    ),
    c(
        "ins:C-w at start of insert",
        &["foo bar"],
        1,
        5,
        "i<C-w><Esc>",
    ),
    c(
        "ins:A then BS past insert start",
        &["ab"],
        1,
        1,
        "A<BS><BS><BS><Esc>",
    ),
    c("ins:C-t then C-d", &["a"], 1, 1, "i<C-t><C-t><C-d><Esc>"),
    c("ins:C-v u00e9", &["a"], 1, 1, "i<C-v>u00e9<Esc>"),
    c(
        "ins:insert Tab then BS (sts)",
        &["a"],
        1,
        1,
        "A<Tab><BS>x<Esc>",
    ),
    c("ins:C-e beyond line", &["abc", "d"], 1, 1, "A<C-e><Esc>"),
    c(
        "ins:i with count and Esc cursor",
        &["abc"],
        1,
        2,
        "2ix<Esc>",
    ),
    c("ins:A with count and CR", &["a"], 1, 1, "2Ax<CR><Esc>"),
    c(
        "ins:C-r register linewise mid line",
        &["a", "bc"],
        1,
        1,
        "yyjli<C-r>\"<Esc>",
    ),
    c(
        "ins:C-r with tab in register",
        &["a\tb", "c"],
        1,
        1,
        "yyjA<C-r>\"<Esc>",
    ),
    c("ins:C-w C-w", &["a b c"], 1, 1, "A<C-w><C-w><Esc>"),
    c(
        "ins:autoindent with tabs noet",
        &["\tfoo"],
        1,
        1,
        ":set noet<CR>obar<Esc>",
    ),
    c("ins:C-t noet", &["a"], 1, 1, ":set noet<CR>i<C-t><Esc>"),
    c("ins:C-k digraph skip", &["a"], 1, 1, "l"),
    c(
        "ins:BS join with autoindent",
        &["a", "    b"],
        2,
        5,
        "i<BS><BS><BS><BS><BS><Esc>",
    ),
    c("ins:i then Esc then . twice", &["a"], 1, 1, "ix<Esc>.."),
];

// ─────────────────────────── J. visual ───────────────────────────
const CASES_VIS: &[Case] = &[
    c("vis:vjd", &["abc", "def", "ghi"], 1, 2, "vjd"),
    c("vis:Vjd", &["abc", "def", "ghi"], 1, 2, "Vjd"),
    c("vis:vjy cursor", &["abc", "def", "ghi"], 2, 2, "vjy"),
    c("vis:vky cursor", &["abc", "def", "ghi"], 2, 2, "vky"),
    c("vis:v$d joins", &["abc", "def"], 1, 2, "v$d"),
    c("vis:v$y p", &["abc", "def"], 1, 1, "v$yjp"),
    c("vis:vec", &["foo bar"], 1, 1, "vecX<Esc>"),
    c("vis:vjJ", &["a", "b", "c"], 1, 1, "vjJ"),
    c("vis:VjjJ", &["a", "b", "c", "d"], 1, 1, "VjjJ"),
    c("vis:vj>", &["a", "b", "c"], 1, 1, "vj>"),
    c("vis:Vj> cursor", &["a", "b", "c"], 1, 1, "Vj>"),
    c("vis:v2jd", &["a", "b", "c", "d"], 1, 1, "v2jd"),
    c("vis:vipd", &["a", "b", "", "c"], 1, 1, "vipd"),
    c("vis:vllohd", &["abcdef"], 1, 3, "vllohd"),
    c("vis:gv", &["abcdef"], 1, 1, "vly<Esc>$gvd"),
    c(
        "vis:gv after Vjd",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "Vjdgvd",
    ),
    c("vis:vGd", &["abc", "def"], 1, 2, "vGd"),
    c("vis:vggd", &["abc", "def"], 2, 2, "vggd"),
    c("vis:v0d", &["abcdef"], 1, 4, "v0d"),
    c("vis:v^d", &["   abc"], 1, 6, "v^d"),
    c("vis:vjr-", &["abc", "def"], 1, 2, "vjr-"),
    c("vis:Vr-", &["abc"], 1, 2, "Vr-"),
    c("vis:vj~", &["abc", "def"], 1, 2, "vj~"),
    c("vis:vjU", &["abc", "def"], 1, 2, "vjU"),
    c("vis:Vju", &["ABC", "DEF"], 1, 1, "Vju"),
    c("vis:vjD", &["abc", "def", "ghi"], 1, 2, "vjD"),
    c("vis:vjX", &["abc", "def", "ghi"], 1, 2, "vjX"),
    c("vis:vjY p", &["abc", "def", "ghi"], 1, 2, "vjYGp"),
    c("vis:vjC", &["abc", "def", "ghi"], 1, 2, "vjCX<Esc>"),
    c("vis:vjS", &["abc", "def", "ghi"], 1, 2, "vjSX<Esc>"),
    c("vis:vjR", &["abc", "def", "ghi"], 1, 2, "vjRX<Esc>"),
    c("vis:v3ld", &["abcdef"], 1, 1, "v3ld"),
    c("vis:vlp linewise reg", &["a", "b", "xyz"], 1, 1, "yyjjvlp"),
    c("vis:Vp charwise reg", &["ab", "cd"], 1, 1, "ylVp"),
    c("vis:vly$P", &["abc"], 1, 1, "vly$P"),
    c("vis:vi(d", &["f(a, b)"], 1, 3, "vi(d"),
    c("vis:va(d", &["f(a, b)"], 1, 3, "va(d"),
    c("vis:vi(i( expands", &["((a))"], 1, 3, "vi(i(d"),
    c("vis:va\"d", &["x \"ab\" y"], 1, 4, "va\"d"),
    c("vis:vi\"d", &["x \"ab\" y"], 1, 4, "vi\"d"),
    c("vis:viwiwiwd", &["a b c d"], 1, 1, "viwiwiwd"),
    c("vis:v3iwd", &["a b c d"], 1, 1, "v3iwd"),
    c("vis:v2awd", &["a b c d"], 1, 1, "v2awd"),
    c("vis:vapd", &["a", "b", "", "c", "d"], 1, 1, "vapd"),
    c("vis:Vj:normal", &["a", "b", "c"], 1, 1, "Vj:normal Ax<CR>"),
    c("vis:Vj=", &["  a", "    b"], 1, 1, "Vj="),
    c("vis:vll Esc cursor", &["abc"], 1, 1, "vll<Esc>"),
    c("vis:vjkd shrink", &["abc", "def"], 1, 2, "vjkd"),
    c("vis:Vd cursor", &["  a", "  b"], 1, 3, "Vd"),
    c("vis:viwd on whitespace", &["a   b"], 1, 2, "viwd"),
    c("vis:vawd at eol", &["foo bar"], 1, 5, "vawd"),
    c("vis:vjy then P", &["abc", "def"], 1, 2, "vjyP"),
    c("vis:vj< ", &["    a", "    b"], 1, 1, "vj<"),
    c("vis:V3>", &["a"], 1, 1, "V3>"),
    c("vis:vjo then d", &["abc", "def"], 1, 2, "vjod"),
    c("vis:Vjo k d", &["a", "b", "c", "d"], 2, 1, "Vjokd"),
    c(
        "vis:vip then ip extends",
        &["a", "", "b", "", "c"],
        1,
        1,
        "vipipd",
    ),
    c("vis:v% d", &["(abc) d"], 1, 1, "v%d"),
    c("vis:vf,d", &["a,b,c"], 1, 1, "vf,d"),
    c("vis:vt,d", &["a,b,c"], 1, 1, "vt,d"),
    c("vis:v/pat d", &["foo bar baz"], 1, 1, "v/baz<CR>d"),
    c("vis:vnd", &["foo x foo y foo"], 1, 1, "/foo<CR>vnd"),
    c("vis:v'a? mark d", &["abc", "def"], 2, 2, "magg0v`ad"),
    c("vis:vjc then u", &["abc", "def"], 1, 2, "vjcX<Esc>u"),
    c("vis:V then count j >", &["a", "b", "c", "d"], 1, 1, "V2j>"),
    c("vis:v$ on last line", &["abc"], 1, 1, "v$d"),
    c("vis:v$h", &["abcd"], 1, 1, "v$hd"),
    c("vis:vgUiw", &["foo bar"], 1, 1, "wviwU"),
    c(
        "vis:vjgq",
        &["one two three four five six", "seven"],
        1,
        1,
        ":set tw=10<CR>Vjgq",
    ),
    c(
        "vis:vj: shows range then s",
        &["a", "a", "a"],
        1,
        1,
        "vj:s/a/b/g<CR>",
    ),
    c("vis:vjy \"0 then p", &["ab", "cd"], 1, 1, "vjy\"0P"),
    c(
        "vis:vjp charwise reg into multi",
        &["abc", "def", "x"],
        1,
        1,
        "ylvjp",
    ),
    c("vis:vjd cursor", &["abc", "def", "ghi"], 1, 2, "vjd"),
    c("vis:V G d cursor", &["a", "b", "c"], 2, 1, "VGd"),
    c("vis:v iw at word end", &["foo bar"], 1, 3, "viwd"),
    c(
        "vis:v aw on last word with leading space",
        &["foo bar"],
        1,
        7,
        "vawd",
    ),
    c("vis:v x", &["abcd"], 1, 2, "vlx"),
    c("vis:v s", &["abcd"], 1, 2, "vlsX<Esc>"),
    c("vis:v with count 3v? skip", &["abcd"], 1, 1, "l"),
    c("vis:vjy count 2p", &["ab", "cd"], 1, 1, "vjy$2p"),
    c("vis:V y cursor col", &["  abc"], 1, 4, "Vy"),
    c("vis:v gv after y", &["abcdef"], 1, 3, "vly0gvd"),
    c("vis:v then gv toggles", &["abcdef"], 1, 1, "vl<Esc>$vgvd"),
    c("vis:v_gJ", &["a", "  b", "c"], 1, 1, "VjgJ"),
    c("vis:v_r CR", &["abc"], 1, 2, "vr<CR>"),
    c("vis:v_J count", &["a", "b", "c", "d"], 1, 1, "V2jJ"),
    c("vis:v ip on blank", &["a", "", "", "b"], 2, 1, "vipd"),
    c("vis:v ap trailing", &["a", "", "b", "c"], 3, 1, "vapd"),
    c("vis:vjd then p", &["abc", "def", "ghi"], 1, 2, "vjdp"),
    c("vis:VjdP", &["a", "b", "c"], 1, 1, "VjdP"),
    c("vis:Vjy then p count", &["a", "b"], 1, 1, "Vjy2p"),
    c("vis:v ip then y cursor", &["a", "b", "", "c"], 2, 1, "vipy"),
    c(
        "vis:vip on last para no trailing",
        &["a", "", "b", "c"],
        4,
        1,
        "vipd",
    ),
    c("vis:v_O charwise same as o", &["abcdef"], 1, 3, "vllOhd"),
    c("vis:v then < count", &["        a"], 1, 1, "V2<"),
    c(
        "vis:v mode Esc then cursor",
        &["abc", "def"],
        1,
        1,
        "vj<Esc>",
    ),
    c(
        "vis:v with $ then j keeps eol",
        &["ab", "abcd", "abc"],
        1,
        1,
        "v$jd",
    ),
    c(
        "vis:v with $ then j then y p",
        &["ab", "abcd", "abc"],
        1,
        1,
        "v$jyGp",
    ),
    // #1005: first UTF-8 multi-byte cases — see the CASES_OP `mb:` block for
    // the starting-column rule these are held to.
    c(
        "vis:mb:vld deletes two emoji chars",
        &["😀😀 world"],
        1,
        1,
        "vld",
    ),
    c("vis:mb:vld charwise over CJK", &["日本語"], 1, 1, "vld"),
];

// ─────────────────────────── K. visual block ───────────────────────────
const CASES_VB: &[Case] = &[
    c("vb:jjd", &["abc", "def", "ghi"], 1, 2, "<C-v>jjd"),
    c("vb:jjld", &["abc", "def", "ghi"], 1, 2, "<C-v>jjld"),
    c("vb:jjIx", &["abc", "def", "ghi"], 1, 2, "<C-v>jjIx<Esc>"),
    c("vb:jjAx", &["abc", "def", "ghi"], 1, 2, "<C-v>jjAx<Esc>"),
    c("vb:jj$Ax", &["ab", "abcd", "a"], 1, 1, "<C-v>jj$Ax<Esc>"),
    c("vb:jlrx", &["abc", "def"], 1, 2, "<C-v>jlrx"),
    c("vb:jlcX", &["abc", "def"], 1, 2, "<C-v>jlcX<Esc>"),
    c("vb:j>", &["abc", "def"], 1, 2, "<C-v>j>"),
    c("vb:jy then Gp", &["ab", "cd", "", "xy"], 1, 1, "<C-v>jyGp"),
    c("vb:jy then P", &["ab", "cd"], 1, 1, "<C-v>jy$P"),
    c(
        "vb:ragged d",
        &["abcdef", "ab", "abcdef"],
        1,
        3,
        "<C-v>jjlld",
    ),
    c(
        "vb:I on short line skipped",
        &["abcdef", "ab", "abcdef"],
        1,
        4,
        "<C-v>jjIx<Esc>",
    ),
    c(
        "vb:A on short line padded",
        &["abcdef", "ab", "abcdef"],
        1,
        4,
        "<C-v>jjAx<Esc>",
    ),
    c("vb:jj$d", &["abcdef", "ab", "abcd"], 1, 3, "<C-v>jj$d"),
    c("vb:o", &["abcdef", "abcdef"], 1, 2, "<C-v>jllohd"),
    c("vb:O", &["abcdef", "abcdef"], 1, 2, "<C-v>jllOhd"),
    c("vb:jx", &["abc", "def"], 1, 2, "<C-v>jx"),
    c("vb:jsX", &["abc", "def"], 1, 2, "<C-v>jsX<Esc>"),
    c("vb:jJ", &["abc", "def", "ghi"], 1, 2, "<C-v>jJ"),
    c("vb:jl~", &["abc", "def"], 1, 2, "<C-v>jl~"),
    c("vb:jlU", &["abc", "def"], 1, 2, "<C-v>jlU"),
    c("vb:jCX", &["abcdef", "abcdef"], 1, 3, "<C-v>jCX<Esc>"),
    c("vb:jD", &["abcdef", "abcdef"], 1, 3, "<C-v>jD"),
    c(
        "vb:jIx then .",
        &["ab", "ab", "ab", "ab"],
        1,
        1,
        "<C-v>jIx<Esc>jj.",
    ),
    c(
        "vb:I on empty middle line",
        &["ab", "", "ab"],
        1,
        1,
        "<C-v>jjIx<Esc>",
    ),
    c(
        "vb:A on empty middle line",
        &["ab", "", "ab"],
        1,
        1,
        "<C-v>jjAx<Esc>",
    ),
    c(
        "vb:$A on empty middle line",
        &["ab", "", "ab"],
        1,
        1,
        "<C-v>jj$Ax<Esc>",
    ),
    c("vb:jjy p at eol", &["ab", "cd", "ef"], 1, 1, "<C-v>jjy$p"),
    c(
        "vb:jjy p on shorter",
        &["abc", "abc", "x"],
        1,
        2,
        "<C-v>jjyGp",
    ),
    c("vb:jly then p", &["abc", "def"], 1, 1, "<C-v>jly$p"),
    c("vb:jIx with CR", &["ab", "ab"], 1, 1, "<C-v>jIx<CR><Esc>"),
    c(
        "vb:jc with multi chars",
        &["abcd", "abcd"],
        1,
        2,
        "<C-v>jlcXYZ<Esc>",
    ),
    c("vb:j< ", &["    ab", "    ab"], 1, 5, "<C-v>j<"),
    c("vb:jr<CR>", &["abc", "def"], 1, 2, "<C-v>jr<CR>"),
    c("vb:cursor after d", &["abcdef", "abcdef"], 1, 3, "<C-v>jld"),
    c(
        "vb:cursor after y",
        &["abcdef", "abcdef"],
        2,
        4,
        "<C-v>khhy",
    ),
    c(
        "vb:jjAx then u",
        &["ab", "ab", "ab"],
        1,
        1,
        "<C-v>jjAx<Esc>u",
    ),
    c(
        "vb:2j then o then j",
        &["abc", "abc", "abc", "abc"],
        1,
        1,
        "<C-v>2jlojd",
    ),
    c("vb:I with count? 2I", &["ab", "ab"], 1, 1, "<C-v>j2Ix<Esc>"),
    c(
        "vb:jjp block over block",
        &["ab", "cd", "ef", "gh"],
        1,
        1,
        "<C-v>jy2j<C-v>jp",
    ),
    c(
        "vb:vb yank then p linewise reg? P",
        &["ab", "cd"],
        1,
        1,
        "<C-v>jyP",
    ),
    c("vb:$ then I", &["ab", "abcd"], 1, 1, "<C-v>j$Ix<Esc>"),
    c(
        "vb:d then .",
        &["abcd", "abcd", "abcd", "abcd"],
        1,
        1,
        "<C-v>jdjj.",
    ),
    c(
        "vb:r then .",
        &["abcd", "abcd", "abcd", "abcd"],
        1,
        1,
        "<C-v>jlrxjj.",
    ),
    c(
        "vb:c then .",
        &["abcd", "abcd", "abcd", "abcd"],
        1,
        1,
        "<C-v>jcX<Esc>jj.",
    ),
    c(
        "vb:jjIx on tab lines",
        &["\tab", "\tab"],
        1,
        2,
        "<C-v>jIx<Esc>",
    ),
    c("vb:g C-a", &["1", "1", "1"], 1, 1, "<C-v>jjg<C-a>"),
    c("vb:jjy then gv", &["ab", "cd", "ef"], 1, 1, "<C-v>jjygvd"),
    c("vb:v then C-v switch", &["abc", "def"], 1, 1, "vj<C-v>d"),
    c("vb:V then C-v switch", &["abc", "def"], 1, 2, "Vj<C-v>d"),
    c("vb:C-v then v switch", &["abc", "def"], 1, 2, "<C-v>jvd"),
    c("vb:C-v then V", &["abc", "def"], 1, 2, "<C-v>jVd"),
    // #1005: first UTF-8 multi-byte cases, ragged rows of differing
    // byte/char length. Both start at col 1 (see the CASES_OP `mb:` block's
    // doc on why). `<C-v>jjd` over this same fixture was investigated too —
    // Vim's block mode measures in *screen* columns and widens a
    // partially-covered wide (CJK) character out to the whole character, so
    // a block `d` here deletes different amounts per row than a naive
    // char-count model predicts. That is a real, separate gap (virtual/
    // screen-column tracking for visual-block, which this engine does not
    // have) rather than an off-by-one, so it is reported as a follow-up
    // rather than added here — see the PR description.
    c(
        "vb:mb:Ix on ragged CJK rows",
        &["日ab", "ab", "日日ab"],
        1,
        1,
        "<C-v>jjIx<Esc>",
    ),
    c(
        "vb:mb:$A on ragged CJK rows",
        &["日ab", "abcd", "a"],
        1,
        1,
        "<C-v>jj$Ax<Esc>",
    ),
];

// ─────────────────────────── L. C-a / C-x ───────────────────────────
const CASES_NUM: &[Case] = &[
    c("num:C-a on number", &["x 5 y"], 1, 3, "<C-a>"),
    c("num:C-a before number", &["x 5 y"], 1, 1, "<C-a>"),
    c("num:5C-a", &["x 5 y"], 1, 1, "5<C-a>"),
    c("num:C-x to negative", &["x 0 y"], 1, 3, "<C-x>"),
    c("num:C-a on -5", &["x -5 y"], 1, 3, "<C-a>"),
    c("num:C-a on -1", &["x -1 y"], 1, 3, "<C-a>"),
    c("num:C-x on -1", &["x -1 y"], 1, 3, "<C-x>"),
    c("num:hex 0x0f", &["0x0f"], 1, 1, "<C-a>"),
    c("num:hex 0xff", &["0xff"], 1, 1, "<C-a>"),
    c("num:hex 0xFF", &["0xFF"], 1, 1, "<C-a>"),
    c("num:hex 0xaB", &["0xaB"], 1, 1, "<C-a>"),
    c("num:hex C-x below zero", &["0x0"], 1, 1, "<C-x>"),
    c("num:octal not default 007", &["007"], 1, 1, "<C-a>"),
    cs(
        "num:octal nf=octal 007",
        &["007"],
        1,
        1,
        "<C-a>",
        "vim.o.nrformats='bin,octal,hex'",
    ),
    c("num:binary 0b101", &["0b101"], 1, 1, "<C-a>"),
    c("num:leading zeros 009", &["009"], 1, 1, "<C-a>"),
    c("num:leading zeros 0099 C-x", &["0099"], 1, 1, "<C-x>"),
    c("num:word digits foo9", &["foo9"], 1, 1, "<C-a>"),
    c("num:a-5", &["a-5"], 1, 1, "<C-a>"),
    c("num:1.5 on 1", &["1.5"], 1, 1, "<C-a>"),
    c("num:1.5 on 5", &["1.5"], 1, 3, "<C-a>"),
    c("num:no number after cursor", &["5 x"], 1, 3, "<C-a>"),
    c("num:cursor after C-a", &["x 5 y"], 1, 1, "<C-a>"),
    c("num:cursor on last digit", &["x 100 y"], 1, 1, "<C-a>"),
    c("num:C-a .", &["x 5"], 1, 1, "<C-a>."),
    c("num:3C-a .", &["x 5"], 1, 1, "3<C-a>."),
    c("num:3C-a 2.", &["x 5"], 1, 1, "3<C-a>2."),
    c("num:200C-x", &["100"], 1, 1, "200<C-x>"),
    c("num:V C-a", &["1", "1", "1"], 1, 1, "Vjj<C-a>"),
    c("num:V g C-a", &["1", "1", "1"], 1, 1, "Vjjg<C-a>"),
    c("num:V 2g C-a", &["1", "1", "1"], 1, 1, "Vjj2g<C-a>"),
    c("num:v C-a partial", &["1 1", "1 1"], 1, 1, "vj<C-a>"),
    c("num:C-v block C-a", &["1 1", "1 1"], 1, 3, "<C-v>j<C-a>"),
    c("num:C-a hex mid", &["0x10"], 1, 3, "<C-a>"),
    c("num:5C-a on -3", &["-3"], 1, 1, "5<C-a>"),
    c("num:C-x foo0", &["foo0"], 1, 1, "<C-x>"),
    c("num:1-2 on 1", &["1-2"], 1, 1, "<C-a>"),
    c("num:1-2 on -", &["1-2"], 1, 2, "<C-a>"),
    c("num:1-2 on 2", &["1-2"], 1, 3, "<C-a>"),
    cs(
        "num:alpha",
        &["a"],
        1,
        1,
        "<C-a>",
        "vim.o.nrformats='alpha'",
    ),
    c("num:C-a on 9 width", &["9"], 1, 1, "<C-a>"),
    c(
        "num:C-a on 99999999999999999999 overflow",
        &["99999999999999999999"],
        1,
        1,
        "<C-a>",
    ),
    c(
        "num:V C-a skips lines without numbers",
        &["1", "x", "1"],
        1,
        1,
        "Vjjg<C-a>",
    ),
    c(
        "num:C-a on number after word char",
        &["ab12"],
        1,
        3,
        "<C-a>",
    ),
    c(
        "num:V C-a only first number per line",
        &["1 2", "3 4"],
        1,
        1,
        "Vj<C-a>",
    ),
    c("num:C-a 0x with uppercase X", &["0X0f"], 1, 1, "<C-a>"),
    c("num:C-a on negative hex? -0x1", &["-0x1"], 1, 1, "<C-a>"),
    c("num:C-x on 0 leading zeros 000", &["000"], 1, 1, "<C-x>"),
    c(
        "num:C-a cursor on space before number",
        &["a 1"],
        1,
        2,
        "<C-a>",
    ),
    c("num:C-a on 10 then u", &["10"], 1, 1, "<C-a>u"),
    c("num:V C-a cursor", &["1", "1"], 1, 1, "Vj<C-a>"),
    c(
        "num:v C-a on -5 in visual (no minus)",
        &["x -5"],
        1,
        4,
        "vl<C-a>",
    ),
];

// ─────────────────────────── M. scrolling ───────────────────────────
const CASES_SCROLL: &[Case] = &[
    c("scroll:C-d", LONG, 1, 1, "<C-d>"),
    c("scroll:C-u", LONG, 30, 1, "<C-u>"),
    c("scroll:C-f", LONG, 1, 1, "<C-f>"),
    c("scroll:C-b", LONG, 60, 1, "<C-b>"),
    c("scroll:C-e pushes cursor", LONG, 1, 1, "<C-e>"),
    c("scroll:3C-e", LONG, 1, 1, "3<C-e>"),
    c("scroll:G C-y", LONG, 1, 1, "G<C-y>"),
    c("scroll:H from 30", LONG, 30, 1, "H"),
    c("scroll:M from 30", LONG, 30, 1, "M"),
    c("scroll:L from 30", LONG, 30, 1, "L"),
    c("scroll:3H", LONG, 30, 1, "3H"),
    c("scroll:3L", LONG, 30, 1, "3L"),
    c("scroll:ztL", LONG, 20, 1, "ztL"),
    c("scroll:zzH", LONG, 20, 1, "zzH"),
    c("scroll:zbH", LONG, 30, 1, "zbH"),
    c("scroll:z<CR>L", LONG, 20, 1, "z<CR>L"),
    c("scroll:z.H", LONG, 20, 1, "z.H"),
    c("scroll:z-H", LONG, 30, 1, "z-H"),
    c("scroll:C-d C-d", LONG, 1, 1, "<C-d><C-d>"),
    c("scroll:5C-d C-d", LONG, 1, 1, "5<C-d><C-d>"),
    c("scroll:C-d near end", LONG, 55, 1, "<C-d>"),
    c("scroll:C-f at end", LONG, 60, 1, "<C-f>"),
    c("scroll:C-b at start", LONG, 1, 1, "<C-b>"),
    c("scroll:C-f C-f", LONG, 1, 1, "<C-f><C-f>"),
    c("scroll:C-f C-b", LONG, 1, 1, "<C-f><C-b>"),
    c("scroll:G H", LONG, 1, 1, "GH"),
    c("scroll:G M", LONG, 1, 1, "GM"),
    c("scroll:dL", LONG, 1, 1, "dL"),
    c("scroll:dH", LONG, 10, 1, "dH"),
    c("scroll:dM", LONG, 1, 1, "dM"),
    c("scroll:C-d col kept (nosol)", LONG, 1, 3, "<C-d>"),
    cs(
        "scroll:C-d col sol",
        LONG,
        1,
        3,
        "<C-d>",
        "vim.o.startofline=true",
    ),
    c("scroll:so=5 30G H", LONG, 1, 1, ":set so=5<CR>30GH"),
    c("scroll:so=5 30G L", LONG, 1, 1, ":set so=5<CR>30GL"),
    c("scroll:so=5 C-e", LONG, 1, 1, ":set so=5<CR><C-e>"),
    c("scroll:25j H", LONG, 1, 1, "25jH"),
    c("scroll:25j L", LONG, 1, 1, "25jL"),
    c("scroll:C-u at top", LONG, 1, 1, "<C-u>"),
    c("scroll:C-u C-u from end", LONG, 60, 1, "<C-u><C-u>"),
    c("scroll:C-e C-e H", LONG, 1, 1, "<C-e><C-e>H"),
    c("scroll:G C-y C-y H", LONG, 1, 1, "G<C-y><C-y>H"),
    c("scroll:zt C-e", LONG, 20, 1, "zt<C-e>"),
    c("scroll:zt k", LONG, 20, 1, "ztk"),
    c("scroll:zt k H", LONG, 20, 1, "ztkH"),
    c("scroll:zb j L", LONG, 30, 1, "zbjL"),
    c("scroll:C-d then H L", LONG, 1, 1, "<C-d>H"),
    c("scroll:C-d then L", LONG, 1, 1, "<C-d>L"),
    c("scroll:C-f then H", LONG, 1, 1, "<C-f>H"),
    c("scroll:C-f then L", LONG, 1, 1, "<C-f>L"),
    c("scroll:C-b after G then H", LONG, 60, 1, "<C-b>H"),
    c("scroll:C-b after G then L", LONG, 60, 1, "<C-b>L"),
    c("scroll:10C-e H", LONG, 1, 1, "10<C-e>H"),
    c("scroll:50% H", LONG, 1, 1, "50%H"),
    c("scroll:30G zz H L", LONG, 1, 1, "30GzzHjL"),
    c("scroll:j at bottom scrolls one", LONG, 1, 1, "22jH"),
    c("scroll:k at top", LONG, 30, 1, "ztk"),
    c("scroll:G then k ×5 H", LONG, 1, 1, "GkkkkkH"),
    c("scroll:cursor after zt col", LONG, 20, 3, "zt"),
    c("scroll:C-d twice then C-u", LONG, 1, 1, "<C-d><C-d><C-u>"),
    c(
        "scroll:3C-d sets scroll then C-u",
        LONG,
        30,
        1,
        "3<C-d><C-u>",
    ),
    c("scroll:3<C-f>", LONG, 1, 1, "3<C-f>"),
    // #1008: `2<C-b>` is the case the attached-UI oracle was built for. The
    // second `<C-b>` cannot scroll a whole page (it clamps at the top of the
    // buffer), which is exactly where "cursor to the last line of the new
    // window" stops agreeing with "cursor to a fixed offset from the old
    // window" — see `page_up` in `src/core/engine/motions.rs`. The two
    // below pin the clamped form; the unclamped one is `scroll:C-b`
    // above, which passed before and still does.
    c("scroll:2<C-b>", LONG, 60, 1, "2<C-b>"),
    c("scroll:3<C-b> clamped at top", LONG, 60, 1, "3<C-b>"),
    c(
        "scroll:z<CR> col first nonblank",
        &["a", "   b", "c"],
        2,
        1,
        "z<CR>",
    ),
    c("scroll:zt col kept", &["a", "   b", "c"], 2, 1, "zt"),
    c("scroll:H on short buffer", &["a", "b", "c"], 3, 1, "H"),
    c("scroll:L on short buffer", &["a", "b", "c"], 1, 1, "L"),
    c(
        "scroll:M on short buffer",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "M",
    ),
    c(
        "scroll:C-d on short buffer",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "<C-d>",
    ),
    c(
        "scroll:C-f on short buffer",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "<C-f>",
    ),
    c(
        "scroll:C-e on short buffer",
        &["a", "b", "c"],
        1,
        1,
        "<C-e>",
    ),
    c(
        "scroll:C-u on short buffer",
        &["a", "b", "c", "d", "e"],
        5,
        1,
        "<C-u>",
    ),
    c(
        "scroll:3<C-d> on short",
        &["a", "b", "c", "d", "e", "f", "g", "h"],
        1,
        1,
        "3<C-d>",
    ),
    c("scroll:C-d at last line", LONG, 60, 1, "<C-d>"),
    c("scroll:C-u at line 2", LONG, 2, 1, "<C-u>"),
];

// ─────────────────────────── N. word motions & misc motions ───────────────────────────
const CASES_WORD: &[Case] = &[
    c("word:w punctuation", &["foo.bar baz"], 1, 1, "w"),
    c("word:w on punct", &["foo.bar baz"], 1, 4, "w"),
    c("word:w punct run", &["a..b"], 1, 1, "w"),
    c("word:ww punct run", &["a..b"], 1, 1, "ww"),
    c("word:w to next line", &["foo", "  bar"], 1, 1, "w"),
    c("word:w at last word of buffer", &["foo bar"], 1, 5, "w"),
    c("word:w at end of buffer", &["foo bar"], 1, 7, "w"),
    c("word:w onto blank line", &["foo", "", "bar"], 1, 1, "w"),
    c("word:w from blank line", &["foo", "", "bar"], 2, 1, "w"),
    c("word:w over trailing spaces", &["foo   ", "bar"], 1, 1, "w"),
    c("word:3w", &["a b c d e"], 1, 1, "3w"),
    c("word:w underscore", &["foo_bar baz"], 1, 1, "w"),
    c("word:w digits", &["foo123 bar"], 1, 1, "w"),
    c("word:w a-b", &["a-b c"], 1, 1, "w"),
    c("word:W", &["a.b c"], 1, 1, "W"),
    c("word:e", &["foo bar"], 1, 1, "e"),
    c("word:ee", &["foo bar"], 1, 1, "ee"),
    c("word:e on last char", &["foo bar"], 1, 3, "e"),
    c("word:e punctuation", &["foo.bar"], 1, 1, "e"),
    c("word:ee punctuation", &["foo.bar"], 1, 1, "ee"),
    c("word:eee punctuation", &["foo.bar"], 1, 1, "eee"),
    c("word:e across blank line", &["foo", "", "bar"], 1, 3, "e"),
    c("word:E", &["a.b c.d"], 1, 1, "E"),
    c("word:b at start", &["foo"], 1, 1, "b"),
    c("word:b across line", &["foo", "bar"], 2, 1, "b"),
    c("word:b from mid word", &["foo bar"], 1, 6, "b"),
    c("word:b punctuation", &["foo.bar"], 1, 7, "b"),
    c("word:bb punctuation", &["foo.bar"], 1, 7, "bb"),
    c("word:bbb punctuation", &["foo.bar"], 1, 7, "bbb"),
    c("word:B", &["a.b c.d"], 1, 7, "B"),
    c("word:ge", &["foo bar"], 1, 5, "ge"),
    c("word:ge at start", &["foo bar"], 1, 1, "ge"),
    c("word:ge across lines", &["foo", "bar"], 2, 1, "ge"),
    c("word:ge onto blank", &["foo", "", "bar"], 3, 1, "ge"),
    c("word:ge punctuation", &["foo.bar"], 1, 5, "ge"),
    c("word:gE", &["a.b c.d"], 1, 5, "gE"),
    c("word:2e", &["a b c"], 1, 1, "2e"),
    c("word:2b", &["a b c"], 1, 5, "2b"),
    c("word:10w beyond", &["a b"], 1, 1, "10w"),
    c("word:ww single chars", &["a b c"], 1, 1, "ww"),
    c("word:e single char", &["a b"], 1, 1, "e"),
    c("word:w tabs", &["a\tb"], 1, 1, "w"),
    c("word:w on last char of line", &["ab", "cd"], 1, 2, "w"),
    c("word:b onto blank line", &["foo", "", "bar"], 3, 1, "b"),
    c("word:e at end of buffer", &["ab"], 1, 2, "e"),
    c(
        "word:w over multiple blank lines",
        &["a", "", "", "b"],
        1,
        1,
        "w",
    ),
    c(
        "word:ww over multiple blank lines",
        &["a", "", "", "b"],
        1,
        1,
        "ww",
    ),
    c(
        "word:w from whitespace-only line",
        &["a", "   ", "b"],
        2,
        1,
        "w",
    ),
    c(
        "word:e from whitespace-only line",
        &["a", "   ", "b"],
        2,
        1,
        "e",
    ),
    c(
        "word:) sentences",
        &["Hello world.  Second one.  Third."],
        1,
        1,
        ")",
    ),
    c(
        "word:)) sentences",
        &["Hello world.  Second one.  Third."],
        1,
        1,
        "))",
    ),
    c(
        "word:( sentence",
        &["Hello world.  Second one.  Third."],
        1,
        20,
        "(",
    ),
    c(
        "word:) single space",
        &["Hello world. Second one."],
        1,
        1,
        ")",
    ),
    c("word:) across lines", &["Hello.", "World."], 1, 1, ")"),
    c("word:( para", &["a", "", "b"], 3, 1, "("),
    c("word:) with ! ?", &["A! B? C."], 1, 1, ")"),
    c("word:) with quote", &["A.\" B."], 1, 1, ")"),
    c("word:) at end goes to eol", &["A. B."], 1, 4, ")"),
    c("word:}", &["a", "", "b", "", "c"], 1, 1, "}"),
    c("word:}}", &["a", "", "b", "", "c"], 1, 1, "}}"),
    c("word:}}}", &["a", "", "b", "", "c"], 1, 1, "}}}"),
    c("word:{", &["a", "", "b", "", "c"], 5, 1, "{"),
    c("word:2}", &["a", "", "b", "", "c"], 1, 1, "2}"),
    c("word:} multiple blanks", &["a", "", "", "b"], 1, 1, "}"),
    c("word:}} multiple blanks", &["a", "", "", "b"], 1, 1, "}}"),
    c("word:} from blank", &["a", "", "", "b"], 2, 1, "}"),
    c("word:{ at start", &["a", "b"], 2, 1, "{"),
    c(
        "word:} whitespace-only line not blank",
        &["a", "   ", "b", ""],
        1,
        1,
        "}",
    ),
    c("word:]]", &["{", "a", "}", "{", "b"], 1, 1, "]]"),
    c("word:[[", &["{", "a", "}", "{", "b"], 5, 1, "[["),
    c("word:[{", &["{", "a", "}"], 2, 1, "[{"),
    c("word:]}", &["{", "a", "}"], 2, 1, "]}"),
    c("word:[(", &["(a (b) c)"], 1, 5, "[("),
    c("word:])", &["(a (b) c)"], 1, 5, "])"),
    c("word:% on (", &["(a (b) c)"], 1, 1, "%"),
    c("word:% inside", &["(a (b) c)"], 1, 2, "%"),
    c("word:% on [", &["[a]"], 1, 1, "%"),
    c("word:% on ]", &["[a]"], 1, 3, "%"),
    c("word:% not found", &["abc"], 1, 1, "%"),
    c("word:% multiline", &["{", "a", "}"], 1, 1, "%"),
    c("word:% nested", &["((a))"], 1, 1, "%"),
    c("word:% on closing nested", &["((a))"], 1, 5, "%"),
    c("word:% in quotes", &["\"(\" )"], 1, 2, "%"),
    c("word:% on quote char", &["\"a\" (b)"], 1, 1, "%"),
    c("word:50%", LONG, 1, 1, "50%"),
    c("word:_", &["  ab"], 1, 4, "_"),
    c("word:2_", &["  ab", "  cd"], 1, 4, "2_"),
    c("word:+", &["  a", "  b"], 1, 1, "+"),
    c("word:-", &["  a", "  b"], 2, 1, "-"),
    c("word:<CR>", &["  a", "  b"], 1, 1, "<CR>"),
    c("word:4|", &["abcdef"], 1, 1, "4|"),
    c("word:|", &["abcdef"], 1, 4, "|"),
    c("word:2$", &["ab", "cd", "ef"], 1, 1, "2$"),
    c("word:$jj", &["abcdef", "ab", "abcdef"], 1, 1, "$jj"),
    c(
        "word:jj col memory",
        &["abcdef", "ab", "abcdef"],
        1,
        5,
        "jj",
    ),
    c("word:j col memory short", &["abcdef", "ab"], 1, 5, "j"),
    c("word:5l past end", &["abc"], 1, 1, "5l"),
    c("word:h at start", &["abc"], 1, 1, "h"),
    c("word:0", &["  ab"], 1, 4, "0"),
    c("word:^", &["  ab"], 1, 4, "^"),
    c("word:g_", &["ab  "], 1, 1, "g_"),
    c("word:gg indented (nosol)", &["  a", "b"], 2, 1, "gg"),
    cs(
        "word:gg indented (sol)",
        &["  a", "b"],
        2,
        1,
        "gg",
        "vim.o.startofline=true",
    ),
    c("word:G indented (nosol)", &["a", "  b"], 1, 1, "G"),
    cs(
        "word:G indented (sol)",
        &["a", "  b"],
        1,
        1,
        "G",
        "vim.o.startofline=true",
    ),
    c("word:5G", &["1", "2", "3", "4", "5", "6"], 1, 1, "5G"),
    c("word:5gg", &["1", "2", "3", "4", "5", "6"], 1, 1, "5gg"),
    c("word:10j beyond", &["a", "b", "c"], 1, 1, "10j"),
    c("word:10k beyond", &["a", "b", "c"], 3, 1, "10k"),
    c("word:w on empty buffer", &[""], 1, 1, "w"),
    c("word:x on empty buffer", &[""], 1, 1, "x"),
    c("word:dd on empty buffer", &[""], 1, 1, "dd"),
    c("word:yyp on empty buffer", &[""], 1, 1, "yyp"),
    c("word:$ then j to longer", &["ab", "abcdef"], 1, 1, "$j"),
    c(
        "word:$ then k then j",
        &["abcdef", "ab", "abcdef"],
        2,
        1,
        "$kj",
    ),
    c("word:gj gk nowrap", &["abc", "def", "ghi"], 1, 2, "gjgk"),
    c("word:go", &["ab", "cd"], 1, 1, "5go"),
    c("word:$ with count 1", &["ab", "cd"], 1, 1, "1$"),
    c("word:d$ then j col", &["abcdef", "abcdef"], 1, 3, "d$j"),
    c("word:x at eol then j", &["abc", "abcdef"], 1, 3, "xj"),
    c(
        "word:A esc then j col memory",
        &["ab", "abcdef"],
        1,
        1,
        "A<Esc>j",
    ),
    c("word:i esc then j", &["abcdef", "abcdef"], 1, 4, "i<Esc>j"),
    c("word:$ then h then j", &["abcdef", "abcdef"], 1, 1, "$hj"),
    // #1153 'virtualedit' — `$` still lands on the last character, but a
    // following `l` moves one column past it. Verified against
    // `nvim --headless`.
    c(
        "word:virtualedit=all lets l pass $",
        &["abc"],
        1,
        1,
        ":set ve=all<CR>$l",
    ),
    c("word:virtualedit off blocks l at $", &["abc"], 1, 1, "$l"),
    c("word:w then j col", &["ab cd", "abcdef"], 1, 1, "wj"),
    c("word:e then j", &["abc def", "abcdef"], 1, 1, "ej"),
    c("word:yy then j col", &["abcdef", "abcdef"], 1, 3, "yyj"),
    c("word:p then j col", &["abcdef", "abcdef"], 1, 3, "ylpj"),
    c(
        "word:dd then j col? (nosol)",
        &["abcdef", "abcdef", "abcdef"],
        1,
        3,
        "ddj",
    ),
    c("word:>> then j col", &["abcdef", "abcdef"], 1, 3, ">>j"),
    c("word:u then j col", &["abcdef", "abcdef"], 1, 3, "xuj"),
    c(
        "word:: then j col",
        &["abcdef", "abcdef"],
        1,
        3,
        ":noh<CR>j",
    ),
    c("word:/ then j col", &["abcdef", "abcdef"], 1, 1, "/c<CR>j"),
    c("word:zz then j col", &["abcdef", "abcdef"], 1, 3, "zzj"),
    c(
        "word:5G then j col (nosol)",
        &["abcdef", "abcdef", "abcdef"],
        1,
        3,
        "2Gj",
    ),
    cs(
        "word:5G then j col (sol)",
        &["abcdef", "abcdef", "abcdef"],
        1,
        3,
        "2Gj",
        "vim.o.startofline=true",
    ),
    c(
        "word:H then j col",
        &["abcdef", "abcdef", "abcdef"],
        2,
        3,
        "Hj",
    ),
    c("word:( ) with tab", &["A.\tB."], 1, 1, ")"),
    c("word:w on CJK? skip", &["a"], 1, 1, "l"),
    c("word:e on 2-char word end", &["ab cd"], 1, 2, "e"),
    c("word:cw on last char of word", &["ab cd"], 1, 2, "cwX<Esc>"),
    c(
        "word:w at eol with trailing space",
        &["ab ", "cd"],
        1,
        2,
        "w",
    ),
    c(
        "word:b from col1 of indented line",
        &["ab", "  cd"],
        2,
        3,
        "b",
    ),
    c(
        "word:b from start of indented line",
        &["ab", "  cd"],
        2,
        1,
        "b",
    ),
    c(
        "word:e from indented line start",
        &["ab", "  cd"],
        2,
        1,
        "e",
    ),
    c(
        "word:ge from indented line start",
        &["ab", "  cd"],
        2,
        1,
        "ge",
    ),
    c("word:W across lines", &["a.b", "c.d"], 1, 3, "W"),
    c("word:E across lines", &["a.b", "c.d"], 1, 3, "E"),
    c("word:B across lines", &["a.b", "c.d"], 2, 1, "B"),
    c("word:w over punct then blank", &["a.", "", "b"], 1, 2, "w"),
    c("word:dw over punct at eol", &["a.", "b"], 1, 2, "dw"),
    c("word:cw at eol punct", &["a.", "b"], 1, 2, "cwX<Esc>"),
    c("word:3e beyond", &["a b"], 1, 1, "3e"),
    c("word:w keyword vs nonkeyword @", &["a@b c"], 1, 1, "w"),
    c("word:w with iskeyword dash? -", &["a-b-c d"], 1, 1, "www"),
    // #1005: first UTF-8 multi-byte cases — see the CASES_OP `mb:` block
    // above for why starting columns are held to col 1 here.
    c(
        "word:mb:w stops at CJK boundary",
        &["foo日本語bar"],
        1,
        1,
        "w",
    ),
    // A companion "w does not split on cyrillic" case (`helloжworld next`)
    // was investigated and dropped: nvim's `col()` reports byte offset 14
    // there, this engine's char-based cursor reports 13 — the byte/char
    // architecture gap documented in the CASES_OP `mb:` block, hit here
    // because the Cyrillic char precedes the final cursor position rather
    // than being it. Reported as a follow-up rather than added — see the PR
    // description.
    c(
        "word:mb:e stops before CJK run",
        &["foo日本語bar"],
        1,
        1,
        "e",
    ),
    c(
        "word:mb:wb round-trips ascii/CJK boundary",
        &["foo日本語bar"],
        1,
        1,
        "wb",
    ),
    c(
        "word:mb:w then ge crosses back over CJK",
        &["foo日本語bar"],
        1,
        1,
        "wge",
    ),
    // #1191: 'iskeyword' is a real input to word motions — `a-b` is two
    // words under the default `'iskeyword'` (see "word:w a-b" above), but
    // one word once `-` is added to the keyword class, exactly like real
    // Vim. Before #1191, `vim.o.iskeyword=...` in a case's `setup` could
    // only ever configure the oracle: `apply_setup` had no arm for it, so
    // this case would have errored out of `every_corpus_setup_is_understood`
    // (a hard failure, not a silent default) rather than compare a
    // configured vimcode against a configured oracle.
    cs(
        "word:w with iskeyword+=- treats hyphen as a word char",
        &["foo-bar baz"],
        1,
        1,
        "w",
        "vim.o.iskeyword='@,48-57,_,192-255,-'",
    ),
    cs(
        "word:e with iskeyword+=- treats hyphen as a word char",
        &["foo-bar baz"],
        1,
        1,
        "e",
        "vim.o.iskeyword='@,48-57,_,192-255,-'",
    ),
    cs(
        "word:b with iskeyword+=- treats hyphen as a word char",
        &["foo-bar baz"],
        1,
        9,
        "b",
        "vim.o.iskeyword='@,48-57,_,192-255,-'",
    ),
];

// ─────────────────────────── O. text objects ───────────────────────────
const CASES_TO: &[Case] = &[
    c("to:daw mid", &["foo bar baz"], 1, 5, "daw"),
    c("to:daw start", &["foo bar baz"], 1, 1, "daw"),
    c("to:daw last word", &["foo bar baz"], 1, 9, "daw"),
    c("to:daw on whitespace", &["foo  bar"], 1, 4, "daw"),
    c("to:diw on whitespace", &["foo  bar"], 1, 4, "diw"),
    c("to:diw punctuation", &["foo.bar"], 1, 4, "diw"),
    c("to:daw punctuation", &["foo.bar"], 1, 4, "daw"),
    c("to:d2aw", &["a b c d"], 1, 1, "d2aw"),
    c("to:d3iw", &["a b c d"], 1, 1, "d3iw"),
    c("to:c2aw", &["a b c d"], 1, 1, "c2awX<Esc>"),
    c("to:daw single word line", &["foo"], 1, 1, "daw"),
    c("to:daw leading space only", &["  foo"], 1, 3, "daw"),
    c("to:daW", &["foo.bar baz"], 1, 2, "daW"),
    c("to:diW", &["foo.bar baz"], 1, 2, "diW"),
    c("to:das", &["One two.  Three four.  Five."], 1, 12, "das"),
    c("to:dis", &["One two.  Three four.  Five."], 1, 12, "dis"),
    c(
        "to:das last sentence",
        &["One two.  Three four."],
        1,
        12,
        "das",
    ),
    c(
        "to:das first sentence",
        &["One two.  Three four."],
        1,
        2,
        "das",
    ),
    c(
        "to:dis on whitespace between",
        &["One two.  Three four."],
        1,
        10,
        "dis",
    ),
    c("to:dip", &["a", "b", "", "c"], 1, 1, "dip"),
    c("to:dap", &["a", "b", "", "c"], 1, 1, "dap"),
    c(
        "to:dap trailing no blank",
        &["a", "", "b", "c"],
        3,
        1,
        "dap",
    ),
    c("to:dip on blank lines", &["a", "", "", "b"], 2, 1, "dip"),
    c("to:dap on blank", &["a", "", "", "b"], 2, 1, "dap"),
    c("to:d2ap", &["a", "", "b", "", "c"], 1, 1, "d2ap"),
    c("to:yap cursor", &["a", "", "b", "c"], 3, 1, "yap"),
    c("to:yip cursor", &["a", "", "b", "c"], 4, 1, "yip"),
    c("to:dip at last para", &["a", "", "b"], 3, 1, "dip"),
    c("to:dap only para", &["a", "b"], 1, 1, "dap"),
    c("to:di( inside", &["f(a, b)"], 1, 3, "di("),
    c("to:di( on (", &["f(a, b)"], 1, 2, "di("),
    c("to:di( on )", &["f(a, b)"], 1, 7, "di("),
    c("to:di( nested inner", &["f(a, (b), c)"], 1, 7, "di("),
    c("to:d2i(", &["f(a, (b), c)"], 1, 7, "d2i("),
    c("to:da( nested", &["f(a, (b), c)"], 1, 7, "da("),
    c("to:di( before paren same line", &["x f(a)"], 1, 1, "di("),
    c("to:di( not inside", &["abc"], 1, 1, "di("),
    c("to:dib", &["f(a)"], 1, 3, "dib"),
    c("to:diB", &["f{a}"], 1, 3, "diB"),
    c("to:di{ multiline", &["{", "  a", "  b", "}"], 2, 3, "di{"),
    c("to:da{ multiline", &["{", "  a", "  b", "}"], 2, 3, "da{"),
    c(
        "to:ci{ multiline",
        &["{", "  a", "  b", "}"],
        2,
        3,
        "ci{X<Esc>",
    ),
    c("to:di{ same line", &["f {a}"], 1, 4, "di{"),
    c(
        "to:di{ on line with brace and text",
        &["if (x) {", "  a", "}"],
        2,
        3,
        "di{",
    ),
    c(
        "to:yi{ cursor multiline",
        &["{", "  a", "  b", "}"],
        3,
        3,
        "yi{",
    ),
    c("to:di[", &["a[1]"], 1, 3, "di["),
    c("to:da[", &["a[1]"], 1, 3, "da["),
    c("to:di\" inside", &["x \"ab\" y"], 1, 4, "di\""),
    c("to:di\" on opening", &["x \"ab\" y"], 1, 3, "di\""),
    c("to:di\" on closing", &["x \"ab\" y"], 1, 6, "di\""),
    c("to:di\" before quotes", &["x \"ab\" y"], 1, 1, "di\""),
    c("to:da\" before quotes", &["x \"ab\" y"], 1, 1, "da\""),
    c("to:di\" escaped", &["\"a\\\"b\""], 1, 2, "di\""),
    c(
        "to:di\" between two strings",
        &["\"a\" x \"b\""],
        1,
        5,
        "di\"",
    ),
    c("to:di\" second string", &["\"a\" x \"b\""], 1, 8, "di\""),
    c("to:di'", &["x 'ab' y"], 1, 4, "di'"),
    c("to:di`", &["x `ab` y"], 1, 4, "di`"),
    c("to:ci\"", &["x \"ab\" y"], 1, 4, "ci\"X<Esc>"),
    c("to:yi\" cursor", &["x \"ab\" y"], 1, 5, "yi\""),
    c("to:di\" empty string", &["x \"\" y"], 1, 4, "di\""),
    c("to:di\" after last quote", &["\"ab\" y"], 1, 6, "di\""),
    c(
        "to:da\" with leading and trailing ws",
        &["a  \"b\"  c"],
        1,
        5,
        "da\"",
    ),
    c("to:dit", &["<a><b>x</b></a>"], 1, 7, "dit"),
    c("to:dat", &["<a><b>x</b></a>"], 1, 7, "dat"),
    c("to:d2it", &["<a><b>x</b></a>"], 1, 7, "d2it"),
    c("to:dit on tag", &["<a><b>x</b></a>"], 1, 2, "dit"),
    c("to:dit multiline", &["<div>", "  x", "</div>"], 2, 3, "dit"),
    c("to:cit", &["<a>x</a>"], 1, 4, "citY<Esc>"),
    c("to:dit with attrs", &["<a href=\"x\">y</a>"], 1, 15, "dit"),
    c(
        "to:dat self-closing inside",
        &["<a><br/>x</a>"],
        1,
        9,
        "dat",
    ),
    c("to:d5aw too many", &["a b"], 1, 1, "d5aw"),
    c("to:diw single char", &["a b"], 1, 1, "diw"),
    c("to:daw leading whitespace", &["  foo bar"], 1, 1, "daw"),
    c("to:daw eol trailing space", &["foo bar "], 1, 5, "daw"),
    c("to:ciw on whitespace", &["a   b"], 1, 3, "ciwX<Esc>"),
    c("to:cip", &["a", "b", "", "c"], 1, 1, "cipX<Esc>"),
    c("to:di( across lines", &["f(a,", "  b)"], 1, 3, "di("),
    c("to:yi( cursor", &["f(a, b)"], 1, 5, "yi("),
    c("to:ya( cursor", &["f(a, b)"], 1, 5, "ya("),
    c("to:daw punctuation attached", &["foo, bar"], 1, 1, "daw"),
    c("to:2daw", &["a b c d"], 1, 1, "2daw"),
    c("to:daw at end with count", &["a b c"], 1, 5, "d2aw"),
    c("to:di< nested", &["<a<b>c>"], 1, 5, "di<"),
    c("to:da< outer", &["<a<b>c>"], 1, 2, "da<"),
    c("to:di( empty", &["f()"], 1, 2, "di("),
    c("to:ci( empty", &["f()"], 1, 2, "ci(X<Esc>"),
    c("to:da( empty", &["f()"], 1, 2, "da("),
    c(
        "to:di( with newline after open",
        &["f(", "a", "b)"],
        2,
        1,
        "di(",
    ),
    c("to:da{ with trailing text", &["x {a} y"], 1, 4, "da{"),
    c("to:diw at eol on space", &["foo "], 1, 4, "diw"),
    c("to:daw on multi spaces at start", &["   foo"], 1, 1, "daw"),
    c(
        "to:dis multi-line sentence",
        &["One two", "three.  Four."],
        1,
        1,
        "dis",
    ),
    c(
        "to:das multi-line",
        &["One two", "three.  Four."],
        1,
        1,
        "das",
    ),
    c(
        "to:dip with indented lines",
        &["  a", "  b", "", "c"],
        1,
        1,
        "dip",
    ),
    c("to:vipJ", &["a", "b", "", "c"], 1, 1, "vipJ"),
    c("to:>ap", &["a", "b", "", "c"], 1, 1, ">ap"),
    c("to:=ip", &["  a", "    b", "", "c"], 1, 1, "=ip"),
    c("to:gUip", &["a", "b", "", "c"], 1, 1, "gUip"),
    c("to:yi\" then P", &["x \"ab\" y"], 1, 4, "yi\"P"),
    c("to:ci' then .", &["'a' 'b'"], 1, 2, "ci'X<Esc>4l."),
    c(
        "to:di\" cursor on quote when 3 quotes",
        &["a \"b\" c \" d"],
        1,
        8,
        "di\"",
    ),
    c("to:dit nested same tag", &["<a><a>x</a></a>"], 1, 7, "dit"),
    c("to:dat on closing tag", &["<a>x</a> y"], 1, 6, "dat"),
    c("to:dit no tag", &["abc"], 1, 1, "dit"),
    c("to:di( count 3 too many", &["(a)"], 1, 2, "d3i("),
    c("to:daw on only whitespace line", &["   "], 1, 2, "daw"),
    c("to:diw digits", &["ab 12 cd"], 1, 5, "diw"),
    c(
        "to:dip cursor after",
        &["a", "", "b", "c", "", "d"],
        3,
        1,
        "dip",
    ),
    c(
        "to:dap cursor after",
        &["a", "", "b", "c", "", "d"],
        3,
        1,
        "dap",
    ),
    // #1005: first UTF-8 multi-byte cases. Cursor starts at col 4, on the
    // CJK run — the preceding "foo" is pure ASCII, so col 4 is a valid
    // starting column on both sides (see the CASES_OP `mb:` block's doc).
    c(
        "to:mb:diw on CJK run leaves ascii neighbors",
        &["foo日本語bar"],
        1,
        4,
        "diw",
    ),
    c(
        "to:mb:daw on CJK run leaves ascii neighbors",
        &["foo日本語bar"],
        1,
        4,
        "daw",
    ),
    // #1191: `iw`/`aw` (`find_word_object`) are the third documented
    // 'iskeyword' consumer — with `-` added to the keyword class, the whole
    // hyphenated run is one word, so `diw` from inside "bar" deletes all of
    // "foo-bar", not just "bar".
    cs(
        "to:diw with iskeyword+=- deletes the whole hyphenated word",
        &["foo-bar baz"],
        1,
        5,
        "diw",
        "vim.o.iskeyword='@,48-57,_,192-255,-'",
    ),
];

// ─────────────────────────── P. misc ───────────────────────────
const CASES_MISC: &[Case] = &[
    c("misc:5p charwise", &["ab"], 1, 1, "yl5p"),
    c("misc:p at last line linewise", &["a", "b"], 2, 1, "yyp"),
    c("misc:P at first", &["a", "b"], 1, 1, "yyP"),
    c("misc:yyP cursor", &["  a", "b"], 1, 3, "yyP"),
    c("misc:ddP", &["a", "b", "c"], 2, 1, "ddP"),
    c("misc:2yy P", &["a", "b", "c"], 2, 1, "2yyP"),
    c(
        "misc:p multi-line charwise cursor",
        &["ab", "cd"],
        1,
        1,
        "vjy$p",
    ),
    c("misc:gp charwise multi", &["ab", "cd"], 1, 1, "vjy$gp"),
    c("misc:gP charwise", &["abc"], 1, 2, "ylgP"),
    c("misc:2gp", &["a", "b"], 1, 1, "yy2gp"),
    c(
        "misc:p with count linewise cursor",
        &["a", "b"],
        1,
        1,
        "yy3p",
    ),
    c("misc:P count", &["ab"], 1, 2, "yl3P"),
    c("misc:xp end", &["ab"], 1, 2, "xp"),
    c("misc:deep count 100x", &["abc"], 1, 1, "100x"),
    c("misc:count on i then esc col", &["abc"], 1, 3, "3ix<Esc>"),
    c("misc:count 0 not a count", &["abc def"], 1, 5, "0"),
    c("misc:10 then 0", &["abcdefghijklmnop"], 1, 1, "10l0"),
    c("misc:d10l", &["abcdefghijklmnop"], 1, 1, "d10l"),
    c("misc:20|", &["abc"], 1, 1, "20|"),
    c("misc:~ count past eol cursor", &["abc"], 1, 1, "10~"),
    c("misc:J with 2 count", &["a", "b", "c"], 1, 1, "2J"),
    c("misc:r on empty line", &["", "a"], 1, 1, "rx"),
    c("misc:r Tab", &["ab"], 1, 1, "r<Tab>"),
    c(
        "misc:R at eol then BS",
        &["ab"],
        1,
        2,
        "Rxyz<BS><BS><BS><BS><Esc>",
    ),
    c("misc:s on empty line", &[""], 1, 1, "sX<Esc>"),
    c("misc:cw on space at eol", &["ab "], 1, 3, "cwX<Esc>"),
    c("misc:D then p", &["abc"], 1, 2, "Dp"),
    c("misc:C then .", &["abc", "def"], 1, 2, "CX<Esc>j0."),
    c("misc:cc then p", &["  a", "b"], 1, 1, "ccX<Esc>jp"),
    c("misc:S then P", &["a", "b"], 1, 1, "SX<Esc>jP"),
    c(
        "misc:& after &&",
        &["a a a", "a a a"],
        1,
        1,
        ":s/a/b/g<CR>j&",
    ),
    c(
        "misc:g& after range",
        &["a", "a", "a"],
        1,
        1,
        ":1s/a/b/<CR>g&",
    ),
    c("misc:: then Esc", &["abc"], 1, 2, ":<Esc>x"),
    c("misc:/ then Esc", &["abc"], 1, 2, "/b<Esc>x"),
    c("misc:d then Esc", &["abc"], 1, 2, "d<Esc>x"),
    c("misc:2d then Esc", &["abc"], 1, 2, "2d<Esc>x"),
    c("misc:3 then Esc then x", &["abcdef"], 1, 1, "3<Esc>x"),
    c("misc:\"a then Esc then x", &["abcdef"], 1, 1, "\"a<Esc>x"),
    c("misc:q then Esc", &["abc"], 1, 1, "q<Esc>x"),
    c("misc:m then Esc", &["abc"], 1, 1, "m<Esc>x"),
    c("misc:' then Esc", &["abc"], 1, 1, "'<Esc>x"),
    c("misc:g then Esc", &["abc"], 1, 1, "g<Esc>x"),
    c("misc:z then Esc", &["abc"], 1, 1, "z<Esc>x"),
    c("misc:f then Esc", &["abc"], 1, 1, "f<Esc>x"),
    c("misc:r then Esc", &["abc"], 1, 1, "r<Esc>x"),
    c("misc:ci then Esc", &["abc"], 1, 1, "ci<Esc>x"),
    c("misc:C-w then Esc", &["abc"], 1, 1, "<C-w><Esc>x"),
    c("misc:Esc in normal no-op", &["abc"], 1, 2, "<Esc>"),
    c("misc:: with count", &["a", "b", "c", "d"], 1, 1, "3:d<CR>"),
    c("misc:3:s", &["a", "a", "a", "a"], 1, 1, "3:s/a/b/<CR>"),
    c("misc:Q skip", &["a"], 1, 1, "l"),
    c("misc:gv after p", &["ab", "cd"], 1, 1, "yyjVpgvd"),
    c("misc:C-l noop", &["abc"], 1, 2, "<C-l>x"),
    c("misc:C-c in insert", &["abc"], 1, 1, "ix<C-c>x"),
    c("misc:C-[ in insert", &["abc"], 1, 1, "ix<C-[>x"),
    c("misc:insert Esc with count 0", &["abc"], 1, 1, "0ix<Esc>"),
    c(
        "misc:count then : then range",
        &["a", "b", "c"],
        1,
        1,
        "2:normal Ax<CR>",
    ),
    c("misc:d3d? invalid", &["a", "b", "c", "d", "e"], 1, 1, "d3d"),
    c("misc:2d2d", &["a", "b", "c", "d", "e", "f"], 1, 1, "2d2d"),
    c("misc:y then y with count", &["a", "b", "c"], 1, 1, "y2yP"),
    c("misc:c3c", &["a", "b", "c", "d"], 1, 1, "c3cX<Esc>"),
    c("misc:>3>", &["a", "b", "c", "d"], 1, 1, ">3>"),
    c("misc:g~3~", &["a", "b", "c", "d"], 1, 1, "g~3~"),
    c("misc:gu2u", &["A", "B", "C"], 1, 1, "gu2u"),
    c("misc:gU2U", &["a", "b", "c"], 1, 1, "gU2U"),
    c("misc:gUgU", &["a b"], 1, 1, "gUgU"),
    c("misc:gugu", &["A B"], 1, 1, "gugu"),
    c("misc:g~g~", &["aB"], 1, 1, "g~g~"),
    c("misc:g?g?", &["ab"], 1, 1, "g?g?"),
    c(
        "misc:gqgq",
        &["one two three four five six"],
        1,
        1,
        ":set tw=10<CR>gqgq",
    ),
    c(
        "misc:gwgw",
        &["one two three four five six"],
        1,
        8,
        ":set tw=10<CR>gwgw",
    ),
    c(
        "misc:d then d with register",
        &["a", "b"],
        1,
        1,
        "\"add\"ap",
    ),
    c("misc:ZZ skip", &["a"], 1, 1, "l"),
    c(
        "misc:. after :normal",
        &["ab", "cd"],
        1,
        1,
        ":normal x<CR>j.",
    ),
    c(
        "misc:. after :g normal",
        &["ab", "cd", "ef"],
        1,
        1,
        ":1,2g/./normal x<CR>G.",
    ),
    c("misc:xp then . ", &["abcd"], 1, 1, "xp."),
    c(
        "misc:dot after @: ",
        &["a", "b", "c", "d"],
        1,
        1,
        ":d<CR>@:.",
    ),
    c("misc:tilde op setting? tildeop skip", &["ab"], 1, 1, "l"),
    c(
        "misc:count with text object c",
        &["a b c d"],
        1,
        1,
        "3ciwX<Esc>",
    ),
    c(
        "misc:count before and after with textobj",
        &["a b c d e f g"],
        1,
        1,
        "2d2aw",
    ),
    c("misc:count on ) motion", &["A. B. C. D."], 1, 1, "2)"),
    c("misc:d2)", &["A. B. C. D."], 1, 1, "d2)"),
    c("misc:d2}", &["a", "", "b", "", "c"], 1, 1, "d2}"),
    c("misc:y2j P", &["a", "b", "c", "d"], 1, 1, "y2jGP"),
    c("misc:2yy 3p", &["a", "b"], 1, 1, "2yy3p"),
    c("misc:3J then u", &["a", "b", "c", "d"], 1, 1, "3Ju"),
    c("misc:count 2 on ~", &["ab"], 1, 1, "2~"),
    c("misc:2r", &["abc"], 1, 1, "2rx"),
    c("misc:5J on 3 lines", &["a", "b", "c"], 1, 1, "5J"),
    c("misc:x on last char then p", &["ab"], 1, 2, "xP"),
    c("misc:dd on last then P", &["a", "b"], 2, 1, "ddP"),
    c("misc:2dd on last", &["a", "b"], 2, 1, "2dd"),
    c(
        "misc:cc on last line indent",
        &["a", "  b"],
        2,
        3,
        "ccX<Esc>",
    ),
    c("misc:cc with count beyond", &["a", "b"], 1, 1, "5ccX<Esc>"),
    c("misc:5>>", &["a", "b"], 1, 1, "5>>"),
    c("misc:5J last", &["a", "b"], 2, 1, "5J"),
    c("misc:d then count then motion 0", &["abc def"], 1, 5, "d0"),
    c("misc:count then i on line start", &["ab"], 1, 1, "2Ix<Esc>"),
    c("misc:count then o with indent", &["  a"], 1, 1, "2ox<Esc>"),
    // #1005: first UTF-8 multi-byte case for pure cursor-column reporting
    // (category 7 of the issue). `$` on a line ending in a single 2-byte
    // char is one of the few multi-byte final-column assertions that does
    // NOT hit the byte-vs-char architecture gap documented in the CASES_OP
    // `mb:` block: nvim's `col()` is a byte offset and this engine's cursor
    // column is a char offset, and the two coincide here because nothing
    // *before* the final cursor position is multi-byte (the accented char
    // IS the final position, so only its own multi-byte-ness would matter,
    // and a character's own width never affects the column *of* that
    // character — only of anything after it on the line). A case built to
    // actually cross that boundary (e.g. `$` on an all-Cyrillic line) was
    // investigated and does diverge — see the PR description for that
    // follow-up.
    c(
        "misc:mb:dollar reports column after multi-byte-final line",
        &["café"],
        1,
        1,
        "$",
    ),
];

// ─────────────────────────── Q. folds (#1006) ───────────────────────────
//
// Folds are established through the case `setup` mechanism (#1002's
// `apply_setup`, extended here to map `'foldmethod'`/`'foldlevel'` onto
// `Settings`), applied to both sides — never a bespoke runner escape hatch.
// Manual folds (`'foldmethod'` defaults to `"manual"`, matching Vim) are
// created in-band by the case's own `keys` (`zf{motion}`); indent folds set
// `vim.o.foldmethod='indent'` (+ optionally `vim.o.foldlevel=N`) in `setup`,
// which `run_in_vimcode` applies via `Engine::apply_foldlevel` once the
// buffer exists (mirroring how Neovim computes that method's hierarchy as
// soon as the option is set, with no `zf` involved).
//
// Folding never changes buffer text, so every case here is read through the
// cursor position after a motion/operator whose result depends on fold
// state — that's what actually exercises vimcode's fold-aware
// `next_visible_line`/`prev_visible_line` (used by `scroll_and_move_by` and
// scroll-top snapping in `src/core/engine/motions.rs`/`accessors.rs`), not
// just the fold commands in isolation.
//
// Verified by hand against `nvim --headless -u NONE` before trusting the
// harness (#1006's "verify by hand" note): both the `foldmethod=manual`
// zf/zo/zc round trip and the `foldmethod=indent`/`foldlevel` nested-range
// math below were probed directly against a real Neovim process, not just
// inferred from `:h fold` — see the two nested/#1006 engine fixes in
// `src/core/view.rs` and `src/core/engine/motions.rs` this issue's slice
// shipped as a result (folds no longer forget their definition on `zo`, and
// a nested closed fold no longer gets silently absorbed by its parent).

const FOLDTXT: &[&str] = &[
    "one", "two", "three", "four", "five", "six", "seven", "eight",
];

const FOLDPARA: &[&str] = &["alpha", "beta", "", "gamma", "delta", ""];

// Two-level nested indent fixture (shiftwidth 4, both harnesses set it) —
// verified line-for-line against `nvim --headless` with
// `foldmethod=indent`: at `foldlevel=0` lines 2-7 close as one fold (the
// "fn main() {" header on line 1 is NOT itself folded — Vim's indent method
// gives every line its own level, `indent/shiftwidth`, independent of its
// neighbors, so an unindented header is level 0); at `foldlevel=1` that
// fold is open but the nested lines 4-5 are still closed; at
// `foldlevel=2` nothing is closed.
const FOLDNEST: &[&str] = &[
    "fn main() {",
    "    let a = 1;",
    "    if true {",
    "        x();",
    "        y();",
    "    }",
    "    let b = 2;",
    "}",
];

// Two-level nested `foldmethod=marker` fixture (#1159), companion to
// FOLDNEST above but nested via `{{{`/`}}}` pairs instead of indentation.
// Verified line-for-line against `nvim --headless`: at `foldlevel=0` lines
// 1-6 close as one fold (level 1 — the marker on line 1 itself, unlike the
// indent method's header line, IS part of its own fold, since the marker
// pair's *first* line is what opens the region); at `foldlevel=1` that fold
// is open but the nested lines 2-4 are still closed (level 2); at
// `foldlevel=2` nothing is closed.
const FOLDMARKERNEST: &[&str] = &[
    "fn main() { // {{{",
    "    if true { // {{{",
    "        x();",
    "        } // }}}",
    "    let a = 1;",
    "} // }}}",
    "// trailing",
];

// Minimal fixture for a non-default `'foldmarker'` pair (#1159).
const FOLDMARKER_CUSTOM: &[&str] = &["alpha [[[", "beta", "]]]", "gamma"];

const CASES_FOLD: &[Case] = &[
    // ── manual folds: zf{motion} ─────────────────────────────────────────
    c("fold:zfj hides one line", FOLDTXT, 1, 1, "zfjj"),
    c("fold:zf2j hides two lines", FOLDTXT, 1, 1, "zf2jj"),
    c("fold:zfap folds paragraph", FOLDPARA, 1, 1, "zfapj"),
    // ── zo/zc/za round trip (#1006) ──────────────────────────────────────
    c("fold:zo reopens a closed fold", FOLDTXT, 1, 1, "zfjzoj"),
    c(
        "fold:zo then zc recloses the same fold",
        FOLDTXT,
        1,
        1,
        "zfjzozcj",
    ),
    c(
        "fold:za closes an open defined fold",
        FOLDTXT,
        1,
        1,
        "zfjzozaj",
    ),
    c("fold:za opens a closed fold", FOLDTXT, 1, 1, "zfjzaj"),
    // ── zR/zM ─────────────────────────────────────────────────────────────
    c("fold:zR opens all folds", FOLDTXT, 1, 1, "zfjzRj"),
    c("fold:zM recloses a defined fold", FOLDTXT, 1, 1, "zfjzozMj"),
    // ── zO/zC ─────────────────────────────────────────────────────────────
    c("fold:zO opens recursively", FOLDTXT, 1, 1, "zfjzOj"),
    c("fold:zC recloses recursively", FOLDTXT, 1, 1, "zfjzozCj"),
    // ── zj/zk fold navigation ────────────────────────────────────────────
    c(
        "fold:zj moves to the defined fold header",
        FOLDTXT,
        3,
        1,
        "zfjggzj",
    ),
    c(
        "fold:zk moves to the defined fold header",
        FOLDTXT,
        3,
        1,
        "zfjGzk",
    ),
    // ── [z/]z: boundaries of the current open fold ───────────────────────
    c(
        "fold:]z moves to end of open fold",
        FOLDTXT,
        3,
        1,
        "zf2jzoj]z",
    ),
    c(
        "fold:[z moves to start of open fold",
        FOLDTXT,
        3,
        1,
        "zf2jzo2j[z",
    ),
    // ── zd/zD ─────────────────────────────────────────────────────────────
    c("fold:zd deletes a fold", FOLDTXT, 1, 1, "zfjzdj"),
    c(
        "fold:zD deletes a fold recursively",
        FOLDTXT,
        1,
        1,
        "jzfjkzf3jzDj",
    ),
    // ── operators on a closed fold ───────────────────────────────────────
    c(
        "fold:dd on closed fold deletes every line",
        FOLDTXT,
        1,
        1,
        "zfjdd",
    ),
    c(
        "fold:yy p on closed fold yanks every line",
        FOLDTXT,
        1,
        1,
        "zfjyyGp",
    ),
    c(
        "fold:J on closed fold joins every line",
        FOLDTXT,
        1,
        1,
        "zfjJ",
    ),
    // ── scroll interaction (next_visible_line is load-bearing here) ──────
    c(
        "fold:C-d skips a closed fold in the viewport",
        LONG,
        10,
        1,
        "zf10j<C-d>",
    ),
    c(
        "fold:C-e with a closed fold in the viewport",
        LONG,
        10,
        1,
        "zf10j<C-e>",
    ),
    c("fold:zt then L past a closed fold", LONG, 10, 1, "zf10jztL"),
    c(
        "fold:zz then H before a closed fold",
        LONG,
        30,
        1,
        "10Gzf10j30GzzH",
    ),
    // ── foldmethod=indent / foldlevel ────────────────────────────────────
    // #1153: foldmethod/foldlevel are now reachable from the real `:set`
    // command (previously only `apply_setup`'s Lua-setup bypass could set
    // them for this suite — see the issue: "exist in settings.json but are
    // not reachable from :set"). This case drives the actual `:set fdm=
    // indent<CR>` ex command through both sides, unlike the `cs(..)` cases
    // below it which pin the option before the key sequence starts.
    c(
        "fold:indent:set fdm=indent via :set",
        FOLDNEST,
        1,
        1,
        ":set fdm=indent<CR>j",
    ),
    cs(
        "fold:indent:foldlevel0 j crosses the whole outer fold",
        FOLDNEST,
        1,
        1,
        "j",
        "vim.o.foldmethod='indent'\nvim.o.foldlevel=0",
    ),
    cs(
        "fold:indent:foldlevel1 j steps to the nested fold header",
        FOLDNEST,
        1,
        1,
        "jjj",
        "vim.o.foldmethod='indent'\nvim.o.foldlevel=1",
    ),
    cs(
        "fold:indent:foldlevel1 j skips the closed nested fold",
        FOLDNEST,
        1,
        1,
        "jjjj",
        "vim.o.foldmethod='indent'\nvim.o.foldlevel=1",
    ),
    cs(
        "fold:indent:foldlevel2 nothing is folded",
        FOLDNEST,
        1,
        1,
        "jjjj",
        "vim.o.foldmethod='indent'\nvim.o.foldlevel=2",
    ),
    cs(
        "fold:indent:zR opens everything",
        FOLDNEST,
        1,
        1,
        "zRjjjj",
        "vim.o.foldmethod='indent'",
    ),
    cs(
        "fold:indent:zM recloses after zR",
        FOLDNEST,
        1,
        1,
        "zRzMj",
        "vim.o.foldmethod='indent'",
    ),
    cs(
        "fold:indent:zo opens the level-1 fold, inner stays closed",
        FOLDNEST,
        2,
        1,
        "zoj",
        "vim.o.foldmethod='indent'",
    ),
    cs(
        "fold:indent:zc recloses the level-1 fold",
        FOLDNEST,
        2,
        1,
        "zozcj",
        "vim.o.foldmethod='indent'",
    ),
    // ── 'foldnestmax' (#1159) — only affects "indent"/"syntax", not
    // "marker" (`:h 'foldnestmax'`; verified against `nvim --headless`:
    // capping FOLDNEST's would-be level-2 inner fold to foldnestmax=1
    // absorbs it into the level-1 outer fold instead of leaving it its own
    // closeable region, so at foldlevel=1 the inner lines are *not* closed
    // — contrast the default-nestmax case right below it, where they are).
    cs(
        "fold:indent:foldnestmax=1 absorbs the level-2 fold into level-1",
        FOLDNEST,
        1,
        1,
        "jjjj",
        "vim.o.foldmethod='indent'\nvim.o.foldnestmax=1\nvim.o.foldlevel=1",
    ),
    cs(
        "fold:indent:default foldnestmax leaves the level-2 fold closed",
        FOLDNEST,
        1,
        1,
        "jjjj",
        "vim.o.foldmethod='indent'\nvim.o.foldlevel=1",
    ),
    // ── foldmethod=marker (#1159) ────────────────────────────────────────
    c(
        "fold:marker:set fdm=marker via :set",
        FOLDMARKERNEST,
        1,
        1,
        ":set fdm=marker<CR>j",
    ),
    cs(
        "fold:marker:foldlevel0 j crosses the whole outer fold",
        FOLDMARKERNEST,
        1,
        1,
        "j",
        "vim.o.foldmethod='marker'\nvim.o.foldlevel=0",
    ),
    cs(
        "fold:marker:foldlevel1 j steps to the nested fold header",
        FOLDMARKERNEST,
        1,
        1,
        "jj",
        "vim.o.foldmethod='marker'\nvim.o.foldlevel=1",
    ),
    cs(
        "fold:marker:foldlevel1 j skips the closed nested fold",
        FOLDMARKERNEST,
        1,
        1,
        "jjj",
        "vim.o.foldmethod='marker'\nvim.o.foldlevel=1",
    ),
    cs(
        "fold:marker:foldlevel2 nothing is folded",
        FOLDMARKERNEST,
        1,
        1,
        "jjjjjj",
        "vim.o.foldmethod='marker'\nvim.o.foldlevel=2",
    ),
    cs(
        "fold:marker:zR opens everything",
        FOLDMARKERNEST,
        1,
        1,
        "zRjjjjjj",
        "vim.o.foldmethod='marker'",
    ),
    cs(
        "fold:marker:zM recloses after zR",
        FOLDMARKERNEST,
        1,
        1,
        "zRzMj",
        "vim.o.foldmethod='marker'",
    ),
    cs(
        "fold:marker:zo opens the level-1 fold, inner stays closed",
        FOLDMARKERNEST,
        1,
        1,
        "zoj",
        "vim.o.foldmethod='marker'",
    ),
    cs(
        "fold:marker:zc recloses the level-1 fold",
        FOLDMARKERNEST,
        1,
        1,
        "zozcj",
        "vim.o.foldmethod='marker'",
    ),
    cs(
        "fold:marker:custom foldmarker pair",
        FOLDMARKER_CUSTOM,
        1,
        1,
        "j",
        "vim.o.foldmethod='marker'\nvim.o.foldmarker='[[[,]]]'",
    ),
    // ── `:fold*` ex commands (#1159) — all on FOLDTXT/manual folds, driven
    // through the real ex-command path so they exercise
    // `try_execute_fold_command`/`ex_fold_create`/`ex_fold_open_close`
    // directly, the same way "fold:indent:set fdm=indent via :set" above
    // exercises the `:set` path rather than the `cs(..)` setup bypass.
    c(
        "fold:ex::fold creates and closes a manual fold",
        FOLDTXT,
        1,
        1,
        ":2,4fold<CR>jj",
    ),
    c(
        "fold:ex::foldopen with a matching range reopens it",
        FOLDTXT,
        1,
        1,
        ":2,4fold<CR>:2,4foldopen<CR>jj",
    ),
    c(
        "fold:ex::foldopen on a nested range opens only the outer level",
        FOLDTXT,
        1,
        1,
        ":3,4fold<CR>:2,5fold<CR>:2,5foldopen<CR>jj",
    ),
    c(
        "fold:ex::foldclose on a range spanning two nested headers closes only the outer",
        FOLDTXT,
        1,
        1,
        ":3,4fold<CR>:2,5fold<CR>:2,5foldopen!<CR>:2,3foldclose<CR>2Gzojj",
    ),
    c(
        "fold:ex::foldclose! on a nested range closes every level",
        FOLDTXT,
        1,
        1,
        ":3,4fold<CR>:2,5fold<CR>:2,5foldopen!<CR>:2,3foldclose!<CR>2Gzojj",
    ),
    c(
        "fold:ex::folddoopen only touches lines outside the closed fold",
        FOLDTXT,
        1,
        1,
        ":2,4fold<CR>:folddoopen s/^/X/<CR>",
    ),
    c(
        "fold:ex::folddoclosed only touches lines inside the closed fold",
        FOLDTXT,
        1,
        1,
        ":2,4fold<CR>:folddoclosed s/^/Z/<CR>",
    ),
];

// ───────────────────────── G2. :map family (#1151) ─────────────────────────
//
// The oracle corpus's coverage of vim's `:map`/`:nmap`/`:nnoremap`/… family —
// key-to-keys remapping, noremap-vs-map recursion, `<leader>` expansion, and
// operator-pending maps. Mapping definitions with no `<Notation>` in the rhs
// (`:nmap a b`, `:onoremap p i(`) are typed as literal ex-command keystrokes,
// identically on both sides, via `c(..)`. A definition whose rhs needs
// `<Notation>` that isn't `<CR>`/`<Esc>`/etc. already known to this suite's
// own key tokenizer (`<Esc>` *would* tokenize fine, but sending it while
// typing a `:` command line would send a real Escape keystroke and cancel
// the command instead of typing the four literal characters "E","s","c" —
// see `tokenize_keys`) instead defines the mapping via a `cs(..)` Lua
// `vim.keymap.set(...)` / `vim.g.mapleader` setup statement, which
// `apply_keymap_set` / `apply_setup` translate onto vimcode's
// `Settings::keymaps` without going through any keystroke path at all.
const CASES_MAP: &[Case] = &[
    // `inoremap jk <Esc>` — the single most common line in any vimrc (#1151's
    // motivating example). If the mapping fires, "jk" is consumed entirely
    // by the mapping (buffered waiting for the "k" — same prefix contract as
    // any other multi-key mapping) and nothing is inserted; escaping also
    // steps the cursor back one column, same as a real `<Esc>` keypress.
    cs(
        "map:inoremap_jk_to_escape",
        &["ab"],
        1,
        1,
        "ijk",
        "vim.keymap.set('i', 'jk', '<Esc>')",
    ),
    // A single 'j' not followed by 'k' must still just be typed — the
    // prefix-buffering must not eat keys it doesn't end up needing.
    cs(
        "map:inoremap_jk_single_j_falls_through",
        &["ab"],
        1,
        1,
        "ijx",
        "vim.keymap.set('i', 'jk', '<Esc>')",
    ),
    // `:nmap` (recursive): a -> b, b -> x (delete char under cursor). Chases
    // through both hops, so pressing 'a' deletes a character.
    c(
        "map:nmap_chases_recursively",
        &["abc"],
        1,
        1,
        ":nmap a b<CR>:nmap b x<CR>a",
    ),
    // `:nnoremap` (non-recursive): a -> b, and b is *separately* mapped to x.
    // The noremap rhs 'b' must be taken literally — the built-in word-back
    // motion, which touches nothing at the start of the buffer — not chase
    // into the a-priori-unrelated b -> x mapping (which would delete a char).
    c(
        "map:nnoremap_does_not_chase",
        &["abc def"],
        1,
        1,
        ":nnoremap a b<CR>:nmap b x<CR>a",
    ),
    // A cycle with no base case (`nmap a b` + `nmap b a`) must stop — vim's
    // `maxmapdepth` — rather than hang. Neither side should have touched the
    // buffer or cursor by the time the guard aborts it.
    c(
        "map:recursive_cycle_hits_depth_guard",
        &["hello"],
        1,
        1,
        ":nmap a b<CR>:nmap b a<CR>a",
    ),
    // `<leader>` expansion in the lhs. Using ',' rather than the real default
    // ('\') sidesteps Lua/Rust string-escaping noise without weakening the
    // case: what's under test is substitution of *whatever* `mapleader` is,
    // not the specific default character.
    cs(
        "map:leader_expands_in_lhs",
        &["abc"],
        1,
        1,
        ",w",
        "vim.g.mapleader = ','\nvim.keymap.set('n', '<leader>w', 'x')",
    ),
    // Operator-pending (`o`) maps: `onoremap p i(` makes 'p', while an
    // operator awaits its motion, behave like the "inside parens" text
    // object — so "dp" deletes inside the parens the cursor is in, same as
    // "di(" would.
    c(
        "map:onoremap_extends_a_motion",
        &["foo(bar)baz"],
        1,
        6,
        ":onoremap p i(<CR>dp",
    ),
    // The same mapping must not fire outside operator-pending — bare 'p'
    // (paste, nothing yanked) should stay a no-op, not enter Insert mode and
    // type a literal '(' the way firing the rhs "i(" directly would.
    c(
        "map:onoremap_does_not_fire_without_pending_operator",
        &["foo(bar)baz"],
        1,
        6,
        ":onoremap p i(<CR>p",
    ),
];

// ─────────────────── H. multi-file jumplist (#985) ───────────────────
//
// Cross-buffer/cross-tab/cross-split `<C-o>`/`<C-i>`, plus `:jumps` list
// contents — see the `MultiFileCase`/`MultiJumpsCase` harness above. Unlike
// every other `CASES_*` array, these do not feed `CATEGORIES`/`run_case`
// (wrong shape); `nvim_conformance_jumplist_multi_file` below runs them
// through `run_multi_case`/`run_jumps_case` instead, but reuses the same
// `classify` bidirectional-gate function as `KNOWN_DEVIATIONS` above.

const MFA: &[&str] = &["a1", "a2", "a3", "a4", "a5"];
const MFB: &[&str] = &["b1", "b2", "b3"];
const MFC: &[&str] = &["c1", "c2"];

const CASES_MULTI_JUMP: &[MultiFileCase] = &[
    // "<C-o> after opening file B from file A returns to A, at A's recorded
    // position." (issue's first minimum case.)
    mfc(
        "jump:multi C-o after :e returns to A",
        &[("a.txt", MFA), ("b.txt", MFB)],
        0,
        3,
        1,
        ":e {F1}<CR><C-o>",
    ),
    // "<C-i> from there returns forward to B."
    mfc(
        "jump:multi C-i forward to B",
        &[("a.txt", MFA), ("b.txt", MFB)],
        0,
        3,
        1,
        ":e {F1}<CR><C-o><C-i>",
    ),
    // "The same across a tab boundary".
    mfc(
        "jump:multi C-o across tab",
        &[("a.txt", MFA), ("b.txt", MFB)],
        0,
        3,
        1,
        ":tabnew {F1}<CR><C-o>",
    ),
    mfc(
        "jump:multi C-i across tab",
        &[("a.txt", MFA), ("b.txt", MFB)],
        0,
        3,
        1,
        ":tabnew {F1}<CR><C-o><C-i>",
    ),
    // "...and across a split."
    mfc(
        "jump:multi C-o across split",
        &[("a.txt", MFA), ("b.txt", MFB)],
        0,
        3,
        1,
        ":split {F1}<CR><C-o>",
    ),
    mfc(
        "jump:multi C-o across vsplit",
        &[("a.txt", MFA), ("b.txt", MFB)],
        0,
        3,
        1,
        ":vsplit {F1}<CR><C-o>",
    ),
    // "<C-o> when the recorded pane still exists but its buffer was swapped
    // in place (:e over it)" -- `jump_pane_buffer_matches`. A single window,
    // buffer swapped twice via :e, exercises exactly that path.
    mfc(
        "jump:multi C-o after buffer swap in place",
        &[("a.txt", MFA), ("b.txt", MFB), ("c.txt", MFC)],
        0,
        2,
        1,
        ":e {F1}<CR>G:e {F2}<CR><C-o>",
    ),
    mfc(
        "jump:multi C-o twice across three files",
        &[("a.txt", MFA), ("b.txt", MFB), ("c.txt", MFC)],
        0,
        1,
        1,
        ":e {F1}<CR>:e {F2}<CR><C-o><C-o>",
    ),
    // "A jump that does not change line but does change file" -- the
    // `record_jump_from` same-line early-return the issue calls out as the
    // single most likely culprit. Both files' line 1 read identically so a
    // line-only comparison could not tell the files apart.
    mfc(
        "jump:multi C-o same line different file",
        &[("a.txt", &["same", "a2"]), ("b.txt", &["same", "b2"])],
        0,
        1,
        1,
        ":e {F1}<CR><C-o>",
    ),
];

const CASES_MULTI_JUMPS_LIST: &[MultiJumpsCase] = &[
    // ":jumps output: count, ordering, ... and the file column." Single
    // window, two :e's -- deliberately avoids the documented global-vs-
    // per-window jumplist divergence (see the harness doc comment above),
    // so this isolates the file-recording question the issue asks about.
    mjc(
        "jumps:multi list after two :e",
        &[("a.txt", MFA), ("b.txt", MFB), ("c.txt", MFC)],
        0,
        2,
        1,
        ":e {F1}<CR>G:e {F2}<CR>",
    ),
    // Same, but with the "> current-position marker" exercised by walking
    // one step back into the list instead of staying at the live end.
    mjc(
        "jumps:multi list after C-o marker",
        &[("a.txt", MFA), ("b.txt", MFB), ("c.txt", MFC)],
        0,
        2,
        1,
        ":e {F1}<CR>G:e {F2}<CR><C-o>",
    ),
];

// ---------------------------------------------------------------------------
// Categories — the runner flattens these; the split is for editability only.
// ---------------------------------------------------------------------------

const CATEGORIES: &[(&str, &[Case])] = &[
    ("op      operators x motions", CASES_OP),
    ("dot     dot repeat", CASES_DOT),
    ("undo    undo/redo", CASES_UNDO),
    ("reg     registers", CASES_REG),
    ("mac     macros", CASES_MAC),
    ("mark    marks & jumps", CASES_MARK),
    ("search  search", CASES_SEARCH),
    ("ex      :s / :g / ex", CASES_EX),
    (
        "abbrev  :abbreviate / :iabbrev / :cabbrev (#1152)",
        CASES_ABBREV,
    ),
    ("ins     insert-mode keys", CASES_INS),
    ("vis     visual", CASES_VIS),
    ("vb      visual block", CASES_VB),
    ("num     <C-a> / <C-x>", CASES_NUM),
    ("scroll  scrolling", CASES_SCROLL),
    ("word    word & misc motions", CASES_WORD),
    ("to      text objects", CASES_TO),
    ("misc    misc", CASES_MISC),
    ("fold    folds (#1006)", CASES_FOLD),
    ("map     :map family (#1151)", CASES_MAP),
];

// ---------------------------------------------------------------------------
// KNOWN_DEVIATIONS — see the module docs. This list may only ever SHRINK.
//
// Deleting an entry is how a Vim-compat fix proves itself: the runner fails if
// a listed label starts passing, and fails if an unlisted label fails.
// ---------------------------------------------------------------------------

const KNOWN_DEVIATIONS: &[&str] = &[
    // #1002 deleted the six `setup`-dependent entries that used to head this
    // list ("op:J after period (vim joinspaces)", "op:>> cursor sol",
    // "scroll:C-d col sol", "word:gg indented (sol)", "word:G indented (sol)",
    // "word:5G then j col (sol)"). None was ever a vimcode deviation: they were
    // excused because `run_in_vimcode` took no `setup` parameter at all, so
    // every `cs(..)` case drove a *configured* Neovim against a *default*
    // vimcode. With `setup` wired through (`apply_setup`, next to
    // `run_in_vimcode`), all six pass.
    //
    // #1003 deleted the last two `dot:` entries ("dot:i<C-w> .": past-eol
    // cursor clamping on entering insert via `i`, now fixed by clamping in
    // the `i` handler; "dot:>> 2.": `2>>` failed to abort when the count
    // exceeded the lines available, now fixed by aborting the doubled `>>`/
    // `<<` operator instead of silently clamping). No `dot:` deviations
    // remain.
    //
    // "undo:U" / "undo:UU" (#885): verified this is a fixture-loading
    // artifact of the harness's `undolevels = -1` dance around the initial
    // `nvim_buf_set_lines` write (see the "Harness fidelity" doc comment
    // at the top of this file), not a real Vim/Neovim behaviour difference:
    //   - `:edit`-loading "abcdef" for real, then feeding `xxxU`, gives the
    //     documented `U` result ("abcdef" restored) — vimcode already
    //     matches this.
    //   - The harness's synthetic `nvim_buf_set_lines` fixture write leaves
    //     line 1's "U-saved-original" pointer stale at "" (the buffer's
    //     pre-existing empty line, from *before* the fixture text was
    //     written) because that write itself isn't undo-synced. Only line 1
    //     of a freshly-loaded fixture is affected — the same `xU` on line 2
    //     of a multi-line fixture undoes correctly.
    // Left in deliberately: "fixing" vimcode to match would mean making `U`
    // always discard real line content on a freshly-opened single-line
    // buffer, which is wrong for actual usage.
    "undo:U",
    "undo:UU",
    // "mac:\"ay then @a executes text" (#890): the buffer edit already
    // matches the oracle exactly — `@a` has always executed whatever text
    // sits in register `a`, yanked or recorded, so that half of this
    // label's premise was never a bug. Only the final cursor differs by one
    // column, and it's a harness artifact, not a vimcode bug: this macro's
    // register content ends mid-Insert-mode with no real `<Esc>` (the
    // literal string "<Esc>" was yanked as ordinary text, not typed as a
    // keystroke), and `nvim_feedkeys(.., "ntx")` shares Neovim's
    // `exec_normal()`/`:normal!` machinery, which force-exits an
    // unterminated Insert/Replace mode with a synthetic `<Esc>` once
    // typeahead drains (shifting the cursor left by one). Verified against
    // a real interactive Neovim session (headless `--listen` + `tmux
    // send-keys`, not this harness's `feedkeys`) that a live `"ay$j@a`
    // leaves the editor genuinely in Insert mode with the cursor exactly
    // where vimcode puts it — matching the long-documented "a macro that
    // ends mid-insert leaves you in Insert mode" Vim behaviour. Adding a
    // matching force-Escape-on-drain to vimcode's own macro playback would
    // "fix" this label by breaking that real, load-bearing behaviour for
    // every other macro that intentionally ends in Insert mode.
    // "ins:BS over indent (nosmarttab)", "ins:Tab at start (nosmarttab)",
    // "num:octal nf=octal 007" and "num:alpha" were moved to HARNESS_LIMITED
    // by #875 and deleted outright by #1002 — the `setup`-is-dropped harness
    // gap they were pinned to is closed and all four pass. (Note "num:octal
    // not default 007" — no `setup`, plain Neovim defaults — was never
    // affected and still passes as `008`.)
    // ── #805 / #1008: the scroll group is gone, and so is its excuse ────
    //
    // Everything between here and the `sub:c` block used to be a long
    // explanation of why `scroll:*` labels could not pass: `nvim --headless
    // -l script.lua` attaches no UI, so no redraw runs, so the window's
    // `w_topline` / `w_botline` / `w_empty_rows` are never re-validated
    // between the keystrokes of one `nvim_feedkeys()` burst. That was real,
    // it was measured, and the comment closed by saying the remainder "needs
    // either a non-headless oracle for window-relative state or explicit
    // sign-off to close them as a tracked harness limitation".
    //
    // #1008 built the non-headless oracle (see the module docs and
    // `NvimRpc`), and there is nothing left to list: every `scroll:` case in
    // the corpus passes, including the four chained sequences #805 measured
    // (`<C-d><C-d>`, `5<C-d><C-d>`, `<C-d><C-d><C-u>`, `<C-f><C-f>`) and the
    // `2<C-b>` that outlived them in `HARNESS_LIMITED`.
    //
    // Two things worth keeping, because they are easy to re-derive wrongly:
    //
    //   * The direct engine tests that pinned vimcode to the *interactive*
    //     column while the oracle was wrong — `test_ctrl_d_chain_*`,
    //     `test_ctrl_f_chain_*` and now `test_ctrl_b_chain_*` in
    //     `tests/new_vim_features.rs` — are deliberately **kept**. They are
    //     what holds the line if the oracle transport ever regresses again,
    //     and they cost nothing.
    //
    //   * `2<C-b>` turned out **not** to be an oracle artifact at all.
    //     Headless and interactive Neovim 0.12 agree on it (60-line buffer,
    //     22-row window, cursor on line 60: both answer 22), and vimcode
    //     answered 20. It had been excused as a broken oracle for three
    //     issues. #1008 fixed `page_up` in `src/core/engine/motions.rs`; see
    //     that function's comment. An excuse that nobody can falsify is how
    //     that happens, which is the whole argument for owning the oracle.
    //
    // `page_up`/`page_down`/`scroll_cursor_center` in
    // `src/core/engine/motions.rs` also carry the #805 fixes that were always
    // real (the 2-line buffer-start/end no-op guards and the `zz`/`z.`
    // centering off-by-one), each with a source comment pointing back here.
    //
    // ── RESOLVED (#1002): the 'startofline' group was never either ──
    //
    // An earlier revision of this comment claimed, above "scroll:C-d col sol"
    // and the three "word:...(sol)" labels, that they had a "separate, genuine
    // root cause: vimcode doesn't implement Vim's 'startofline' option at
    // all". That was wrong by the time it was read: `Settings::startofline`
    // landed in #876 and is honoured by `land_line_jump_cursor` /
    // `land_vertical_scroll_cursor` (`src/core/engine/motions.rs`), with the
    // `gg`/`G` call sites in `src/core/engine/keys.rs` — which is why the
    // `(nosol)` twin of every one of those cases already passed. The real and
    // only cause was that `run_in_vimcode` dropped the case's `setup`, so the
    // vimcode side ran with `startofline` off no matter what the option did.
    // #1002 wired `setup` through and deleted all four entries. Left as a
    // note, not an excuse: a stale "feature is missing" claim cost a future
    // reader the whole re-derivation once already.
    //
    // ── RESOLVED (#1008): "mac:\"ay then @a executes text" ──
    //
    // Deleted, and it was never a vimcode bug either. The entry documented a
    // one-column cursor difference caused by `nvim_feedkeys(.., "ntx")`
    // sharing Neovim's `exec_normal()` machinery, which force-`<Esc>`s an
    // unterminated Insert mode once typeahead drains. The note said, in as
    // many words, that a real interactive session leaves the editor in Insert
    // mode exactly where vimcode leaves it. #1008's oracle types the keys
    // through `nvim_input` instead of `feedkeys`, so there is no synthetic
    // `<Esc>` and the case simply passes.
    //
    // ── RESOLVED (#1031, #801 Phase 2): "sub:c ..." ──
    //
    // #986's v0.11.0 bug suite listed 11 confirm-prompt cases here because
    // `execute.rs`'s `flags.contains('c')` check always errored loudly
    // instead of entering a confirm loop, so the keystrokes meant for the
    // prompt (y/n/a/q/l/<Esc>) fell through to ordinary Normal-mode command
    // dispatch on the vimcode side. #1031 built the real confirm loop
    // (`Engine::confirm_sub`, `execute.rs`) and all 11 now pass, alongside
    // the 5 new cases #1031 itself added right after them in `CASES_EX`.
];

// ---------------------------------------------------------------------------
// HARNESS_LIMITED (#875) — cases this harness cannot faithfully probe.
//
// **Currently empty, and that is the point.** An entry here is not a claim
// that vimcode differs from Vim; it is a claim that *this test* cannot tell,
// because the failure traces to a gap in the harness (`run_in_vimcode` /
// `oracle_probe`) or to the oracle process itself rather than to `Engine`.
// Counting those against the "how far from Neovim is vimcode" number would be
// dishonest bookkeeping, so the runner reports them separately and excludes
// them from the KNOWN_DEVIATIONS bidirectional gate entirely — they can fail
// forever without being a regression, and pass without being "a fix landed"
// that forces an entry deletion.
//
// That exemption is exactly why the array has to stay empty unless something
// genuinely unprobeable turns up. Both gaps it ever held are closed, and
// **neither turned out to be what its entry said it was**:
//
//   * #1002's gap was real — `run_in_vimcode` dropped a case's Lua `setup`,
//     so every `cs(..)` case drove vimcode's defaults against a
//     differently-configured Neovim and could not pass by construction. Four
//     labels left when `apply_setup` landed ("ins:BS over indent
//     (nosmarttab)", "ins:Tab at start (nosmarttab)", "num:octal nf=octal
//     007", "num:alpha").
//
//   * #875's gap — "scroll:2<C-b>", excused since #805 as a headless-oracle
//     artifact — was **not real**. The entry asserted at length, with a
//     measured table, that headless nvim's topline collapses to the cursor
//     line so `<C-b>` reads a window position no interactive session shows.
//     Upstream fixed that in 0.12, and on the pinned oracle headless and
//     interactive agree: 60-line buffer, 22-row window, cursor on line 60,
//     `2<C-b>` lands on line 22 in both. vimcode answered 20. The entry had
//     been excusing a genuine vimcode bug for three issues; #1008 fixed
//     `page_up` (`src/core/engine/motions.rs`) and the case passes as an
//     ordinary one.
//
// So: an entry here must name a mechanism *and* an experiment that would
// falsify it, and the array must only ever change because such a gap is
// closed (fix it, delete the entry, the case rejoins ordinary accounting) or
// because a case left the corpus. Never add an entry to paper over a failure
// you have not falsified.
// ---------------------------------------------------------------------------

const HARNESS_LIMITED: &[&str] = &[];

// ---------------------------------------------------------------------------
// Coverage ratchet (#1007) — what the corpus does NOT reach
//
// `KNOWN_DEVIATIONS` above holds ground already taken: it is excellent at
// noticing a command that used to match Neovim and stopped. It says nothing
// about ground never entered — a command with *zero* oracle cases is
// indistinguishable, to every other gate in this file, from a well-covered
// one. The two artifacts that look like they measure reach don't:
// `VIM_COMPATIBILITY.md` is a hand-maintained checklist of command
// *existence* (it reads 422/424, "Remaining Missing Commands: None"), and
// `COVERAGE_PHASE5.md` is a partial audit, 2 of 6 focus areas done.
//
// So this gate measures it, with the same shape `KNOWN_DEVIATIONS` proved:
//
//   * [`parse_compatibility_doc`] reads every ✅/⚠️ row of
//     `VIM_COMPATIBILITY.md` into a `<section>:<keystroke>` id;
//   * [`COMMAND_PROBES`] gives every one of those ids an **explicit**
//     label-substring or keys-substring predicate — the readable answer to
//     "why does this command count as covered";
//   * [`COVERAGE_EXEMPT`] lists the ids whose probe is expected to match
//     *nothing*: today's measured gap, seeded so this lands green.
//
// The gate is **bidirectional**, exactly like `KNOWN_DEVIATIONS`:
//
//   * an id not in `COVERAGE_EXEMPT` whose probe matches no case → fail;
//   * an id *in* `COVERAGE_EXEMPT` whose probe now matches a case → fail
//     until the entry is deleted.
//
// So the list may only ever SHRINK, and adding oracle cases for an exempt
// command is forced to prove itself by deleting its entry.
//
// ## The probe is deliberately dumb
//
// The command→case mapping is the part that could quietly make this test
// worse than nothing. A fuzzy match that counted `dw` as covering `d}` would
// manufacture coverage that does not exist. So there is no inference at all:
// every id carries a hand-written needle, and a human reading
// `p("move:}", Keys("d}"))` can check the claim in one grep. Prefer a needle
// that cannot match a *different* command — `Label("to:i( ")` over
// `Keys("i(")` — and when in doubt leave the id exempt. Over-crediting is the
// failure mode this test exists to prevent; under-crediting just leaves a
// shrinkable entry behind.
//
// ## The doc is read, never written
//
// `VIM_COMPATIBILITY.md` is a shared doc owned by the coordinator. This test
// only reads it (`include_str!`, so a doc edit rebuilds the test), and it
// refuses to *silently* skip anything it cannot parse: an unrecognised table
// header, a row with the wrong cell count, an unknown Status marker or a
// Command cell with no inline-code span all fail by naming the file and line.
// A lenient parser would drop rows, and dropped rows understate the gap —
// which is the exact failure mode of the status quo.
// ---------------------------------------------------------------------------

/// How one `VIM_COMPATIBILITY.md` command is proven to be exercised by the
/// oracle corpus. Substring, not regex, not fuzzy — see the section doc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Probe {
    /// At least one case whose **label** contains this substring.
    Label(&'static str),
    /// At least one case whose **keys** contain this substring.
    Keys(&'static str),
}

impl Probe {
    fn matches(&self, label: &str, keys: &str) -> bool {
        match self {
            Probe::Label(n) => label.contains(n),
            Probe::Keys(n) => keys.contains(n),
        }
    }

    fn needle(&self) -> &'static str {
        match self {
            Probe::Label(n) | Probe::Keys(n) => n,
        }
    }
}

// Imported so [`COMMAND_PROBES`] reads as `Label("…")` / `Keys("…")`: the table
// is ~560 rows and the whole point is that a human can skim it.
use crate::Probe::{Keys, Label};

struct CommandProbe {
    /// `"<section>:<keystroke>"`, exactly as [`parse_compatibility_doc`]
    /// derives it from a `VIM_COMPATIBILITY.md` row.
    id: &'static str,
    probe: Probe,
}

const fn p(id: &'static str, probe: Probe) -> CommandProbe {
    CommandProbe { id, probe }
}

// ---------------------------------------------------------------------------
// VIM_COMPATIBILITY.md parsing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocStatus {
    /// ✅ — claimed implemented.
    Implemented,
    /// ⚠️ — claimed partially implemented. Still in scope: a partial
    /// implementation is exactly the kind that benefits from an oracle case.
    Partial,
    /// ❌ — claimed not implemented. Out of scope for this gate.
    Missing,
    /// N/A — deliberately not in scope (VimScript, digraphs, and anything
    /// needing an expression evaluator). **Not** spelling: `src/core/spell.rs`
    /// implements it and #1163 moved those rows to ✅.
    NotApplicable,
}

impl DocStatus {
    fn in_scope(self) -> bool {
        matches!(self, DocStatus::Implemented | DocStatus::Partial)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DocCommand {
    /// `"<section>:<keystroke>"` — the id [`COMMAND_PROBES`] and
    /// [`COVERAGE_EXEMPT`] key off. Split on the *first* `:` (a keystroke may
    /// itself be `:wq`).
    id: String,
    /// 1-indexed line in `VIM_COMPATIBILITY.md`, so a failure can point at it.
    line: usize,
    status: DocStatus,
}

/// Table headers that introduce a command inventory. `Motion` is the
/// Operator-Pending table's spelling of the same column.
const COMMAND_TABLE_HEADERS: &[&[&str]] = &[
    &["Command", "Description", "Status", "Notes"],
    &["Motion", "Description", "Status", "Notes"],
];

/// Tables in the doc that deliberately are *not* command inventories. Listed
/// explicitly (rather than "anything without a Status column") so that a new
/// table shape shows up as a parse failure to be triaged, not as silence.
const NON_COMMAND_TABLE_HEADERS: &[(&[&str], &str)] = &[
    (
        &["Supported", "Notes"],
        "`Search pattern syntax` — regex features, not keystrokes; no Status column",
    ),
    (
        &["Command", "Description"],
        "`VimCode-Specific Ex Commands` — not Vim commands, and no Status column, so it marks nothing implemented",
    ),
    (
        &["Category", "Implemented", "Total", "Coverage"],
        "the Summary roll-up — counts, already covered by the rows they total",
    ),
];

/// `VIM_COMPATIBILITY.md` heading → the short section key used in command ids.
/// A command table under an unlisted heading is a hard parse failure: silently
/// inventing a key would make ids unstable, and a renamed heading must be a
/// visible, reviewable change to this list.
const SECTION_KEYS: &[(&str, &str)] = &[
    ("Insert Mode", "ins"),
    ("Normal Mode — Movement", "move"),
    ("Normal Mode — Editing", "edit"),
    ("Normal Mode — Search & Marks", "search"),
    ("Normal Mode — Other", "other"),
    ("Text Objects", "textobj"),
    ("g-Commands", "g"),
    ("z-Commands", "z"),
    ("Window Commands (CTRL-W)", "win"),
    ("Bracket Commands ([ and ])", "bracket"),
    ("Operator-Pending Mode", "oppend"),
    ("Visual Mode", "visual"),
    ("Core Vim Ex Commands", "ex"),
    // #1163: the "Not implemented" section. Its rows are ❌ (or N/A), so they
    // are out of `in_scope` and carry no probes — but they must still parse,
    // because the whole point of the section is that a command with no row
    // cannot be counted as missing. Three keys, not one, because a `###`
    // subheading resets `section`.
    ("Not implemented", "missing"),
    ("Not implemented — options", "missingopt"),
    ("Not implemented — modes", "missingmode"),
];

/// Rows whose Command cell is prose rather than a `` `keystroke` `` span.
/// They are real, in-scope entries — not noise — so they are allowlisted with
/// a reason and still have to carry a probe, rather than being dropped.
const PROSE_ROWS: &[(&str, &str)] = &[
    (
        "visual:Movement keys",
        "\"every motion extends the selection\" — a class of keys, not one keystroke",
    ),
    (
        "ex:Ex ranges",
        "the range grammar accepted by :s/:g/:d/… — a syntax, not one keystroke",
    ),
];

fn run_len(chars: &[char], i: usize) -> usize {
    let mut n = 0;
    while i + n < chars.len() && chars[i + n] == '`' {
        n += 1;
    }
    n
}

/// Extract the inline code spans from a markdown cell, CommonMark-style: an
/// opening run of N backticks closes on the next run of *exactly* N, and a
/// span padded with one space on both sides has it stripped. The doc needs
/// both forms — `` `gt` `` and the double-backtick ``` `` g` `` ``` used
/// wherever the keystroke itself contains a backtick.
///
/// `Err` on an unclosed run: better a named failure than a dropped command.
fn code_spans(cell: &str) -> Result<Vec<String>, &'static str> {
    let chars: Vec<char> = cell.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '`' {
            i += 1;
            continue;
        }
        let open = run_len(&chars, i);
        let mut j = i + open;
        let close = loop {
            if j >= chars.len() {
                return Err("unterminated inline code span");
            }
            if chars[j] == '`' {
                let n = run_len(&chars, j);
                if n == open {
                    break j;
                }
                j += n;
            } else {
                j += 1;
            }
        };
        let span: String = chars[i + open..close].iter().collect();
        let trimmed = match span.strip_prefix(' ').and_then(|s| s.strip_suffix(' ')) {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => span,
        };
        out.push(trimmed);
        i = close + open;
    }
    Ok(out)
}

/// Split a markdown table row on its *unescaped* `|` separators. The doc
/// writes a literal pipe inside a code span as `\|` (`` `\|` `` — "go to
/// column N"; `` `CTRL-W \|` `` — "maximize width"), and splitting naively
/// would tear those rows into the wrong number of cells.
fn split_table_row(line: &str) -> Vec<String> {
    let mut cells: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut it = line.chars().peekable();
    while let Some(ch) = it.next() {
        match ch {
            '\\' if it.peek() == Some(&'|') => {
                cur.push('\\');
                cur.push(it.next().expect("peeked"));
            }
            '|' => cells.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    cells.push(cur);
    // The row's outer pipes yield one empty cell at each end — drop exactly
    // one from each, never more: a trailing *empty Notes column* is the
    // overwhelmingly common case and must survive as a real cell.
    if cells.first().is_some_and(|c| c.trim().is_empty()) {
        cells.remove(0);
    }
    if cells.last().is_some_and(|c| c.trim().is_empty()) {
        cells.pop();
    }
    cells.iter().map(|c| c.trim().to_string()).collect()
}

fn is_separator_row(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
}

fn parse_status(cell: &str) -> Option<DocStatus> {
    // U+FE0F (variation selector-16) rides along with ⚠️ in the doc; strip it
    // so the marker compares equal either way.
    let c: String = cell.chars().filter(|ch| *ch != '\u{fe0f}').collect();
    match c.trim() {
        "✅" => Some(DocStatus::Implemented),
        "⚠" => Some(DocStatus::Partial),
        "❌" => Some(DocStatus::Missing),
        "N/A" => Some(DocStatus::NotApplicable),
        _ => None,
    }
}

enum TableKind {
    /// A command inventory, under the named section key.
    Commands(&'static str),
    /// A known non-command table — rows are skipped deliberately.
    Ignored,
}

fn classify_header(
    cells: &[String],
    section: Option<&str>,
    lineno: usize,
) -> Result<TableKind, String> {
    let as_strs: Vec<&str> = cells.iter().map(|s| s.as_str()).collect();
    if COMMAND_TABLE_HEADERS.contains(&as_strs.as_slice()) {
        let heading = section.ok_or_else(|| {
            format!("VIM_COMPATIBILITY.md:{lineno}: command table before any heading")
        })?;
        let key = SECTION_KEYS
            .iter()
            .find(|(h, _)| *h == heading)
            .map(|(_, k)| *k)
            .ok_or_else(|| {
                format!(
                    "VIM_COMPATIBILITY.md:{lineno}: command table under unknown heading {heading:?} \
                     — add it to SECTION_KEYS in tests/nvim_conformance.rs (with the id prefix \
                     you want its commands to use). Guessing a key would make command ids \
                     unstable and silently orphan every COMMAND_PROBES entry under it."
                )
            })?;
        return Ok(TableKind::Commands(key));
    }
    if NON_COMMAND_TABLE_HEADERS
        .iter()
        .any(|(h, _)| *h == as_strs.as_slice())
    {
        return Ok(TableKind::Ignored);
    }
    Err(format!(
        "VIM_COMPATIBILITY.md:{lineno}: unrecognised table header {as_strs:?}. Either it is a new \
         command inventory (add its header to COMMAND_TABLE_HEADERS) or it is not (add it to \
         NON_COMMAND_TABLE_HEADERS with a reason). Skipping unknown tables is how a coverage gap \
         goes unmeasured (#1007)."
    ))
}

/// Parse every command row of `VIM_COMPATIBILITY.md`.
///
/// Tolerant of the doc's existing formatting — mid-table blank lines, escaped
/// pipes, double-backtick spans, `/`-joined aliases in one cell — and
/// intolerant of anything it does not recognise: every failure names the line.
fn parse_compatibility_doc(doc: &str) -> Result<Vec<DocCommand>, String> {
    let mut out: Vec<DocCommand> = Vec::new();
    let mut section: Option<String> = None;
    let mut table: Option<TableKind> = None;

    for (idx, raw) in doc.lines().enumerate() {
        let lineno = idx + 1;
        let line = raw.trim();

        if let Some(rest) = line.strip_prefix('#') {
            section = Some(rest.trim_start_matches('#').trim().to_string());
            table = None;
            continue;
        }
        if !line.starts_with('|') {
            // A blank line does NOT close a table: `Core Vim Ex Commands` has
            // one in the middle of its table, before the `:Explore` rows.
            if !line.is_empty() {
                table = None;
            }
            continue;
        }

        let cells = split_table_row(line);
        let kind = match &table {
            Some(k) => k,
            None => {
                table = Some(classify_header(&cells, section.as_deref(), lineno)?);
                continue;
            }
        };
        if is_separator_row(&cells) {
            continue;
        }
        let sec = match kind {
            TableKind::Ignored => continue,
            TableKind::Commands(s) => *s,
        };

        if cells.len() != 4 {
            return Err(format!(
                "VIM_COMPATIBILITY.md:{lineno}: command row has {} cell(s), expected 4 \
                 (Command | Description | Status | Notes): {line}",
                cells.len()
            ));
        }
        let status = parse_status(&cells[2]).ok_or_else(|| {
            format!(
                "VIM_COMPATIBILITY.md:{lineno}: unrecognised Status cell {:?} \
                 (expected ✅, ⚠️, ❌ or N/A): {line}",
                cells[2]
            )
        })?;
        let spans = code_spans(&cells[0]).map_err(|e| {
            format!("VIM_COMPATIBILITY.md:{lineno}: {e} in the Command cell: {line}")
        })?;

        let keystrokes: Vec<String> = if spans.is_empty() {
            let id = format!("{sec}:{}", cells[0]);
            if !PROSE_ROWS.iter().any(|(p, _)| *p == id) {
                return Err(format!(
                    "VIM_COMPATIBILITY.md:{lineno}: Command cell {:?} has no `code span` and is \
                     not in PROSE_ROWS. Add it there with a reason (it still needs a probe), or \
                     fix the row — dropping it would understate the coverage gap (#1007): {line}",
                    cells[0]
                ));
            }
            vec![cells[0].clone()]
        } else {
            spans
        };

        for k in keystrokes {
            let id = format!("{sec}:{k}");
            if let Some(prev) = out.iter().find(|d| d.id == id) {
                return Err(format!(
                    "VIM_COMPATIBILITY.md:{lineno}: duplicate command id {id:?} (first seen at \
                     line {}). Ids must be unique — they key COMMAND_PROBES.",
                    prev.line
                ));
            }
            out.push(DocCommand {
                id,
                line: lineno,
                status,
            });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// The bidirectional coverage gate
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq, Eq)]
struct CoverageVerdict {
    /// In-scope doc commands with no [`COMMAND_PROBES`] entry — unmeasurable.
    unprobed: Vec<String>,
    /// [`COMMAND_PROBES`] entries naming no in-scope doc command — stale.
    stale_probes: Vec<String>,
    /// [`COVERAGE_EXEMPT`] entries with no probe, or naming nothing — stale.
    stale_exempt: Vec<String>,
    /// Ids appearing more than once in [`COMMAND_PROBES`]/[`COVERAGE_EXEMPT`].
    duplicates: Vec<String>,
    /// Not exempt, but the probe matches no case — coverage went backwards.
    uncovered: Vec<String>,
    /// Exempt, but the probe now matches a case — delete the entry.
    newly_covered: Vec<String>,
}

impl CoverageVerdict {
    fn is_clean(&self) -> bool {
        self.unprobed.is_empty()
            && self.stale_probes.is_empty()
            && self.stale_exempt.is_empty()
            && self.duplicates.is_empty()
            && self.uncovered.is_empty()
            && self.newly_covered.is_empty()
    }

    /// A readable diff naming the specific commands — never a bare count.
    /// Same reasoning as `bullet_list`/`print_unmissable` above: this has to
    /// survive being read in a CI log by someone who did not write it.
    fn report(&self) -> String {
        let mut out = String::new();
        let mut section = |title: &str, fix: &str, items: &[String]| {
            if items.is_empty() {
                return;
            }
            out.push_str(&format!("\n{title} ({}):\n  -> {fix}\n", items.len()));
            for i in items {
                out.push_str(&format!("    {i}\n"));
            }
        };
        section(
            "UNCOVERED — implemented, but no case exercises it",
            "add an oracle case, or add the id to COVERAGE_EXEMPT only if you \
             are seeding a newly-documented command",
            &self.uncovered,
        );
        section(
            "NEWLY COVERED — a case now exercises an exempt command",
            "delete these from COVERAGE_EXEMPT; that is how the list shrinks",
            &self.newly_covered,
        );
        section(
            "UNPROBED — implemented, but no COMMAND_PROBES entry",
            "add `p(\"<id>\", Label(..)|Keys(..))` naming the case that proves \
             it (or the case that would)",
            &self.unprobed,
        );
        section(
            "STALE PROBES — named command is not marked implemented",
            "the doc row was renamed, removed or downgraded — update the id",
            &self.stale_probes,
        );
        section(
            "STALE EXEMPTIONS — no such implemented command, or no probe",
            "delete the COVERAGE_EXEMPT entry, or give the id a probe",
            &self.stale_exempt,
        );
        section(
            "DUPLICATES — an id listed twice",
            "ids are unique keys; remove the duplicate",
            &self.duplicates,
        );
        out
    }
}

/// `cases` is `(label, keys)` for the whole corpus.
fn classify_coverage(
    commands: &[DocCommand],
    probes: &[CommandProbe],
    exempt: &[&str],
    cases: &[(&str, &str)],
) -> CoverageVerdict {
    use std::collections::{HashMap, HashSet};
    let mut v = CoverageVerdict::default();

    let mut probe_by_id: HashMap<&str, &Probe> = HashMap::new();
    for cp in probes {
        if probe_by_id.insert(cp.id, &cp.probe).is_some() {
            v.duplicates.push(format!("COMMAND_PROBES: {}", cp.id));
        }
    }
    let mut exempt_set: HashSet<&str> = HashSet::new();
    for e in exempt {
        if !exempt_set.insert(e) {
            v.duplicates.push(format!("COVERAGE_EXEMPT: {e}"));
        }
    }

    let in_scope: HashSet<&str> = commands
        .iter()
        .filter(|c| c.status.in_scope())
        .map(|c| c.id.as_str())
        .collect();

    for id in probe_by_id.keys() {
        if !in_scope.contains(id) {
            v.stale_probes.push((*id).to_string());
        }
    }
    for id in &exempt_set {
        if !in_scope.contains(id) || !probe_by_id.contains_key(id) {
            v.stale_exempt.push((*id).to_string());
        }
    }

    for cmd in commands.iter().filter(|c| c.status.in_scope()) {
        let Some(probe) = probe_by_id.get(cmd.id.as_str()) else {
            v.unprobed
                .push(format!("{}  (VIM_COMPATIBILITY.md:{})", cmd.id, cmd.line));
            continue;
        };
        let hit = cases.iter().find(|(l, k)| probe.matches(l, k));
        match (exempt_set.contains(cmd.id.as_str()), hit) {
            (false, None) => v.uncovered.push(format!(
                "{}  probe {:?} matched 0 cases  (VIM_COMPATIBILITY.md:{})",
                cmd.id,
                probe.needle(),
                cmd.line
            )),
            (true, Some((label, _))) => v.newly_covered.push(format!(
                "{}  probe {:?} now matches {label:?}",
                cmd.id,
                probe.needle()
            )),
            _ => {}
        }
    }

    v.unprobed.sort();
    v.stale_probes.sort();
    v.stale_exempt.sort();
    v.duplicates.sort();
    v.uncovered.sort();
    v.newly_covered.sort();
    v
}

/// Every `(label, keys)` in the corpus — the single-buffer cases plus the
/// multi-file jumplist ones, which are oracle-backed the same way.
fn all_corpus_cases() -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&str, &str)> = CATEGORIES
        .iter()
        .flat_map(|(_, g)| g.iter())
        .map(|c| (c.label, c.keys))
        .collect();
    out.extend(CASES_MULTI_JUMP.iter().map(|c| (c.label, c.keys)));
    out.extend(CASES_MULTI_JUMPS_LIST.iter().map(|c| (c.label, c.keys)));
    out
}

// ---------------------------------------------------------------------------
// COMMAND_PROBES — one explicit predicate per implemented command.
//
// Every ✅/⚠️ row of `VIM_COMPATIBILITY.md` appears here exactly once, in doc
// order, keyed `<section>:<keystroke>`. The predicate is the *readable proof*:
//
//     p("edit:dd", Label("op:dd last line cursor")),
//
// says "`dd` is exercised by the case labelled `op:dd last line cursor`" — one
// grep to check. Where the command is **not** covered (its id is in
// [`COVERAGE_EXEMPT`]) the predicate instead describes the case that *would*
// cover it, so adding one trips the gate:
//
//     p("win:CTRL-W h", Keys("<C-w>h")),   // no case presses this today
//
// Rules of thumb, learned while seeding this list:
//
//   * A `Keys` needle is a **substring of the key sequence**, so it credits
//     anything containing it. `Keys("w")` would count `dw` as covering the `w`
//     motion; `Keys("gh")` matched `i<Right><Right>X<Esc>`; `Keys("do")`
//     matches `:undo`. Prefer `Label`, which names one case by its unique id.
//   * Distinct spellings are distinct ids on purpose: `:split` having a case
//     does **not** cover `:sp`, and `:normal` does not cover `:norm`. That
//     strictness is the point — an honest number, not a flattering one.
// ---------------------------------------------------------------------------

const COMMAND_PROBES: &[CommandProbe] = &[
    // --- Insert Mode (ins) ---
    p("ins:<Esc>", Label("ins:Esc cursor left")),
    p("ins:<CR>", Label("ins:CR autoindent")),
    p("ins:<BS>", Label("ins:BS at col1 joins")),
    p("ins:<Del>", Label("ins:Del")),
    p("ins:<Tab>", Label("ins:Tab at start (smarttab)")),
    p("ins:<Up>/<Down>/<Left>/<Right>", Label("ins:Right Right X")),
    p("ins:<Home>/<End>", Label("ins:End Home")),
    p("ins:CTRL-H", Label("ins:C-h as BS")),
    p("ins:CTRL-W", Label("ins:C-w")),
    p("ins:CTRL-U", Label("ins:C-u inserted")),
    p("ins:CTRL-T", Label("ins:C-t")),
    p("ins:CTRL-D", Label("ins:C-d")),
    p(
        "ins:CTRL-R {reg}",
        Label("ins:C-r register linewise mid line"),
    ),
    p("ins:CTRL-N", Label("ins:C-n completion")),
    p("ins:CTRL-P", Label("ins:C-p completion")),
    p("ins:CTRL-O", Label("ins:C-o dw")),
    p("ins:CTRL-E", Label("ins:C-e")),
    p("ins:CTRL-Y", Label("ins:C-y")),
    p("ins:CTRL-A", Label("ins:C-a reinsert")),
    p("ins:CTRL-@", Keys("<C-@>")),
    p("ins:CTRL-V {char}", Label("ins:C-v Tab")),
    p("ins:CTRL-G u", Label("undo:C-g u splits")),
    p("ins:CTRL-G j/k", Keys("<C-g>j")),
    // --- Normal Mode - Movement (move) ---
    p("move:h", Label("word:h at start")),
    p("move:j", Label("word:j col memory short")),
    p("move:k", Label("word:10k beyond")),
    p("move:l", Label("word:l")),
    p("move:w", Label("word:w punctuation")),
    p("move:b", Label("word:b at start")),
    p("move:e", Label("word:e")),
    p("move:ge", Label("word:ge")),
    p("move:W", Label("word:W")),
    p("move:B", Label("word:B")),
    p("move:E", Label("word:E")),
    p("move:gE", Label("word:gE")),
    p("move:0", Label("op:mb:0 from mid multi-byte line")),
    p("move:^", Label("word:^")),
    p("move:$", Label("word:2$")),
    p("move:g_", Label("word:g_")),
    p("move:g0", Label("word:g0")),
    p("move:gm", Keys("gm")),
    p("move:gM", Keys("gM")),
    p("move:f{char}", Label("op:f, then ;")),
    p("move:F{char}", Label("op:F, F, then ,")),
    p("move:t{char}", Label("op:t; then ; repeat")),
    p("move:T{char}", Label("op:T, then ;")),
    p("move:;", Label("op:t; then ; repeat")),
    p("move:,", Label("op:F, F, then ,")),
    p("move:gg", Label("word:gg indented (nosol)")),
    p("move:G", Label("word:G indented (nosol)")),
    p("move:{", Label("word:{")),
    p("move:}", Label("word:}")),
    p("move:(", Label("word:( sentence")),
    p("move:)", Label("word:) sentences")),
    p("move:H", Label("scroll:H from 30")),
    p("move:M", Label("scroll:M from 30")),
    p("move:L", Label("scroll:L from 30")),
    p("move:%", Label("word:% on (")),
    p("move:+", Label("word:+")),
    p("move:-", Label("word:-")),
    p("move:_", Label("word:_")),
    p("move:\\|", Label("word:|")),
    p("move:N%", Label("jump:C-o after 50%")),
    p("move:gj", Label("word:gj gk nowrap")),
    p("move:gk", Label("word:gj gk nowrap")),
    p("move:CTRL-D", Label("scroll:C-d")),
    p("move:CTRL-U", Label("scroll:C-u")),
    p("move:CTRL-F", Label("scroll:C-f")),
    p("move:CTRL-B", Label("scroll:C-b")),
    p("move:CTRL-E", Label("scroll:C-e pushes cursor")),
    p("move:CTRL-Y", Label("scroll:G C-y")),
    // --- Normal Mode - Editing (edit) ---
    p("edit:i", Label("op:i at eol esc")),
    p("edit:I", Label("op:I indented")),
    p("edit:a", Label("op:3a")),
    p("edit:A", Label("op:A esc cursor")),
    p("edit:o", Label("op:o esc removes indent")),
    p("edit:O", Label("op:O autoindent")),
    p("edit:x", Label("op:x at eol")),
    p("edit:X", Label("op:X at col1")),
    p("edit:d{motion}", Label("op:d) sentence")),
    p("edit:dd", Label("op:dd last line cursor")),
    p("edit:D", Label("op:D on empty")),
    p("edit:c{motion}", Label("op:c$")),
    p("edit:cc", Label("op:cc keeps indent")),
    p("edit:C", Label("op:C")),
    p("edit:s", Label("op:s")),
    p("edit:S", Label("op:S keeps indent")),
    p("edit:y{motion}", Label("op:y$ then P")),
    p("edit:yy", Label("op:yy 3p")),
    p("edit:Y", Label("op:Y is linewise")),
    p("edit:p", Label("op:p linewise cursor first nonblank")),
    p("edit:P", Label("op:P linewise cursor")),
    p("edit:]p", Label("op:]p")),
    p("edit:[p", Keys("[p")),
    p("edit:gp", Label("op:gp linewise")),
    p("edit:gP", Label("op:gP linewise")),
    p("edit:r{char}", Label("op:r<CR>")),
    p("edit:R", Label("op:R")),
    p("edit:J", Label("op:J basic")),
    p("edit:gJ", Label("op:gJ")),
    p("edit:u", Label("undo:u on unchanged")),
    p("edit:U", Label("undo:U")),
    p("edit:CTRL-R", Label("undo:xxx uu C-r")),
    p("edit:.", Label("dot:cw . next word")),
    p("edit:~", Label("op:~")),
    p("edit:g~{motion}", Label("op:g~~ cursor")),
    p("edit:gu{motion}", Label("op:gu$")),
    p("edit:gU{motion}", Label("op:gUU")),
    p("edit:>{motion}", Label("op:>>")),
    p("edit:>>", Label("op:>>")),
    p("edit:<{motion}", Label("op:<< partial indent")),
    p("edit:<<", Label("op:<< partial indent")),
    p("edit:={motion}", Label("op:== single")),
    p("edit:==", Label("op:== single")),
    p("edit:CTRL-A", Label("num:C-a on number")),
    p("edit:CTRL-X", Label("num:C-x to negative")),
    p("edit:gq{motion}", Label("op:gqq tw0 no wrap")),
    p("edit:gw{motion}", Label("op:gwip cursor")),
    p("edit:!{motion}{filter}", Label("op:!!tr")),
    p("edit:&", Label("dot:& repeat sub")),
    p("edit:g&", Label("dot:g&")),
    // --- Normal Mode - Search & Marks (search) ---
    p("search:/pattern", Label("search:/ n")),
    p("search:?pattern", Label("search:? then n backward")),
    p(
        "search:/pat/{offset}",
        Label("search:/pat/e then n keeps offset"),
    ),
    p("search://", Label("search:// repeat")),
    p("search:/<CR>", Label("search:// repeat")),
    p("search:n", Label("search:/ n")),
    p("search:N", Label("search:/ N wraps")),
    p("search:*", Label("search:*")),
    p("search:#", Label("search:#")),
    p("search:g*", Label("search:g*")),
    p("search:g#", Label("search:g#")),
    p("search:gn", Label("search:gn selects")),
    p("search:gN", Label("search:gN")),
    p("search:m{a-z}", Label("mark:'a first nonblank")),
    p("search:m{A-Z}", Label("mark:mA global")),
    p("search:'{a-z}", Label("mark:'a first nonblank")),
    p("search:`{a-z}", Label("mark:`a exact")),
    p("search:'{A-Z}", Label("mark:mA global")),
    p("search:`{A-Z}", Keys("`A")),
    p("search:''", Label("mark:'' after G")),
    p("search:``", Label("mark:`` after gg")),
    p("search:'.", Label("mark:'.")),
    p("search:`.", Label("mark:`.")),
    p("search:'<", Label("mark:'< after v")),
    p("search:'>", Label("mark:'> after V")),
    p("search:CTRL-O", Label("jump:C-o at start")),
    p("search:CTRL-I", Label("jump:gg G C-o C-o C-i")),
    p("search:g;", Label("jump:g;")),
    p("search:g,", Label("jump:g; g; g,")),
    p("search:g'", Label("mark:g'")),
    p("search:g`", Label("mark:g`")),
    // --- Normal Mode - Other (other) ---
    p("other:q{a-z}", Label("mac:qaxjq @a")),
    p("other:q", Label("mac:qaxjq @a")),
    p("other:@{a-z}", Label("mac:qaxjq @a")),
    p("other:@@", Label("mac:@a @@")),
    p("other:@:", Label("misc:dot after @: ")),
    p("other:\"{reg}", Label("reg:\"ayy \"ap")),
    p("other:v", Label("vis:v$d joins")),
    p("other:V", Label("vis:Vjd")),
    p("other:CTRL-V", Label("vb:jjd")),
    p("other:gv", Label("vis:gv")),
    p("other::", Label("ex:>")),
    p("other:gt", Keys("gt")),
    p("other:gT", Keys("gT")),
    p("other:gd", Label("search:gd")),
    p("other:gf", Keys("gf")),
    p("other:gF", Keys("gF")),
    p("other:K", Label("misc:K")),
    p("other:ga", Keys("ga")),
    p("other:g8", Keys("g8")),
    p("other:go", Label("word:go")),
    p("other:gx", Keys("gx")),
    p("other:gi", Label("op:gi")),
    p("other:gI", Label("op:gI")),
    p("other:g?{motion}", Label("misc:g?g?")),
    p("other:CTRL-^", Keys("<C-^>")),
    p("other:CTRL-]", Keys("<C-]>")),
    p("other:CTRL-G", Label("misc:C-g")),
    p("other:CTRL-L", Label("misc:C-l noop")),
    p("other:do", Label("misc:do diff obtain")),
    p("other:dp", Label("misc:dp diff put")),
    p("other:q:", Keys("q:")),
    p("other:q/", Keys("q/")),
    p("other:q?", Keys("q?")),
    p("other:cgn", Label("search:gn selects")),
    // --- Text Objects (textobj) ---
    p("textobj:iw", Label("to:diw on whitespace")),
    p("textobj:aw", Label("to:daw mid")),
    p("textobj:iW", Label("to:diW")),
    p("textobj:aW", Label("to:daW")),
    p("textobj:is", Label("to:dis")),
    p("textobj:as", Label("to:das")),
    p("textobj:ip", Label("to:dip")),
    p("textobj:ap", Label("to:dap")),
    p("textobj:i\"", Label("to:di\" inside")),
    p("textobj:a\"", Label("to:da\" before quotes")),
    p("textobj:i'", Label("to:di'")),
    p("textobj:a'", Label("to:da'")),
    p("textobj:i`", Label("to:di`")),
    p("textobj:a`", Label("to:da`")),
    p("textobj:i(", Label("to:di( inside")),
    p("textobj:a(", Label("to:da( nested")),
    p("textobj:i)", Label("to:di)")),
    p("textobj:a)", Label("to:da)")),
    p("textobj:i{", Label("to:di{ multiline")),
    p("textobj:a{", Label("to:da{ multiline")),
    p("textobj:i}", Label("to:di}")),
    p("textobj:a}", Label("to:da}")),
    p("textobj:i[", Label("to:di[")),
    p("textobj:a[", Label("to:da[")),
    p("textobj:i]", Label("to:di]")),
    p("textobj:a]", Label("to:da]")),
    p("textobj:i<", Label("to:di< nested")),
    p("textobj:a<", Label("to:da< outer")),
    p("textobj:i>", Label("to:di>")),
    p("textobj:a>", Label("to:da>")),
    p("textobj:it", Label("to:dit")),
    p("textobj:at", Label("to:dat")),
    // --- g-Commands (g) ---
    p("g:gg", Label("word:gg indented (nosol)")),
    p("g:g_", Label("word:g_")),
    p("g:g0", Label("word:g0")),
    p("g:g<Home>", Keys("g<Home>")),
    p("g:g^", Keys("g^")),
    p("g:g$", Keys("g$")),
    p("g:g<End>", Keys("g<End>")),
    p("g:gj", Label("word:gj gk nowrap")),
    p("g:gk", Label("word:gj gk nowrap")),
    p("g:gE", Label("word:gE")),
    p("g:ge", Label("word:ge")),
    p("g:gn", Label("search:gn selects")),
    p("g:gN", Label("search:gN")),
    p("g:g*", Label("search:g*")),
    p("g:g#", Label("search:g#")),
    p("g:gv", Label("vis:gv")),
    p("g:gd", Label("search:gd")),
    p("g:gf", Keys("gf")),
    p("g:gF", Keys("gF")),
    p("g:gt", Keys("gt")),
    p("g:gT", Keys("gT")),
    p("g:g<Tab>", Keys("g<Tab>")),
    p("g:g~{motion}", Label("op:g~~ cursor")),
    p("g:gu{motion}", Label("op:gu$")),
    p("g:gU{motion}", Label("op:gUU")),
    p("g:gJ", Label("op:gJ")),
    p("g:g;", Label("jump:g;")),
    p("g:g,", Label("jump:g; g; g,")),
    p("g:g.", Keys("g.")),
    p("g:gp", Label("op:gp linewise")),
    p("g:gP", Label("op:gP linewise")),
    p("g:gq{motion}", Label("op:gqq tw20")),
    p("g:gw{motion}", Label("op:gwip cursor")),
    p("g:gx", Keys("gx")),
    p("g:ga", Keys("ga")),
    p("g:g8", Keys("g8")),
    p("g:go", Label("word:go")),
    p("g:gi", Label("op:gi")),
    p("g:gI", Label("op:gI")),
    p("g:gm", Keys("gm")),
    p("g:gM", Keys("gM")),
    p("g:g?{motion}", Label("op:g?? rot13")),
    p("g:g@{motion}", Keys("g@")),
    p("g:g+", Keys("g+")),
    p("g:g-", Keys("g-")),
    p("g:gR", Keys("gR")),
    p("g:g'", Label("mark:g'")),
    p("g:g`", Label("mark:g`")),
    p("g:g&", Label("dot:g&")),
    p("g:gh", Label("misc:gh")),
    // --- z-Commands (z) ---
    p("z:zz", Label("scroll:zzH")),
    p("z:zt", Label("scroll:zt C-e")),
    p("z:zb", Label("scroll:zbH")),
    p("z:z<CR>", Label("scroll:z<CR> col first nonblank")),
    p("z:z.", Label("scroll:z.H")),
    p("z:z-", Label("scroll:z-H")),
    p("z:za", Label("fold:za closes an open defined fold")),
    p("z:zo", Label("fold:zo reopens a closed fold")),
    p("z:zc", Label("fold:zo then zc recloses the same fold")),
    p("z:zR", Label("fold:zR opens all folds")),
    p("z:zM", Label("fold:zM recloses a defined fold")),
    p("z:zA", Keys("zA")),
    p("z:zO", Label("fold:zO opens recursively")),
    p("z:zC", Label("fold:zC recloses recursively")),
    p("z:zd", Label("fold:zd deletes a fold")),
    p("z:zD", Label("fold:zD deletes a fold recursively")),
    p("z:zf{motion}", Label("fold:zfj hides one line")),
    p("z:zF", Keys("zF")),
    p("z:zv", Keys("zv")),
    p("z:zx", Keys("zx")),
    // #1163: spell is implemented (`src/core/spell.rs`) and these rows moved
    // N/A → ✅, so they are in scope and need probes. The corpus has no spell
    // cases at all today, so all seven are seeded into COVERAGE_EXEMPT — that
    // is a measured gap, and the bidirectional gate forces them to be deleted
    // from there the moment a case starts matching.
    p("z:z=", Label("spell:z= suggestions")),
    p("z:zg", Label("spell:zg good word")),
    p("z:zw", Label("spell:zw bad word")),
    p("z:zG", Label("spell:zG good word internal")),
    p("z:zW", Label("spell:zW bad word internal")),
    p("z:zj", Label("fold:zj moves to the defined fold header")),
    p("z:zk", Label("fold:zk moves to the defined fold header")),
    p("z:zh", Keys("zh")),
    p("z:zl", Keys("zl")),
    p("z:zH", Label("scroll:zH")),
    p("z:zL", Keys("zL")),
    p("z:ze", Label("scroll:ze")),
    p("z:zs", Label("scroll:zs")),
    // --- Window Commands (CTRL-W) (win) ---
    p("win:CTRL-W h", Keys("<C-w>h")),
    p("win:CTRL-W j", Keys("<C-w>j")),
    p("win:CTRL-W k", Keys("<C-w>k")),
    p("win:CTRL-W l", Keys("<C-w>l")),
    p("win:CTRL-W w", Keys("<C-w>w")),
    p("win:CTRL-W W", Keys("<C-w>W")),
    p("win:CTRL-W c", Keys("<C-w>c")),
    p("win:CTRL-W o", Keys("<C-w>o")),
    p("win:CTRL-W s", Keys("<C-w>s")),
    p("win:CTRL-W v", Keys("<C-w>v")),
    p("win:CTRL-W e/E", Keys("<C-w>e/E")),
    p("win:CTRL-W +", Keys("<C-w>+")),
    p("win:CTRL-W -", Keys("<C-w>-")),
    p("win:CTRL-W <", Label("win:C-w <")),
    p("win:CTRL-W >", Keys("<C-w>>")),
    p("win:CTRL-W =", Keys("<C-w>=")),
    p("win:CTRL-W _", Keys("<C-w>_")),
    p("win:CTRL-W \\|", Keys("<C-w>\\|")),
    p("win:CTRL-W H", Keys("<C-w>H")),
    p("win:CTRL-W J", Keys("<C-w>J")),
    p("win:CTRL-W K", Keys("<C-w>K")),
    p("win:CTRL-W L", Keys("<C-w>L")),
    p("win:CTRL-W T", Keys("<C-w>T")),
    p("win:CTRL-W x", Keys("<C-w>x")),
    p("win:CTRL-W r", Keys("<C-w>r")),
    p("win:CTRL-W R", Keys("<C-w>R")),
    p("win:CTRL-W p", Keys("<C-w>p")),
    p("win:CTRL-W n", Keys("<C-w>n")),
    p("win:CTRL-W t", Keys("<C-w>t")),
    p("win:CTRL-W b", Keys("<C-w>b")),
    p("win:CTRL-W q", Keys("<C-w>q")),
    p("win:CTRL-W f", Keys("<C-w>f")),
    p("win:CTRL-W d", Keys("<C-w>d")),
    // --- Bracket Commands (bracket) ---
    p("bracket:]c", Keys("]c")),
    // #1163: see the z-section note — spell moved N/A → ✅.
    p("bracket:[s", Label("spell:[s prev misspelling")),
    p("bracket:]s", Label("spell:]s next misspelling")),
    p("bracket:[c", Keys("[c")),
    p("bracket:]d", Keys("]d")),
    p("bracket:[d", Keys("[d")),
    p("bracket:]p", Label("op:]p")),
    p("bracket:[p", Keys("[p")),
    p("bracket:[[", Label("word:[[")),
    p("bracket:]]", Label("word:]]")),
    p("bracket:[]", Keys("[]")),
    p("bracket:][", Keys("][")),
    p("bracket:[m", Keys("[m")),
    p("bracket:]m", Keys("]m")),
    p("bracket:[M", Keys("[M")),
    p("bracket:]M", Keys("]M")),
    p("bracket:[{", Label("word:[{")),
    p("bracket:]}", Label("word:]}")),
    p("bracket:[(", Label("word:[(")),
    p("bracket:])", Label("word:])")),
    p("bracket:[*", Keys("[*")),
    p("bracket:]*", Keys("]*")),
    p("bracket:[/", Label("word:[/")),
    p("bracket:]/", Label("word:]/")),
    p("bracket:[#", Keys("[#")),
    p("bracket:]#", Keys("]#")),
    p("bracket:[z", Label("fold:[z moves to start of open fold")),
    p("bracket:]z", Label("fold:]z moves to end of open fold")),
    // --- Operator-Pending Mode (oppend) ---
    p("oppend:w", Label("op:dw last word of line does not join")),
    p("oppend:b", Label("op:cb")),
    p("oppend:e", Label("op:ce")),
    p("oppend:ge", Label("op:dge")),
    p("oppend:W", Label("op:cW")),
    p("oppend:B", Label("op:dB")),
    p("oppend:E", Label("op:dE")),
    p("oppend:gE", Label("op:dgE")),
    p("oppend:0", Label("op:c0")),
    p("oppend:^", Label("op:d^")),
    p("oppend:$", Label("op:c$")),
    p("oppend:g_", Keys("dg_")),
    p("oppend:f", Label("op:d2f,")),
    p("oppend:t", Label("op:dt;")),
    p("oppend:F", Keys("dF")),
    p("oppend:T", Keys("dT")),
    p("oppend:;", Keys("d;")),
    p("oppend:,", Keys("d,")),
    p("oppend:h", Label("op:dh at col1")),
    p("oppend:j", Label("op:dj last line")),
    p("oppend:k", Label("op:dk first line")),
    p("oppend:l", Label("op:dl at eol")),
    p("oppend:{", Keys("d{")),
    p("oppend:}", Keys("d}")),
    p("oppend:(", Label("op:d( sentence")),
    p("oppend:)", Label("op:d) sentence")),
    p("oppend:H", Label("scroll:dH")),
    p("oppend:M", Label("scroll:dM")),
    p("oppend:L", Label("scroll:dL")),
    p("oppend:gg", Label("op:dgg mid")),
    p("oppend:G", Label("op:dG mid")),
    p("oppend:%", Label("op:d% on paren")),
    p("oppend:iw", Label("op:yiw cursor")),
    p("oppend:aw", Label("to:daw mid")),
    p("oppend:iW", Label("to:diW")),
    p("oppend:aW", Label("to:daW")),
    p("oppend:i\"", Label("to:di\" inside")),
    p("oppend:a\"", Label("to:da\" before quotes")),
    p("oppend:i'", Label("to:di'")),
    p("oppend:a'", Keys("da'")),
    p("oppend:i(", Label("to:di( inside")),
    p("oppend:a(", Label("to:da( nested")),
    p("oppend:i{", Label("to:di{ multiline")),
    p("oppend:a{", Label("to:da{ multiline")),
    p("oppend:i[", Label("to:di[")),
    p("oppend:a[", Label("to:da[")),
    p("oppend:ip", Label("op:>ip")),
    p("oppend:ap", Label("to:dap")),
    p("oppend:is", Label("op:dis")),
    p("oppend:as", Label("op:das")),
    p("oppend:it", Label("to:dit")),
    p("oppend:at", Label("to:dat")),
    p("oppend:i<", Label("to:di< nested")),
    p("oppend:a<", Label("to:da< outer")),
    p("oppend:i`", Label("to:di`")),
    p("oppend:a`", Keys("da`")),
    p("oppend:o_v", Label("op:dvj charwise force")),
    p("oppend:o_V", Keys("dVj")),
    p("oppend:o_CTRL-V", Label("op:d<C-v>j blockwise force")),
    // --- Visual Mode (visual) ---
    p("visual:v", Label("vis:vjd")),
    p("visual:V", Label("vis:Vjd")),
    p("visual:CTRL-V", Label("vb:jjd")),
    p("visual:o", Label("vis:vllohd")),
    p("visual:O", Label("vis:v_O charwise same as o")),
    p("visual:gv", Label("vis:gv after Vjd")),
    p("visual:d", Label("vis:vjd")),
    p("visual:x", Label("vis:v x")),
    p("visual:c", Label("vis:vec")),
    p("visual:s", Label("vis:v s")),
    p("visual:y", Label("vis:vjy cursor")),
    p("visual:>", Label("vis:vj>")),
    p("visual:<", Label("vis:vj<")),
    p("visual:~", Label("vis:vj~")),
    p("visual:u", Label("vis:Vju")),
    p("visual:U", Label("vis:vjU")),
    p("visual:=", Label("vis:Vj=")),
    p("visual:p", Label("vis:vlp linewise reg")),
    p("visual:P", Label("vis:vjP")),
    p("visual::", Label("vis:vj: shows range then s")),
    p("visual:J", Label("vis:vjJ")),
    p("visual:gJ", Label("vis:v_gJ")),
    p("visual:D", Label("vis:vjD")),
    p("visual:X", Label("vis:vjX")),
    p("visual:C", Label("vis:vjC")),
    p("visual:S", Label("vis:vjS")),
    p("visual:R", Label("vis:vjR")),
    p("visual:Y", Label("vis:vjY p")),
    p("visual:CTRL-A", Label("num:V C-a")),
    p("visual:CTRL-X", Label("num:V C-x")),
    p("visual:%", Label("vis:v% d")),
    p("visual:r{char}", Label("vis:vjr-")),
    p("visual:I", Label("vb:jjIx")),
    p("visual:A", Label("vb:jjAx")),
    p("visual:gq", Label("vis:vjgq")),
    p("visual:g CTRL-A", Label("num:V g C-a")),
    p("visual:g CTRL-X", Label("num:V g C-x")),
    p("visual:Movement keys", Label("vis:vjd")),
    // --- Core Vim Ex Commands (ex) ---
    p("ex::w", Keys(":w")),
    p("ex::write", Keys(":write")),
    p("ex::q", Keys(":q")),
    p("ex::quit", Keys(":quit")),
    p("ex::q!", Keys(":q!")),
    p("ex::wq", Keys(":wq")),
    p("ex::x", Keys(":x")),
    p("ex::qa", Keys(":qa")),
    p("ex::qa!", Keys(":qa!")),
    p("ex::wa", Keys(":wa")),
    p("ex::wqa", Keys(":wqa")),
    p("ex::xa", Keys(":xa")),
    p(
        "ex::e {file}",
        Label("jump:multi C-o after :e returns to A"),
    ),
    p("ex::edit", Keys(":edit")),
    p("ex::enew", Keys(":enew")),
    p("ex::bn", Keys(":bn")),
    p("ex::bp", Keys(":bp")),
    p("ex::bfirst", Keys(":bfirst")),
    p("ex::blast", Keys(":blast")),
    p("ex::b#", Keys(":b#")),
    p("ex::b {N}", Label("ex:b by number")),
    p("ex::bd", Keys(":bd")),
    p("ex::bdelete", Keys(":bdelete")),
    p("ex::bw", Keys(":bw")),
    p("ex::bwipeout", Keys(":bwipeout")),
    p("ex::ls", Keys(":ls")),
    p("ex::buffers", Keys(":buffers")),
    p("ex::split", Label("jump:multi C-o across split")),
    p("ex::sp", Keys(":sp ")),
    p("ex::vsplit", Label("jump:multi C-o across vsplit")),
    p("ex::vs", Keys(":vs ")),
    p("ex::close", Keys(":close")),
    p("ex::only", Keys(":only")),
    p("ex::hide", Keys(":hide")),
    p("ex::new", Keys(":new")),
    p("ex::vnew", Keys(":vnew")),
    p("ex::tabnew", Label("jump:multi C-o across tab")),
    p("ex::tabe", Keys(":tabe")),
    p("ex::tabclose", Keys(":tabclose")),
    p("ex::tabonly", Keys(":tabonly")),
    p("ex::tabnext", Keys(":tabnext")),
    p("ex::tabprevious", Keys(":tabprevious")),
    p("ex::tabfirst", Keys(":tabfirst")),
    p("ex::tablast", Keys(":tablast")),
    p("ex::tabmove", Keys(":tabmove")),
    p("ex::[range]s/pat/rep/[flags] [count]", Label("sub:basic")),
    p("ex::%s/pat/rep/", Label("sub:%")),
    p("ex::[range]g/pat/cmd", Label("g:d")),
    p("ex::v/pat/cmd", Label("g:v")),
    p("ex::d", Label("ex:d")),
    p("ex::delete", Keys(":delete")),
    p("ex::m", Label("ex:m0")),
    p("ex::move", Keys(":move")),
    p("ex::t", Label("ex:t.")),
    p("ex::co", Label("ex:1co$")),
    p("ex::copy", Keys(":copy")),
    p("ex::j", Label("ex:j")),
    p("ex::join", Keys(":join")),
    p("ex::y", Label("ex:y a")),
    p("ex::yank", Keys(":yank")),
    p("ex::pu", Label("ex:pu")),
    p("ex::put", Label("ex:put a")),
    p("ex::sort", Label("ex:sort")),
    p("ex::norm", Keys(":norm ")),
    p("ex::normal", Label("ex:normal Ax")),
    p("ex::noh", Label("ex:noh no effect")),
    p("ex::nohlsearch", Keys(":nohlsearch")),
    p("ex::startinsert", Keys(":startinsert")),
    p("ex::stopinsert", Keys(":stopinsert")),
    p("ex:Ex ranges", Label("ex:2;+1d")),
    p("ex::set {option}", Label("op:cc noautoindent")),
    p("ex::r {file}", Label("ex:r !echo")),
    p("ex::read", Keys(":read")),
    p("ex::!{cmd}", Label("ex:%!sort")),
    p("ex::reg", Keys(":reg")),
    p("ex::registers", Keys(":registers")),
    p("ex::marks", Keys(":marks")),
    p("ex::jumps", Keys(":jumps")),
    p("ex::changes", Keys(":changes")),
    p("ex::history", Keys(":history")),
    p("ex::echo {text}", Keys(":echo")),
    p("ex::pwd", Keys(":pwd")),
    p("ex::file", Keys(":file")),
    p("ex::>", Label("ex:>")),
    p("ex::<", Label("ex:<")),
    p("ex::=", Keys(":=")),
    p("ex::#", Keys(":#")),
    p("ex::number", Keys(":number")),
    p("ex::print", Keys(":print")),
    p("ex::ma", Keys(":ma ")),
    p("ex::mark", Label("ex:2mark a")),
    p("ex::delmarks", Keys(":delmarks")),
    p("ex::delm", Keys(":delm ")),
    p("ex::retab", Label("ex:retab")),
    p("ex::saveas {file}", Keys(":saveas")),
    p("ex::update", Keys(":update")),
    p("ex::cquit", Keys(":cquit")),
    p("ex::version", Keys(":version")),
    p("ex::help", Keys(":help")),
    // A bare `Keys(":h")` needle is exactly the over-crediting trap the
    // module doc's "the probe is deliberately dumb" section warns about: it
    // is a substring of `:help` (the id right above) and, since #1154, of
    // `:hide` too — so it would silently "cover" `:h` off the back of an
    // oracle case that never once typed the standalone `:h` help command.
    // `<CR>` immediately after pins it to the bare, no-argument invocation.
    p("ex::h", Keys(":h<CR>")),
    p("ex::windo {cmd}", Keys(":windo")),
    p("ex::bufdo {cmd}", Keys(":bufdo")),
    p("ex::tabdo {cmd}", Keys(":tabdo")),
    p("ex::diffsplit", Keys(":diffsplit")),
    p("ex::diffthis", Keys(":diffthis")),
    p("ex::diffoff", Keys(":diffoff")),
    p("ex::grep", Keys(":grep")),
    p("ex::vimgrep", Keys(":vimgrep")),
    p("ex::copen", Keys(":copen")),
    p("ex::cclose", Keys(":cclose")),
    p("ex::cn", Keys(":cn")),
    p("ex::cp", Keys(":cp")),
    p("ex::cc", Keys(":cc")),
    // --- #1155: quickfix completion + the location-list family ---
    p("ex::cfirst", Keys(":cfirst")),
    p("ex::clast", Keys(":clast")),
    p("ex::cwindow", Keys(":cwindow")),
    p("ex::clist", Keys(":clist")),
    p("ex::colder", Keys(":colder")),
    p("ex::cnewer", Keys(":cnewer")),
    p("ex::cdo", Keys(":cdo")),
    p("ex::cfdo", Keys(":cfdo")),
    p("ex::lopen", Keys(":lopen")),
    p("ex::lclose", Keys(":lclose")),
    p("ex::lwindow", Keys(":lwindow")),
    p("ex::lnext", Keys(":lnext")),
    p("ex::lprevious", Keys(":lprevious")),
    p("ex::lfirst", Keys(":lfirst")),
    p("ex::llast", Keys(":llast")),
    p("ex::ll", Keys(":ll")),
    p("ex::llist", Keys(":llist")),
    p("ex::ldo", Keys(":ldo")),
    p("ex::lfdo", Keys(":lfdo")),
    p("ex::lgrep", Keys(":lgrep")),
    p("ex::lvimgrep", Keys(":lvimgrep")),
    p("ex::cd {path}", Keys(":cd")),
    p("ex::colorscheme", Keys(":colorscheme")),
    // #1151
    p("ex::map", Label("map:nmap_chases_recursively")),
    p("ex::nmap", Label("map:nmap_chases_recursively")),
    p("ex::imap", Label("map:inoremap_jk_to_escape")),
    p("ex::make", Keys(":make")),
    p("ex::b {name}", Label("ex:b by name")),
    p("ex::Explore", Keys(":Explore")),
    p("ex::Ex", Keys(":Ex")),
    p("ex::Sexplore", Keys(":Sexplore")),
    p("ex::Sex", Keys(":Sex")),
    p("ex::Vexplore", Keys(":Vexplore")),
    p("ex::Vex", Keys(":Vex")),
];

// ---------------------------------------------------------------------------
// COVERAGE_EXEMPT — this list may only ever SHRINK.
//
// **217 of the 563 commands `VIM_COMPATIBILITY.md` marks ✅/⚠️ have no oracle
// case at all** (38.5%; 346 are covered). That number is the measurement this
// gate exists to produce, and it is the first one anybody has taken: the doc
// itself reads "422/424, 100% — Remaining Missing Commands: None", which is a
// claim about *existence*, and `COVERAGE_PHASE5.md` audits 2 of 6 areas.
//
// Where the gap is, at a glance (uncovered / in-scope, seeded 2026-09):
//
//     Core Vim ex commands       84/111  :w :q :bn :ls :marks :grep …
//     Window commands (CTRL-W)   33/33   nothing in the corpus presses <C-w>
//     g-commands                 23/50   gt gT gf gF ga g8 gx gR g@ g+ g- …
//     Bracket commands           17/26   ]c [c ]d [d [m ]m [* ]* [# ]# …
//     Normal — other             16/34   gt gT gf gF K ga g8 gx q: q/ q? …
//     Text objects               10/32   every closing-bracket alias, a' a`
//     Operator-pending           10/59   d{ d} d; d, dF dT and the o_ forces
//     z-commands                 10/28   zA zF zv zx zh zl zH zL ze zs
//     Normal — search & marks     4/31   // /<CR> aliases, `{A-Z}, '<, g' g`
//     Normal — movement           4/48   l g0 gm gM
//     Visual mode                 3/38   P, CTRL-X, g CTRL-X
//     Insert mode                 2/23   CTRL-@, CTRL-G j/k
//     Normal — editing            1/50   [p
//
// Deleting an entry is how an oracle case proves itself: the gate fails if a
// listed id's probe starts matching, and fails if an unlisted id's probe
// matches nothing. Never add an entry to paper over a deleted case.
// ---------------------------------------------------------------------------

const COVERAGE_EXEMPT: &[&str] = &[
    // --- Spell (#1163) ---
    // `src/core/spell.rs` implements all seven, so they are ✅ in the doc and
    // in scope here — but the oracle corpus contains no spell case whatsoever,
    // so every probe above matches nothing. Exempt, and shrinkable: adding one
    // real spell case forces its entry to be deleted from this list.
    "z:z=",
    "z:zg",
    "z:zw",
    "z:zG",
    "z:zW",
    "bracket:[s",
    "bracket:]s",
    // --- Insert Mode (ins) ---
    "ins:CTRL-@",
    "ins:CTRL-G j/k",
    // --- Normal Mode - Movement (move) ---
    "move:l",
    "move:g0",
    "move:gm",
    "move:gM",
    // --- Normal Mode - Editing (edit) ---
    "edit:[p",
    // --- Normal Mode - Search & Marks (search) ---
    "search:`{A-Z}",
    "search:'<",
    "search:g'",
    "search:g`",
    // --- Normal Mode - Other (other) ---
    "other:gt",
    "other:gT",
    "other:gf",
    "other:gF",
    "other:K",
    "other:ga",
    "other:g8",
    "other:gx",
    "other:CTRL-^",
    "other:CTRL-]",
    "other:CTRL-G",
    "other:do",
    "other:dp",
    "other:q:",
    "other:q/",
    "other:q?",
    // --- Text Objects (textobj) ---
    "textobj:a'",
    "textobj:a`",
    "textobj:i)",
    "textobj:a)",
    "textobj:i}",
    "textobj:a}",
    "textobj:i]",
    "textobj:a]",
    "textobj:i>",
    "textobj:a>",
    // --- g-Commands (g) ---
    "g:g0",
    "g:g<Home>",
    "g:g^",
    "g:g$",
    "g:g<End>",
    "g:gf",
    "g:gF",
    "g:gt",
    "g:gT",
    "g:g<Tab>",
    "g:g.",
    "g:gx",
    "g:ga",
    "g:g8",
    "g:gm",
    "g:gM",
    "g:g@{motion}",
    "g:g+",
    "g:g-",
    "g:gR",
    "g:g'",
    "g:g`",
    "g:gh",
    // --- z-Commands (z) ---
    "z:zA",
    "z:zF",
    "z:zv",
    "z:zx",
    "z:zh",
    "z:zl",
    "z:zH",
    "z:zL",
    "z:ze",
    "z:zs",
    // --- Window Commands (CTRL-W) (win) ---
    "win:CTRL-W h",
    "win:CTRL-W j",
    "win:CTRL-W k",
    "win:CTRL-W l",
    "win:CTRL-W w",
    "win:CTRL-W W",
    "win:CTRL-W c",
    "win:CTRL-W o",
    "win:CTRL-W s",
    "win:CTRL-W v",
    "win:CTRL-W e/E",
    "win:CTRL-W +",
    "win:CTRL-W -",
    "win:CTRL-W <",
    "win:CTRL-W >",
    "win:CTRL-W =",
    "win:CTRL-W _",
    "win:CTRL-W \\|",
    "win:CTRL-W H",
    "win:CTRL-W J",
    "win:CTRL-W K",
    "win:CTRL-W L",
    "win:CTRL-W T",
    "win:CTRL-W x",
    "win:CTRL-W r",
    "win:CTRL-W R",
    "win:CTRL-W p",
    "win:CTRL-W n",
    "win:CTRL-W t",
    "win:CTRL-W b",
    "win:CTRL-W q",
    "win:CTRL-W f",
    "win:CTRL-W d",
    // --- Bracket Commands (bracket) ---
    "bracket:]c",
    "bracket:[c",
    "bracket:]d",
    "bracket:[d",
    "bracket:[p",
    "bracket:[]",
    "bracket:][",
    "bracket:[m",
    "bracket:]m",
    "bracket:[M",
    "bracket:]M",
    "bracket:[*",
    "bracket:]*",
    "bracket:[/",
    "bracket:]/",
    "bracket:[#",
    "bracket:]#",
    // --- Operator-Pending Mode (oppend) ---
    "oppend:g_",
    "oppend:F",
    "oppend:T",
    "oppend:;",
    "oppend:,",
    "oppend:{",
    "oppend:}",
    "oppend:a'",
    "oppend:a`",
    "oppend:o_V",
    // --- Visual Mode (visual) ---
    "visual:P",
    "visual:CTRL-X",
    "visual:g CTRL-X",
    // --- Core Vim Ex Commands (ex) ---
    "ex::w",
    "ex::write",
    "ex::q",
    "ex::quit",
    "ex::q!",
    "ex::wq",
    "ex::x",
    "ex::qa",
    "ex::qa!",
    "ex::wa",
    "ex::wqa",
    "ex::xa",
    "ex::edit",
    "ex::bn",
    "ex::bp",
    "ex::b#",
    "ex::b {N}",
    "ex::bd",
    "ex::bdelete",
    "ex::ls",
    "ex::buffers",
    "ex::sp",
    "ex::vs",
    "ex::close",
    "ex::only",
    "ex::new",
    "ex::vnew",
    "ex::tabe",
    "ex::tabclose",
    "ex::tabnext",
    "ex::tabprevious",
    "ex::tabmove",
    "ex::delete",
    "ex::move",
    "ex::copy",
    "ex::join",
    "ex::yank",
    "ex::norm",
    "ex::nohlsearch",
    "ex::read",
    "ex::reg",
    "ex::registers",
    "ex::marks",
    "ex::jumps",
    "ex::changes",
    "ex::history",
    "ex::echo {text}",
    "ex::pwd",
    "ex::file",
    "ex::=",
    "ex::#",
    "ex::number",
    "ex::print",
    "ex::ma",
    "ex::saveas {file}",
    "ex::update",
    "ex::cquit",
    "ex::version",
    "ex::help",
    "ex::h",
    "ex::windo {cmd}",
    "ex::bufdo {cmd}",
    "ex::tabdo {cmd}",
    "ex::diffsplit",
    "ex::diffthis",
    "ex::diffoff",
    "ex::grep",
    "ex::vimgrep",
    "ex::copen",
    "ex::cclose",
    "ex::cn",
    "ex::cp",
    // --- #1155: quickfix completion + the location-list family — no oracle
    // case exercises any of these yet, same gap as the rest of "Core Vim ex
    // commands" above.
    "ex::cfirst",
    "ex::clast",
    "ex::cwindow",
    "ex::clist",
    "ex::colder",
    "ex::cnewer",
    "ex::cdo",
    "ex::cfdo",
    "ex::lopen",
    "ex::lclose",
    "ex::lwindow",
    "ex::lnext",
    "ex::lprevious",
    "ex::lfirst",
    "ex::llast",
    "ex::ll",
    "ex::llist",
    "ex::ldo",
    "ex::lfdo",
    "ex::lgrep",
    "ex::lvimgrep",
    "ex::cd {path}",
    "ex::colorscheme",
    "ex::make",
    "ex::b {name}",
    "ex::Explore",
    "ex::Ex",
    "ex::Sexplore",
    "ex::Sex",
    "ex::Vexplore",
    "ex::Vex",
];

// ---------------------------------------------------------------------------
// Oracle version (#865, #872) — deliberately adjacent to KNOWN_DEVIATIONS
// above, because a deviation list is only meaningful against the oracle that
// produced it. If you regenerate the list, move `DEVIATIONS_ORACLE` in the
// same commit.
// ---------------------------------------------------------------------------

/// The `(major, minor)` Neovim that [`KNOWN_DEVIATIONS`] was last regenerated
/// against — the fleet-standard Homebrew/upstream `v0.12.5` that every agent
/// host and both CI jobs run (#872; `ubuntu-24.04` apt's 0.9.5 is below
/// [`MIN_NVIM_VERSION`] and was replaced as CI's oracle by #865). See the
/// "Oracle version skew" section of the module docs: a run against anything
/// else prints a loud banner, and the "a listed label now passes" direction of
/// the gate is downgraded to an advisory (see [`fixes_are_enforced`]).
const DEVIATIONS_ORACLE: (u32, u32) = (0, 12);

/// The minimum `(major, minor)` Neovim this suite will accept as an oracle
/// (#865). The fleet standard — every agent host, and both CI jobs — is
/// Homebrew's / upstream's current stable `v0.12.5`; `0.12` is the floor that
/// pins. A host below it fails loudly rather than quietly producing verdicts
/// from a different Vim.
const MIN_NVIM_VERSION: (u32, u32) = (0, 12);

/// Opt out of the whole suite on a machine that cannot supply a usable oracle
/// (#865). Deliberately an explicit, greppable env var rather than an implicit
/// "`CI` is unset" skip: skipping must be a visible act on the command line,
/// not the default a coordinator Test leg silently inherits.
const ALLOW_SKIP_VAR: &str = "NVIM_CONFORMANCE_ALLOW_SKIP";

// ---------------------------------------------------------------------------
// Test runner
// ---------------------------------------------------------------------------

enum Outcome {
    Pass,
    Fail(String),
    NvimBroke,
}

fn run_case(case: &Case) -> Outcome {
    let nvim = match run_in_neovim(
        case.lines,
        case.cursor_line,
        case.cursor_col,
        case.keys,
        case.setup,
    ) {
        Some(r) => r,
        None => return Outcome::NvimBroke,
    };
    let (vc_buf, vc_line, vc_col) = run_in_vimcode(
        case.label,
        case.lines,
        case.cursor_line,
        case.cursor_col,
        case.keys,
        nvim.rows,
        case.setup,
    );
    let nvim_buf = nvim.buf.join("\n");
    let buf_match = vc_buf.trim_end_matches('\n') == nvim_buf.trim_end_matches('\n');
    // Neovim counts a trailing newline as an extra empty line; ropey does not.
    // Allow the VimCode cursor to be one line earlier in exactly that case.
    let cursor_match = (vc_line == nvim.line && vc_col == nvim.col)
        || (vc_buf.ends_with('\n')
            && nvim.buf.last().map(|s| s.is_empty()).unwrap_or(false)
            && vc_line + 1 == nvim.line
            && vc_col == nvim.col);
    if buf_match && cursor_match {
        return Outcome::Pass;
    }
    let what = match (buf_match, cursor_match) {
        (false, false) => "BUF+CUR",
        (false, true) => "BUF",
        _ => "CUR",
    };
    Outcome::Fail(format!(
        "{} [{}] keys={:?} start={:?}@({},{})\n  buffer: nvim={:?} vimcode={:?}\n  cursor: nvim=({},{}) vimcode=({},{})\n  nvim line('w0') after each key: {:?}",
        what,
        case.label,
        case.keys,
        case.lines,
        case.cursor_line,
        case.cursor_col,
        nvim_buf,
        vc_buf.trim_end_matches('\n'),
        nvim.line,
        nvim.col,
        vc_line,
        vc_col,
        // #1008: the post-redraw `w_topline` the oracle saw between
        // keystrokes. A scroll failure is usually readable straight off this
        // trace — which key moved the window, and by how much.
        nvim.toplines,
    ))
}

// ---------------------------------------------------------------------------
// The bidirectional KNOWN_DEVIATIONS gate, extracted as a pure function so the
// gate itself can be tested without an nvim on PATH — see
// `known_deviation_gate_is_bidirectional` at the bottom of this file. A gate
// that has never been observed to fail is not a gate (#553).
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq, Eq)]
struct Verdict<'a> {
    /// Failing labels that are *not* in the deviation list — regressions.
    regressions: Vec<&'a str>,
    /// Passing labels that *are* in the deviation list — a fix landed and the
    /// entry must be deleted, so the list can only shrink.
    fixed: Vec<&'a str>,
    /// Deviation-list entries matching no case label at all — stale.
    stale: Vec<&'a str>,
}

impl Verdict<'_> {
    fn is_clean(&self) -> bool {
        self.regressions.is_empty() && self.fixed.is_empty() && self.stale.is_empty()
    }
}

/// `outcomes` is `(label, passed)` for every case that actually ran.
/// `all_labels` is the full corpus, used only for the stale-entry check; pass
/// `None` when the run was filtered, since most labels legitimately didn't run.
fn classify<'a>(
    outcomes: &[(&'a str, bool)],
    known: &[&'a str],
    all_labels: Option<&[&'a str]>,
) -> Verdict<'a> {
    let known_set: std::collections::HashSet<&str> = known.iter().copied().collect();
    let mut verdict = Verdict::default();
    for (label, passed) in outcomes {
        match (known_set.contains(label), passed) {
            (false, false) => verdict.regressions.push(label),
            (true, true) => verdict.fixed.push(label),
            _ => {}
        }
    }
    if let Some(all) = all_labels {
        let all_set: std::collections::HashSet<&str> = all.iter().copied().collect();
        verdict.stale = known
            .iter()
            .copied()
            .filter(|l| !all_set.contains(l))
            .collect();
    }
    verdict
}

/// Parse `(major, minor)` out of `nvim --version`'s first line, which looks like
/// `NVIM v0.12.5` (or `NVIM v0.9.5` / `NVIM v0.11.0-dev+1234-gabcdef`).
/// `None` when the line isn't in that shape — an unknown version is treated as
/// "assume it matches", so a parse failure can only ever make the gate stricter.
fn parse_nvim_version(version_output: &str) -> Option<(u32, u32)> {
    let first = version_output.lines().next()?;
    let v = first.split_whitespace().nth(1)?.strip_prefix('v')?;
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor_field = parts.next()?;
    // `0.11.0-dev+123` → the dash belongs to the patch field, but be tolerant.
    let minor = minor_field.split(['-', '+']).next()?.parse::<u32>().ok()?;
    Some((major, minor))
}

/// Is the "a listed label now PASSES" direction of the gate enforcing?
///
/// Yes when the running Neovim is the one [`KNOWN_DEVIATIONS`] was captured
/// against, or when its version could not be determined (failing closed, so a
/// `nvim --version` format change cannot silently disable the gate). Otherwise
/// the oracle legitimately disagrees on some labels (#868) and deleting them
/// would hand the capture oracle the same count back as false regressions, so
/// the direction is downgraded to a printed advisory.
///
/// #865 removed the former `in_ci ||` short-circuit: it was correct only while
/// CI *was* the capture oracle. CI now runs the pinned fleet oracle (v0.12.5)
/// and, as of #872, so does [`DEVIATIONS_ORACLE`] — the enforcing lane is
/// "whichever lane runs [`DEVIATIONS_ORACLE`]", which is every lane again.
///
/// Note this only ever relaxes the *fixed* direction. Regressions and stale
/// entries stay fatal on every machine, CI included.
fn fixes_are_enforced(nvim: Option<(u32, u32)>) -> bool {
    nvim.is_none_or(|v| v == DEVIATIONS_ORACLE)
}

fn bullet_list(labels: &[&str]) -> String {
    labels
        .iter()
        .map(|l| format!("    {l:?},"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// First line of `nvim --version` (`NVIM v0.12.5`), trimmed — what the runner
/// echoes so a log names the exact oracle it used.
fn version_banner_line(version_output: &str) -> &str {
    version_output
        .lines()
        .next()
        .unwrap_or("(no output)")
        .trim()
}

/// Resolve `nvim` against `PATH` the way the OS would, for *reporting* only —
/// [`std::process::Command`] does its own lookup, but it will not tell us which
/// binary it picked, and "which nvim did this verdict come from" is exactly the
/// question #865 exists to answer. Platform-neutral: no `which`/`where` shell-out.
fn resolve_on_path(exe: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    // Windows carries the extension; trying both on every platform is harmless
    // because the miss simply doesn't exist on disk.
    let candidates = [exe.to_string(), format!("{exe}.exe")];
    std::env::split_paths(&path).find_map(|dir| {
        candidates
            .iter()
            .map(|name| dir.join(name))
            .find(|p| p.is_file() && is_executable(p))
    })
}

/// A regular file ahead of the real oracle on `PATH` but lacking the
/// executable bit would never actually be picked by the real `execvp`-style
/// lookup [`std::process::Command`] performs — only `is_file()` would still
/// name it as "resolved", which is misleading in a reporting-only helper
/// whose entire job is "which binary produced this verdict" (review finding
/// on #865). Windows has no executable bit in this sense; `.exe`/no-extension
/// matching in [`resolve_on_path`] is already the whole story there.
#[cfg(unix)]
fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_p: &std::path::Path) -> bool {
    true
}

/// Write `s` straight to the real OS-level stderr, bypassing libtest's
/// stdout/stderr capture.
///
/// libtest intercepts `print!`/`eprintln!` via a thread-local sink
/// (`io::set_output_capture`) and *discards* it for a passing test unless
/// `--nocapture`/`--show-output` was passed — see the rustc source for
/// `libtest::helpers::concurrency` capture handling. Neither of CI's `Run
/// tests` steps nor the coordinator's plain `cargo test` Test leg passes
/// either flag, and `nvim_conformance` passes on a healthy run — exactly the
/// common case this banner exists to narrate (#865 review finding: a
/// `println!` banner is invisible on precisely the lanes it was built for).
/// Opening `/dev/stderr` writes to fd 2 directly, underneath that capture, so
/// the banner reaches the log on a plain `cargo test` too.
#[cfg(unix)]
fn print_unmissable(s: &str) {
    use std::io::Write as _;
    match std::fs::OpenOptions::new().write(true).open("/dev/stderr") {
        Ok(mut f) => {
            let _ = f.write_all(s.as_bytes());
            let _ = f.flush();
        }
        // No /dev/stderr (sandboxed or unusual environment) — fall back to
        // the macro. Still correct on a failing run; better than nothing on
        // a passing one.
        Err(_) => eprintln!("{s}"),
    }
}

#[cfg(not(unix))]
fn print_unmissable(s: &str) {
    eprintln!("{s}");
}

// ---------------------------------------------------------------------------
// Preflight (#865) — decide what to do about the oracle BEFORE running 1,436
// cases. Pure, so the decision (and the exact wording of every message) is
// testable on a machine with no nvim at all; `nvim_conformance` only probes
// the environment and then does what this says.
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum Preflight {
    /// Oracle is usable. `banner` names the resolved path and version, and
    /// carries the skew warning when it is not [`DEVIATIONS_ORACLE`].
    Run { banner: String, version: (u32, u32) },
    /// Oracle is unusable but [`ALLOW_SKIP_VAR`] was set — skip deliberately.
    Skip { reason: String },
    /// Oracle is unusable and no opt-out was given — fail the run.
    Refuse { reason: String },
}

/// `probe` is `None` when `nvim` could not be run at all, otherwise the
/// resolved path (for the message) and the raw `nvim --version` output.
fn preflight(probe: Option<(&str, &str)>, allow_skip: bool) -> Preflight {
    let required = format!("{}.{}", MIN_NVIM_VERSION.0, MIN_NVIM_VERSION.1);
    let refuse = |reason: String| {
        if allow_skip {
            Preflight::Skip { reason }
        } else {
            Preflight::Refuse { reason }
        }
    };

    let Some((path, version_output)) = probe else {
        return refuse(format!(
            "nvim not found on PATH (or `nvim --version` failed).\n\n\
             tests/nvim_conformance.rs is this repo's only oracle-backed \
             Vim-behaviour suite — a missing `nvim` is NOT a pass, it is 1,436 \
             cases that did not run. Install Neovim >= {required} (the fleet \
             standard is v0.12.5) and make sure the `nvim` binary is on the PATH \
             this process inherits.\n\n\
             To skip this suite deliberately instead, set {ALLOW_SKIP_VAR}=1."
        ));
    };

    let banner_line = version_banner_line(version_output);
    let Some(version) = parse_nvim_version(version_output) else {
        return refuse(format!(
            "could not parse a version out of `nvim --version` for the oracle at \
             {path}.\n  first line: {banner_line:?}\n  required:   >= {required}\n\n\
             Refusing to run the conformance corpus against an oracle of unknown \
             vintage — KNOWN_DEVIATIONS is only meaningful against a known \
             version. Set {ALLOW_SKIP_VAR}=1 to skip this suite instead."
        ));
    };

    if version < MIN_NVIM_VERSION {
        return refuse(format!(
            "oracle Neovim is too old.\n  path:     {path}\n  found:    {banner_line} \
             (parsed {}.{}.x)\n  required: >= {required}\n\n\
             Neovim's own option defaults and headless behaviour move between \
             versions, so a verdict from {}.{}.x is not comparable with the fleet's \
             (v0.12.5) — see the \"Oracle version skew\" section of the module docs. \
             Upgrade, or set {ALLOW_SKIP_VAR}=1 to skip this suite.",
            version.0, version.1, version.0, version.1
        ));
    }

    let mut banner = format!(
        "\nnvim conformance oracle: {path}\n  version: {banner_line}\n  \
         KNOWN_DEVIATIONS captured against: {}.{}.x\n",
        DEVIATIONS_ORACLE.0, DEVIATIONS_ORACLE.1
    );
    if version != DEVIATIONS_ORACLE {
        banner.push_str(&format!(
            "\n\
             !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\n\
             !! ORACLE VERSION SKEW: running {}.{}.x, but KNOWN_DEVIATIONS was\n\
             !! last regenerated against {}.{}.x. Verdicts that move are far more\n\
             !! likely to be the oracle having changed than vimcode. The \"a listed\n\
             !! label now passes\" direction is therefore ADVISORY on this run —\n\
             !! do NOT delete entries to make it green. Regenerate the list and\n\
             !! bump DEVIATIONS_ORACLE in one deliberate commit instead.\n\
             !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\n",
            version.0, version.1, DEVIATIONS_ORACLE.0, DEVIATIONS_ORACLE.1
        ));
    }

    Preflight::Run { banner, version }
}

#[test]
fn nvim_conformance() {
    let version_output = std::process::Command::new("nvim")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let resolved = resolve_on_path("nvim");
    let resolved_display = resolved
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "nvim (resolved by PATH lookup)".to_string());
    let probe = version_output
        .as_deref()
        .map(|out| (resolved_display.as_str(), out));

    // #865: a missing/too-old oracle must never read as 1,436 passing cases.
    // Failing is the default on every lane — CI, a coordinator Test leg and a
    // developer laptop alike; skipping requires the explicit opt-out.
    let allow_skip = std::env::var_os(ALLOW_SKIP_VAR).is_some();
    let nvim_version = match preflight(probe, allow_skip) {
        Preflight::Refuse { reason } => panic!("\n\n{reason}\n"),
        Preflight::Skip { reason } => {
            eprintln!("SKIP ({ALLOW_SKIP_VAR} set): {reason}");
            return;
        }
        Preflight::Run { banner, version } => {
            // #865 review: println! is captured-and-discarded by libtest on a
            // passing test unless --nocapture/--show-output is passed, which
            // neither CI nor the coordinator's Test leg does — so this must
            // bypass that capture, not just write through it.
            print_unmissable(&banner);
            Some(version)
        }
    };
    let in_ci = std::env::var_os("CI").is_some();

    // Labels are the identity used by KNOWN_DEVIATIONS, so they must be unique.
    {
        let mut seen = std::collections::HashSet::new();
        let dupes: Vec<&str> = CATEGORIES
            .iter()
            .flat_map(|(_, g)| g.iter())
            .filter(|c| !seen.insert(c.label))
            .map(|c| c.label)
            .collect();
        assert!(
            dupes.is_empty(),
            "duplicate conformance case labels (labels key KNOWN_DEVIATIONS, so they must be unique): {dupes:?}"
        );
    }

    // #875: KNOWN_DEVIATIONS and HARNESS_LIMITED are mutually exclusive
    // buckets — a label wired into both would silently take whichever branch
    // the loop below checks first, masking the other array's accounting.
    {
        let known_set: std::collections::HashSet<&str> = KNOWN_DEVIATIONS.iter().copied().collect();
        let both: Vec<&str> = HARNESS_LIMITED
            .iter()
            .copied()
            .filter(|l| known_set.contains(l))
            .collect();
        assert!(
            both.is_empty(),
            "label(s) listed in both KNOWN_DEVIATIONS and HARNESS_LIMITED: {both:?}"
        );
    }

    let filter = std::env::var("PROBE_FILTER").ok();
    let dump_to = std::env::var("CONFORMANCE_DUMP_DEVIATIONS").ok();
    let verbose = std::env::var_os("PROBE_VERBOSE").is_some();

    let selected: Vec<(usize, &Case)> = CATEGORIES
        .iter()
        .enumerate()
        .flat_map(|(ci, (_, g))| g.iter().map(move |c| (ci, c)))
        .filter(|(_, c)| filter.as_deref().is_none_or(|f| c.label.contains(f)))
        .collect();

    // Each case is an independent `nvim` process, so fan them out; 1,400+
    // serial process spawns is minutes of wall clock for no reason.
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 16);
    let chunk = selected.len().div_ceil(workers).max(1);
    let mut results: Vec<(usize, &Case, Outcome)> = std::thread::scope(|scope| {
        let handles: Vec<_> = selected
            .chunks(chunk)
            .map(|slice| {
                scope.spawn(move || {
                    slice
                        .iter()
                        .map(|(ci, case)| (*ci, *case, run_case(case)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("conformance worker panicked"))
            .collect()
    });
    results.sort_by_key(|(ci, _, _)| *ci);

    let known: std::collections::HashSet<&str> = KNOWN_DEVIATIONS.iter().copied().collect();
    let harness_limited: std::collections::HashSet<&str> =
        HARNESS_LIMITED.iter().copied().collect();
    let mut totals = vec![(0usize, 0usize, 0usize); CATEGORIES.len()]; // (pass, known-fail, unexpected-fail)
    let mut outcomes: Vec<(&str, bool)> = Vec::new();
    let mut detail: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    let mut deviating: Vec<&str> = Vec::new();
    let mut nvim_broke: Vec<&str> = Vec::new();
    // #875: harness-limited cases are tracked in a wholly separate bucket —
    // (pass, fail) — and never touch `outcomes`/`totals`/`deviating`, so they
    // cannot become a regression, cannot force a KNOWN_DEVIATIONS deletion,
    // and cannot inflate the deviation count. See the HARNESS_LIMITED doc
    // comment for why.
    let mut harness_totals = (0usize, 0usize);
    let mut harness_seen: Vec<&str> = Vec::new();

    for (ci, case, outcome) in &results {
        if harness_limited.contains(case.label) {
            harness_seen.push(case.label);
            match outcome {
                Outcome::NvimBroke => {
                    nvim_broke.push(case.label);
                    if in_ci {
                        harness_totals.1 += 1;
                    }
                }
                Outcome::Pass => harness_totals.0 += 1,
                Outcome::Fail(msg) => {
                    harness_totals.1 += 1;
                    if verbose {
                        println!("HARNESS-LIMITED-FAIL {msg}");
                    }
                }
            }
            continue;
        }
        let listed = known.contains(case.label);
        match outcome {
            Outcome::NvimBroke => {
                nvim_broke.push(case.label);
                // A broken oracle is not a deviation — it's a broken harness,
                // so it is never excusable via KNOWN_DEVIATIONS under CI.
                if in_ci {
                    totals[*ci].2 += 1;
                    outcomes.push((case.label, false));
                    detail.insert(
                        case.label,
                        format!(
                            "NVIM-FAIL [{}]: nvim execution failed (treated as failure under CI)",
                            case.label
                        ),
                    );
                }
            }
            Outcome::Pass => {
                totals[*ci].0 += 1;
                outcomes.push((case.label, true));
                if verbose {
                    println!("PASS [{}]", case.label);
                }
            }
            Outcome::Fail(msg) => {
                deviating.push(case.label);
                outcomes.push((case.label, false));
                detail.insert(case.label, format!("FAIL {msg}"));
                if listed {
                    totals[*ci].1 += 1;
                    if verbose {
                        // Print the full diff, not just the label: auditing a
                        // KNOWN_DEVIATIONS entry (is it a real bug or a harness
                        // artifact?) needs the expected-vs-actual values, and
                        // they were previously only reachable by temporarily
                        // deleting the entry to turn it into a "regression".
                        println!("KNOWN-FAIL {msg}");
                    }
                } else {
                    totals[*ci].2 += 1;
                }
            }
        }
    }

    println!("\n=== Neovim Conformance Results ===");
    println!(
        "{:<32} {:>6} {:>6} {:>6} {:>6}",
        "category", "cases", "pass", "known", "FAIL"
    );
    for (ci, (name, _)) in CATEGORIES.iter().enumerate() {
        let (p, k, f) = totals[ci];
        if p + k + f == 0 {
            continue;
        }
        println!("{:<32} {:>6} {:>6} {:>6} {:>6}", name, p + k + f, p, k, f);
    }
    let (tp, tk, tf) = totals
        .iter()
        .fold((0, 0, 0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2));
    println!(
        "{:<32} {:>6} {:>6} {:>6} {:>6}",
        "TOTAL",
        tp + tk + tf,
        tp,
        tk,
        tf
    );
    if !nvim_broke.is_empty() {
        println!(
            "\nnvim execution failed for {} case(s): {:?}",
            nvim_broke.len(),
            nvim_broke
        );
    }
    if !harness_seen.is_empty() {
        // #875: reported, but deliberately excluded from every column above
        // and from the deviation gate below — see the HARNESS_LIMITED doc
        // comment for why these don't count as "how far from Neovim".
        println!(
            "\n{} case(s) excluded as HARNESS_LIMITED (not counted as deviations — see doc \
             comment): {} pass, {} fail",
            harness_seen.len(),
            harness_totals.0,
            harness_totals.1
        );
    }

    if let Some(path) = dump_to {
        let mut out = String::new();
        for label in &deviating {
            out.push_str(&format!("    {label:?},\n"));
        }
        std::fs::write(&path, &out).expect("failed to write CONFORMANCE_DUMP_DEVIATIONS file");
        println!(
            "\nCONFORMANCE_DUMP_DEVIATIONS: wrote {} failing label(s) to {path}",
            deviating.len()
        );
        return;
    }

    // Stale entries (labels that no longer exist) would silently mask a
    // regression if the case were ever re-added, so treat them as errors too.
    // Only meaningful on an unfiltered run.
    let all_labels: Vec<&str> = CATEGORIES
        .iter()
        .flat_map(|(_, g)| g.iter())
        .map(|c| c.label)
        .collect();
    let mut verdict = classify(
        &outcomes,
        KNOWN_DEVIATIONS,
        filter.is_none().then_some(all_labels.as_slice()),
    );

    // #875: HARNESS_LIMITED gets the same stale-entry hygiene as
    // KNOWN_DEVIATIONS — a label matching no case would silently exclude
    // nothing and nobody would notice. Only meaningful on an unfiltered run;
    // reuse `verdict.stale` so it goes through the existing reporting path.
    if filter.is_none() {
        let all_set: std::collections::HashSet<&str> = all_labels.iter().copied().collect();
        verdict.stale.extend(
            HARNESS_LIMITED
                .iter()
                .copied()
                .filter(|l| !all_set.contains(l)),
        );
    }

    // #868: on a Neovim that is not the one the list was captured against,
    // "this listed label now passes" is ambiguous — it is far more often the
    // oracle having improved than vimcode having. Deleting the entries to
    // satisfy such a run would hand the capture oracle the same count back as
    // false regressions. Report, don't fail. See the module docs.
    if !verdict.fixed.is_empty() && !fixes_are_enforced(nvim_version) {
        println!(
            "\nNOTE: {} KNOWN_DEVIATIONS entr(y/ies) pass against this run's \
             Neovim {} but the list was captured against {}.{}.x, \
             so this is oracle-version skew, not a landed fix — do NOT delete them \
             (see the module docs). Not failing the run:\n{}",
            verdict.fixed.len(),
            nvim_version
                .map(|(a, b)| format!("{a}.{b}.x"))
                .unwrap_or_else(|| "(unknown)".into()),
            DEVIATIONS_ORACLE.0,
            DEVIATIONS_ORACLE.1,
            bullet_list(&verdict.fixed)
        );
        verdict.fixed.clear();
    }

    if verdict.is_clean() {
        return;
    }

    let mut problems: Vec<String> = Vec::new();
    if !verdict.stale.is_empty() {
        problems.push(format!(
            "{} KNOWN_DEVIATIONS entr(y/ies) match no case label — delete them:\n{}",
            verdict.stale.len(),
            bullet_list(&verdict.stale)
        ));
    }
    if !verdict.regressions.is_empty() {
        problems.push(format!(
            "{} conformance REGRESSION(S) — cases not in KNOWN_DEVIATIONS that do not match Neovim:\n\n{}",
            verdict.regressions.len(),
            verdict
                .regressions
                .iter()
                .map(|l| detail.get(l).cloned().unwrap_or_else(|| (*l).to_string()))
                .collect::<Vec<_>>()
                .join("\n\n")
        ));
    }
    if !verdict.fixed.is_empty() {
        problems.push(format!(
            "{} case(s) listed in KNOWN_DEVIATIONS now PASS. \
             Good — delete these entries from KNOWN_DEVIATIONS so the list keeps shrinking:\n{}",
            verdict.fixed.len(),
            bullet_list(&verdict.fixed)
        ));
    }
    panic!("\n\n{}\n", problems.join("\n\n"));
}

// ---------------------------------------------------------------------------
// KNOWN_DEVIATIONS_MULTI — same bidirectional-gate idiom as KNOWN_DEVIATIONS
// above (`classify`), applied to CASES_MULTI_JUMP / CASES_MULTI_JUMPS_LIST.
// Reusing `classify` here rather than the `CATEGORIES`/`Case` scaffolding
// keeps the (well-exercised, 1,400+ case) single-buffer harness above
// untouched while still going through the identical bidirectional mechanism:
// an unlisted label that fails is a regression, and a listed label that
// starts passing must have its entry deleted. May only ever SHRINK.
// ---------------------------------------------------------------------------

// #985 added the multi-file cases; #1158 fixed the underlying bug --
// `open_file_with_mode` (`:e`/`:edit`), `new_tab` (`:tabnew`/`:tabe`), and
// `split_window_with_new_first` (`:split`/`:vsplit`) now call
// `push_jump_location` before switching the active buffer/window/tab away,
// matching Neovim's rule that opening a different file this way is
// jump-worthy regardless of line. All 9 entries this array used to carry
// now PASS against the real oracle, so the array — which may only ever
// SHRINK — is empty.
const KNOWN_DEVIATIONS_MULTI: &[&str] = &[];

#[test]
fn nvim_conformance_jumplist_multi_file() {
    let version_output = std::process::Command::new("nvim")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let resolved = resolve_on_path("nvim");
    let resolved_display = resolved
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "nvim (resolved by PATH lookup)".to_string());
    let probe = version_output
        .as_deref()
        .map(|out| (resolved_display.as_str(), out));

    let allow_skip = std::env::var_os(ALLOW_SKIP_VAR).is_some();
    let nvim_version = match preflight(probe, allow_skip) {
        Preflight::Refuse { reason } => panic!("\n\n{reason}\n"),
        Preflight::Skip { reason } => {
            eprintln!("SKIP ({ALLOW_SKIP_VAR} set): {reason}");
            return;
        }
        Preflight::Run { banner, version } => {
            print_unmissable(&banner);
            Some(version)
        }
    };

    let filter = std::env::var("PROBE_FILTER").ok();
    let verbose = std::env::var_os("PROBE_VERBOSE").is_some();

    let mut outcomes: Vec<(&str, bool)> = Vec::new();
    let mut detail: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    let mut nvim_broke: Vec<&str> = Vec::new();

    for case in CASES_MULTI_JUMP
        .iter()
        .filter(|c| filter.as_deref().is_none_or(|f| c.label.contains(f)))
    {
        match run_multi_case(case) {
            Outcome::NvimBroke => nvim_broke.push(case.label),
            Outcome::Pass => {
                outcomes.push((case.label, true));
                if verbose {
                    println!("PASS [{}]", case.label);
                }
            }
            Outcome::Fail(msg) => {
                outcomes.push((case.label, false));
                if verbose {
                    println!("FAIL {msg}");
                }
                detail.insert(case.label, msg);
            }
        }
    }
    for case in CASES_MULTI_JUMPS_LIST
        .iter()
        .filter(|c| filter.as_deref().is_none_or(|f| c.label.contains(f)))
    {
        match run_jumps_case(case) {
            Outcome::NvimBroke => nvim_broke.push(case.label),
            Outcome::Pass => {
                outcomes.push((case.label, true));
                if verbose {
                    println!("PASS [{}]", case.label);
                }
            }
            Outcome::Fail(msg) => {
                outcomes.push((case.label, false));
                if verbose {
                    println!("FAIL {msg}");
                }
                detail.insert(case.label, msg);
            }
        }
    }

    println!("\n=== Neovim Conformance Results: multi-file jumplist (#985) ===");
    println!(
        "cases run: {}  pass: {}  known-fail: {}",
        outcomes.len(),
        outcomes.iter().filter(|(_, p)| *p).count(),
        outcomes
            .iter()
            .filter(|(l, p)| !*p && KNOWN_DEVIATIONS_MULTI.contains(l))
            .count(),
    );
    if !nvim_broke.is_empty() {
        println!(
            "\nnvim execution failed for {} case(s): {:?}",
            nvim_broke.len(),
            nvim_broke
        );
    }

    let all_labels: Vec<&str> = CASES_MULTI_JUMP
        .iter()
        .map(|c| c.label)
        .chain(CASES_MULTI_JUMPS_LIST.iter().map(|c| c.label))
        .collect();
    let mut verdict = classify(
        &outcomes,
        KNOWN_DEVIATIONS_MULTI,
        filter.is_none().then_some(all_labels.as_slice()),
    );

    if !verdict.fixed.is_empty() && !fixes_are_enforced(nvim_version) {
        println!(
            "\nNOTE: {} KNOWN_DEVIATIONS_MULTI entr(y/ies) pass against this run's \
             Neovim but the list was captured against {}.{}.x, so this is oracle-version \
             skew, not a landed fix — do NOT delete them. Not failing the run:\n{}",
            verdict.fixed.len(),
            DEVIATIONS_ORACLE.0,
            DEVIATIONS_ORACLE.1,
            bullet_list(&verdict.fixed)
        );
        verdict.fixed.clear();
    }

    if verdict.is_clean() {
        return;
    }

    let mut problems: Vec<String> = Vec::new();
    if !verdict.stale.is_empty() {
        problems.push(format!(
            "{} KNOWN_DEVIATIONS_MULTI entr(y/ies) match no case label — delete them:\n{}",
            verdict.stale.len(),
            bullet_list(&verdict.stale)
        ));
    }
    if !verdict.regressions.is_empty() {
        problems.push(format!(
            "{} multi-file jumplist REGRESSION(S) — cases not in KNOWN_DEVIATIONS_MULTI \
             that do not match Neovim:\n\n{}",
            verdict.regressions.len(),
            verdict
                .regressions
                .iter()
                .map(|l| detail.get(l).cloned().unwrap_or_else(|| (*l).to_string()))
                .collect::<Vec<_>>()
                .join("\n\n")
        ));
    }
    if !verdict.fixed.is_empty() {
        problems.push(format!(
            "{} case(s) listed in KNOWN_DEVIATIONS_MULTI now PASS. \
             Good — delete these entries so the list keeps shrinking:\n{}",
            verdict.fixed.len(),
            bullet_list(&verdict.fixed)
        ));
    }
    panic!("\n\n{}\n", problems.join("\n\n"));
}

// ---------------------------------------------------------------------------
// #1002: `setup` must actually reach the vimcode side.
//
// These need no nvim — they drive `run_in_vimcode` directly, which is the
// function that used to drop `setup` entirely. Each one is written so that
// *deleting the `apply_setup` call* makes it fail: the "with setup" and
// "without setup" arms of every pair assert different results, so a refactor
// that silently reverts to defaults collapses the pair and the test goes red.
// A test that passes whether or not `setup` is wired would be worthless here —
// that is exactly the shape of the bug being guarded against.
// ---------------------------------------------------------------------------

/// Probe the vimcode side the way a `cs(..)` case does, with and without the
/// option, and return `(cursor_line, cursor_col)` for each.
fn sol_probe(setup: &str) -> (usize, usize) {
    let (_, line, col) = run_in_vimcode(
        "self-test:setup probe",
        &["a", "    b"],
        1,
        1,
        "G",
        24,
        setup,
    );
    (line, col)
}

/// `'startofline'` changes where `G` lands: on with it, the first non-blank
/// column; off, the column is kept. If `setup` is dropped, both arms return the
/// default (off) answer and the inequality assert below fails.
#[test]
fn case_setup_reaches_the_vimcode_side() {
    let with_sol = sol_probe("vim.o.startofline=true");
    let without_sol = sol_probe("");
    assert_ne!(
        with_sol, without_sol,
        "`setup` is being dropped on the vimcode side again (#1002): `G` landed at \
         {with_sol:?} both with and without `vim.o.startofline=true`"
    );
    // Pin the actual values too, so "different" can't be satisfied by a
    // regression that moves the wrong arm.
    assert_eq!(
        with_sol,
        (2, 5),
        "startofline=true lands on the first non-blank"
    );
    assert_eq!(without_sol, (2, 1), "startofline off keeps the column");
}

/// The other options the corpus pins, each proved to flip a real behaviour.
#[test]
fn every_setup_option_the_corpus_uses_changes_vimcode_behaviour() {
    // 'joinspaces' — two spaces after a `.` when joining.
    let joined =
        |setup: &str| run_in_vimcode("self-test:js", &["end.", "next"], 1, 1, "J", 24, setup).0;
    assert_eq!(joined("vim.o.joinspaces=true"), "end.  next");
    assert_eq!(joined(""), "end. next");

    // 'smarttab' — <BS> at the start of indent eats a whole shiftwidth with it
    // on, one column with it off.
    let bs =
        |setup: &str| run_in_vimcode("self-test:sta", &["    a"], 1, 5, "i<BS><Esc>", 24, setup).0;
    assert_eq!(bs("vim.o.smarttab=false"), "   a");
    assert_eq!(bs(""), "a");

    // 'nrformats' — octal must be opted into; alpha likewise.
    let inc = |lines: &'static [&'static str], setup: &str| {
        run_in_vimcode("self-test:nf", lines, 1, 1, "<C-a>", 24, setup).0
    };
    assert_eq!(inc(&["007"], "vim.o.nrformats='bin,octal,hex'"), "010");
    assert_eq!(inc(&["007"], ""), "008");
    assert_eq!(inc(&["a"], "vim.o.nrformats='alpha'"), "b");
    assert_eq!(inc(&["a"], ""), "a");

    // 'autoindent' — `o` off a `    foo` line.
    let open =
        |setup: &str| run_in_vimcode("self-test:ai", &["    foo"], 1, 1, "ox<Esc>", 24, setup).0;
    assert_eq!(open("vim.o.autoindent=false"), "    foo\nx");
    assert_eq!(open(""), "    foo\n    x");
}

/// An option `apply_setup` has no mapping for must be a hard, named failure —
/// never a silent fall-back to vimcode's defaults, which is the entire bug
/// #1002 fixed. The message has to name the offending statement so the reader
/// knows what to add.
#[test]
fn unrecognised_setup_is_a_hard_failure_naming_the_statement() {
    let mut s = Settings::default();
    // #1153 added `virtualedit` to `apply_setup` — swapped this example for
    // `listchars`, still unmapped (it's in vimcode's own "recognised but not
    // implemented" `:set` table, not wired to any behaviour).
    let err = apply_setup(&mut s, "vim.o.listchars='eol:$'").expect_err("must not be accepted");
    assert!(err.contains("listchars"), "must name the option: {err}");

    // Not the `vim.o.` statement form at all.
    let err = apply_setup(&mut s, "vim.cmd('set sol')").expect_err("must not be accepted");
    assert!(
        err.contains("vim.cmd('set sol')"),
        "must quote the statement: {err}"
    );

    // Recognised option, unparseable value.
    let err = apply_setup(&mut s, "vim.o.startofline=1").expect_err("must not be accepted");
    assert!(err.contains("startofline"), "must name the option: {err}");

    // Recognised statement form, missing `=`.
    assert!(apply_setup(&mut s, "vim.o.startofline").is_err());

    // A non-empty 'completeopt' is not the no-op the empty one is.
    assert!(apply_setup(&mut s, "vim.o.completeopt='menuone'").is_err());
    assert!(apply_setup(&mut s, "vim.o.completeopt=\"\"").is_ok());

    // ...and none of the rejected statements left a partial mutation behind
    // that would quietly reconfigure a later case.
    assert_eq!(s.startofline, Settings::default().startofline);
}

/// Every `setup` string in the corpus must be one `apply_setup` understands —
/// otherwise the failure only surfaces on a host with nvim installed. Runs
/// without an oracle, so CI's no-nvim lane catches a bad `cs(..)` too.
#[test]
fn every_corpus_setup_is_understood() {
    let mut bad = Vec::new();
    for case in CATEGORIES.iter().flat_map(|(_, cases)| cases.iter()) {
        if let Err(why) = apply_setup(&mut Settings::default(), case.setup) {
            bad.push(format!("  [{}] {}", case.label, why));
        }
    }
    assert!(
        bad.is_empty(),
        "conformance case(s) with a `setup` apply_setup cannot map:\n{}",
        bad.join("\n")
    );
}

/// The gate itself, exercised without needing nvim: both directions must be
/// able to fail, or `KNOWN_DEVIATIONS` is decoration rather than a gate (#553).
#[test]
fn known_deviation_gate_is_bidirectional() {
    let all = ["a:one", "a:two", "b:three"];
    let known = ["a:two"];

    // Steady state: the listed label fails, the unlisted ones pass → clean.
    let steady = classify(
        &[("a:one", true), ("a:two", false), ("b:three", true)],
        &known,
        Some(&all),
    );
    assert!(steady.is_clean(), "steady state should pass: {steady:?}");

    // Direction 1 — an unlisted label starts failing: regression, must fail.
    let regressed = classify(
        &[("a:one", true), ("a:two", false), ("b:three", false)],
        &known,
        Some(&all),
    );
    assert_eq!(regressed.regressions, vec!["b:three"]);
    assert!(!regressed.is_clean());

    // Direction 2 — a listed label starts passing: the fix must delete its
    // entry, so the run must fail until it does.
    let improved = classify(
        &[("a:one", true), ("a:two", true), ("b:three", true)],
        &known,
        Some(&all),
    );
    assert_eq!(improved.fixed, vec!["a:two"]);
    assert!(!improved.is_clean());

    // A deviation entry naming a case that no longer exists is stale — it would
    // silently excuse the case if it were ever re-added.
    let stale = classify(&[("a:one", true)], &["a:gone"], Some(&all));
    assert_eq!(stale.stale, vec!["a:gone"]);
    assert!(!stale.is_clean());

    // Filtered runs cannot judge staleness (most labels didn't run), so the
    // stale check is skipped rather than firing on every `PROBE_FILTER` run.
    let filtered = classify(&[("a:one", true)], &["a:gone"], None);
    assert!(filtered.stale.is_empty());
    assert!(filtered.is_clean());
}

/// #868/#865: the oracle-version escape hatch must relax the *fixed* direction
/// only, and only on a Neovim that is not the one the list was captured
/// against. Every other case stays enforcing.
#[test]
fn fixed_direction_is_advisory_only_on_a_different_nvim() {
    // The relaxed case: a newer Neovim than the capture oracle (0.12.5, as of
    // #872 — the version 0.12.5 that surfaced #868's 37 `scroll:` "passes" is
    // now the oracle itself, so a still-newer minor is used here instead).
    assert!(!fixes_are_enforced(Some((0, 13))));
    // ...and an *older* one skews just as legitimately.
    assert!(!fixes_are_enforced(Some((0, 8))));

    // Running exactly the Neovim the list was captured against: enforcing, so a
    // genuinely landed fix is still forced to delete its entry.
    assert!(fixes_are_enforced(Some(DEVIATIONS_ORACLE)));

    // An unparseable version is treated as "assume it matches" — failing closed,
    // so a `nvim --version` format change cannot silently disable the gate.
    assert!(fixes_are_enforced(None));
}

// ---------------------------------------------------------------------------
// #865 preflight: a missing or too-old oracle must never read as a pass. These
// assert on the exact text the runner emits (the messages are the product here
// — an operator reading a CI log is the consumer), not on some internal flag.
// ---------------------------------------------------------------------------

/// Acceptance #1: nvim absent, no opt-out → FAIL, and the message names both the
/// missing binary and the opt-out variable so the reader knows the way out.
#[test]
fn missing_nvim_fails_by_default_and_names_the_binary_and_the_opt_out() {
    let Preflight::Refuse { reason } = preflight(None, false) else {
        panic!(
            "a missing nvim with no opt-out must REFUSE, not skip: {:?}",
            preflight(None, false)
        );
    };
    assert!(reason.contains("nvim"), "must name the binary: {reason}");
    assert!(
        reason.contains(ALLOW_SKIP_VAR),
        "must name the opt-out variable: {reason}"
    );
    // The floor is part of "what would make this work".
    assert!(
        reason.contains(&format!("{}.{}", MIN_NVIM_VERSION.0, MIN_NVIM_VERSION.1)),
        "must name the required version: {reason}"
    );
}

/// Acceptance #2: with the opt-out set, a missing nvim skips as it used to.
#[test]
fn missing_nvim_skips_when_the_opt_out_is_set() {
    let Preflight::Skip { reason } = preflight(None, true) else {
        panic!("{ALLOW_SKIP_VAR}=1 with no nvim must SKIP");
    };
    assert!(reason.contains(ALLOW_SKIP_VAR));
}

/// Acceptance #3: an nvim below the floor fails naming path, found and required.
#[test]
fn nvim_below_the_declared_minimum_fails_naming_path_found_and_required() {
    let old = "NVIM v0.9.5\nBuild type: Release\nLuaJIT 2.1.0\n";
    let Preflight::Refuse { reason } = preflight(Some(("/usr/bin/nvim", old)), false) else {
        panic!("an oracle below {MIN_NVIM_VERSION:?} must REFUSE");
    };
    assert!(reason.contains("/usr/bin/nvim"), "resolved path: {reason}");
    assert!(reason.contains("NVIM v0.9.5"), "found version: {reason}");
    assert!(
        reason.contains(&format!(">= {}.{}", MIN_NVIM_VERSION.0, MIN_NVIM_VERSION.1)),
        "required version: {reason}"
    );

    // Exactly at the floor is fine — the check is `<`, not `<=`.
    let floor = format!("NVIM v{}.{}.0\n", MIN_NVIM_VERSION.0, MIN_NVIM_VERSION.1);
    assert!(matches!(
        preflight(Some(("/usr/bin/nvim", &floor)), false),
        Preflight::Run { .. }
    ));

    // And the opt-out covers this case too, for a host that genuinely can't
    // upgrade — but only when it is asked for explicitly.
    assert!(matches!(
        preflight(Some(("/usr/bin/nvim", old)), true),
        Preflight::Skip { .. }
    ));
}

/// An oracle whose version can't be parsed is of unknown vintage, so it is
/// refused rather than trusted — otherwise renaming the banner would silently
/// re-open the hole #865 closed.
#[test]
fn unparseable_nvim_version_is_refused_not_trusted() {
    let Preflight::Refuse { reason } = preflight(Some(("/opt/nvim", "NVIM vX.Y.Z\n")), false)
    else {
        panic!("an unparseable oracle version must REFUSE");
    };
    assert!(reason.contains("/opt/nvim"));
    assert!(reason.contains("NVIM vX.Y.Z"));
    assert!(reason.contains(ALLOW_SKIP_VAR));
}

/// Acceptance #4: a normal run names the binary it used and its version, and a
/// version other than the capture oracle is loudly (but non-fatally) flagged.
#[test]
fn a_usable_oracle_runs_and_the_banner_names_path_version_and_skew() {
    // A minor above the capture oracle ((0, 12) as of #872) — legitimate skew,
    // not the fleet standard itself.
    let fleet = "NVIM v0.13.5\nBuild type: Release\n";
    let Preflight::Run { banner, version } =
        preflight(Some(("/home/x/.local/bin/nvim", fleet)), false)
    else {
        panic!("a Neovim above the floor must be runnable");
    };
    assert_eq!(version, (0, 13));
    assert!(banner.contains("/home/x/.local/bin/nvim"), "path: {banner}");
    assert!(banner.contains("NVIM v0.13.5"), "version: {banner}");
    // 0.13.5 is not the capture oracle, so the skew banner must be there and
    // must name both versions.
    assert!(
        banner.contains("ORACLE VERSION SKEW"),
        "skew banner: {banner}"
    );
    assert!(
        banner.contains("0.13.x") && banner.contains("0.12.x"),
        "{banner}"
    );

    // Now the capture oracle itself. As of #872 it sits exactly at the floor
    // (both are 0.12), so the branch below always takes the "runnable, no
    // skew" arm today — kept as an if/else rather than collapsed so a future
    // DEVIATIONS_ORACLE bump that again falls below MIN_NVIM_VERSION is still
    // caught by this same assertion instead of silently going untested.
    let capture = format!("NVIM v{}.{}.5\n", DEVIATIONS_ORACLE.0, DEVIATIONS_ORACLE.1);
    let verdict = preflight(Some(("/usr/bin/nvim", &capture)), false);
    if DEVIATIONS_ORACLE < MIN_NVIM_VERSION {
        assert!(
            matches!(verdict, Preflight::Refuse { .. }),
            "a capture oracle below MIN_NVIM_VERSION must still be refused: {verdict:?}"
        );
    } else {
        let Preflight::Run { banner, .. } = verdict else {
            panic!("the capture oracle must be runnable once it meets the floor");
        };
        // Running exactly what the list was captured against: named, but no shout.
        assert!(banner.contains("/usr/bin/nvim"));
        assert!(!banner.contains("ORACLE VERSION SKEW"), "{banner}");
    }
}

/// The same three acceptance cases, end-to-end on the *real* `nvim_conformance`
/// test rather than on `preflight` alone: re-invoke this very test binary with a
/// PATH that contains no `nvim` (or a deliberately ancient fake one) and assert
/// on the exit status and the operator-visible output. A gate that has only ever
/// been exercised through a helper is not known to be wired up (#553).
///
/// `cfg(unix)`: the fake-oracle arm writes a `#!/bin/sh` stub. The `preflight`
/// tests above carry the same coverage on every platform.
#[cfg(unix)]
#[test]
fn nvim_conformance_end_to_end_refuses_a_missing_or_ancient_oracle() {
    use std::os::unix::fs::PermissionsExt;

    // A directory that is the entire PATH of the child process.
    let dir = std::env::temp_dir().join(format!(
        "vimcode-nvim-oracle-gate-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("temp PATH dir");

    let run_child = |allow_skip: bool| -> (bool, String) {
        let exe = std::env::current_exe().expect("test binary path");
        let mut cmd = std::process::Command::new(exe);
        cmd.args(["nvim_conformance", "--exact", "--nocapture"])
            .env("PATH", &dir)
            .env_remove("PROBE_FILTER")
            .env_remove("PROBE_VERBOSE")
            .env_remove("CONFORMANCE_DUMP_DEVIATIONS")
            .env_remove(ALLOW_SKIP_VAR);
        if allow_skip {
            cmd.env(ALLOW_SKIP_VAR, "1");
        }
        let out = cmd.output().expect("re-invoke the test binary");
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.success(), text)
    };

    // 1. No nvim anywhere on PATH, no opt-out → the test FAILS (before #865 it
    //    printed "SKIP" and reported `ok`).
    let (ok, output) = run_child(false);
    assert!(
        !ok,
        "a missing nvim must fail the suite, not pass it:\n{output}"
    );
    assert!(output.contains("nvim not found on PATH"), "{output}");
    assert!(output.contains(ALLOW_SKIP_VAR), "{output}");

    // 2. Same, with the opt-out set → skips, and the run is green.
    let (ok, output) = run_child(true);
    assert!(ok, "{ALLOW_SKIP_VAR}=1 must skip cleanly:\n{output}");
    assert!(output.contains("SKIP"), "{output}");

    // 3. An nvim that is present but below the floor → FAILS, naming the
    //    resolved path, the version it found and the one it needs.
    let fake = dir.join("nvim");
    std::fs::write(
        &fake,
        "#!/bin/sh\necho 'NVIM v0.9.5'\necho 'Build type: Release'\n",
    )
    .expect("write fake oracle");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
        .expect("chmod fake oracle");

    let (ok, output) = run_child(false);
    std::fs::remove_dir_all(&dir).ok();
    assert!(!ok, "an oracle below the floor must fail:\n{output}");
    assert!(
        output.contains(&fake.display().to_string()),
        "must name the resolved path:\n{output}"
    );
    assert!(
        output.contains("NVIM v0.9.5"),
        "must name what it found:\n{output}"
    );
    assert!(
        output.contains(&format!(">= {}.{}", MIN_NVIM_VERSION.0, MIN_NVIM_VERSION.1)),
        "must name what it requires:\n{output}"
    );
}

/// #865 review finding: every other assertion on the banner (path, version,
/// skew warning) either calls `preflight()` directly — which never goes
/// through libtest at all — or re-invokes the binary with `--nocapture`,
/// which CI and the coordinator's Test leg never pass. Neither proves the
/// banner is visible on the run that actually ships: a PASSING run of a
/// plain `cargo test` (no flags). Prove that here: re-invoke the real
/// `nvim_conformance` test against a fake-but-healthy oracle running a minor
/// above `DEVIATIONS_ORACLE` ((0, 12) as of #872) so the skew banner fires,
/// filtered to match zero cases (fast, and doesn't need a real `nvim
/// --headless`), *without* `--nocapture`, and confirm the banner — including
/// the version-skew warning — reached the child's real stdout/stderr anyway.
#[cfg(unix)]
#[test]
fn nvim_conformance_end_to_end_banner_visible_on_a_passing_uncaptured_run() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!(
        "vimcode-nvim-oracle-banner-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("temp PATH dir");
    let fake = dir.join("nvim");
    std::fs::write(
        &fake,
        "#!/bin/sh\necho 'NVIM v0.13.5'\necho 'Build type: Release'\n",
    )
    .expect("write fake oracle");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
        .expect("chmod fake oracle");

    let exe = std::env::current_exe().expect("test binary path");
    let out = std::process::Command::new(exe)
        .args(["nvim_conformance", "--exact"]) // deliberately no --nocapture
        .env("PATH", &dir)
        // Matches no case label, so zero cases actually run — the fake
        // oracle only needs to answer `--version`, and the run stays fast.
        .env("PROBE_FILTER", "zzz-no-such-conformance-case-zzz")
        .env_remove("PROBE_VERBOSE")
        .env_remove("CONFORMANCE_DUMP_DEVIATIONS")
        .env_remove(ALLOW_SKIP_VAR)
        .output()
        .expect("re-invoke the test binary");
    std::fs::remove_dir_all(&dir).ok();

    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));

    assert!(
        out.status.success(),
        "a healthy oracle with no matching cases must pass:\n{text}"
    );
    assert!(
        text.contains(&fake.display().to_string()),
        "the resolved path must reach a plain, uncaptured `cargo test` run \
         even though the test PASSED (#865 review finding — libtest discards \
         println! output for passing tests unless --nocapture/--show-output \
         is passed, and neither CI nor the coordinator's Test leg passes \
         them):\n{text}"
    );
    assert!(text.contains("NVIM v0.13.5"), "{text}");
    assert!(
        text.contains("ORACLE VERSION SKEW"),
        "the skew warning must be visible on a passing, uncaptured run too:\n{text}"
    );
}

#[test]
fn nvim_version_parses_release_and_dev_banners() {
    // The two that matter: CI's apt build, and the local build that hit #868.
    assert_eq!(
        parse_nvim_version("NVIM v0.9.5\nBuild type: Release\n"),
        Some((0, 9))
    );
    assert_eq!(
        parse_nvim_version("NVIM v0.12.5\nBuild type: Release\nLuaJIT 2.1\n"),
        Some((0, 12))
    );
    // Nightly/dev banners carry a suffix on the patch field; the minor is still
    // the thing we key on, and `0.11` must not be mistaken for `0.1`.
    assert_eq!(
        parse_nvim_version("NVIM v0.11.0-dev+1234-gabcdef\n"),
        Some((0, 11))
    );
    // Anything not in that shape is "unknown", which `fixes_are_enforced`
    // deliberately treats as enforcing rather than as a free pass.
    assert_eq!(parse_nvim_version(""), None);
    assert_eq!(parse_nvim_version("NVIM\n"), None);
    assert_eq!(parse_nvim_version("some other tool 1.2.3\n"), None);
    assert_eq!(parse_nvim_version("NVIM vX.Y.Z\n"), None);
}

/// #1007 — the shrink-only coverage ratchet. Pure: no `nvim`, no subprocess,
/// so it runs on every lane including `--no-default-features` and a laptop
/// with no oracle installed.
#[test]
fn conformance_corpus_covers_every_implemented_command() {
    // `include_str!` (not a runtime read) so editing the doc rebuilds this
    // test, and so it works from any CWD.
    let doc = include_str!("../VIM_COMPATIBILITY.md");
    let commands = parse_compatibility_doc(doc).unwrap_or_else(|e| panic!("{e}"));
    let cases = all_corpus_cases();

    // Regeneration aid, mirroring CONFORMANCE_DUMP_DEVIATIONS: dump the parsed
    // command inventory and the corpus's (label, keys) pairs as TSV so a human
    // re-seeding COMMAND_PROBES has the raw material, instead of re-deriving it
    // by eye from a 6,700-line file.
    //
    //   CONFORMANCE_DUMP_COVERAGE=/tmp/cov.tsv \
    //     cargo test --no-default-features --test nvim_conformance \
    //       conformance_corpus_covers
    if let Ok(path) = std::env::var("CONFORMANCE_DUMP_COVERAGE") {
        let mut s = String::new();
        for c in &commands {
            s.push_str(&format!(
                "CMD\t{}\t{:?}\tVIM_COMPATIBILITY.md:{}\n",
                c.id, c.status, c.line
            ));
        }
        for (label, keys) in &cases {
            s.push_str(&format!("CASE\t{label}\t{keys}\n"));
        }
        std::fs::write(&path, s).unwrap_or_else(|e| panic!("dump to {path}: {e}"));
        return;
    }

    let verdict = classify_coverage(&commands, COMMAND_PROBES, COVERAGE_EXEMPT, &cases);
    let in_scope = commands.iter().filter(|c| c.status.in_scope()).count();
    let exempt = COVERAGE_EXEMPT.len();

    assert!(
        verdict.is_clean(),
        "\n== conformance coverage ratchet (#1007) =={}\n\
         COVERAGE_EXEMPT may only ever SHRINK. See COMMAND_PROBES in this file\n\
         for what each id's probe means.\n",
        verdict.report()
    );

    // The headline this gate exists to produce. Printed under libtest's capture
    // (see `print_unmissable`) so a plain `cargo test` shows it on a *passing*
    // run, which is the run it is about.
    print_unmissable(&format!(
        "\n  conformance coverage: {covered}/{in_scope} implemented commands have \
         an oracle case ({pct}%), {exempt} in COVERAGE_EXEMPT ({cases} cases)\n",
        covered = in_scope - exempt,
        pct = (in_scope - exempt) * 100 / in_scope.max(1),
        cases = cases.len(),
    ));
}

/// The ratchet's two directions, on **synthetic** input so each is observed
/// failing. A gate that has never been seen to fail is not a gate (#553).
#[test]
fn coverage_ratchet_is_bidirectional() {
    let commands = vec![
        DocCommand {
            id: "z:za".to_string(),
            line: 10,
            status: DocStatus::Implemented,
        },
        DocCommand {
            id: "win:CTRL-W h".to_string(),
            line: 11,
            status: DocStatus::Implemented,
        },
        DocCommand {
            id: "ins:CTRL-K".to_string(),
            line: 12,
            status: DocStatus::NotApplicable,
        },
    ];
    let probes = [
        p("z:za", Label("fold:za closes")),
        p("win:CTRL-W h", Keys("<C-w>h")),
    ];
    let exempt = ["win:CTRL-W h"];
    let cases = [("fold:za closes", "zfjzozaj")];

    // Steady state: the covered id has a case, the exempt id has none.
    let steady = classify_coverage(&commands, &probes, &exempt, &cases);
    assert!(steady.is_clean(), "steady state should pass: {steady:?}");

    // Direction 1 — the entry is deleted but no case was added. Must fail, and
    // must name the command, not just a count.
    let deleted = classify_coverage(&commands, &probes, &[], &cases);
    assert_eq!(deleted.uncovered.len(), 1);
    assert!(deleted.uncovered[0].starts_with("win:CTRL-W h"));
    assert!(deleted.uncovered[0].contains("<C-w>h"));
    assert!(!deleted.is_clean());

    // Direction 2 — a case is added for an exempt command and the entry is left
    // in place. Must fail until the entry is deleted.
    let cases_plus = [
        ("fold:za closes", "zfjzozaj"),
        ("win:C-w h focuses left", "<C-w>hx"),
    ];
    let improved = classify_coverage(&commands, &probes, &exempt, &cases_plus);
    assert_eq!(improved.newly_covered.len(), 1);
    assert!(improved.newly_covered[0].starts_with("win:CTRL-W h"));
    assert!(improved.newly_covered[0].contains("win:C-w h focuses left"));
    assert!(!improved.is_clean());
    // …and deleting it then passes, so the list really can shrink.
    let shrunk = classify_coverage(&commands, &probes, &[], &cases_plus);
    assert!(
        shrunk.is_clean(),
        "after deletion it should pass: {shrunk:?}"
    );

    // A command with no probe at all is unmeasurable, not "covered".
    let unprobed = classify_coverage(&commands, &probes[..1], &[], &cases);
    assert_eq!(unprobed.unprobed.len(), 1);
    assert!(unprobed.unprobed[0].starts_with("win:CTRL-W h"));

    // A probe (or an exempt entry) naming a command the doc no longer marks
    // implemented is stale — it would silently excuse the id if it came back.
    let stale = classify_coverage(
        &commands,
        &[p("z:za", Label("fold:za closes")), p("z:zzz", Keys("zzz"))],
        &["z:zzz"],
        &cases,
    );
    assert_eq!(stale.stale_probes, vec!["z:zzz"]);
    assert_eq!(stale.stale_exempt, vec!["z:zzz"]);
    assert!(!stale.is_clean());

    // N/A rows are out of scope: no probe needed, and no complaint.
    assert!(!classify_coverage(&commands, &probes, &exempt, &cases)
        .unprobed
        .iter()
        .any(|u| u.contains("ins:CTRL-K")));
}

/// The same two directions, against the **real** tables and the real corpus —
/// so the demonstration is not confined to a toy fixture.
#[test]
fn coverage_ratchet_is_bidirectional_against_the_real_corpus() {
    let doc = include_str!("../VIM_COMPATIBILITY.md");
    let commands = parse_compatibility_doc(doc).unwrap_or_else(|e| panic!("{e}"));
    let cases = all_corpus_cases();

    // Direction 1 — delete a real entry without adding cases.
    let victim = "win:CTRL-W h";
    assert!(COVERAGE_EXEMPT.contains(&victim), "fixture drifted");
    let without: Vec<&str> = COVERAGE_EXEMPT
        .iter()
        .copied()
        .filter(|e| *e != victim)
        .collect();
    let deleted = classify_coverage(&commands, COMMAND_PROBES, &without, &cases);
    assert!(
        deleted.uncovered.iter().any(|u| u.starts_with(victim)),
        "deleting {victim:?} from COVERAGE_EXEMPT must fail the gate: {deleted:?}"
    );

    // Direction 2 — add a case for an exempt command, leave the entry alone.
    let mut plus = cases.clone();
    plus.push(("win:C-w h focuses the window to the left", "<C-w>hx"));
    let improved = classify_coverage(&commands, COMMAND_PROBES, COVERAGE_EXEMPT, &plus);
    assert!(
        improved.newly_covered.iter().any(|u| u.starts_with(victim)),
        "a new case for {victim:?} must fail the gate until its entry is \
         deleted: {improved:?}"
    );
}

/// Criterion 5 of #1007: the doc is parsed strictly. Every one of these is a
/// row that a lenient parser would drop, understating the gap — which is the
/// failure mode of the status quo this test replaces.
#[test]
fn an_unparseable_compatibility_row_fails_and_names_the_line() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "unknown table header",
            "## Insert Mode\n\n| Keystroke | Status |\n|---|---|\n| `x` | ✅ |\n",
            "unrecognised table header",
        ),
        (
            "command table under an unknown heading",
            "## Brand New Section\n\n| Command | Description | Status | Notes |\n\
             |---|---|---|---|\n| `x` | d | ✅ | |\n",
            "unknown heading",
        ),
        (
            "wrong cell count",
            "## Insert Mode\n\n| Command | Description | Status | Notes |\n\
             |---|---|---|---|\n| `x` | d | ✅ |\n",
            "expected 4",
        ),
        (
            "unknown status marker",
            "## Insert Mode\n\n| Command | Description | Status | Notes |\n\
             |---|---|---|---|\n| `x` | d | done | |\n",
            "unrecognised Status cell",
        ),
        (
            "prose command cell that is not allowlisted",
            "## Insert Mode\n\n| Command | Description | Status | Notes |\n\
             |---|---|---|---|\n| arrow keys | d | ✅ | |\n",
            "not in PROSE_ROWS",
        ),
        (
            "unterminated code span",
            "## Insert Mode\n\n| Command | Description | Status | Notes |\n\
             |---|---|---|---|\n| `x | d | ✅ | |\n",
            "unterminated inline code span",
        ),
        (
            "duplicate command id",
            "## Insert Mode\n\n| Command | Description | Status | Notes |\n\
             |---|---|---|---|\n| `x` | d | ✅ | |\n| `x` | e | ✅ | |\n",
            "duplicate command id",
        ),
    ];
    for (what, doc, needle) in cases {
        let err = parse_compatibility_doc(doc)
            .err()
            .unwrap_or_else(|| panic!("{what}: expected a parse failure, got Ok"));
        assert!(
            err.contains(needle),
            "{what}: error should mention {needle:?}, got: {err}"
        );
        assert!(
            err.starts_with("VIM_COMPATIBILITY.md:"),
            "{what}: error must name the line, got: {err}"
        );
    }
}

/// …and the formatting the doc *does* use parses, rather than being papered
/// over by a fallback. Each of these is a real row shape from the file.
#[test]
fn the_compatibility_doc_formatting_quirks_all_parse() {
    let doc = "\
## Normal Mode — Movement

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `h` | Left | ✅ | |
| `\\|` | Go to column N | ✅ | |
| `gt` / `gT` | Next/prev tab | ✅ | |
| `` g` `` | Mark without jumplist | ✅ | |
| `gH` / `gV` | Select mode | N/A | No Select mode |
| `:b {N}` | Go to buffer N | ⚠️ | By number only |

**Movement: 6/6 (100%)**

## Normal Mode — Editing

| Command | Description | Status | Notes |
|---------|-------------|--------|-------|
| `x` | Delete char | ✅ | |

| `y` | Yank | ✅ | after a mid-table blank line |
";
    let got = parse_compatibility_doc(doc).unwrap_or_else(|e| panic!("{e}"));
    let ids: Vec<&str> = got.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "move:h",
            // The escaped pipe survives as one cell, not three.
            "move:\\|",
            // `/`-joined aliases become one id each.
            "move:gt",
            "move:gT",
            // A double-backtick span whose content is itself a backtick.
            "move:g`",
            "move:gH",
            "move:gV",
            "move::b {N}",
            "edit:x",
            // A blank line inside a table does not end it (the doc has one
            // before the `:Explore` rows).
            "edit:y",
        ]
    );
    let scoped: Vec<&str> = got
        .iter()
        .filter(|c| c.status.in_scope())
        .map(|c| c.id.as_str())
        .collect();
    // N/A drops out of scope; ⚠️ stays in — a partial implementation is exactly
    // the kind that benefits from an oracle case.
    assert!(!scoped.contains(&"move:gH"));
    assert!(scoped.contains(&"move::b {N}"));
}

/// The real doc parses, and the in-scope command count matches what
/// COMMAND_PROBES was seeded against — so a doc edit that changes the
/// inventory is a visible, reviewable change here rather than silent drift.
#[test]
fn the_real_compatibility_doc_parses_with_no_dropped_rows() {
    let doc = include_str!("../VIM_COMPATIBILITY.md");
    let commands = parse_compatibility_doc(doc).unwrap_or_else(|e| panic!("{e}"));
    let in_scope = commands.iter().filter(|c| c.status.in_scope()).count();
    assert_eq!(
        in_scope,
        COMMAND_PROBES.len(),
        "every in-scope command needs exactly one COMMAND_PROBES entry"
    );
    // Sanity floor: the doc's own Summary claims 422 implemented *rows*, and
    // rows carrying `/`-joined aliases expand to more than one command each.
    assert!(
        in_scope > 422,
        "expected more command ids than the doc's 422 rows, got {in_scope}"
    );
    // Every PROSE_ROWS entry must still correspond to a real row, or it is a
    // stale allowlist entry that would start swallowing a future prose row.
    for (id, _) in PROSE_ROWS {
        assert!(
            commands.iter().any(|c| c.id == *id),
            "stale PROSE_ROWS entry {id:?}"
        );
    }
}

/// #1008: prove the oracle is what it claims to be — an **attached UI**, fed
/// **one key at a time**, re-validating the window between keystrokes.
///
/// Every assertion below was red against the `nvim --headless -l script.lua`
/// oracle this replaced, and for three distinct reasons, so this is not one
/// fact asserted three ways:
///
///   * the per-key `line('w0')` trace did not exist at all — the old oracle
///     handed nvim the whole sequence in one `nvim_feedkeys()` call and could
///     not have sampled between keys;
///   * `<C-d><C-d>` answered 22, because the second `<C-d>` inherited an
///     un-revalidated `w_botline`/`w_empty_rows` from the first. Real Neovim,
///     headless or interactive, answers 23;
///   * the window top moved between the two keystrokes, which is precisely
///     the state change the old oracle never performed.
///
/// Deliberately *not* folded into the `scroll:` corpus: those cases compare
/// vimcode against the oracle and would stay green if both sides regressed
/// together. This one pins the oracle's own answer to a number measured from
/// a real interactive Neovim session
/// (`scripts/nvim_headless_vs_interactive_repro.sh`).
#[test]
fn oracle_revalidates_the_window_between_keystrokes() {
    let Some(()) = oracle_available_for_unit_test() else {
        return;
    };

    let probe = oracle_probe(LONG, 1, 1, "<C-d><C-d>", "").expect("oracle probe failed");
    assert_eq!(
        probe.rows, 22,
        "the attached UI must give the same 22-row window the corpus assumes"
    );
    assert_eq!(
        probe.toplines.len(),
        2,
        "one `line('w0')` sample per keystroke — a burst oracle cannot produce this"
    );
    assert!(
        probe.toplines[0] < probe.toplines[1],
        "the window must move between the two keystrokes, not only after both: {:?}",
        probe.toplines
    );
    assert_eq!(
        probe.line, 23,
        "chained <C-d> lands on line 23 in a real Neovim; the headless-burst oracle said 22"
    );

    // The case that outlived every other #805 excuse, and the reason
    // `HARNESS_LIMITED` is empty: clamped `<C-b>` lands on the window bottom.
    let probe = oracle_probe(LONG, 60, 1, "2<C-b>", "").expect("oracle probe failed");
    assert_eq!(probe.line, 22, "2<C-b> from line 60 lands on line 22");
    assert_eq!(
        probe.toplines,
        vec![39, 1],
        "the count keystroke leaves the window alone; the <C-b> clamps it to the top"
    );
}

/// Shared guard for the unit tests that drive a real oracle: honour the same
/// version floor and the same explicit opt-out the suite itself does (#865),
/// rather than inventing a third skip rule.
fn oracle_available_for_unit_test() -> Option<()> {
    let version_output = std::process::Command::new("nvim")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let resolved = resolve_on_path("nvim")
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "nvim".to_string());
    let probe = version_output
        .as_deref()
        .map(|out| (resolved.as_str(), out));
    match preflight(probe, std::env::var_os(ALLOW_SKIP_VAR).is_some()) {
        Preflight::Run { .. } => Some(()),
        Preflight::Skip { reason } => {
            eprintln!("SKIP ({ALLOW_SKIP_VAR} set): {reason}");
            None
        }
        Preflight::Refuse { reason } => panic!("\n\n{reason}\n"),
    }
}
