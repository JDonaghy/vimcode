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
            // #1207
            "magic" => settings.magic = parse_lua_bool(name, value)?,
            "smartindent" | "si" => settings.smartindent = parse_lua_bool(name, value)?,
            "cindent" | "cin" => settings.cindent = parse_lua_bool(name, value)?,
            "showmatch" | "sm" => settings.showmatch = parse_lua_bool(name, value)?,
            // #1206
            "whichwrap" | "ww" => settings.whichwrap = value.to_string(),
            "backspace" | "bs" => settings.backspace = value.to_string(),
            "scrolljump" | "sj" => {
                settings.scrolljump = value.parse::<usize>().map_err(|_| {
                    format!("'scrolljump' expects a non-negative integer, got {raw_value:?}")
                })?;
            }
            "sidescrolloff" | "siso" => {
                settings.sidescrolloff = value.parse::<usize>().map_err(|_| {
                    format!("'sidescrolloff' expects a non-negative integer, got {raw_value:?}")
                })?;
            }
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
    // #1156: the issue's own gating scenario — `u` followed by a *different*
    // edit must not permanently discard the branch `u` left; `g-` walks the
    // whole undo tree in chronological order and can still reach it. This is
    // real Vim/Neovim undo-tree behavior (`:h undo-tree`), not a vimcode
    // invention, so it belongs in the oracle corpus rather than only as the
    // engine-level `test_g_minus_reaches_branch_abandoned_by_undo_then_edit`
    // self-check.
    c(
        "undo:g- crosses a branch abandoned by u then edit",
        &["a"],
        1,
        1,
        "ihello<Esc>uiworld<Esc>g-g-",
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
    // #1156: undo tree ex-commands (`:undolist`/`:earlier`/`:later`/`:undojoin`).
    c(
        "ex:undolist is a no-op on the buffer",
        &["abc"],
        1,
        1,
        "x:undolist<CR>",
    ),
    c(
        "ex:earlier 1 undoes one step",
        &["abc"],
        1,
        1,
        "x:earlier 1<CR>",
    ),
    c(
        "ex:earlier then later round-trips",
        &["abc"],
        1,
        1,
        "x:earlier 1<CR>:later 1<CR>",
    ),
    // `:undojoin` typed interactively (through Command-line mode, which
    // redraws on <CR> back to Normal) is documented by Vim itself as
    // "fragile" and meant only for script/function use, not interactive
    // typing (`:h :undojoin`) — confirmed empirically against this
    // repo's own oracle transport (`nvim_input`, one key at a time, which
    // *does* redraw between keys, unlike a bulk `nvim_feedkeys` burst):
    // "x:undojoin<CR>xu" does NOT merge in real Neovim typed this way, so
    // it is not a stable cross-oracle case. `:undojoin`'s one reliably
    // deterministic, non-fragile behavior is `E790` when there is no
    // previous change to join with — that's what this case probes; the
    // *merging* behavior is covered self-referentially (not against nvim)
    // by `test_undojoin_merges_next_change_into_previous_undo_step` in
    // `src/core/engine/tests.rs`.
    c(
        "ex:undojoin with no previous change errors, doesn't touch the buffer",
        &["abc"],
        1,
        1,
        ":undojoin<CR>",
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
    // #1160: <C-k> digraph entry and the <C-x> completion submode.
    c("ins:C-k a: digraph", &["x"], 1, 1, "A<C-k>a:<Esc>"),
    // Escape must land the cursor back *on* the just-inserted multi-byte
    // character (not past it) — nvim's cursor column is byte-based and
    // vimcode's is char-based, so a trailing ASCII character after the
    // arrow would make the two column numbering schemes disagree even
    // though both editors agree on the buffer content and cursor cell.
    c("ins:C-k -> digraph arrow", &["x"], 1, 1, "A<C-k>-><Esc>"),
    c(
        "ins:C-x C-l line completion",
        &["hello world", "hel"],
        2,
        4,
        "A<C-x><C-l><Esc>",
    ),
    // Both the oracle (nvim) and vimcode's own test process run with the
    // crate root as their working directory (`cargo test` sets it; neither
    // side passes `current_dir` for this single-buffer harness — #1160), so
    // a real, checked-in, unambiguously-prefixed filename is a stable
    // cross-process fixture: "Cargo.tom" has exactly one match in the repo
    // root, `Cargo.toml` (`Cargo.lock` diverges at the 8th character).
    c(
        "ins:C-x C-f filename completion",
        &["Cargo.tom"],
        1,
        10,
        "A<C-x><C-f><Esc>",
    ),
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

// ───────────────────── G3. value options (#1206) ─────────────────────
//
// Oracle coverage for the buffer/cursor-observable half of #1206's option
// tranche: 'whichwrap' and 'backspace'. ('wildmode', 'scrolljump' and
// 'sidescrolloff' only affect scroll position/command-line state, which
// this per-case harness doesn't capture — see the module docs' "Nothing
// here is hand-authored" note on what `run_case` actually diffs.)
const CASES_OPT: &[Case] = &[
    cs(
        "opt:whichwrap h wraps to end of previous line",
        &["ab", "cd"],
        2,
        1,
        "h",
        "vim.o.whichwrap = 'h'",
    ),
    cs(
        "opt:whichwrap h does not wrap without the token",
        &["ab", "cd"],
        2,
        1,
        "h",
        "vim.o.whichwrap = 's'",
    ),
    cs(
        "opt:whichwrap l wraps to start of next line",
        &["ab", "cd"],
        1,
        2,
        "l",
        "vim.o.whichwrap = 'l'",
    ),
    cs(
        "opt:backspace without eol keeps lines separate",
        &["ab", "cd"],
        2,
        1,
        "i<BS><Esc>",
        "vim.o.backspace = 'indent,start'",
    ),
    cs(
        "opt:backspace with eol still joins (matches default)",
        &["ab", "cd"],
        2,
        1,
        "i<BS><Esc>",
        "vim.o.backspace = 'indent,eol,start'",
    ),
    // Review finding (#1206 iteration 1): without "start", BackSpace must
    // be refused outright once the cursor has moved (even just an <Up>,
    // no typing) onto a line the current Insert session never touched —
    // not only the exact line Insert was entered on. Before this fix,
    // `backspace_may_delete_before`'s `line != insert_enter_line` check
    // was true for ANY line other than the literal entry line, so it
    // wrongly allowed deleting into "AAAA" here. Verified RED against
    // unfixed `develop` via a local revert of the `split_insert_undo_group`
    // re-anchoring fix (`git stash`-equivalent revert + rerun), which
    // failed BUF: vimcode produced "AAA\nBBBB\nCCCC" (deleted the trailing
    // 'A') while real Neovim left the buffer untouched.
    cs(
        "opt:backspace without start blocks BS after cursor moves off the typed line",
        &["AAAA", "BBBB", "CCCC"],
        2,
        3,
        "i<Up><BS><Esc>",
        "vim.o.backspace = 'indent,eol'",
    ),
    // Non-blocking review finding (#1206 iteration 1): `'whichwrap'`'s
    // `<BS>`/`<Space>` `"b"`/`"s"` tokens were only wired into
    // `handle_normal_key`'s "BackSpace"/"space"/"Space" arms — Visual mode
    // (a separate match, not falling through to Normal-mode handling) had
    // no arm for either key at all, so `<BS>` in Visual mode stayed a
    // silent no-op even with the default `'whichwrap'` (`"b,s"`).
    cs(
        "opt:whichwrap Visual-mode <BS> wraps like Normal-mode h",
        &["ab", "cd"],
        2,
        1,
        "v<BS>d",
        "vim.o.whichwrap = 'b,s'",
    ),
];

// ───────────────── G4. boolean options, tranche 2 (#1207) ─────────────────
//
// Oracle coverage for the buffer/cursor-observable half of #1207's tranche:
// 'magic' (search/`:s` pattern semantics), 'smartindent' and 'cindent'
// (newline/brace/hash indent behaviour). `-u NONE` (see `NvimRpc::spawn`)
// means no filetype/indent plugins are loaded, so only 'smartindent'/
// 'cindent' behaviour that is built into Vim core itself (brace-based
// indent/outdent, '#' to column 0) is exercised here — not vimcode's own
// `line_triggers_indent` language-aware extras (Python `:`, Lua/Ruby/Shell
// `do`/`then`, ...), which are a vimcode-only enhancement layered on top of
// real Vim's 'smartindent'/'autoindent' and have no oracle to check against.
// ('showmatch' doesn't belong here — see `bool_opt:showmatch does not
// disturb the buffer or final cursor position` below for why its own
// resulting-state is not, in fact, oracle-observable.)
const CASES_BOOLOPT: &[Case] = &[
    // ── 'magic' ──────────────────────────────────────────────────────────
    cs(
        "bool_opt:magic search treats '.' as any-char wildcard",
        &["xxx", "acb", "a.b"],
        1,
        1,
        "/a.b<CR>",
        "vim.o.magic = true",
    ),
    cs(
        "bool_opt:nomagic search treats '.' as a literal dot",
        &["xxx", "acb", "a.b"],
        1,
        1,
        "/a.b<CR>",
        "vim.o.magic = false",
    ),
    cs(
        "bool_opt:magic :s treats '.' as any-char wildcard",
        &["aXc"],
        1,
        1,
        ":s/a.c/REPL/<CR>",
        "vim.o.magic = true",
    ),
    cs(
        "bool_opt:nomagic :s treats '.' as a literal dot, no match",
        &["aXc"],
        1,
        1,
        ":s/a.c/REPL/<CR>",
        "vim.o.magic = false",
    ),
    cs(
        "bool_opt:nomagic :s still substitutes an escaped \\.",
        &["aXc"],
        1,
        1,
        ":s/a\\.c/REPL/<CR>",
        "vim.o.magic = false",
    ),
    // ── 'smartindent' (brace-after-newline only — see note below) ───────
    //
    // Real Vim's own `:h 'smartindent'` documents the "'}'/'#' as first
    // char outdents/resets" behaviour too, but empirically (verified by
    // hand against this exact oracle while writing these cases) it only
    // fires when the current line's *entire* existing indent was itself
    // produced by auto-indenting earlier in the very same Insert session
    // (Vim's internal `did_ai` flag) — not when the leading whitespace was
    // already sitting in the buffer before Insert was entered, which is
    // what every case in this corpus's shared harness starts from (a fixed
    // starting buffer, cursor placed by `nvim_win_set_cursor`, not typed).
    // A same-session repro (`A<CR>}<Esc>` starting from a bare `{`-ending
    // line, so the auto-indent and the `}` land in one Insert session) confirms
    // this is a real Vim quirk, not a harness artifact: it produced the
    // outdent nvim's side, and does not with a pre-existing indent. This
    // repo's `smartindent` deliberately implements the *simpler*,
    // unconditional form the issue (#1207) scoped — outdent/`#`-reset
    // whenever the typed character is the first non-blank on the line,
    // regardless of how the existing indent got there — so it does not
    // chase Vim's `did_ai` gating. That divergence is covered by this
    // repo's own engine-level tests (`src/core/engine/tests.rs`), not the
    // oracle corpus here: `did_ai` isn't observable through this harness's
    // `Case` shape without adding session-provenance tracking neither this
    // issue nor a real user-facing gap calls for. `'cindent'`'s outdent/`#`
    // rule has no such gating (see below — it fires unconditionally in
    // both Vim and this repo), so only `'cindent'` gets oracle cases for
    // those two rules.
    cs(
        "bool_opt:smartindent alone (no autoindent) indents after '{'",
        &["if (x) {"],
        1,
        1,
        "A<CR>y<Esc>",
        "vim.o.autoindent = false\nvim.o.smartindent = true",
    ),
    // ── 'cindent' ─────────────────────────────────────────────────────────
    cs(
        "bool_opt:cindent alone (no autoindent) indents after '{'",
        &["if (x) {"],
        1,
        1,
        "A<CR>y<Esc>",
        "vim.o.autoindent = false\nvim.o.cindent = true",
    ),
    cs(
        "bool_opt:cindent alone outdents a lone closing brace",
        &["if (x) {", "    "],
        2,
        5,
        "A}<Esc>",
        "vim.o.autoindent = false\nvim.o.cindent = true",
    ),
    cs(
        "bool_opt:cindent alone moves a typed '#' to column 0",
        &["    "],
        1,
        5,
        "A#<Esc>",
        "vim.o.autoindent = false\nvim.o.cindent = true",
    ),
    // ── 'showmatch' ───────────────────────────────────────────────────────
    //
    // 'showmatch' is a momentary *display* effect (`:h 'showmatch'`): Vim
    // really does move the cursor to the matching bracket and back before
    // the next redraw, gated on 'matchtime' — out of scope for #1207 (see
    // the issue). Both here and in real Vim, once the dust settles the
    // buffer and the *final* cursor position are unaffected by whether
    // 'showmatch' was on at all — this repo's own `showmatch_flash` state
    // (asserted directly in `src/core/engine/tests.rs`, not observable
    // through this harness's `Case` shape) is what actually proves the
    // flash happened; this case is a regression guard that turning
    // 'showmatch' on doesn't accidentally leave the *real* cursor stuck at
    // the match, which would show up here as a genuine buffer/cursor
    // mismatch against Neovim.
    cs(
        "bool_opt:showmatch does not disturb the buffer or final cursor position",
        &["(foo"],
        1,
        5,
        "A)<Esc>",
        "vim.o.showmatch = true",
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
    ("opt     value options (#1206)", CASES_OPT),
    ("bool_opt boolean options tranche 2 (#1207)", CASES_BOOLOPT),
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
    /// N/A — deliberately not in scope (VimScript and anything needing an
    /// expression evaluator). **Not** spelling or digraphs: `src/core/spell.rs`
    /// implements the former (#1163 moved those rows to ✅) and
    /// `src/core/digraphs.rs` the latter (#1160 moved those rows to ✅).
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
    // #1160: digraphs + the <C-x> completion submode.
    p("ins:CTRL-K {c1}{c2}", Label("ins:C-k a: digraph")),
    p("ins:CTRL-X CTRL-N/CTRL-P", Label("ins:C-x C-n")),
    p("ins:CTRL-X CTRL-L", Label("ins:C-x C-l line completion")),
    p(
        "ins:CTRL-X CTRL-F",
        Label("ins:C-x C-f filename completion"),
    ),
    p("ins:CTRL-X CTRL-K", Label("ins:C-x C-k dictionary")),
    p("ins:CTRL-X CTRL-S", Label("ins:C-x C-s spell")),
    p("ins:CTRL-X CTRL-O", Label("ins:C-x C-o omni")),
    p("ins:CTRL-X CTRL-E/CTRL-Y", Label("ins:C-x C-e scroll")),
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
    // #1160 — same message-only-command shape as `:marks`/`:jumps` above.
    p("ex::digraphs", Keys(":digraphs")),
    p("ex::changes", Keys(":changes")),
    p("ex::history", Keys(":history")),
    // #1156 — undo tree ex-commands.
    p("ex::undolist", Keys(":undolist")),
    p("ex::earlier", Keys(":earlier")),
    p("ex::later", Keys(":later")),
    p("ex::undojoin", Keys(":undojoin")),
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
//     g-commands                 22/50   gt gT gf gF ga g8 gx gR g@ g+ …
//                                        (`g-` left this list in #1156)
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
//
// **A permanent entry carries a reason; an unannotated entry is debt (#1278).**
// Most of this list is plain uncovered work — nobody has written the oracle
// case yet, and the entry should read as a TODO. A minority of entries can
// never gain a case no matter how much work goes in: the command diverges
// from Neovim on purpose, the command ends the very session the probe is
// running in, or the result depends on the host machine rather than on the
// editor. Those, and only those, are annotated below under one of the four
// "permanent" headings, each with the reason inline. If you are looking at
// an entry with no comment above it, it is debt: pick it up, add a
// `COMMAND_PROBES` entry and an oracle case, and delete it here. If you
// believe an *annotated* entry is actually coverable, that is a finding for
// whatever issue is doing that work, not license to just delete the
// annotation — file it and let that slice make the case.
// ---------------------------------------------------------------------------

const COVERAGE_EXEMPT: &[&str] = &[
    // --- Spell (#1163, deferred by #1278) ---
    // `src/core/spell.rs` implements all seven, so they are ✅ in the doc and
    // in scope here — but the oracle corpus contains no spell case whatsoever,
    // so every probe above matches nothing. Exempt today, but — unlike the 46
    // rows below this block — NOT permanent: these are deliberately left out
    // of the "permanent" headings, because covering them is possible.
    //
    // #1278 looked at scoping that fixture and is deferring it rather than
    // building it, for a concrete reason: `zg`/`zw`/`zG`/`zW` mutate word
    // membership in a dictionary, and `z=`/`[s`/`]s` read the current
    // dictionary's verdict — so every one of the seven is only comparable
    // against the oracle if both sides agree on the same word list. They do
    // not. `src/core/spell.rs` uses the `spellbook` crate (a Hunspell-format
    // parser) against `dictionaries/en_US.dic`/`.aff`, compiled into the
    // vimcode binary. The nvim oracle uses its own compiled `.spl` binary
    // format, loaded at runtime from `$VIMRUNTIME/spell/en.utf-8.spl` —
    // shipped with the Neovim *install*, not pinned by this repo, so it can
    // silently change word list and suggestion ranking across a Neovim
    // version bump. The two are unrelated implementations with unrelated
    // word lists: confirmed empirically (2026-09, nvim 0.12.5) that
    // `z=`-style suggestions for "helo" already differ in ranking between
    // the two.
    //
    // That kills `z=` outright — its entire observable behavior *is* the
    // suggestion list, so there is no dictionary-independent slice of it
    // left to test. `zg`/`zw`/`zG`/`zW`/`[s`/`]s` are less broken: their
    // effect is mechanical (does this word now report bad/good; does the
    // cursor land on the next/prev bad word), which only needs both
    // dictionaries to agree a *specific* word is bad — true for something
    // like "helo", regardless of suggestion-list differences. A real fixture
    // for those six would need to either (a) bundle a pinned nvim `.spl`
    // fixture in this repo and point the oracle at it via `spellfile`, so
    // the oracle's word list stops drifting with the host's Neovim install,
    // or (b) hand-pick fixture words unambiguous enough (obvious nonsense vs.
    // common real words) that both dictionaries' bad/good verdict is safe to
    // assume without pinning. Either is scoped work for a future slice, not
    // done here — this comment is that slice's starting point. `z=` itself
    // stays exempt regardless of which path is taken.
    "z:z=",
    "z:zg",
    "z:zw",
    "z:zG",
    "z:zW",
    "bracket:[s",
    "bracket:]s",
    // ===========================================================================
    // Permanent exemptions (#1278) — the 46 ids below will never gain an
    // oracle case, each for one of the four reasons grouped under the
    // headings that follow. See the array's own doc comment above for the
    // rule: a permanent entry carries a reason; an unannotated entry (every
    // entry past this block) is plain debt.
    // ===========================================================================

    // --- Deliberate semantic divergence ---
    // vimcode implements each of these, but on purpose differently from
    // Neovim (see `VIM_COMPATIBILITY.md`, cited per row below), so a
    // byte-for-byte oracle comparison is meaningless: a mismatch would be
    // the intended behavior, not a bug.
    //
    // Next/prev git hunk (git integration), not Neovim's diff-mode hunk
    // navigation (VIM_COMPATIBILITY.md:488).
    "bracket:]c",
    "bracket:[c",
    // Next/prev LSP diagnostic — needs a live, attached LSP client, which
    // the oracle (`-u NONE -i NONE`, no LSP) never has
    // (VIM_COMPATIBILITY.md:489).
    "bracket:]d",
    "bracket:[d",
    // LSP goto-definition, not a ctags-file jump (VIM_COMPATIBILITY.md:319).
    "other:CTRL-]",
    // LSP hover info, not `:help`/man-page lookup (VIM_COMPATIBILITY.md:310).
    "other:K",
    // vimcode's own diff engine, not Neovim's internal diff algorithm
    // (VIM_COMPATIBILITY.md:322-323,657).
    "other:do",
    "other:dp",
    "ex::diffsplit",
    "ex::diffthis",
    "ex::diffoff",
    // Shells out to the host's default browser/opener — a side effect on
    // the OS, not the buffer, that an oracle probe cannot observe
    // (VIM_COMPATIBILITY.md:314,386).
    "other:gx",
    "g:gx",
    // --- Ends or leaves the session ---
    // Each of these exits, or would exit, the very vimcode process the probe
    // is driving. There is no "after" state left for the probe to read —
    // the harness cannot observe a command that tears down the thing it is
    // observing.
    "ex::q",
    "ex::quit",
    "ex::q!",
    "ex::wq",
    "ex::x",
    "ex::qa",
    "ex::qa!",
    "ex::wqa",
    "ex::xa",
    "ex::cquit",
    // `:version` prints build/version info to the message line, not buffer
    // state — nothing here is a buffer diff. `:help`/`:h` open a help buffer
    // whose *content* is Neovim's own bundled runtime docs, which vimcode
    // does not reproduce and should not try to.
    "ex::version",
    "ex::help",
    "ex::h",
    // --- Environment-dependent ---
    // The result depends on the machine running the test (an installed
    // `grep`/`make`, the set of installed colorschemes, the filesystem
    // layout under the cwd), not on the editor. Comparing against the
    // oracle here would be comparing hosts, not implementations, and a case
    // that happened to pass would be pinned to this machine's environment.
    "ex::grep",
    "ex::vimgrep",
    "ex::lgrep",
    "ex::lvimgrep",
    "ex::make",
    "ex::colorscheme",
    "ex::cd {path}",
    "ex::Explore",
    "ex::Ex",
    "ex::Sexplore",
    "ex::Sex",
    "ex::Vexplore",
    "ex::Vex",
    // --- Already decided into unit tests (#1160) ---
    "ins:CTRL-@",
    "ins:CTRL-G j/k",
    // #1160: source-dependent <C-x> sub-modes — oracle cases exist for
    // CTRL-X CTRL-L/CTRL-F above; the rest are covered by unit tests instead
    // (per the issue: "the rest of the CTRL-X family is source-dependent and
    // belongs in unit tests" — buffer-scoped keyword completion, the bundled
    // dictionary, spell suggestions, LSP-backed omni, and window scrolling
    // are all either non-deterministic against a live oracle or already
    // exercised at the engine-test level in `src/core/engine/tests.rs`).
    "ins:CTRL-X CTRL-N/CTRL-P",
    "ins:CTRL-X CTRL-K",
    "ins:CTRL-X CTRL-S",
    "ins:CTRL-X CTRL-O",
    "ins:CTRL-X CTRL-E/CTRL-Y",
    // ===========================================================================
    // Uncovered work — plain debt, no permanent reason. Every id below this
    // line has no annotation because it needs none: pick one up, write a
    // `COMMAND_PROBES` entry and an oracle case, and delete it.
    // ===========================================================================

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
    "other:ga",
    "other:g8",
    "other:CTRL-^",
    "other:CTRL-G",
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
    "g:ga",
    "g:g8",
    "g:gm",
    "g:gM",
    "g:g@{motion}",
    "g:g+",
    // `g:g-` was here until #1156: the new
    // `undo:g- crosses a branch abandoned by u then edit` case presses `g-`,
    // so the probe matches and the ratchet demands the entry be deleted.
    // That is the list shrinking as designed — do not re-add it.
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
    "ex::wa",
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
    "ex::digraphs",
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
    "ex::windo {cmd}",
    "ex::bufdo {cmd}",
    "ex::tabdo {cmd}",
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
    "ex::b {name}",
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

// ═══════════════════════════════════════════════════════════════════════════
// Phase 5 audit slice — `:set` options (#1225)
//
// `:help option-list` (Vim 9.1's `quickref.txt`) is a flat inventory of every
// option Vim has: **421 of them**. [`OPTION_AUDIT`] below tags every one
// Implemented / Partial / NotImplemented / Skipped against the `SettingDef`
// registry in `src/core/settings.rs`, in `:help` order, so the tagging is
// machine-checked rather than prose in a markdown file that nothing runs.
//
// The measurement, as of this slice (`undofile`/`undodir`/`undolevels` moved
// ❌ → ✅/🟡/🟡 when #1156 landed, which is the gate working, not drift):
//
//     ✅ Implemented       45
//     🟡 Partial           12
//     ❌ Not implemented  182
//     ⏭️  Intentionally skipped  182   (each with a reason from SKIP_REASONS)
//                        ────
//                         421
//
// ## Why an options slice goes first, measured rather than assumed
//
// #26 deprioritised this audit on the grounds that "bugs found here tend to be
// 'missing feature' not 'wrong behavior'". That held for the `g`-prefix slice;
// it does not hold here. The oracle corpus probes seven options through its
// `cs(..)` Lua `setup`, and three of them — `'joinspaces'`, `'smarttab'`,
// `'nrformats'` — did not exist in `Settings` at all. They were found the
// expensive way, as unexplained conformance deviations blamed on a harness
// bug (#1000, #1001). The three *new* findings below are the same shape,
// found in an afternoon by tagging instead of by debugging.
//
// ## Gate 1 — the recorded surface must match the live registry
//
// Every row records the `:set` **surface** vimcode actually exposes for that
// option name, and [`option_audit_matches_the_live_settings_registry`]
// recomputes it by driving `Settings::parse_set_option` and diffs the two.
// That is the bidirectional half: tagging an option `NotImplemented` and then
// implementing it fails the gate until the row is re-tagged, and a row
// claiming `Implemented` for a name `:set` rejects fails immediately.
//
// The surface is finer-grained than a bool on purpose. Three of this slice's
// findings are *asymmetries* — a name the mutation path knows and the query
// path does not, or the reverse — which a yes/no "is it implemented" check
// cannot see and which is exactly what a user hits when `:set autoread` works
// and `:set autoread?` answers "Unknown option".
//
// ## Gate 2 — oracle coverage, same shrink-only shape as #1007
//
// Every Implemented/Partial row also carries a probe naming the oracle case
// that exercises it; [`OPTION_COVERAGE_EXEMPT`] lists the ones no case
// reaches today. Both directions fail, exactly as in `COVERAGE_EXEMPT`: an
// unexempt row whose probe matches nothing, and an exempt row whose probe
// starts matching. Writing the missing cases is #1162's job, not this
// slice's — the exempt list is the measurement it starts from.
// ═══════════════════════════════════════════════════════════════════════════

/// The audit verdict for one `:help option-list` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OptStatus {
    /// ✅ — `:set` accepts the option and it drives real behaviour.
    Implemented,
    /// 🟡 — recognised, but incomplete: a missing value, a missing
    /// abbreviation, or one of `:set x` / `:set x?` missing.
    Partial,
    /// ❌ — not in the registry (or recognised only to be rejected).
    NotImplemented,
    /// ⏭️ — deliberately out of scope. Carries the **reason**, per the bar
    /// the insert-mode slice set ("requires VimScript eval", "no digraph
    /// support planned") — a bare ⏭️ is not a tag, it is a shrug.
    Skipped(&'static str),
}

// The skip-reason vocabulary. A fixed set, asserted by
// [`option_audit_is_internally_consistent`], so a future slice cannot invent
// a one-off excuse per row.
const VIMSCRIPT: &str = "requires VimScript eval";
const SCRIPTRT: &str = "no VimScript runtime (no :source, .vimrc or plugin scripts)";
const BIDI: &str = "no right-to-left / input-method support planned";
const ENCODING: &str = "vimcode is UTF-8 only; no encoding-conversion layer";
const TERMCAP: &str = "terminal control belongs to quadraui; vimcode reads no termcap";
const VIMGUI: &str = "Vim GUI-toolkit option with no GTK4/quadraui counterpart";
const OBSOLETE: &str = "obsolete in Vim itself";
const INTERP: &str = "language-binding dynamic library";
const PRINTING: &str = "no :hardcopy printing planned";
const CSCOPE: &str = "no cscope integration planned";
const MAKE: &str = "no :make/:grep compiler integration (vimcode uses LSP diagnostics)";
const SELECT: &str = "Select mode not supported (see the g-prefix slice: gH/gV/g CTRL-H)";
const VICOMPAT: &str = "Vi-compatibility switch; vimcode targets nocompatible behaviour only";
const EXMODE: &str = "Ex mode / legacy pager is out of scope (see gQ in the g-prefix slice)";
const SESSION: &str = "no :mksession/:mkview support planned";
const ARCH: &str = "no counterpart in vimcode's architecture (Ropey buffers, no line cache)";
const PLATFORM: &str = "option of a Vim build for a platform vimcode does not target";
/// Added by the registers/marks slice (#1226) for `':` — Neovim's
/// prompt-buffer mark. vimcode has no `:h prompt-buffer` buffer type, so the
/// mark has nothing to point at.
const PROMPTBUF: &str = "no prompt buffers (:h prompt-buffer) in vimcode";
/// Added by the ex-command slice (#1227) for the `:menu`/`:emenu`/`:popup`
/// family. Distinct from [`VIMGUI`], which is about Vim *options* of a GUI
/// build: vimcode does have menus, they are just quadraui widgets built from
/// the accelerator registry, not entries a `:menu` command can define.
const MENU: &str =
    "Vim's GUI menu commands (:menu/:emenu/:popup); vimcode's menus are quadraui widgets";
/// Added by #1227 for `:tag`/`:ptag`/`:dsearch`/`:ilist` and the rest of the
/// tags and 'include'-search families.
const CTAGS: &str =
    "no ctags or 'include' file search planned; vimcode uses LSP definitions/references";
/// Added by #1227 for `:rshada`/`:wshada`/`:rviminfo`/`:wviminfo`.
const SHADA: &str = "no ShaDa/viminfo file; vimcode persists its own session state";

const SKIP_REASONS: &[&str] = &[
    VIMSCRIPT, SCRIPTRT, BIDI, ENCODING, TERMCAP, VIMGUI, OBSOLETE, INTERP, PRINTING, CSCOPE, MAKE,
    SELECT, VICOMPAT, EXMODE, SESSION, ARCH, PLATFORM, PROMPTBUF, MENU, CTAGS, SHADA, PREVIEW,
];

/// What `:set` actually does with an option name today — measured, never
/// asserted by hand. See [`measured_surface`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Surface {
    /// Both `:set x` / `:set x=v` and `:set x?` are accepted.
    Full,
    /// Mutation works; `:set x?` answers "Unknown option".
    SetOnly,
    /// `:set x?` works; mutation answers "Unknown option".
    QueryOnly,
    /// Recognised by name, rejected with settings.rs's
    /// "recognised but not implemented yet" message (`UNIMPLEMENTED_*`).
    Stub,
    /// Every form answers "Unknown option" — not in the registry.
    Absent,
}

/// How an audited option is proven to be exercised by the oracle corpus.
/// Separate from #1007's [`Probe`] because an option is pinned by a case's
/// Lua `setup`, which `Probe` cannot see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OptProbe {
    /// At least one case whose `setup`, with **all whitespace removed**,
    /// contains this substring. The corpus writes both `vim.o.magic = false`
    /// and `vim.o.smarttab=false`, so a needle like `"vim.o.magic="` must
    /// match either spelling; stripping whitespace is what makes the `=`
    /// usable, and the `=` is what stops `"vim.o.list="` from crediting
    /// `vim.o.listchars=...`.
    OptSetup(&'static str),
    /// At least one case whose **keys** contain this substring — for options
    /// the corpus pins with a literal `:set …<CR>` prefix instead of `setup`.
    OptKeys(&'static str),
}

struct OptionAudit {
    /// Full option name, as `:help option-list` spells it (without quotes).
    name: &'static str,
    /// Vim's abbreviation, or `""` when the option has none.
    short: &'static str,
    status: OptStatus,
    /// The `:set` surface vimcode exposes **today**, re-measured by gate 1.
    surface: Surface,
    /// Oracle probe — `Some` exactly for Implemented/Partial rows.
    probe: Option<OptProbe>,
    /// For ❌ rows: Vim's one-line description plus this slice's assessment of
    /// whether it is worth implementing. For 🟡: what specifically is missing.
    note: &'static str,
}

#[allow(clippy::too_many_arguments)]
const fn o(
    name: &'static str,
    short: &'static str,
    status: OptStatus,
    surface: Surface,
    probe: Option<OptProbe>,
    note: &'static str,
) -> OptionAudit {
    OptionAudit {
        name,
        short,
        status,
        surface,
        probe,
        note,
    }
}

use crate::OptProbe::{OptKeys, OptSetup};
use crate::OptStatus::{Implemented, NotImplemented, Partial, Skipped};
use crate::Surface::{Absent, Full, QueryOnly, SetOnly, Stub};

/// Every option in Vim 9.1's `:help option-list`, in `:help` order.
///
/// 421 rows, no "TODO" and no unreviewed row: adding a row, deleting one, or
/// leaving one out of order fails [`option_audit_is_internally_consistent`].
const OPTION_AUDIT: &[OptionAudit] = &[
    o("aleph", "al", Skipped(BIDI), Absent, None,
      "ASCII code of the letter Aleph (Hebrew)"),
    o("allowrevins", "ari", Skipped(BIDI), Absent, None,
      "allow CTRL-_ in Insert and Command-line mode"),
    o("altkeymap", "akm", Skipped(OBSOLETE), Absent, None,
      "obsolete option for Farsi"),
    o("ambiwidth", "ambw", Skipped(ENCODING), Absent, None,
      "what to do with Unicode chars of ambiguous width"),
    o("antialias", "anti", Skipped(VIMGUI), Absent, None,
      "Mac OS X: use smooth, antialiased fonts"),
    o("arabic", "arab", Skipped(BIDI), Absent, None,
      "for Arabic as a default second language"),
    o("arabicshape", "arshape", Skipped(BIDI), Absent, None,
      "do shaping for Arabic characters"),
    o("autochdir", "acd", NotImplemented, Absent, None,
      "change directory to the file in the current window — nice to have"),
    o("autoindent", "ai", Implemented, Full, Some(OptSetup("vim.o.autoindent=")),
      "take indent for new line from previous line"),
    o("autoread", "ar", Partial, SetOnly, Some(OptSetup("vim.o.autoread=")),
      "settable (:set autoread / :set noautoread) but NOT queryable — :set autoread? answers \"Unknown option\""),
    o("autoshelldir", "asd", Skipped(PLATFORM), Absent, None,
      "change directory to the shell's current directory"),
    o("autowrite", "aw", NotImplemented, Absent, None,
      "automatically write file if changed — nice to have"),
    o("autowriteall", "awa", NotImplemented, Absent, None,
      "as 'autowrite', but works with more commands — nice to have"),
    o("background", "bg", NotImplemented, Absent, None,
      "\"dark\" or \"light\", used for highlight colors — nice to have"),
    o("backspace", "bs", Partial, Full, Some(OptSetup("vim.o.backspace=")),
      "indent/eol/start honoured; Vim's numeric shorthand (0/1/2/3) is accepted as a list but not translated"),
    o("backup", "bk", NotImplemented, Absent, None,
      "keep backup file after overwriting a file — nice to have"),
    o("backupcopy", "bkc", NotImplemented, Absent, None,
      "make backup as a copy, don't rename the file — nice to have"),
    o("backupdir", "bdir", NotImplemented, Absent, None,
      "list of directories for the backup file — nice to have"),
    o("backupext", "bex", NotImplemented, Absent, None,
      "extension used for the backup file — nice to have"),
    o("backupskip", "bsk", NotImplemented, Absent, None,
      "no backup for files that match these patterns — nice to have"),
    o("balloondelay", "bdlay", Skipped(VIMGUI), Absent, None,
      "delay in mS before a balloon may pop up"),
    o("ballooneval", "beval", Skipped(VIMGUI), Absent, None,
      "switch on balloon evaluation in the GUI"),
    o("balloonevalterm", "bevalterm", Skipped(VIMGUI), Absent, None,
      "switch on balloon evaluation in the terminal"),
    o("balloonexpr", "bexpr", Skipped(VIMSCRIPT), Absent, None,
      "expression to show in balloon"),
    o("belloff", "bo", NotImplemented, Absent, None,
      "do not ring the bell for these reasons — nice to have"),
    o("binary", "bin", NotImplemented, Absent, None,
      "read/write/edit file in binary mode — nice to have"),
    o("bioskey", "biosk", Skipped(TERMCAP), Absent, None,
      "MS-DOS: use bios calls for input characters"),
    o("bomb", "", Skipped(ENCODING), Absent, None,
      "prepend a Byte Order Mark to the file"),
    o("breakat", "brk", NotImplemented, Absent, None,
      "characters that may cause a line break — nice to have"),
    o("breakindent", "bri", NotImplemented, Absent, None,
      "wrapped line repeats indent — nice to have"),
    o("breakindentopt", "briopt", NotImplemented, Absent, None,
      "settings for 'breakindent' — nice to have"),
    o("browsedir", "bsdir", Skipped(VIMGUI), Absent, None,
      "which directory to start browsing in"),
    o("bufhidden", "bh", NotImplemented, Absent, None,
      "what to do when buffer is no longer in window — nice to have"),
    o("buflisted", "bl", NotImplemented, Absent, None,
      "whether the buffer shows up in the buffer list — nice to have"),
    o("buftype", "bt", NotImplemented, Absent, None,
      "special type of buffer — nice to have"),
    o("casemap", "cmp", Skipped(ENCODING), Absent, None,
      "specifies how case of letters is changed"),
    o("cdhome", "cdh", NotImplemented, Absent, None,
      "change directory to the home directory by \":cd\" — low value"),
    o("cdpath", "cd", NotImplemented, Absent, None,
      "list of directories searched with \":cd\" — low value"),
    o("cedit", "", NotImplemented, Absent, None,
      "key used to open the command-line window — nice to have"),
    o("charconvert", "ccv", Skipped(ENCODING), Absent, None,
      "expression for character encoding conversion"),
    o("cindent", "cin", Implemented, Full, Some(OptSetup("vim.o.cindent=")),
      "do C program indenting"),
    o("cinkeys", "cink", NotImplemented, Absent, None,
      "keys that trigger indent when 'cindent' is set — nice to have"),
    o("cinoptions", "cino", NotImplemented, Absent, None,
      "how to do indenting when 'cindent' is set — nice to have"),
    o("cinscopedecls", "cinsd", NotImplemented, Absent, None,
      "words that are recognized by 'cino-g' — nice to have"),
    o("cinwords", "cinw", NotImplemented, Absent, None,
      "words where 'si' and 'cin' add an indent — nice to have"),
    o("clipboard", "cb", NotImplemented, Stub, None,
      "recognised but rejected (\"not implemented yet\"); #1100 landed backend.services().clipboard(), so the blocker is gone — worth implementing"),
    o("cmdheight", "ch", NotImplemented, Absent, None,
      "number of lines to use for the command-line — nice to have"),
    o("cmdwinheight", "cwh", NotImplemented, Absent, None,
      "height of the command-line window — nice to have"),
    o("colorcolumn", "cc", Implemented, Full, Some(OptSetup("vim.o.colorcolumn=")),
      "columns to highlight"),
    o("columns", "co", NotImplemented, Absent, None,
      "number of columns in the display — low value"),
    o("comments", "com", NotImplemented, Absent, None,
      "affects gq and auto-indent of comment leaders; worth implementing"),
    o("commentstring", "cms", NotImplemented, Absent, None,
      "commentary.vim support exists with a built-in filetype table; worth implementing"),
    o("compatible", "cp", Skipped(VICOMPAT), Absent, None,
      "behave Vi-compatible as much as possible"),
    o("complete", "cpt", NotImplemented, Absent, None,
      "specify how Insert mode completion works — worth implementing"),
    o("completefunc", "cfu", Skipped(VIMSCRIPT), Absent, None,
      "function to be used for Insert mode completion"),
    o("completeopt", "cot", NotImplemented, Absent, None,
      "options for Insert mode completion — low value"),
    o("completepopup", "cpp", Skipped(VIMGUI), Absent, None,
      "options for the Insert mode completion info popup"),
    o("completeslash", "csl", Skipped(PLATFORM), Absent, None,
      "like 'shellslash' for completion"),
    o("concealcursor", "cocu", NotImplemented, Absent, None,
      "whether concealable text is hidden in cursor line — nice to have"),
    o("conceallevel", "cole", NotImplemented, Absent, None,
      "whether concealable text is shown or hidden — nice to have"),
    o("confirm", "cf", NotImplemented, Absent, None,
      "ask what to do about unsaved/read-only files — nice to have"),
    o("conskey", "consk", Skipped(TERMCAP), Absent, None,
      "get keys directly from console (MS-DOS only)"),
    o("copyindent", "ci", NotImplemented, Absent, None,
      "make 'autoindent' use existing indent structure — worth implementing"),
    o("cpoptions", "cpo", Skipped(VICOMPAT), Absent, None,
      "flags for Vi-compatible behavior"),
    o("cryptmethod", "cm", NotImplemented, Absent, None,
      "type of encryption to use for file writing — nice to have"),
    o("cscopepathcomp", "cspc", Skipped(CSCOPE), Absent, None,
      "how many components of the path to show"),
    o("cscopeprg", "csprg", Skipped(CSCOPE), Absent, None,
      "command to execute cscope"),
    o("cscopequickfix", "csqf", Skipped(CSCOPE), Absent, None,
      "use quickfix window for cscope results"),
    o("cscoperelative", "csre", Skipped(CSCOPE), Absent, None,
      "Use cscope.out path basename as prefix"),
    o("cscopetag", "cst", Skipped(CSCOPE), Absent, None,
      "use cscope for tag commands"),
    o("cscopetagorder", "csto", Skipped(CSCOPE), Absent, None,
      "determines \":cstag\" search order"),
    o("cscopeverbose", "csverb", Skipped(CSCOPE), Absent, None,
      "give messages when adding a cscope database"),
    o("cursorbind", "crb", NotImplemented, Absent, None,
      "move cursor in window as it moves in other windows — nice to have"),
    o("cursorcolumn", "cuc", NotImplemented, Absent, None,
      "highlight the screen column of the cursor — worth implementing"),
    o("cursorline", "cul", Implemented, Full, Some(OptSetup("vim.o.cursorline=")),
      "highlight the screen line of the cursor"),
    o("cursorlineopt", "culopt", NotImplemented, Absent, None,
      "settings for 'cursorline' — nice to have"),
    o("debug", "", Skipped(ARCH), Absent, None,
      "set to \"msg\" to see all error messages"),
    o("define", "def", NotImplemented, Absent, None,
      "pattern to be used to find a macro definition — nice to have"),
    o("delcombine", "deco", Skipped(ENCODING), Absent, None,
      "delete combining characters on their own"),
    o("dictionary", "dict", NotImplemented, Absent, None,
      "list of file names used for keyword completion — worth implementing"),
    o("diff", "", NotImplemented, Absent, None,
      "use diff mode for the current window — nice to have"),
    o("diffexpr", "dex", Skipped(VIMSCRIPT), Absent, None,
      "expression used to obtain a diff file"),
    o("diffopt", "dip", NotImplemented, Absent, None,
      "options for using diff mode — nice to have"),
    o("digraph", "dg", NotImplemented, Absent, None,
      "#1160 shipped <C-k> digraphs, so the table exists; this option only adds the char-<BS>-char entry form; worth implementing"),
    o("directory", "dir", NotImplemented, Absent, None,
      "list of directory names for the swap file — nice to have"),
    o("display", "dy", NotImplemented, Absent, None,
      "list of flags for how to display text — worth implementing"),
    o("eadirection", "ead", NotImplemented, Absent, None,
      "in which direction 'equalalways' works — low value"),
    o("edcompatible", "ed", Skipped(VICOMPAT), Absent, None,
      "toggle flags of \":substitute\" command"),
    o("emoji", "emo", Skipped(ENCODING), Absent, None,
      "emoji characters are considered full width"),
    o("encoding", "enc", Skipped(ENCODING), Absent, None,
      "encoding used internally"),
    o("endoffile", "eof", NotImplemented, Absent, None,
      "write CTRL-Z at end of the file — low value"),
    o("endofline", "eol", NotImplemented, Absent, None,
      "write <EOL> for last line in file — worth implementing"),
    o("equalalways", "ea", NotImplemented, Absent, None,
      "windows are automatically made the same size — nice to have"),
    o("equalprg", "ep", NotImplemented, Absent, None,
      "external program to use for \"=\" command — nice to have"),
    o("errorbells", "eb", NotImplemented, Absent, None,
      "ring the bell for error messages — nice to have"),
    o("errorfile", "ef", Skipped(MAKE), Absent, None,
      "name of the errorfile for the QuickFix mode"),
    o("errorformat", "efm", Skipped(MAKE), Absent, None,
      "description of the lines in the error file"),
    o("esckeys", "ek", Skipped(TERMCAP), Absent, None,
      "recognize function keys in Insert mode"),
    o("eventignore", "ei", NotImplemented, Absent, None,
      "autocommand events that are ignored — nice to have"),
    o("expandtab", "et", Implemented, Full, Some(OptKeys(":set noet")),
      "use spaces when <Tab> is inserted"),
    o("exrc", "ex", Skipped(SCRIPTRT), Absent, None,
      "read .vimrc and .exrc in the current directory"),
    o("fileencoding", "fenc", Skipped(ENCODING), Absent, None,
      "file encoding for multibyte text"),
    o("fileencodings", "fencs", Skipped(ENCODING), Absent, None,
      "automatically detected character encodings"),
    o("fileformat", "ff", NotImplemented, Absent, None,
      "file format used for file I/O — worth implementing"),
    o("fileformats", "ffs", NotImplemented, Absent, None,
      "automatically detected values for 'fileformat' — worth implementing"),
    o("fileignorecase", "fic", NotImplemented, Absent, None,
      "ignore case when using file names — nice to have"),
    o("filetype", "ft", NotImplemented, Absent, None,
      "type of file, used for autocommands — nice to have"),
    o("fillchars", "fcs", NotImplemented, Absent, None,
      "characters to use for displaying special items — nice to have"),
    o("fixendofline", "fixeol", NotImplemented, Absent, None,
      "make sure last line in file has <EOL> — worth implementing"),
    o("fkmap", "fk", Skipped(OBSOLETE), Absent, None,
      "obsolete option for Farsi"),
    o("foldclose", "fcl", NotImplemented, Absent, None,
      "close a fold when the cursor leaves it — worth implementing"),
    o("foldcolumn", "fdc", NotImplemented, Absent, None,
      "width of the column used to indicate folds — worth implementing"),
    o("foldenable", "fen", NotImplemented, Absent, None,
      "folds exist (foldmethod/foldlevel/foldmarker/foldnestmax); the remaining fold options are cheap follow-ons; worth implementing"),
    o("foldexpr", "fde", Skipped(VIMSCRIPT), Absent, None,
      "expression used when 'foldmethod' is \"expr\""),
    o("foldignore", "fdi", NotImplemented, Absent, None,
      "ignore lines when 'foldmethod' is \"indent\" — worth implementing"),
    o("foldlevel", "fdl", Implemented, Full, Some(OptSetup("vim.o.foldlevel=")),
      "close folds with a level higher than this"),
    o("foldlevelstart", "fdls", NotImplemented, Absent, None,
      "'foldlevel' when starting to edit a file — worth implementing"),
    o("foldmarker", "fmr", Implemented, Full, Some(OptSetup("vim.o.foldmarker=")),
      "markers used when 'foldmethod' is \"marker\""),
    o("foldmethod", "fdm", Partial, Full, Some(OptSetup("vim.o.foldmethod=")),
      "only manual/indent/marker; expr/syntax/diff are rejected"),
    o("foldminlines", "fml", NotImplemented, Absent, None,
      "minimum number of lines for a fold to be closed — worth implementing"),
    o("foldnestmax", "fdn", Implemented, Full, Some(OptSetup("vim.o.foldnestmax=")),
      "maximum fold depth"),
    o("foldopen", "fdo", NotImplemented, Absent, None,
      "for which commands a fold will be opened — worth implementing"),
    o("foldtext", "fdt", Skipped(VIMSCRIPT), Absent, None,
      "expression used to display for a closed fold"),
    o("formatexpr", "fex", Skipped(VIMSCRIPT), Absent, None,
      "expression used with \"gq\" command"),
    o("formatlistpat", "flp", NotImplemented, Absent, None,
      "pattern used to recognize a list header — nice to have"),
    o("formatoptions", "fo", NotImplemented, Absent, None,
      "gq/gw and auto-wrap are implemented but not configurable; worth implementing"),
    o("formatprg", "fp", NotImplemented, Absent, None,
      "name of external program used with \"gq\" command — nice to have"),
    o("fsync", "fs", NotImplemented, Absent, None,
      "whether to invoke fsync() after file write — nice to have"),
    o("gdefault", "gd", Implemented, Full, Some(OptKeys(":set gdefault")),
      "the \":substitute\" flag 'g' is default on"),
    o("grepformat", "gfm", Skipped(MAKE), Absent, None,
      "format of 'grepprg' output"),
    o("grepprg", "gp", Skipped(MAKE), Absent, None,
      "program to use for \":grep\""),
    o("guicursor", "gcr", Skipped(VIMGUI), Absent, None,
      "GUI: settings for cursor shape and blinking"),
    o("guifont", "gfn", Skipped(VIMGUI), Absent, None,
      "superseded by vimcode's own font_family/font_size settings"),
    o("guifontset", "gfs", Skipped(VIMGUI), Absent, None,
      "GUI: Names of multibyte fonts to be used"),
    o("guifontwide", "gfw", Skipped(VIMGUI), Absent, None,
      "list of font names for double-wide characters"),
    o("guiheadroom", "ghr", Skipped(VIMGUI), Absent, None,
      "GUI: pixels room for window decorations"),
    o("guiligatures", "gli", Skipped(VIMGUI), Absent, None,
      "GTK GUI: ASCII characters that can form shapes"),
    o("guioptions", "go", Skipped(VIMGUI), Absent, None,
      "GUI: Which components and options are used"),
    o("guipty", "", Skipped(VIMGUI), Absent, None,
      "GUI: try to use a pseudo-tty for \":!\" commands"),
    o("guitablabel", "gtl", Skipped(VIMGUI), Absent, None,
      "GUI: custom label for a tab page"),
    o("guitabtooltip", "gtt", Skipped(VIMGUI), Absent, None,
      "GUI: custom tooltip for a tab page"),
    o("helpfile", "hf", Skipped(SCRIPTRT), Absent, None,
      "full path name of the main help file"),
    o("helpheight", "hh", Skipped(SCRIPTRT), Absent, None,
      "minimum height of a new help window"),
    o("helplang", "hlg", Skipped(SCRIPTRT), Absent, None,
      "preferred help languages"),
    o("hidden", "hid", Implemented, Full, Some(OptSetup("vim.o.hidden=")),
      "don't unload buffer when it is |abandon|ed"),
    o("highlight", "hl", Skipped(VIMGUI), Absent, None,
      "vimcode themes highlight groups through colorscheme JSON, not a flag string"),
    o("history", "hi", NotImplemented, Absent, None,
      "number of command-lines that are remembered — worth implementing"),
    o("hkmap", "hk", Skipped(BIDI), Absent, None,
      "Hebrew keyboard mapping"),
    o("hkmapp", "hkp", Skipped(BIDI), Absent, None,
      "phonetic Hebrew keyboard mapping"),
    o("hlsearch", "hls", Implemented, Full, Some(OptSetup("vim.o.hlsearch=")),
      "highlight matches with last search pattern"),
    o("icon", "", NotImplemented, Absent, None,
      "let Vim set the text of the window icon — low value"),
    o("iconstring", "", Skipped(VIMSCRIPT), Absent, None,
      "string to use for the Vim icon text"),
    o("ignorecase", "ic", Implemented, Full, Some(OptKeys(":set ic")),
      "ignore case in search patterns"),
    o("imactivatefunc", "imaf", Skipped(BIDI), Absent, None,
      "function to enable/disable the X input method"),
    o("imactivatekey", "imak", Skipped(BIDI), Absent, None,
      "key that activates the X input method"),
    o("imcmdline", "imc", Skipped(BIDI), Absent, None,
      "use IM when starting to edit a command line"),
    o("imdisable", "imd", Skipped(BIDI), Absent, None,
      "do not use the IM in any mode"),
    o("iminsert", "imi", Skipped(BIDI), Absent, None,
      "use :lmap or IM in Insert mode"),
    o("imsearch", "ims", Skipped(BIDI), Absent, None,
      "use :lmap or IM when typing a search pattern"),
    o("imstatusfunc", "imsf", Skipped(BIDI), Absent, None,
      "function to obtain X input method status"),
    o("imstyle", "imst", Skipped(BIDI), Absent, None,
      "specifies the input style of the input method"),
    o("include", "inc", NotImplemented, Absent, None,
      "pattern to be used to find an include file — nice to have"),
    o("includeexpr", "inex", Skipped(VIMSCRIPT), Absent, None,
      "expression used to process an include line"),
    o("incsearch", "is", Implemented, Full, Some(OptSetup("vim.o.incsearch=")),
      "highlight match while typing search pattern"),
    o("indentexpr", "inde", Skipped(VIMSCRIPT), Absent, None,
      "expression used to obtain the indent of a line"),
    o("indentkeys", "indk", NotImplemented, Absent, None,
      "keys that trigger indenting with 'indentexpr' — nice to have"),
    o("infercase", "inf", NotImplemented, Absent, None,
      "adjust case of match for keyword completion — worth implementing"),
    o("insertmode", "im", Skipped(VICOMPAT), Absent, None,
      "start the edit of a file in Insert mode"),
    o("isfname", "isf", NotImplemented, Absent, None,
      "affects gf and file-name completion; worth implementing"),
    o("isident", "isi", NotImplemented, Absent, None,
      "companion to the implemented 'iskeyword'; worth implementing"),
    o("iskeyword", "isk", Implemented, Full, Some(OptSetup("vim.o.iskeyword=")),
      "characters included in keywords"),
    o("isprint", "isp", NotImplemented, Absent, None,
      "affects how unprintable chars render in both backends; worth implementing"),
    o("joinspaces", "js", Implemented, Full, Some(OptSetup("vim.o.joinspaces=")),
      "two spaces after a period with a join command"),
    o("jumpoptions", "jop", NotImplemented, Absent, None,
      "specifies how jumping is done — nice to have"),
    o("key", "", NotImplemented, Absent, None,
      "encryption key — nice to have"),
    o("keymap", "kmp", Skipped(BIDI), Absent, None,
      "name of a keyboard mapping"),
    o("keymodel", "km", Skipped(SELECT), Absent, None,
      "enable starting/stopping selection with keys"),
    o("keyprotocol", "kpc", Skipped(TERMCAP), Absent, None,
      "what keyboard protocol to use for what terminal"),
    o("keywordprg", "kp", NotImplemented, Absent, None,
      "program to use for the \"K\" command — nice to have"),
    o("langmap", "lmap", Skipped(BIDI), Absent, None,
      "alphabetic characters for other language mode"),
    o("langmenu", "lm", Skipped(BIDI), Absent, None,
      "language to be used for the menus"),
    o("langnoremap", "lnr", Skipped(BIDI), Absent, None,
      "do not apply 'langmap' to mapped characters"),
    o("langremap", "lrm", Skipped(BIDI), Absent, None,
      "do apply 'langmap' to mapped characters"),
    o("laststatus", "ls", Partial, Full, Some(OptSetup("vim.o.laststatus=")),
      "0/1/2 honoured; 3 (global statusline) falls back to per-window"),
    o("lazyredraw", "lz", Skipped(ARCH), Absent, None,
      "both backends redraw from a single frame-driven paint; nothing to defer"),
    o("linebreak", "lbr", Implemented, Full, Some(OptSetup("vim.o.linebreak=")),
      "wrap long lines at a blank"),
    o("lines", "", NotImplemented, Absent, None,
      "number of lines in the display — low value"),
    o("linespace", "lsp", Skipped(VIMGUI), Absent, None,
      "superseded by vimcode's own font_size/ui_font_size; note its Vim abbreviation 'lsp' is taken by vimcode's own :set lsp"),
    o("lisp", "", NotImplemented, Absent, None,
      "automatic indenting for Lisp — nice to have"),
    o("lispoptions", "lop", NotImplemented, Absent, None,
      "changes how Lisp indenting is done — nice to have"),
    o("lispwords", "lw", NotImplemented, Absent, None,
      "words that change how lisp indenting works — nice to have"),
    o("list", "", Implemented, Full, Some(OptSetup("vim.o.list=")),
      "show <Tab> and <EOL>"),
    o("listchars", "lcs", Partial, Full, Some(OptSetup("vim.o.listchars=")),
      "tab/trail/eol/space rendered; extends/precedes/nbsp/conceal are not"),
    o("loadplugins", "lpl", Skipped(SCRIPTRT), Absent, None,
      "load plugin scripts when starting up"),
    o("luadll", "", Skipped(INTERP), Absent, None,
      "name of the Lua dynamic library"),
    o("macatsui", "", Skipped(VIMGUI), Absent, None,
      "Mac GUI: use ATSUI text drawing"),
    o("magic", "", Implemented, Full, Some(OptSetup("vim.o.magic=")),
      "changes special characters in search patterns"),
    o("makeef", "mef", Skipped(MAKE), Absent, None,
      "name of the errorfile for \":make\""),
    o("makeencoding", "menc", Skipped(ENCODING), Absent, None,
      "encoding of external make/grep commands"),
    o("makeprg", "mp", Skipped(MAKE), Absent, None,
      "program to use for the \":make\" command"),
    o("matchpairs", "mps", NotImplemented, Absent, None,
      "`%` matching is hardcoded to ()[]{}; worth implementing"),
    o("matchtime", "mat", NotImplemented, Absent, None,
      "#1207 shipped 'showmatch' with a fixed flash duration; worth implementing"),
    o("maxcombine", "mco", Skipped(ENCODING), Absent, None,
      "maximum nr of combining characters displayed"),
    o("maxfuncdepth", "mfd", Skipped(SCRIPTRT), Absent, None,
      "maximum recursive depth for user functions"),
    o("maxmapdepth", "mmd", NotImplemented, Absent, None,
      "maximum recursive depth for mapping — nice to have"),
    o("maxmem", "mm", Skipped(ARCH), Absent, None,
      "maximum memory (in Kbyte) used for one buffer"),
    o("maxmempattern", "mmp", Skipped(ARCH), Absent, None,
      "maximum memory (in Kbyte) used for pattern search"),
    o("maxmemtot", "mmt", Skipped(ARCH), Absent, None,
      "maximum memory (in Kbyte) used for all buffers"),
    o("menuitems", "mis", Skipped(VIMGUI), Absent, None,
      "maximum number of items in a menu"),
    o("mkspellmem", "msm", NotImplemented, Absent, None,
      "memory used before |:mkspell| compresses the tree — nice to have"),
    o("modeline", "ml", NotImplemented, Absent, None,
      "recognize modelines at start or end of file — nice to have"),
    o("modelineexpr", "mle", Skipped(VIMSCRIPT), Absent, None,
      "allow setting expression options from a modeline"),
    o("modelines", "mls", NotImplemented, Absent, None,
      "number of lines checked for modelines — nice to have"),
    o("modifiable", "ma", NotImplemented, Absent, None,
      "changes to the text are not possible — worth implementing"),
    o("modified", "mod", NotImplemented, Absent, None,
      "buffer has been modified — worth implementing"),
    o("more", "", Skipped(EXMODE), Absent, None,
      "pause listings when the whole screen is filled"),
    o("mouse", "", NotImplemented, Absent, None,
      "enable the use of mouse clicks — nice to have"),
    o("mousefocus", "mousef", NotImplemented, Absent, None,
      "keyboard focus follows the mouse — nice to have"),
    o("mousehide", "mh", Skipped(VIMGUI), Absent, None,
      "hide mouse pointer while typing"),
    o("mousemodel", "mousem", NotImplemented, Absent, None,
      "changes meaning of mouse buttons — nice to have"),
    o("mousemoveevent", "mousemev", NotImplemented, Absent, None,
      "report mouse moves with <MouseMove> — nice to have"),
    o("mouseshape", "mouses", Skipped(VIMGUI), Absent, None,
      "shape of the mouse pointer in different modes"),
    o("mousetime", "mouset", NotImplemented, Absent, None,
      "max time between mouse double-click — nice to have"),
    o("mzquantum", "mzq", Skipped(INTERP), Absent, None,
      "the interval between polls for MzScheme threads"),
    o("mzschemedll", "", Skipped(INTERP), Absent, None,
      "name of the MzScheme dynamic library"),
    o("mzschemegcdll", "", Skipped(INTERP), Absent, None,
      "name of the MzScheme dynamic library for GC"),
    o("nrformats", "nf", Partial, Full, Some(OptSetup("vim.o.nrformats=")),
      "alpha/octal/hex/bin parsed; 'unsigned' is not honoured"),
    o("number", "nu", Implemented, Full, Some(OptSetup("vim.o.number=")),
      "print the line number in front of each line"),
    o("numberwidth", "nuw", NotImplemented, Absent, None,
      "number of columns used for the line number — worth implementing"),
    o("omnifunc", "ofu", Skipped(VIMSCRIPT), Absent, None,
      "function for filetype-specific completion"),
    o("opendevice", "odev", Skipped(PLATFORM), Absent, None,
      "allow reading/writing devices on MS-Windows"),
    o("operatorfunc", "opfunc", Skipped(VIMSCRIPT), Absent, None,
      "g@ is implemented; the function is registered from Lua (vimcode.set_operatorfunc), not from :set"),
    o("osfiletype", "oft", Skipped(OBSOLETE), Absent, None,
      "no longer supported"),
    o("packpath", "pp", Skipped(SCRIPTRT), Absent, None,
      "list of directories used for packages"),
    o("paragraphs", "para", NotImplemented, Absent, None,
      "affects the { } paragraph motions; worth implementing"),
    o("paste", "", Skipped(VICOMPAT), Absent, None,
      "bracketed paste is handled by the backend; Vim itself deprecates this option"),
    o("pastetoggle", "pt", Skipped(VICOMPAT), Absent, None,
      "key code that causes 'paste' to toggle"),
    o("patchexpr", "pex", Skipped(VIMSCRIPT), Absent, None,
      "expression used to patch a file"),
    o("patchmode", "pm", NotImplemented, Absent, None,
      "keep the oldest version of a file — nice to have"),
    o("path", "pa", NotImplemented, Absent, None,
      "list of directories searched with \"gf\" et.al. — nice to have"),
    o("perldll", "", Skipped(INTERP), Absent, None,
      "name of the Perl dynamic library"),
    o("preserveindent", "pi", NotImplemented, Absent, None,
      "preserve the indent structure when reindenting — worth implementing"),
    o("previewheight", "pvh", NotImplemented, Absent, None,
      "height of the preview window — nice to have"),
    o("previewpopup", "pvp", Skipped(VIMGUI), Absent, None,
      "use popup window for preview"),
    o("previewwindow", "pvw", NotImplemented, Absent, None,
      "identifies the preview window — nice to have"),
    o("printdevice", "pdev", Skipped(PRINTING), Absent, None,
      "name of the printer to be used for :hardcopy"),
    o("printencoding", "penc", Skipped(PRINTING), Absent, None,
      "encoding to be used for printing"),
    o("printexpr", "pexpr", Skipped(PRINTING), Absent, None,
      "expression used to print PostScript for :hardcopy"),
    o("printfont", "pfn", Skipped(PRINTING), Absent, None,
      "name of the font to be used for :hardcopy"),
    o("printheader", "pheader", Skipped(PRINTING), Absent, None,
      "format of the header used for :hardcopy"),
    o("printmbcharset", "pmbcs", Skipped(PRINTING), Absent, None,
      "CJK character set to be used for :hardcopy"),
    o("printmbfont", "pmbfn", Skipped(PRINTING), Absent, None,
      "font names to be used for CJK output of :hardcopy"),
    o("printoptions", "popt", Skipped(PRINTING), Absent, None,
      "controls the format of :hardcopy output"),
    o("prompt", "prompt", Skipped(EXMODE), Absent, None,
      "enable prompt in Ex mode"),
    o("pumheight", "ph", NotImplemented, Absent, None,
      "maximum height of the popup menu — nice to have"),
    o("pumwidth", "pw", NotImplemented, Absent, None,
      "minimum width of the popup menu — nice to have"),
    o("pythondll", "", Skipped(INTERP), Absent, None,
      "name of the Python 2 dynamic library"),
    o("pythonhome", "", Skipped(INTERP), Absent, None,
      "name of the Python 2 home directory"),
    o("pythonthreedll", "", Skipped(INTERP), Absent, None,
      "name of the Python 3 dynamic library"),
    o("pythonthreehome", "", Skipped(INTERP), Absent, None,
      "name of the Python 3 home directory"),
    o("pyxversion", "pyx", Skipped(INTERP), Absent, None,
      "Python version used for pyx* commands"),
    o("quickfixtextfunc", "qftf", Skipped(VIMSCRIPT), Absent, None,
      "function for the text in the quickfix window"),
    o("quoteescape", "qe", NotImplemented, Absent, None,
      "affects the i\"/a\" text objects; worth implementing"),
    o("readonly", "ro", NotImplemented, Absent, None,
      "disallow writing the buffer — worth implementing"),
    o("redrawtime", "rdt", NotImplemented, Absent, None,
      "timeout for 'hlsearch' and |:match| highlighting — nice to have"),
    o("regexpengine", "re", NotImplemented, Absent, None,
      "default regexp engine to use — nice to have"),
    o("relativenumber", "rnu", Implemented, Full, Some(OptSetup("vim.o.relativenumber=")),
      "show relative line number in front of each line"),
    o("remap", "", NotImplemented, Absent, None,
      "allow mappings to work recursively — nice to have"),
    o("renderoptions", "rop", Skipped(VIMGUI), Absent, None,
      "options for text rendering on Windows"),
    o("report", "", NotImplemented, Absent, None,
      "threshold for reporting nr. of lines changed — worth implementing"),
    o("restorescreen", "rs", Skipped(TERMCAP), Absent, None,
      "Win32: restore screen when exiting"),
    o("revins", "ri", Skipped(BIDI), Absent, None,
      "inserting characters will work backwards"),
    o("rightleft", "rl", Skipped(BIDI), Absent, None,
      "window is right-to-left oriented"),
    o("rightleftcmd", "rlc", Skipped(BIDI), Absent, None,
      "commands for which editing works right-to-left"),
    o("rubydll", "", Skipped(INTERP), Absent, None,
      "name of the Ruby dynamic library"),
    o("ruler", "ru", Implemented, Full, Some(OptSetup("vim.o.ruler=")),
      "show cursor line and column in the status line"),
    o("rulerformat", "ruf", Skipped(VIMSCRIPT), Absent, None,
      "custom format for the ruler"),
    o("runtimepath", "rtp", Skipped(SCRIPTRT), Absent, None,
      "list of directories used for runtime files"),
    o("scroll", "scr", NotImplemented, Absent, None,
      "lines to scroll with CTRL-U and CTRL-D — worth implementing"),
    o("scrollbind", "scb", NotImplemented, Absent, None,
      "scroll in window as other windows scroll — nice to have"),
    o("scrollfocus", "scf", NotImplemented, Absent, None,
      "scroll wheel applies to window under pointer — low value"),
    o("scrolljump", "sj", Implemented, Full, Some(OptSetup("vim.o.scrolljump=")),
      "minimum number of lines to scroll"),
    o("scrolloff", "so", Implemented, Full, Some(OptKeys(":set so=")),
      "minimum nr. of lines above and below cursor"),
    o("scrollopt", "sbo", NotImplemented, Absent, None,
      "how 'scrollbind' should behave — nice to have"),
    o("sections", "sect", NotImplemented, Absent, None,
      "affects the [[ ]] section motions; worth implementing"),
    o("secure", "", Skipped(SCRIPTRT), Absent, None,
      "secure mode for reading .vimrc in current dir"),
    o("selection", "sel", NotImplemented, Absent, None,
      "inclusive/exclusive changes every Visual-mode operator's end column; worth implementing"),
    o("selectmode", "slm", Skipped(SELECT), Absent, None,
      "when to use Select mode instead of Visual mode"),
    o("sessionoptions", "ssop", Skipped(SESSION), Absent, None,
      "options for |:mksession|"),
    o("shell", "sh", NotImplemented, Absent, None,
      "`:!cmd` and range filters already shell out via a hardcoded shell — honouring :set shell would finish the feature; worth implementing"),
    o("shellcmdflag", "shcf", NotImplemented, Absent, None,
      "companion to 'shell' for `:!` and range filters; worth implementing"),
    o("shellpipe", "sp", NotImplemented, Absent, None,
      "string to put output of \":make\" in error file — nice to have"),
    o("shellquote", "shq", NotImplemented, Absent, None,
      "quote character(s) for around shell command — nice to have"),
    o("shellredir", "srr", NotImplemented, Absent, None,
      "string to put output of filter in a temp file — nice to have"),
    o("shellslash", "ssl", Skipped(PLATFORM), Absent, None,
      "use forward slash for shell file names"),
    o("shelltemp", "stmp", NotImplemented, Absent, None,
      "whether to use a temp file for shell commands — nice to have"),
    o("shelltype", "st", Skipped(OBSOLETE), Absent, None,
      "Amiga: influences how to use a shell"),
    o("shellxescape", "sxe", Skipped(PLATFORM), Absent, None,
      "characters to escape when 'shellxquote' is ("),
    o("shellxquote", "sxq", Skipped(PLATFORM), Absent, None,
      "like 'shellquote', but include redirection"),
    o("shiftround", "sr", Implemented, Full, Some(OptKeys("et sr")),
      "round indent to multiple of shiftwidth"),
    o("shiftwidth", "sw", Implemented, Full, Some(OptKeys(":set sw=")),
      "number of spaces to use for (auto)indent step"),
    o("shortmess", "shm", NotImplemented, Absent, None,
      "list of flags, reduce length of messages — nice to have"),
    o("shortname", "sn", Skipped(OBSOLETE), Absent, None,
      "Filenames assumed to be 8.3 chars"),
    o("showbreak", "sbr", NotImplemented, Absent, None,
      "string to use at the start of wrapped lines — nice to have"),
    o("showcmd", "sc", Implemented, Full, Some(OptSetup("vim.o.showcmd=")),
      "show (partial) command somewhere"),
    o("showcmdloc", "sloc", NotImplemented, Absent, None,
      "where to show (partial) command — nice to have"),
    o("showfulltag", "sft", NotImplemented, Absent, None,
      "show full tag pattern when completing tag — nice to have"),
    o("showmatch", "sm", Implemented, Full, Some(OptSetup("vim.o.showmatch=")),
      "briefly jump to matching bracket if insert one"),
    o("showmode", "smd", NotImplemented, Absent, None,
      "message on status line to show current mode — worth implementing"),
    o("showtabline", "stal", NotImplemented, Absent, None,
      "tells when the tab pages line is displayed — worth implementing"),
    o("sidescroll", "ss", NotImplemented, Absent, None,
      "minimum number of columns to scroll horizontal — worth implementing"),
    o("sidescrolloff", "siso", Implemented, Full, Some(OptSetup("vim.o.sidescrolloff=")),
      "min. nr. of columns to left and right of cursor"),
    o("signcolumn", "scl", NotImplemented, Absent, None,
      "when to display the sign column — nice to have"),
    o("smartcase", "scs", Implemented, Full, Some(OptKeys(":set ic scs")),
      "no ignore case when pattern has uppercase"),
    o("smartindent", "si", Implemented, Full, Some(OptSetup("vim.o.smartindent=")),
      "smart autoindenting for C programs"),
    o("smarttab", "sta", Implemented, Full, Some(OptSetup("vim.o.smarttab=")),
      "use 'shiftwidth' when inserting <Tab>"),
    o("smoothscroll", "sms", NotImplemented, Absent, None,
      "scroll by screen lines when 'wrap' is set — nice to have"),
    o("softtabstop", "sts", Implemented, Full, Some(OptKeys("sts=2")),
      "number of spaces that <Tab> uses while editing"),
    o("spell", "", Implemented, Full, Some(OptSetup("vim.o.spell=")),
      "enable spell checking"),
    o("spellcapcheck", "spc", NotImplemented, Absent, None,
      "pattern to locate end of a sentence — nice to have"),
    o("spellfile", "spf", NotImplemented, Absent, None,
      "files where |zg| and |zw| store words — nice to have"),
    o("spelllang", "spl", Partial, QueryOnly, Some(OptSetup("vim.o.spelllang=")),
      "queryable (:set spelllang?) but NOT settable — :set spelllang=de answers \"Unknown option\"; the 'spl' abbreviation is unknown in both directions"),
    o("spelloptions", "spo", NotImplemented, Absent, None,
      "options for spell checking — nice to have"),
    o("spellsuggest", "sps", NotImplemented, Absent, None,
      "method(s) used to suggest spelling corrections — nice to have"),
    o("splitbelow", "sb", Implemented, Full, Some(OptSetup("vim.o.splitbelow=")),
      "new window from split is below the current one"),
    o("splitkeep", "spk", NotImplemented, Absent, None,
      "determines scroll behavior for split windows — nice to have"),
    o("splitright", "spr", Implemented, Full, Some(OptSetup("vim.o.splitright=")),
      "new window is put right of the current one"),
    o("startofline", "sol", Implemented, Full, Some(OptSetup("vim.o.startofline=")),
      "commands move cursor to first non-blank in line"),
    o("statusline", "stl", Skipped(VIMSCRIPT), Absent, None,
      "custom format for the status line"),
    o("suffixes", "su", NotImplemented, Absent, None,
      "suffixes that are ignored with multiple match — nice to have"),
    o("suffixesadd", "sua", NotImplemented, Absent, None,
      "suffixes added when searching for a file — nice to have"),
    o("swapfile", "swf", Partial, Full, Some(OptSetup("vim.o.swapfile=")),
      "full name works; Vim's 'swf' abbreviation is not accepted"),
    o("swapsync", "sws", Skipped(OBSOLETE), Absent, None,
      "how to sync the swap file"),
    o("switchbuf", "swb", NotImplemented, Absent, None,
      "sets behavior when switching to another buffer — nice to have"),
    o("synmaxcol", "smc", NotImplemented, Absent, None,
      "maximum column to find syntax items — worth implementing"),
    o("syntax", "syn", NotImplemented, Absent, None,
      "syntax to be loaded for current buffer — nice to have"),
    o("tabline", "tal", Skipped(VIMSCRIPT), Absent, None,
      "custom format for the console tab pages line"),
    o("tabpagemax", "tpm", NotImplemented, Absent, None,
      "maximum number of tab pages for |-p| and \"tab all\" — nice to have"),
    o("tabstop", "ts", Implemented, Full, Some(OptKeys(":set ts=")),
      "number of spaces that <Tab> in file uses"),
    o("tagbsearch", "tbs", NotImplemented, Absent, None,
      "use binary searching in tags files — nice to have"),
    o("tagcase", "tc", NotImplemented, Absent, None,
      "how to handle case when searching in tags files — nice to have"),
    o("tagfunc", "tfu", Skipped(VIMSCRIPT), Absent, None,
      "function to get list of tag matches"),
    o("taglength", "tl", NotImplemented, Absent, None,
      "number of significant characters for a tag — nice to have"),
    o("tagrelative", "tr", NotImplemented, Absent, None,
      "file names in tag file are relative — nice to have"),
    o("tags", "tag", NotImplemented, Absent, None,
      "list of file names used by the tag command — nice to have"),
    o("tagstack", "tgst", NotImplemented, Absent, None,
      "push tags onto the tag stack — nice to have"),
    o("tcldll", "", Skipped(INTERP), Absent, None,
      "name of the Tcl dynamic library"),
    o("term", "", Skipped(TERMCAP), Absent, None,
      "name of the terminal"),
    o("termbidi", "tbidi", Skipped(BIDI), Absent, None,
      "terminal takes care of bi-directionality"),
    o("termencoding", "tenc", Skipped(ENCODING), Absent, None,
      "character encoding used by the terminal"),
    o("termguicolors", "tgc", NotImplemented, Absent, None,
      "use GUI colors for the terminal — nice to have"),
    o("termwinkey", "twk", Skipped(TERMCAP), Absent, None,
      "key that precedes a Vim command in a terminal"),
    o("termwinscroll", "twsl", Skipped(TERMCAP), Absent, None,
      "max number of scrollback lines in a terminal window"),
    o("termwinsize", "tws", Skipped(TERMCAP), Absent, None,
      "size of a terminal window"),
    o("termwintype", "twt", Skipped(TERMCAP), Absent, None,
      "MS-Windows: type of pty to use for terminal window"),
    o("terse", "", Skipped(VICOMPAT), Absent, None,
      "shorten some messages"),
    o("textauto", "ta", Skipped(OBSOLETE), Absent, None,
      "obsolete, use 'fileformats'"),
    o("textmode", "tx", Skipped(OBSOLETE), Absent, None,
      "obsolete, use 'fileformat'"),
    o("textwidth", "tw", Implemented, Full, Some(OptKeys(":set tw=")),
      "maximum width of text that is being inserted"),
    o("thesaurus", "tsr", NotImplemented, Absent, None,
      "list of thesaurus files for keyword completion — nice to have"),
    o("thesaurusfunc", "tsrfu", Skipped(VIMSCRIPT), Absent, None,
      "function to be used for thesaurus completion"),
    o("tildeop", "top", NotImplemented, Absent, None,
      "makes `~` an operator; small, self-contained and conformance-visible; worth implementing"),
    o("timeout", "to", NotImplemented, Absent, None,
      "time out on mappings and key codes — nice to have"),
    o("timeoutlen", "tm", Implemented, Full, Some(OptSetup("vim.o.timeoutlen=")),
      "time out time in milliseconds"),
    o("title", "", NotImplemented, Absent, None,
      "let Vim set the title of the window — nice to have"),
    o("titlelen", "", NotImplemented, Absent, None,
      "percentage of 'columns' used for window title — nice to have"),
    o("titleold", "", NotImplemented, Absent, None,
      "old title, restored when exiting — nice to have"),
    o("titlestring", "", Skipped(VIMSCRIPT), Absent, None,
      "string to use for the Vim window title"),
    o("toolbar", "tb", Skipped(VIMGUI), Absent, None,
      "GUI: which items to show in the toolbar"),
    o("toolbariconsize", "tbis", Skipped(VIMGUI), Absent, None,
      "size of the toolbar icons (for GTK 2 only)"),
    o("ttimeout", "", Skipped(ARCH), Absent, None,
      "vimcode never reads termcap, so there are no key codes to time out on"),
    o("ttimeoutlen", "ttm", Skipped(ARCH), Absent, None,
      "vimcode never reads termcap, so there are no key codes to time out on"),
    o("ttybuiltin", "tbi", Skipped(TERMCAP), Absent, None,
      "use built-in termcap before external termcap"),
    o("ttyfast", "tf", Skipped(TERMCAP), Absent, None,
      "indicates a fast terminal connection"),
    o("ttymouse", "ttym", Skipped(TERMCAP), Absent, None,
      "type of mouse codes generated"),
    o("ttyscroll", "tsl", Skipped(TERMCAP), Absent, None,
      "maximum number of lines for a scroll"),
    o("ttytype", "tty", Skipped(TERMCAP), Absent, None,
      "alias for 'term'"),
    // #1156 implemented these three (they were ❌ Absent when this slice ran).
    o("undodir", "udir", Partial, Full, Some(OptSetup("vim.o.undodir=")),
      "accepts one directory; Vim's is a comma-separated priority list with \
       `.` and `//` forms, which vimcode has no per-directory fallback to drive"),
    o("undofile", "udf", Implemented, Full, Some(OptSetup("vim.o.undofile=")),
      "save undo information in a file"),
    o("undolevels", "ul", Partial, Full, Some(OptSetup("vim.o.undolevels=")),
      "caps live undo-tree states globally; no `ul=-1` (undo disabled) and no \
       buffer-local `:setlocal ul`"),
    o("undoreload", "ur", NotImplemented, Absent, None,
      "max nr of lines to save for undo on a buffer reload — nice to have"),
    o("updatecount", "uc", NotImplemented, Absent, None,
      "after this many characters flush swap file — nice to have"),
    o("updatetime", "ut", Implemented, Full, Some(OptSetup("vim.o.updatetime=")),
      "after this many milliseconds flush swap file"),
    o("varsofttabstop", "vsts", NotImplemented, Absent, None,
      "a list of number of spaces when typing <Tab> — worth implementing"),
    o("vartabstop", "vts", NotImplemented, Absent, None,
      "a list of number of spaces for <Tab>s — worth implementing"),
    o("verbose", "vbs", Skipped(ARCH), Absent, None,
      "give informative messages"),
    o("verbosefile", "vfile", Skipped(ARCH), Absent, None,
      "file to write messages in"),
    o("viewdir", "vdir", Skipped(SESSION), Absent, None,
      "directory where to store files with :mkview"),
    o("viewoptions", "vop", Skipped(SESSION), Absent, None,
      "specifies what to save for :mkview"),
    o("viminfo", "vi", NotImplemented, Absent, None,
      "use .viminfo file upon startup and exiting — nice to have"),
    o("viminfofile", "vif", NotImplemented, Absent, None,
      "file name used for the viminfo file — nice to have"),
    o("virtualedit", "ve", Partial, Full, Some(OptKeys(":set ve=all")),
      "block/insert/all/onemore parsed; 'none' clearing and per-mode semantics are only partly wired"),
    o("visualbell", "vb", NotImplemented, Absent, None,
      "use visual bell instead of beeping — nice to have"),
    o("warn", "", Skipped(VICOMPAT), Absent, None,
      "warn for shell command when buffer was changed"),
    o("weirdinvert", "wiv", Skipped(TERMCAP), Absent, None,
      "for terminals that have weird inversion method"),
    o("whichwrap", "ww", Implemented, Full, Some(OptSetup("vim.o.whichwrap=")),
      "allow specified keys to cross line boundaries"),
    o("wildchar", "wc", NotImplemented, Absent, None,
      "command-line character for wildcard expansion — nice to have"),
    o("wildcharm", "wcm", NotImplemented, Absent, None,
      "like 'wildchar' but also works when mapped — nice to have"),
    o("wildignore", "wig", NotImplemented, Absent, None,
      "files matching these patterns are not completed — nice to have"),
    o("wildignorecase", "wic", NotImplemented, Absent, None,
      "ignore case when completing file names — nice to have"),
    o("wildmenu", "wmnu", Partial, Full, Some(OptSetup("vim.o.wildmenu=")),
      "accepted as a no-op — vimcode's wildmenu is unconditional, so :set nowildmenu does not turn it off"),
    o("wildmode", "wim", Implemented, Full, Some(OptSetup("vim.o.wildmode=")),
      "mode for 'wildchar' command-line expansion"),
    o("wildoptions", "wop", NotImplemented, Absent, None,
      "specifies how command line completion is done — nice to have"),
    o("winaltkeys", "wak", Skipped(VIMGUI), Absent, None,
      "when the windows system handles ALT keys"),
    o("wincolor", "wcr", Skipped(VIMGUI), Absent, None,
      "window-local highlighting"),
    o("window", "wi", NotImplemented, Absent, None,
      "nr of lines to scroll for CTRL-F and CTRL-B — nice to have"),
    o("winfixheight", "wfh", NotImplemented, Absent, None,
      "keep window height when opening/closing windows — nice to have"),
    o("winfixwidth", "wfw", NotImplemented, Absent, None,
      "keep window width when opening/closing windows — nice to have"),
    o("winheight", "wh", NotImplemented, Absent, None,
      "minimum number of lines for the current window — nice to have"),
    o("winminheight", "wmh", NotImplemented, Absent, None,
      "minimum number of lines for any window — nice to have"),
    o("winminwidth", "wmw", NotImplemented, Absent, None,
      "minimal number of columns for any window — nice to have"),
    o("winptydll", "", Skipped(INTERP), Absent, None,
      "name of the winpty dynamic library"),
    o("winwidth", "wiw", NotImplemented, Absent, None,
      "minimal number of columns for current window — nice to have"),
    o("wrap", "", Implemented, Full, Some(OptSetup("vim.o.wrap=")),
      "long lines wrap and continue on the next line"),
    o("wrapmargin", "wm", NotImplemented, Absent, None,
      "chars from the right where wrapping starts — worth implementing"),
    o("wrapscan", "ws", Implemented, Full, Some(OptKeys(":set nowrapscan")),
      "searches wrap around the end of the file"),
    o("write", "", NotImplemented, Absent, None,
      "writing to a file is allowed — nice to have"),
    o("writeany", "wa", NotImplemented, Absent, None,
      "write to file with no need for \"!\" override — nice to have"),
    o("writebackup", "wb", NotImplemented, Absent, None,
      "make a backup before overwriting a file — nice to have"),
    o("writedelay", "wd", Skipped(ARCH), Absent, None,
      "delay this many msec for each char (for debug)"),
    o("xtermcodes", "", Skipped(TERMCAP), Absent, None,
      "request terminal codes from an xterm"),
];

// ---------------------------------------------------------------------------
// OPTION_COVERAGE_EXEMPT — this list may only ever SHRINK.
//
// The Implemented/Partial options the oracle corpus never pins. Seeded from a
// measured run (`CONFORMANCE_DUMP_OPTION_COVERAGE=…`), not by hand, so the
// number is the real one rather than an impression:
//
//     **25 of the 54 options vimcode implements have no oracle case**
//     (29 are pinned, 54%).
//
// #1156 added three more implemented options (see the `undo*` entries at the
// end of the list), so the live figure is now 28 of 57 unpinned — the ratchet
// prints the current one on every passing run.
//
// Writing the cases that delete these entries is #1162's job; the entry is
// deleted by the case, never by an editor's judgement, because gate 2 fails
// the moment a listed option's probe starts matching.
// ---------------------------------------------------------------------------
const OPTION_COVERAGE_EXEMPT: &[&str] = &[
    // Display/gutter options: nothing in the corpus renders a screen, so no
    // case can pin one until #1162 adds render-comparing cases.
    "colorcolumn",
    "cursorline",
    "laststatus",
    "linebreak",
    "list",
    "listchars",
    "number",
    "relativenumber",
    "ruler",
    "showcmd",
    "wrap",
    // Search-highlight and incremental-search state: the corpus compares
    // buffer text and cursor position, never highlight extents.
    "hlsearch",
    "incsearch",
    // Scroll geometry beyond 'scrolloff' — the window-tracking cases pin
    // 'scrolloff' only.
    "scrolljump",
    "sidescrolloff",
    // Window/buffer and session-level behaviour, with no single-buffer
    // keystroke that exposes it.
    "autoread",
    "splitbelow",
    "splitright",
    "swapfile",
    "timeoutlen",
    "updatetime",
    // Command-line completion: no case types <Tab> on a `:` line.
    "wildmenu",
    "wildmode",
    // Spelling: `src/core/spell.rs` is implemented but the corpus has no
    // spell case at all — the same hole COVERAGE_EXEMPT records for z=/zg/zw.
    "spell",
    "spelllang",
    // Undo persistence (#1156), seeded here the one way this list is allowed
    // to grow: an option that was ❌ NotImplemented when this slice ran, and
    // so had no probe and no entry, gained one. None of the three is
    // reachable from a case's Lua `setup`, and not for want of writing one:
    //
    //   * 'undolevels' — `run_in_neovim` *overwrites* it (`= -1` around the
    //     fixture write, then `= 1000`) after the case's `setup` has run, so
    //     a setup that pins it is clobbered before the first keystroke. Only
    //     an `OptKeys(":set ul=")` case could pin it, and that needs
    //     vimcode's pruning order to match Vim's block-for-block first.
    //   * 'undofile' / 'undodir' — the effect is a sidecar file written on
    //     `:w` and re-read on the *next open of the same path*. The case
    //     shape is one buffer, one keystroke sequence, compare text and
    //     cursor; it never reopens a file, and `undofile::write` is a
    //     `cfg!(test)` no-op on the vimcode side besides.
    //
    // Deleting these three is #1162's job, same as every entry above.
    "undodir",
    "undofile",
    "undolevels",
];

/// Options whose **abbreviation** does not have the same surface as their full
/// name. Every entry is a finding, not a convenience: gate 1 otherwise
/// requires `:set sw=4` and `:set shiftwidth=4` to behave identically, which
/// is what Vim guarantees and what every other row satisfies.
const ABBREV_SURFACE_EXCEPTIONS: &[(&str, Surface, &str)] = &[
    (
        "spelllang",
        Absent,
        "'spl' is unknown in both directions, while the full name answers \
         `:set spelllang?`",
    ),
    (
        "swapfile",
        Absent,
        "'swf' is unknown, while `:set swapfile` / `:set noswapfile` work",
    ),
    (
        "linespace",
        Full,
        "vimcode's own `:set lsp` (its LSP toggle) has taken Vim's abbreviation for \
         'linespace' — the same collision the registry deliberately avoided for \
         'nrformats'/'nf' vs nerdfonts",
    ),
];

// ---------------------------------------------------------------------------
// The audit's gates
// ---------------------------------------------------------------------------

/// Drive `Settings::parse_set_option` and report what surface `name` has.
/// Deliberately behavioural: it asks the same public entry point `:set` asks,
/// so nothing here can drift from what a user types.
fn measured_surface(name: &str) -> Surface {
    let try_set = |arg: String| Settings::default().parse_set_option(&arg);
    const NOT_IMPL: &str = "recognised but not implemented";

    let mut queryable = false;
    let mut settable = false;
    let mut stub = false;
    let mut current: Option<String> = None;

    match try_set(format!("{name}?")) {
        Ok(v) => {
            queryable = true;
            if let Some((_, val)) = v.split_once('=') {
                current = Some(val.to_string());
            }
        }
        Err(e) if e.contains(NOT_IMPL) => stub = true,
        Err(_) => {}
    }

    // Boolean and numeric forms both, plus a round-trip of the queried value
    // so a value option is probed with something it actually accepts.
    let mut probes = vec![name.to_string(), format!("no{name}"), format!("{name}=1")];
    if let Some(c) = &current {
        probes.push(format!("{name}={c}"));
    }
    for p in probes {
        match try_set(p) {
            Ok(_) => settable = true,
            Err(e) if e.contains(NOT_IMPL) => stub = true,
            Err(e) if e.starts_with("Unknown option") => {}
            // Any other rejection ("Invalid value for …") means the *name* is
            // in the registry — only the probe value was wrong.
            Err(_) => settable = true,
        }
    }

    match (stub, queryable, settable) {
        (true, _, _) => Surface::Stub,
        (_, true, true) => Surface::Full,
        (_, false, true) => Surface::SetOnly,
        (_, true, false) => Surface::QueryOnly,
        (_, false, false) => Surface::Absent,
    }
}

/// Every row whose recorded `surface` disagrees with what `:set` does now.
/// Split out of the test so [`option_audit_gates_are_bidirectional`] can feed
/// it a deliberately mis-tagged table and observe the gate go red — a gate
/// nobody has seen fail is not a gate (#553).
fn surface_drift(audit: &[OptionAudit], exceptions: &[(&str, Surface, &str)]) -> Vec<String> {
    let mut drift: Vec<String> = Vec::new();
    for e in audit {
        let measured = measured_surface(e.name);
        if measured != e.surface {
            drift.push(format!(
                "  '{}': table says {:?}, `:set` actually gives {:?}",
                e.name, e.surface, measured
            ));
        }
        if e.short.is_empty() {
            continue;
        }
        let expected_short = exceptions
            .iter()
            .find(|(n, _, _)| *n == e.name)
            .map(|(_, s, _)| *s)
            .unwrap_or(e.surface);
        let measured_short = measured_surface(e.short);
        if measured_short != expected_short {
            drift.push(format!(
                "  '{}' abbreviation '{}': expected {:?}, `:set` gives {:?}",
                e.name, e.short, expected_short, measured_short
            ));
        }
    }
    drift
}

/// Gate 1 (#1225) — the table's `surface` column is a claim about live code,
/// and this re-measures every one of the 421 rows against it. Pure: no `nvim`,
/// no subprocess, so it runs on every lane.
#[test]
fn option_audit_matches_the_live_settings_registry() {
    let drift = surface_drift(OPTION_AUDIT, ABBREV_SURFACE_EXCEPTIONS);
    assert!(
        drift.is_empty(),
        "\n\n== :set option audit drifted from src/core/settings.rs (#1225) ==\n\
         The audit table records the surface `:set` exposed when the slice ran. \n\
         Implementing (or breaking) an option changes that surface, so re-tag the\n\
         row — that is how the audit stays true instead of rotting like a\n\
         markdown checklist.\n\n{}\n\n\
         An option that gained an implementation also needs its status changed\n\
         from NotImplemented, a probe naming the case that covers it, and an\n\
         OPTION_COVERAGE_EXEMPT entry if no case does yet.\n",
        drift.join("\n")
    );
}

/// Gate 1b (#1225) — the table describes itself correctly: complete, ordered,
/// unique, no unreviewed row, every ⏭️ carrying a reason from the fixed
/// vocabulary, and a probe on exactly the rows that can have one.
#[test]
fn option_audit_is_internally_consistent() {
    use std::collections::HashSet;
    let mut problems: Vec<String> = Vec::new();

    assert_eq!(
        OPTION_AUDIT.len(),
        421,
        "Vim 9.1's `:help option-list` has 421 entries; the audit must tag all of them"
    );

    let mut seen: HashSet<&str> = HashSet::new();
    let mut prev = "";
    for e in OPTION_AUDIT {
        if !seen.insert(e.name) {
            problems.push(format!("  '{}': listed twice", e.name));
        }
        if e.name <= prev {
            problems.push(format!(
                "  '{}': out of order (follows '{prev}'); the table is in `:help` order",
                e.name
            ));
        }
        prev = e.name;

        if e.note.trim().is_empty() {
            problems.push(format!(
                "  '{}': empty note — every row is reviewed",
                e.name
            ));
        }

        match e.status {
            OptStatus::Skipped(reason) => {
                if !SKIP_REASONS.contains(&reason) {
                    problems.push(format!(
                        "  '{}': skip reason {reason:?} is not in SKIP_REASONS",
                        e.name
                    ));
                }
                if !matches!(e.surface, Surface::Absent) {
                    problems.push(format!(
                        "  '{}': skipped, but `:set` recognises it ({:?}) — it is at least \
                         Partial",
                        e.name, e.surface
                    ));
                }
            }
            OptStatus::Implemented => {
                if !matches!(e.surface, Surface::Full) {
                    problems.push(format!(
                        "  '{}': Implemented, but its surface is {:?} — that is Partial",
                        e.name, e.surface
                    ));
                }
            }
            OptStatus::Partial => {
                if matches!(e.surface, Surface::Absent | Surface::Stub) {
                    problems.push(format!(
                        "  '{}': Partial, but `:set` does not implement it ({:?})",
                        e.name, e.surface
                    ));
                }
            }
            OptStatus::NotImplemented => {
                if !matches!(e.surface, Surface::Absent | Surface::Stub) {
                    problems.push(format!(
                        "  '{}': NotImplemented, but `:set` implements it ({:?})",
                        e.name, e.surface
                    ));
                }
            }
        }

        let wants_probe = matches!(e.status, OptStatus::Implemented | OptStatus::Partial);
        if wants_probe && e.probe.is_none() {
            problems.push(format!(
                "  '{}': in scope for the oracle gate but has no probe",
                e.name
            ));
        }
        if !wants_probe && e.probe.is_some() {
            problems.push(format!(
                "  '{}': not in scope for the oracle gate, so its probe can never fire",
                e.name
            ));
        }
    }

    for (name, _, _) in ABBREV_SURFACE_EXCEPTIONS {
        if !OPTION_AUDIT.iter().any(|e| e.name == *name) {
            problems.push(format!(
                "  ABBREV_SURFACE_EXCEPTIONS names '{name}', which is not an audited option"
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "\n\n== :set option audit table is inconsistent (#1225) ==\n{}\n",
        problems.join("\n")
    );
}

/// Every corpus case as `(label, keys, setup)`. [`all_corpus_cases`] drops the
/// `setup`, which is precisely where an option case pins its option.
fn all_corpus_cases_with_setup() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut out: Vec<(&str, &str, &str)> = CATEGORIES
        .iter()
        .flat_map(|(_, g)| g.iter())
        .map(|c| (c.label, c.keys, c.setup))
        .collect();
    // The multi-file cases carry no `setup` (they pin files, not options), so
    // they contribute an empty one rather than being dropped: a `Keys` probe
    // must still be able to see their key sequences.
    out.extend(CASES_MULTI_JUMP.iter().map(|c| (c.label, c.keys, "")));
    out.extend(CASES_MULTI_JUMPS_LIST.iter().map(|c| (c.label, c.keys, "")));
    out
}

impl OptProbe {
    fn matches(&self, keys: &str, setup_no_ws: &str) -> bool {
        match self {
            OptProbe::OptSetup(n) => setup_no_ws.contains(n),
            OptProbe::OptKeys(n) => keys.contains(n),
        }
    }
}

/// The option gate's verdict, in the two directions [`COVERAGE_EXEMPT`]
/// established: coverage lost, and coverage gained but not yet claimed.
#[derive(Debug, Default, PartialEq, Eq)]
struct OptionCoverage {
    /// In scope, not exempt, probe matches nothing — coverage went backwards.
    uncovered: Vec<&'static str>,
    /// Exempt, but a case now pins it — delete the entry.
    newly_covered: Vec<&'static str>,
    /// Exempt but not an in-scope audited option — stale.
    stale: Vec<&'static str>,
    /// In-scope rows considered (Implemented/Partial).
    in_scope: usize,
}

/// `cases` is `(keys, setup-with-whitespace-stripped)` for the whole corpus.
fn classify_option_coverage(
    audit: &'static [OptionAudit],
    exempt: &[&'static str],
    cases: &[(&str, String)],
) -> OptionCoverage {
    use std::collections::HashSet;
    let exempt_set: HashSet<&str> = exempt.iter().copied().collect();
    let mut v = OptionCoverage::default();
    for e in audit {
        let Some(probe) = e.probe else { continue };
        v.in_scope += 1;
        let covered = cases.iter().any(|(keys, setup)| probe.matches(keys, setup));
        match (covered, exempt_set.contains(e.name)) {
            (false, false) => v.uncovered.push(e.name),
            (true, true) => v.newly_covered.push(e.name),
            _ => {}
        }
    }
    v.stale = exempt
        .iter()
        .copied()
        .filter(|n| !audit.iter().any(|e| e.name == *n && e.probe.is_some()))
        .collect();
    v
}

/// The corpus as [`classify_option_coverage`] wants it: keys verbatim, and the
/// `setup` with **all whitespace removed** so one needle matches both
/// `vim.o.magic = false` and `vim.o.smarttab=false`.
fn option_probe_corpus() -> Vec<(&'static str, String)> {
    all_corpus_cases_with_setup()
        .into_iter()
        .map(|(_, keys, setup)| (keys, setup.chars().filter(|c| !c.is_whitespace()).collect()))
        .collect()
}

/// Gate 2 (#1225) — the #1007 ratchet's shape, applied to the audited options:
/// an in-scope option whose probe matches nothing must be exempt, and an
/// exempt option whose probe now matches must lose its entry. Pure.
#[test]
fn option_audit_oracle_coverage_is_shrink_only() {
    use std::collections::HashSet;
    let corpus = option_probe_corpus();
    let exempt: HashSet<&str> = OPTION_COVERAGE_EXEMPT.iter().copied().collect();
    assert_eq!(
        exempt.len(),
        OPTION_COVERAGE_EXEMPT.len(),
        "OPTION_COVERAGE_EXEMPT lists an option twice"
    );

    let v = classify_option_coverage(OPTION_AUDIT, OPTION_COVERAGE_EXEMPT, &corpus);

    if let Ok(path) = std::env::var("CONFORMANCE_DUMP_OPTION_COVERAGE") {
        let mut s = String::new();
        for n in &v.uncovered {
            s.push_str(&format!("UNCOVERED\t{n}\n"));
        }
        for n in &v.newly_covered {
            s.push_str(&format!("NEWLY_COVERED\t{n}\n"));
        }
        for n in &v.stale {
            s.push_str(&format!("STALE\t{n}\n"));
        }
        std::fs::write(&path, s).unwrap_or_else(|e| panic!("dump to {path}: {e}"));
        return;
    }

    let covered = v.in_scope - OPTION_COVERAGE_EXEMPT.len();
    println!(
        "\n== :set option oracle coverage (#1225) ==\n\
         {covered}/{} implemented options are pinned by an oracle case; {} exempt.\n",
        v.in_scope,
        OPTION_COVERAGE_EXEMPT.len()
    );

    assert!(
        v.uncovered.is_empty() && v.newly_covered.is_empty() && v.stale.is_empty(),
        "\n\n== :set option oracle coverage moved (#1225) ==\n\
         UNCOVERED (probe matches no case — add the case, or exempt it only when \
         seeding a newly-tagged option):\n  {:?}\n\
         NEWLY COVERED (a case now pins it — delete the OPTION_COVERAGE_EXEMPT \
         entry; that is how the list shrinks):\n  {:?}\n\
         STALE (exempt but not an in-scope audited option):\n  {:?}\n",
        v.uncovered,
        v.newly_covered,
        v.stale
    );
}

/// Both gates, observed **failing** — on synthetic input for the shapes a
/// human would otherwise have to take on trust, and on the **real** table and
/// corpus for the two that matter most. #553 shipped black-box tests that
/// stayed green with the bug reinstated; an audit whose gate cannot go red is
/// the same mistake in table form.
#[test]
fn option_audit_gates_are_bidirectional() {
    // ── Gate 1, direction A: a row that under-claims. 'tabstop' is fully
    // implemented, so tagging it Absent must be caught.
    static MIS_ABSENT: &[OptionAudit] = &[o(
        "tabstop",
        "ts",
        NotImplemented,
        Absent,
        None,
        "deliberately mis-tagged fixture",
    )];
    let drift = surface_drift(MIS_ABSENT, &[]);
    assert_eq!(
        drift.len(),
        2,
        "mis-tagging an implemented option must be caught for both the name and \
         the abbreviation: {drift:?}"
    );

    // ── Gate 1, direction B: a row that over-claims. 'mouse' is not in the
    // registry at all, so tagging it Full must be caught. This is the
    // direction that fires when a NotImplemented row gains an implementation.
    static MIS_FULL: &[OptionAudit] = &[o(
        "mouse",
        "",
        Implemented,
        Full,
        Some(OptKeys(":set mouse=")),
        "deliberately mis-tagged fixture",
    )];
    assert_eq!(
        surface_drift(MIS_FULL, &[]).len(),
        1,
        "claiming an absent option is implemented must be caught"
    );

    // ── Gate 1, direction C: the abbreviation half. Dropping the recorded
    // exception for 'swapfile' (whose 'swf' abbreviation is missing) must
    // fail, which is what stops the three asymmetry findings from being
    // quietly "fixed" by deleting their exception rows.
    let without_exceptions = surface_drift(OPTION_AUDIT, &[]);
    assert_eq!(
        without_exceptions.len(),
        ABBREV_SURFACE_EXCEPTIONS.len(),
        "each ABBREV_SURFACE_EXCEPTIONS row must be load-bearing: {without_exceptions:?}"
    );

    // ── Gate 2, direction A (synthetic): an in-scope option nothing pins,
    // and nothing exempts.
    static UNPINNED: &[OptionAudit] = &[o(
        "tabstop",
        "ts",
        Implemented,
        Full,
        Some(OptSetup("vim.o.tabstop=")),
        "fixture",
    )];
    let empty: Vec<(&str, String)> = Vec::new();
    assert_eq!(
        classify_option_coverage(UNPINNED, &[], &empty).uncovered,
        vec!["tabstop"]
    );
    // …and exempting it makes the same table clean.
    assert!(classify_option_coverage(UNPINNED, &["tabstop"], &empty)
        .uncovered
        .is_empty());

    // ── Gate 2, direction B (synthetic): an exempt option a case now pins.
    let pinned = vec![("", "vim.o.tabstop=4".to_string())];
    assert_eq!(
        classify_option_coverage(UNPINNED, &["tabstop"], &pinned).newly_covered,
        vec!["tabstop"]
    );

    // ── Gate 2, direction C (synthetic): a stale exemption.
    assert_eq!(
        classify_option_coverage(UNPINNED, &["tabstop", "wrapmargin"], &pinned).stale,
        vec!["wrapmargin"]
    );

    // ── Gate 2 against the REAL table and corpus, the check #1007 makes for
    // COVERAGE_EXEMPT: deleting an exempt entry must fail, and adding a case
    // that pins an exempt option must fail. Anything less and the list could
    // grow silently.
    let corpus = option_probe_corpus();
    let victim = "listchars";
    assert!(
        OPTION_COVERAGE_EXEMPT.contains(&victim),
        "fixture drifted — {victim} is no longer exempt"
    );
    let without: Vec<&str> = OPTION_COVERAGE_EXEMPT
        .iter()
        .copied()
        .filter(|n| *n != victim)
        .collect();
    assert_eq!(
        classify_option_coverage(OPTION_AUDIT, &without, &corpus).uncovered,
        vec![victim],
        "deleting {victim:?} from OPTION_COVERAGE_EXEMPT must fail the gate"
    );
    let mut plus = corpus.clone();
    plus.push(("", "vim.o.listchars='eol:$'".to_string()));
    assert_eq!(
        classify_option_coverage(OPTION_AUDIT, OPTION_COVERAGE_EXEMPT, &plus).newly_covered,
        vec![victim],
        "a case pinning {victim:?} must force its exemption to be deleted"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 5 audit slice — registers and marks (#1226)
//
// Slice 2 of 5. Walks two `:help` areas end to end and tags every command in
// them Implemented / Partial / NotImplemented / Skipped, the same way #1225
// walked `:help option-list`:
//
//   * `:help registers` — `runtime/doc/change.txt`, "Registers", the ten
//     register *types* plus the commands that address them (`["x]`, `:reg`,
//     `:display`, `:put`, `i_CTRL-R`, `c_CTRL-R`, `:let @x`);
//   * `:help mark-motions` — `runtime/doc/motion.txt` §7, every `m…`, `'…`,
//     `` `… ``, `:mark`/`:k`, `:marks`, `:delmarks`, the mark-relative
//     `]'`/`` ]` ``/`['`/`` [` `` motions and the `:lockmarks` family.
//
// Both were read from the **pinned fleet oracle's own runtime docs** (Neovim
// v0.12.5, [`DEVIATIONS_ORACLE`]) rather than from memory, so "what Vim does"
// here is the same Vim every other gate in this file compares against.
//
// The measurement, as of this slice:
//
//     ✅ Implemented       42
//     🟡 Partial           12
//     ❌ Not implemented   32
//     ⏭️  Intentionally skipped   3   (each with a reason from SKIP_REASONS)
//                         ────
//                          89   (34 registers, 55 marks)
//
// ## Why this slice is about *missing*, not *wrong*
//
// #1226 predicted it: the corpus already carries 44 `reg:` and 55
// `mark:`/`jump:` cases and **none** of them is in [`KNOWN_DEVIATIONS`], so
// the ground already entered is solid. What the walk finds is ground never
// entered — 32 commands with no implementation at all, and 47 of the 86
// in-scope rows that no oracle case names. That is exactly the blind spot
// #1007 exists to measure, and the reason the deliverable is a table rather
// than a report.
//
// Two findings are worse than "missing", and are the reason [`Report`] is a
// recorded column rather than a bool:
//
//   * `` `[ ``/`` `] `` are set by yanks and by `>>`, but **not** by `c`, `d`
//     or `p` — so `mark:`[ after p` in the corpus passes while the mark is
//     unset, because Vim's answer and vimcode's cursor happen to coincide.
//   * Command-line `<C-r>` is bound to a readline-style reverse-i-search over
//     command history, so `:s/bar/<C-r>a/` never pastes register `a` — it
//     runs a *different* substitute (and, before [`replay_live`] started
//     clearing it, whatever the developer last typed at a `:` prompt).
//
// ## Gate 1 — the recorded behaviour must match the live engine
//
// Every non-Skipped row carries a [`Live`] recording: real keystrokes over a
// real buffer, plus the buffer, cursor and `engine.message` vimcode produced
// when the slice ran. [`regmark_audit_matches_the_live_engine`] replays all
// of them and diffs. That is the bidirectional half — implementing `m[` or
// `:marks {arg}` changes its recording and fails the gate until the row is
// re-tagged, and a row claiming Implemented for something that starts
// refusing fails immediately.
//
// The recording is a black-box observation (keys in, rendered buffer +
// cursor + message out), never an engine field: a gate that asserted
// `engine.marks` was populated would have passed throughout the `` `[ ``
// finding above, since the field is written — just not by `p`.
//
// ## Gate 2 — oracle coverage, same shrink-only shape as #1007
//
// Every non-Skipped row also carries a [`Probe`] naming the oracle case that
// exercises it; [`REGMARK_COVERAGE_EXEMPT`] lists the ones no case reaches
// today. Both directions fail, exactly as in `COVERAGE_EXEMPT`. Writing the
// missing cases is #1162's job, not this slice's.
//
// ## Out of scope, deliberately
//
// `q{reg}` / `@{reg}` macro recording and playback read and write registers,
// but they are documented under `:help complex-repeat`, a different `:help`
// area, and the corpus covers them under its own `mac:` prefix. `:help
// jump-motions` (`<C-o>`/`<C-i>`/`g;`/`g,`/`:jumps`) likewise belongs to
// motion.txt §8, not §7, even though the corpus files its cases under
// `jump:` alongside the mark ones.
// ═══════════════════════════════════════════════════════════════════════════

/// Which `:help` area a row was walked from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RmArea {
    /// `:help registers` — change.txt, "Registers".
    Registers,
    /// `:help mark-motions` — motion.txt §7, "Marks".
    Marks,
}

/// Whether the recorded run ended with vimcode printing a refusal.
///
/// A recorded, re-measured column rather than a derived bool, because for a
/// ❌ row "silent" is a strictly worse failure than "says so": a refusal
/// sends the user to `:help`, a silent no-op looks like the command worked.
/// Eight of this slice's 32 ❌ rows are silent — see
/// [`regmark_audit_is_internally_consistent`], which pins that count.
///
/// Note a few ✅ rows are `Refuses` too, because their recording ends with a
/// deliberate probe for absence (`:delmarks a` … `'a` → "Mark 'a' not set").
/// The column describes the recording, not the verdict; the verdict is
/// `status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Report {
    /// vimcode printed a refusal — an `E…` code, "Not an editor command",
    /// "Mark … not set", "Marks must be …", "No previous …", "… is not
    /// implemented", "E471: Argument required".
    Refuses,
    /// vimcode printed nothing, or printed an ordinary informational message
    /// (`:marks`' listing, "3 lines yanked").
    Silent,
}

/// The needles that make a `self.message` a refusal rather than a status
/// line. Explicit and greppable: [`measured_report`] must not be allowed to
/// quietly reclassify a row because a message was reworded.
const REFUSAL_NEEDLES: &[&str] = &[
    "Not an editor command",
    "not set",
    "Marks must be",
    "No previous",
    "is not implemented",
    "Argument required",
    "Invalid argument",
    "no match for",
    "Unknown option",
    "E20",
    "E29",
    "E30",
    "E471",
    "E475",
    "E486",
];

fn measured_report(message: &str) -> Report {
    if REFUSAL_NEEDLES.iter().any(|n| message.contains(n)) {
        Report::Refuses
    } else {
        Report::Silent
    }
}

/// One black-box observation of what vimcode does **today**: keys in,
/// rendered buffer + cursor + message out. Replayed by gate 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Live {
    /// Starting buffer, one `&str` per line.
    lines: &'static [&'static str],
    /// Starting cursor, 1-indexed `(line, col)`.
    at: (usize, usize),
    /// Keys to send, in the corpus's `<C-r>`/`<Esc>`/`<CR>` notation.
    keys: &'static str,
    /// Resulting buffer, lines joined with `|`.
    buffer: &'static str,
    /// Resulting cursor, 1-indexed `(line, col)`.
    cursor: (usize, usize),
    /// `engine.message` afterwards, verbatim; `""` when vimcode said nothing.
    message: &'static str,
}

const fn live(
    lines: &'static [&'static str],
    at: (usize, usize),
    keys: &'static str,
    buffer: &'static str,
    cursor: (usize, usize),
    message: &'static str,
) -> Live {
    Live {
        lines,
        at,
        keys,
        buffer,
        cursor,
        message,
    }
}

struct RegMarkAudit {
    area: RmArea,
    /// The command/register as `:help` writes it — the table's unique key.
    item: &'static str,
    /// The `:help` tag it is documented under.
    help: &'static str,
    status: OptStatus,
    /// Whether vimcode reports a refusal; re-measured by gate 1 from `live`.
    report: Report,
    /// `Some` for every non-Skipped row.
    live: Option<Live>,
    /// Oracle probe; `Some` for every non-Skipped row.
    probe: Option<Probe>,
    /// For ❌: what Vim does, plus this slice's assessment of whether it is
    /// worth implementing. For 🟡: exactly what is missing.
    note: &'static str,
}

#[allow(clippy::too_many_arguments)]
const fn rm(
    area: RmArea,
    item: &'static str,
    help: &'static str,
    status: OptStatus,
    report: Report,
    live: Option<Live>,
    probe: Option<Probe>,
    note: &'static str,
) -> RegMarkAudit {
    RegMarkAudit {
        area,
        item,
        help,
        status,
        report,
        live,
        probe,
        note,
    }
}

use crate::Report::{Refuses, Silent};
use crate::RmArea::{Marks, Registers};

/// Every command in `:help registers` and `:help mark-motions`, in `:help`
/// order, registers first.
///
/// 89 rows, no "TODO" and no unreviewed row: adding one, deleting one, or
/// leaving one without a note fails [`regmark_audit_is_internally_consistent`].
const REGMARK_AUDIT: &[RegMarkAudit] = &[
    // ── 1. The unnamed register ───────────────────────────────────────────
    rm(
        Registers,
        "\"\"",
        "quotequote",
        Implemented,
        Silent,
        Some(live(
            &["alpha", "beta"],
            (1, 1),
            "yyj\"\"p",
            "alpha|beta|alpha|",
            (3, 1),
            "",
        )),
        Some(Label("reg:yiw viwp swaps")),
        "filled by every yank/delete; `p` with no register reads it",
    ),
    // ── 2. Numbered registers "0 to "9 ────────────────────────────────────
    rm(
        Registers,
        "\"0",
        "quote0",
        Implemented,
        Silent,
        Some(live(
            &["alpha", "beta"],
            (1, 1),
            "yyjdd\"0p",
            "alpha|alpha|",
            (2, 1),
            "",
        )),
        Some(Label("reg:yy dd \"0p")),
        "most recent yank; untouched by deletes and by yanks to a named register",
    ),
    rm(
        Registers,
        "\"1",
        "quote1",
        Implemented,
        Silent,
        Some(live(
            &["a", "b", "c"],
            (1, 1),
            "ddj\"1p",
            "b|c|a|",
            (3, 1),
            "",
        )),
        Some(Label("reg:dd \"1p")),
        "most recent delete/change of at least one line",
    ),
    rm(
        Registers,
        "\"1 (special-motion exception)",
        "quote_number",
        Implemented,
        Silent,
        Some(live(&["(ab) cd"], (1, 1), "d%$\"1p", " cd(ab)", (1, 7), "")),
        Some(Label("reg:d% goes to \"1")),
        "`d%`/`d/`/`dn` use \"1 even when the deleted text is under one line \
        (Engine::set_delete_register_special_motion)",
    ),
    rm(
        Registers,
        "\"2 to \"9",
        "quote_number",
        Implemented,
        Silent,
        Some(live(
            &["a", "b", "c"],
            (1, 1),
            "dddd\"2p",
            "c|a|",
            (2, 1),
            "",
        )),
        Some(Label("reg:dd dd \"2p")),
        "the shift chain: each new line-delete pushes \"1 into \"2, \"8 into \"9",
    ),
    // ── 3. The small delete register ──────────────────────────────────────
    rm(
        Registers,
        "\"-",
        "quote-",
        Implemented,
        Silent,
        Some(live(&["foo bar"], (1, 1), "dw$\"-p", "barfoo ", (1, 7), "")),
        Some(Label("reg:dw goes to \"-")),
        "sub-line deletes, and only for an unnamed delete",
    ),
    // ── 4. Named registers ────────────────────────────────────────────────
    rm(
        Registers,
        "\"a to \"z",
        "quotea",
        Implemented,
        Silent,
        Some(live(
            &["alpha", "beta"],
            (1, 1),
            "\"ayyj\"ap",
            "alpha|beta|alpha|",
            (3, 1),
            "",
        )),
        Some(Label("reg:\"ayy \"ap")),
        "explicit named register, replacing its contents",
    ),
    rm(
        Registers,
        "\"A to \"Z",
        "quote_alpha",
        Implemented,
        Silent,
        Some(live(
            &["a", "b"],
            (1, 1),
            "\"ayyj\"Ayy\"ap",
            "a|b|a|b|",
            (3, 1),
            "",
        )),
        Some(Label("reg:\"Ayy linewise append")),
        "uppercase appends; a linewise append widens a charwise register to linewise",
    ),
    // ── 5. Read-only registers ":, "., "% ────────────────────────────────
    rm(
        Registers,
        "\":",
        "quote:",
        Implemented,
        Silent,
        Some(live(
            &["a a"],
            (1, 1),
            ":s/a/b/<CR>\":p",
            "bs/a/b/ a",
            (1, 7),
            "",
        )),
        Some(Label("reg:\": last cmd")),
        "most recent command-line, stored without its leading `:`",
    ),
    rm(
        Registers,
        "\".",
        "quote.",
        Implemented,
        Silent,
        Some(live(
            &["ab"],
            (1, 1),
            "ifoo<Esc>\".p",
            "foofooab",
            (1, 6),
            "",
        )),
        Some(Label("reg:\". insert register")),
        "last inserted text",
    ),
    rm(
        Registers,
        "\"%",
        "quote%",
        Partial,
        Silent,
        Some(live(&["a"], (1, 1), "\"%p", "a", (1, 1), "")),
        Some(Label("reg:\"% file name empty")),
        "returns the file's BASENAME; Vim returns the name of the file as typed \
        (`src/main.rs`, not `main.rs`) — see \
        register_percent_and_hash_paste_basenames_not_paths",
    ),
    // ── 6. Alternate buffer register "# ──────────────────────────────────
    rm(
        Registers,
        "\"#",
        "quote#",
        Partial,
        Silent,
        Some(live(&["a"], (1, 1), "\"#p", "a", (1, 1), "")),
        Some(Label("reg:\"# alternate file")),
        "#1161 added the read path, but it yields the basename rather than the \
        name as typed, and \"# is read-only here (Vim allows `:let @# = bufnr`, \
        which is VimScript and out of scope anyway)",
    ),
    // ── 7. The expression register ────────────────────────────────────────
    rm(
        Registers,
        "\"=",
        "quote=",
        Partial,
        Refuses,
        Some(live(
            &["a"],
            (1, 1),
            "\"='hi'<CR>p",
            "a",
            (1, 1),
            "\"=\" register (expression evaluation) is not implemented",
        )),
        Some(Label("reg:\"= expr")),
        "integer arithmetic only (`\"=1+1`); strings, functions and variables \
        report \"not implemented\" — a full one needs VimScript eval",
    ),
    // ── 8. The selection registers ────────────────────────────────────────
    rm(
        Registers,
        "\"*",
        "quotestar",
        Implemented,
        Silent,
        Some(live(&["abc"], (1, 1), "\"*yl$\"*p", "abca", (1, 4), "")),
        Some(Label("reg:\"* clipboard round trip")),
        "writes through Engine::clipboard_write when a backend supplied one, \
        falling back to the internal register otherwise",
    ),
    rm(
        Registers,
        "\"+",
        "quoteplus",
        Implemented,
        Silent,
        Some(live(&["abc"], (1, 1), "\"+yl$\"+p", "abca", (1, 4), "")),
        Some(Label("reg:\"+ clipboard round trip")),
        "same path as \"*",
    ),
    // ── 9. The black hole register ────────────────────────────────────────
    rm(
        Registers,
        "\"_",
        "quote_",
        Implemented,
        Silent,
        Some(live(
            &["a", "b", "c"],
            (1, 1),
            "yyj\"_ddp",
            "a|c|a|",
            (3, 1),
            "",
        )),
        Some(Label("reg:\"_dd then p")),
        "writes vanish and do not fall through to \"\" / \"1 / \"-",
    ),
    // ── 10. Last search pattern register ──────────────────────────────────
    rm(
        Registers,
        "\"/",
        "quote/",
        Implemented,
        Silent,
        Some(live(
            &["foo bar"],
            (1, 1),
            "/bar<CR>\"/P",
            "foo barbar",
            (1, 7),
            "",
        )),
        Some(Label("reg:\"/ last search")),
        "read path only; Vim's `:let @/ = \"the\"` write is VimScript",
    ),
    rm(
        Registers,
        "\"~",
        "quote_~",
        Skipped(VIMGUI),
        Silent,
        None,
        None,
        "Vim's drag-and-drop register (not in Neovim's help at all); there is no \
        text-drop path in either vimcode backend",
    ),
    // ── Commands that address a register ──────────────────────────────────
    rm(
        Registers,
        "[\"x]{operator}",
        "{register}",
        Implemented,
        Silent,
        Some(live(
            &["a", "b", "c"],
            (1, 1),
            "\"add\"bdd\"ap\"bp",
            "c|a|b|",
            (3, 1),
            "",
        )),
        Some(Label("reg:\"add \"bdd \"ap \"bp")),
        "the `\"x` prefix in front of y/d/c/s/x/p/P",
    ),
    rm(
        Registers,
        ":reg[isters]",
        ":registers",
        Implemented,
        Silent,
        Some(live(
            &["a"],
            (1, 1),
            "yy:registers<CR>",
            "a",
            (1, 1),
            "--- Registers ---\n\"\"  l  a\\n\n\"0  l  a\\n",
        )),
        Some(Keys(":registers")),
        "lists every non-empty register with Vim's c/l/b type column",
    ),
    rm(
        Registers,
        ":reg[isters] {arg}",
        ":registers",
        NotImplemented,
        Refuses,
        Some(live(
            &["a"],
            (1, 1),
            "\"ayy:reg a<CR>",
            "a",
            (1, 1),
            "Not an editor command: registers a",
        )),
        Some(Keys(":reg a")),
        "Vim filters the listing to the named registers (`:reg 1a`); vimcode \
        rejects any argument outright — worth implementing, it is a filter over \
        a list execute.rs already builds",
    ),
    rm(
        Registers,
        ":di[splay]",
        ":display",
        Implemented,
        Silent,
        Some(live(
            &["a"],
            (1, 1),
            "yy:display<CR>",
            "a",
            (1, 1),
            "--- Registers ---\n\"\"  l  a\\n\n\"0  l  a\\n",
        )),
        Some(Keys(":display")),
        "synonym for :registers, same listing",
    ),
    rm(
        Registers,
        ":di[splay] {arg}",
        ":display",
        NotImplemented,
        Refuses,
        Some(live(
            &["a"],
            (1, 1),
            "\"ayy:di a<CR>",
            "a",
            (1, 1),
            "Not an editor command: display a",
        )),
        Some(Keys(":di a")),
        "same gap as `:reg {arg}` and the same one-line fix",
    ),
    rm(
        Registers,
        ":[range]pu[t] [x]",
        ":put",
        Implemented,
        Silent,
        Some(live(
            &["a", "b"],
            (1, 1),
            "\"ayyj:put a<CR>",
            "a|b|a|",
            (3, 1),
            "",
        )),
        Some(Label("ex:put a")),
        "linewise put below the addressed line, with an optional register name",
    ),
    rm(
        Registers,
        ":[range]pu[t]!",
        ":put!",
        Implemented,
        Silent,
        Some(live(
            &["a", "b"],
            (2, 1),
            "yy:put!<CR>",
            "a|b|b",
            (2, 1),
            "",
        )),
        Some(Keys(":put!")),
        "the `!` variant puts above the addressed line",
    ),
    rm(
        Registers,
        ":put ={expr}",
        ":put_=",
        NotImplemented,
        Refuses,
        Some(live(
            &["a"],
            (1, 1),
            ":put =1+1<CR>",
            "a",
            (1, 1),
            "Not an editor command: put =1+1",
        )),
        Some(Keys(":put =")),
        "Vim evaluates the expression and puts the result; vimcode rejects the \
        whole command — worth implementing, `Engine::eval_expr_register` already \
        evaluates exactly what `\"=` accepts",
    ),
    rm(
        Registers,
        "i_CTRL-R {register}",
        "i_CTRL-R",
        Implemented,
        Silent,
        Some(live(
            &["foo bar"],
            (1, 1),
            "\"aywA<C-r>a<Esc>",
            "foo barfoo ",
            (1, 11),
            "",
        )),
        Some(Label("reg:i C-r a")),
        "inserts the register at the cursor, \"as if typed\"",
    ),
    rm(
        Registers,
        "i_CTRL-R =",
        "i_CTRL-R_=",
        Partial,
        Silent,
        Some(live(
            &["a"],
            (1, 1),
            "A<C-r>=2*3<CR><Esc>",
            "a6",
            (1, 2),
            "",
        )),
        Some(Label("reg:C-r = in insert")),
        "opens the expression prompt, but shares `\"=`'s arithmetic-only evaluator",
    ),
    rm(
        Registers,
        "i_CTRL-R CTRL-R {register}",
        "i_CTRL-R_CTRL-R",
        NotImplemented,
        Silent,
        Some(live(
            &["foo bar"],
            (1, 1),
            "ywA<C-r><C-r>\"<Esc>",
            "foo barfoo ",
            (1, 11),
            "",
        )),
        Some(Keys("<C-r><C-r>")),
        "Vim inserts the register LITERALLY (no 'textwidth'/abbreviation/indent \
        processing); vimcode re-arms the pending <C-r> and inserts normally, so \
        the distinction is silently lost — low value while vimcode's plain \
        i_CTRL-R already inserts unprocessed",
    ),
    rm(
        Registers,
        "i_CTRL-R CTRL-O {register}",
        "i_CTRL-R_CTRL-O",
        NotImplemented,
        Silent,
        Some(live(
            &["foo bar"],
            (1, 1),
            "ywA<C-r><C-o>\"<Esc>",
            "foo bar\"",
            (1, 8),
            "",
        )),
        Some(Keys("<C-r><C-o>")),
        "Vim inserts literally and without auto-indent; vimcode reads a register \
        literally NAMED `o` (empty), silently swallows the keystroke and then \
        types the register name as text — silently wrong, not just missing",
    ),
    rm(
        Registers,
        "i_CTRL-R CTRL-P {register}",
        "i_CTRL-R_CTRL-P",
        NotImplemented,
        Silent,
        Some(live(
            &["foo bar"],
            (1, 1),
            "ywA<C-r><C-p>\"<Esc>",
            "foo barfoo ",
            (1, 11),
            "",
        )),
        Some(Keys("<C-r><C-p>")),
        "Vim inserts literally and fixes the indent; vimcode's <C-p> is consumed \
        by the completion handler, leaving the pending <C-r> armed — silently wrong",
    ),
    rm(
        Registers,
        "c_CTRL-R {register}",
        "c_CTRL-R",
        NotImplemented,
        Refuses,
        Some(live(
            &["foo", "bar"],
            (1, 1),
            "\"ayw:s/bar/<C-r>a/<CR>",
            "foo|bar",
            (1, 1),
            "E486: Pattern not found: bar",
        )),
        Some(Keys(":s/bar/<C-r>a/")),
        "the single biggest gap in this slice: vimcode binds command-line <C-r> to \
        a readline-style reverse-i-search over command history, so `:s/x/<C-r>a/` \
        never pastes register a. Worth implementing, and it needs the existing \
        binding moved or dropped",
    ),
    rm(
        Registers,
        "c_CTRL-R CTRL-W",
        "c_CTRL-R_CTRL-W",
        NotImplemented,
        Refuses,
        Some(live(
            &["foo", "bar"],
            (1, 1),
            ":s/bar/<C-r><C-w>/<CR>",
            "foo|bar",
            (1, 1),
            "E486: Pattern not found: bar",
        )),
        Some(Keys("<C-r><C-w>")),
        "insert the word under the cursor on the command line — blocked behind the \
        same c_CTRL-R binding conflict; very commonly used with `:s`",
    ),
    rm(
        Registers,
        ":let @{register} = {expr}",
        ":let-@",
        Skipped(VIMSCRIPT),
        Silent,
        None,
        None,
        "the register write path is a `:let` assignment — VimScript, per the \
        standing decision that vimcode implements Vim keybindings and editing",
    ),
    // ── mark-motions: setting marks ───────────────────────────────────────
    rm(
        Marks,
        "m{a-zA-Z}",
        "m",
        Implemented,
        Silent,
        Some(live(
            &["  a", "b", "c"],
            (1, 3),
            "majj'a",
            "  a|b|c",
            (1, 3),
            "",
        )),
        Some(Label("mark:'a first nonblank")),
        "a-z per buffer (Engine::marks), A-Z global with a file path",
    ),
    rm(
        Marks,
        "m' and m`",
        "m'",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b", "c", "d", "e"],
            (3, 1),
            "m'jj''",
            "a|b|c|d|e",
            (5, 1),
            "No previous jump position",
        )),
        Some(Label("mark:m'")),
        "set the previous-context mark without moving. vimcode answers \"Marks \
        must be a letter (a-z or A-Z)\" — worth implementing, it is one `'`/`` ` `` \
        arm writing `last_jump_pos`",
    ),
    rm(
        Marks,
        "m[ and m]",
        "m[",
        NotImplemented,
        Refuses,
        Some(live(
            &["abc", "def"],
            (2, 2),
            "m[gg'[",
            "abc|def",
            (1, 2),
            "No previous change",
        )),
        Some(Label("mark:m[")),
        "set `'[`/`']` by hand, for simulating an operator with several commands \
        — niche, but it is the same two fields the `'[` gap below already needs",
    ),
    rm(
        Marks,
        "m< and m>",
        "m<",
        NotImplemented,
        Refuses,
        Some(live(
            &["abc", "def"],
            (2, 2),
            "m<gg'<",
            "abc|def",
            (1, 2),
            "No previous visual selection",
        )),
        Some(Label("mark:m<")),
        "set `'<`/`'>` to change what `gv` reselects — worth implementing; \
        visual_mark_start/end already exist and `gv` already reads them",
    ),
    rm(
        Marks,
        ":[range]ma[rk] {a-zA-Z'}",
        ":mark",
        Implemented,
        Silent,
        Some(live(
            &["a", "b", "c"],
            (1, 1),
            ":2mark t<CR>gg't",
            "a|b|c",
            (2, 1),
            "",
        )),
        Some(Label("ex:2mark a")),
        "range-addressed mark, column 0, default cursor line",
    ),
    rm(
        Marks,
        ":[range]k{a-zA-Z'}",
        ":k",
        Implemented,
        Silent,
        Some(live(
            &["a", "b", "c"],
            (1, 1),
            ":2kt<CR>gg't",
            "a|b|c",
            (2, 1),
            "",
        )),
        Some(Keys(":2kt")),
        "the no-space spelling of :mark",
    ),
    // ── mark-motions: jumping to a mark ───────────────────────────────────
    rm(
        Marks,
        "'{a-z}",
        "'a",
        Implemented,
        Silent,
        Some(live(
            &["  a", "b", "c"],
            (1, 3),
            "majj'a",
            "  a|b|c",
            (1, 3),
            "",
        )),
        Some(Label("mark:'a first nonblank")),
        "linewise, lands on the first non-blank, records a jumplist entry",
    ),
    rm(
        Marks,
        "`{a-z}",
        "`a",
        Implemented,
        Silent,
        Some(live(
            &["abc", "def", "ghi"],
            (1, 3),
            "majj`a",
            "abc|def|ghi",
            (1, 3),
            "",
        )),
        Some(Label("mark:`a exact")),
        "exclusive, lands on the exact column",
    ),
    rm(
        Marks,
        "'{A-Z}",
        "'A",
        Implemented,
        Silent,
        Some(live(
            &["a", "b", "c"],
            (3, 1),
            "mAgg'A",
            "a|b|c",
            (3, 1),
            "",
        )),
        Some(Label("mark:mA global")),
        "global file mark, linewise",
    ),
    rm(
        Marks,
        "`{A-Z}",
        "`A",
        Implemented,
        Silent,
        Some(live(
            &["abc", "def"],
            (2, 3),
            "mAgg`A",
            "abc|def",
            (2, 3),
            "",
        )),
        Some(Label("mark:`A")),
        "global file mark, exact column",
    ),
    rm(
        Marks,
        "'{0-9} and `{0-9}",
        "'0",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b"],
            (1, 1),
            "'0",
            "a|b",
            (1, 1),
            "Marks must be a letter or special char",
        )),
        Some(Label("mark:'0")),
        "the shada/viminfo marks — where the cursor was when Vim last exited. \
        vimcode has no shada file, so there is nothing to restore; not worth \
        implementing before a persistent-mark store exists",
    ),
    rm(
        Marks,
        "lowercase marks restored by undo/redo",
        "mark-motions",
        NotImplemented,
        Silent,
        Some(live(
            &["a", "b", "c"],
            (2, 1),
            "maggddu3G'a",
            "a|b|c",
            (1, 1),
            "",
        )),
        Some(Label("mark:undo restores mark")),
        "`:help mark-motions`: \"Lowercase marks are restored when using undo and \
        redo.\" vimcode leaves the mark where the delete shifted it. The offset \
        snapshot/restore machinery exists (Engine::snapshot_marks_as_offsets) but \
        is wired only to join_lines — worth implementing",
    ),
    rm(
        Marks,
        "mark erased when its line is deleted",
        "mark-motions",
        Implemented,
        Refuses,
        Some(live(
            &["a", "b", "c"],
            (2, 1),
            "maddgg'a",
            "a|c",
            (1, 1),
            "Mark 'a' not set",
        )),
        Some(Label("mark:mark on deleted line")),
        "\"If you delete a line that contains a mark, that mark is erased.\"",
    ),
    rm(
        Marks,
        "g'{mark}",
        "g'",
        Partial,
        Silent,
        Some(live(
            &["a", "    bcd", "e"],
            (2, 5),
            "maggg'a",
            "a|    bcd|e",
            (2, 1),
            "",
        )),
        Some(Label("mark:g'")),
        "keeps the jumplist untouched (correct), but lands on COLUMN 0 instead of \
        the first non-blank `'{mark}` uses, and accepts only a-zA-Z — Vim's \
        `` g`\" `` / `g'.` take any mark",
    ),
    rm(
        Marks,
        "g`{mark}",
        "g`",
        Partial,
        Silent,
        Some(live(
            &["abc", "def"],
            (2, 2),
            "maggg`a",
            "abc|def",
            (2, 2),
            "",
        )),
        Some(Label("mark:g`")),
        "correct for a-zA-Z; silently does nothing for the special marks \
        (`` g`\" `` is the canonical last-position-jump idiom)",
    ),
    rm(
        Marks,
        ":marks",
        ":marks",
        Partial,
        Silent,
        Some(live(
            &["a", "b"],
            (1, 1),
            "ma:marks<CR>",
            "a|b",
            (1, 1),
            "mark line  col  file/text\n a      1    0",
        )),
        Some(Keys(":marks")),
        "lists only the letter marks; Vim also lists `' \" [ ] < > . ^` and a \
        file/text column, and numbers the first column from zero",
    ),
    rm(
        Marks,
        ":marks {arg}",
        ":marks",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b"],
            (1, 1),
            "ma:marks a<CR>",
            "a|b",
            (1, 1),
            "Not an editor command: marks a",
        )),
        Some(Keys(":marks a")),
        "Vim filters the listing to the named marks (`:marks aB`); vimcode rejects \
        any argument — same shape and same one-line fix as `:reg {arg}`",
    ),
    rm(
        Marks,
        ":delm[arks] {marks}",
        ":delmarks",
        Implemented,
        Refuses,
        Some(live(
            &["a", "b", "c"],
            (2, 1),
            "ma:delmarks a<CR>gg'a",
            "a|b|c",
            (1, 1),
            "Mark 'a' not set",
        )),
        Some(Label("ex:delmarks a")),
        "#1154; handles space-separated names and `a-c` ranges, with E475 on an \
        inverted or mixed-category range",
    ),
    rm(
        Marks,
        ":delm[arks]!",
        ":delmarks",
        Implemented,
        Refuses,
        Some(live(
            &["a", "b", "c"],
            (2, 1),
            "ma:delmarks!<CR>gg'a",
            "a|b|c",
            (1, 1),
            "Mark 'a' not set",
        )),
        Some(Keys(":delmarks!")),
        "#1154; clears the buffer's lowercase marks, leaving A-Z and 0-9",
    ),
    // ── mark-motions: the special marks ───────────────────────────────────
    rm(
        Marks,
        "'[",
        "'[",
        Partial,
        Refuses,
        Some(live(
            &["abc", "def"],
            (2, 1),
            "yygg'[",
            "abc|def",
            (1, 1),
            "No previous change",
        )),
        Some(Label("mark:'[ after >>")),
        "set by `>>`/`<<` and by charwise yanks, but NOT by `c`, `d`, `p` or a \
        linewise `yy` — after any of those vimcode answers \"No previous change\"",
    ),
    rm(
        Marks,
        "`[",
        "`[",
        Partial,
        Refuses,
        Some(live(
            &["abc def"],
            (1, 1),
            "ciwXY<Esc>$`[",
            "XY def",
            (1, 6),
            "No previous change",
        )),
        Some(Label("mark:`[ after p")),
        "same gap. Note `mark:`[ after p` in the corpus passes only by \
        coincidence — the mark is unset and the cursor happens to already be \
        where Vim would put it",
    ),
    rm(
        Marks,
        "']",
        "']",
        Partial,
        Refuses,
        Some(live(
            &["abc", "def"],
            (1, 1),
            "yjG']",
            "abc|def",
            (2, 1),
            "No previous change",
        )),
        Some(Label("mark:']")),
        "same gap as `'[`",
    ),
    rm(
        Marks,
        "`]",
        "`]",
        Partial,
        Refuses,
        Some(live(
            &["abc def"],
            (1, 1),
            "ciwXY<Esc>0`]",
            "XY def",
            (1, 1),
            "No previous change",
        )),
        Some(Label("mark:`] after yank")),
        "correct after a yank; unset after `c`/`d`/`p`",
    ),
    rm(
        Marks,
        "'<",
        "'<",
        Implemented,
        Silent,
        Some(live(
            &["a", "b", "c", "d"],
            (2, 1),
            "Vj<Esc>gg'<",
            "a|b|c|d",
            (2, 1),
            "",
        )),
        Some(Label("mark:'<")),
        "start of the last visual area, linewise",
    ),
    rm(
        Marks,
        "`<",
        "`<",
        Implemented,
        Silent,
        Some(live(
            &["abc", "def"],
            (1, 2),
            "vjl<Esc>gg`<",
            "abc|def",
            (1, 2),
            "",
        )),
        Some(Label("mark:`< after v")),
        "start of the last visual area, exact",
    ),
    rm(
        Marks,
        "'>",
        "'>",
        Implemented,
        Silent,
        Some(live(
            &["a", "b", "c", "d"],
            (2, 1),
            "Vj<Esc>gg'>",
            "a|b|c|d",
            (3, 1),
            "",
        )),
        Some(Label("mark:'> after V")),
        "end of the last visual area, linewise",
    ),
    rm(
        Marks,
        "`>",
        "`>",
        Implemented,
        Silent,
        Some(live(
            &["abc"],
            (1, 1),
            "vl<Esc>0gv<Esc>`>",
            "abc",
            (1, 2),
            "",
        )),
        Some(Label("mark:`> after gv")),
        "end of the last visual area, exact; survives a `gv` round trip",
    ),
    rm(
        Marks,
        "''",
        "''",
        Implemented,
        Silent,
        Some(live(
            &["a", "b", "c", "d"],
            (2, 1),
            "G''",
            "a|b|c|d",
            (2, 1),
            "",
        )),
        Some(Label("mark:'' after G")),
        "position before the latest jump, and itself a jump, so it toggles",
    ),
    rm(
        Marks,
        "``",
        "``",
        Implemented,
        Silent,
        Some(live(
            &["abc", "def", "ghi"],
            (2, 2),
            "gg``",
            "abc|def|ghi",
            (2, 2),
            "",
        )),
        Some(Label("mark:`` after gg")),
        "exact-column sibling of `''`, toggles the same way",
    ),
    rm(
        Marks,
        "'\"",
        "'quote",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b", "c"],
            (2, 1),
            "'\"",
            "a|b|c",
            (2, 1),
            "Marks must be a letter or special char",
        )),
        Some(Label("mark:'\"")),
        "cursor position when the buffer was last exited, defaulting to line 1. \
        Worth implementing: it is the mark behind the near-universal \
        last-position-jump idiom, and it is per-buffer state vimcode already keeps",
    ),
    rm(
        Marks,
        "`\"",
        "`quote",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b", "c"],
            (2, 1),
            "`\"",
            "a|b|c",
            (2, 1),
            "Marks must be a letter or special char",
        )),
        Some(Label("mark:`\"")),
        "exact-column sibling of `'\"`, and the one `` g`\" `` needs",
    ),
    rm(
        Marks,
        "'^",
        "'^",
        Partial,
        Silent,
        Some(live(
            &["x", "    yz"],
            (1, 1),
            "jA!<Esc>gg'^",
            "x|    yz!",
            (2, 7),
            "",
        )),
        Some(Label("mark:'^")),
        "jumps to the right LINE but keeps the exact column; Vim's `'^` is \
        linewise and lands on the first non-blank (verified against the pinned \
        oracle: col 5 vs vimcode's col 7 on `    yz!`)",
    ),
    rm(
        Marks,
        "`^",
        "`^",
        Implemented,
        Silent,
        Some(live(
            &["ab", "cd"],
            (1, 1),
            "jAx<Esc>gg`^",
            "ab|cdx",
            (2, 3),
            "",
        )),
        Some(Label("mark:`^")),
        "where Insert mode was last left, raw column — what `gi` uses",
    ),
    rm(
        Marks,
        "'.",
        "'.",
        Implemented,
        Silent,
        Some(live(&["a", "b", "c"], (2, 1), "xgg'.", "a||c", (2, 1), "")),
        Some(Label("mark:'.")),
        "line of the last change, first non-blank",
    ),
    rm(
        Marks,
        "`.",
        "`.",
        Implemented,
        Silent,
        Some(live(&["abc", "def"], (2, 2), "xgg`.", "abc|df", (2, 2), "")),
        Some(Label("mark:`.")),
        "exact position of the last change",
    ),
    rm(
        Marks,
        "':",
        "':",
        Skipped(PROMPTBUF),
        Silent,
        None,
        None,
        "Neovim-only: the start of the current user input in a prompt buffer",
    ),
    rm(
        Marks,
        "'(",
        "'(",
        NotImplemented,
        Refuses,
        Some(live(
            &["One two. Three four."],
            (1, 12),
            "'(",
            "One two. Three four.",
            (1, 12),
            "Marks must be a letter or special char",
        )),
        Some(Label("mark:'(")),
        "start of the current sentence, like `(`. Worth implementing — the whole \
        family below is a thin alias layer over motions vimcode already has",
    ),
    rm(
        Marks,
        "`(",
        "`(",
        NotImplemented,
        Refuses,
        Some(live(
            &["One two. Three four."],
            (1, 12),
            "`(",
            "One two. Three four.",
            (1, 12),
            "Marks must be a letter or special char",
        )),
        Some(Label("mark:`(")),
        "exact-column form of `'(`",
    ),
    rm(
        Marks,
        "')",
        "')",
        NotImplemented,
        Refuses,
        Some(live(
            &["One two. Three four."],
            (1, 3),
            "')",
            "One two. Three four.",
            (1, 3),
            "Marks must be a letter or special char",
        )),
        Some(Label("mark:')")),
        "end of the current sentence, like `)`",
    ),
    rm(
        Marks,
        "`)",
        "`)",
        NotImplemented,
        Refuses,
        Some(live(
            &["One two. Three four."],
            (1, 3),
            "`)",
            "One two. Three four.",
            (1, 3),
            "Marks must be a letter or special char",
        )),
        Some(Label("mark:`)")),
        "exact-column form of `')`",
    ),
    rm(
        Marks,
        "'{",
        "'{",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b", "", "c"],
            (2, 1),
            "'{",
            "a|b||c",
            (2, 1),
            "Marks must be a letter or special char",
        )),
        Some(Label("mark:'{")),
        "start of the current paragraph, like `{`",
    ),
    rm(
        Marks,
        "`{",
        "`{",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b", "", "c"],
            (2, 1),
            "`{",
            "a|b||c",
            (2, 1),
            "Marks must be a letter or special char",
        )),
        Some(Label("mark:`{")),
        "exact-column form of `'{`",
    ),
    rm(
        Marks,
        "'}",
        "'}",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b", "", "c"],
            (1, 1),
            "'}",
            "a|b||c",
            (1, 1),
            "Marks must be a letter or special char",
        )),
        Some(Label("mark:'}")),
        "end of the current paragraph, like `}`",
    ),
    rm(
        Marks,
        "`}",
        "`}",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b", "", "c"],
            (1, 1),
            "`}",
            "a|b||c",
            (1, 1),
            "Marks must be a letter or special char",
        )),
        Some(Label("mark:`}")),
        "exact-column form of `'}`",
    ),
    // ── mark-motions: commands that jump BETWEEN marks ────────────────────
    rm(
        Marks,
        "]'",
        "]'",
        NotImplemented,
        Silent,
        Some(live(
            &["a", "b", "  c", "d"],
            (1, 1),
            "3Gmagg]'",
            "a|b|  c|d",
            (1, 1),
            "",
        )),
        Some(Label("mark:]'")),
        "[count] times to the next line with a lowercase mark, first non-blank. \
        Silent no-op today — worth implementing as a set with the three below",
    ),
    rm(
        Marks,
        "]`",
        "]`",
        NotImplemented,
        Silent,
        Some(live(
            &["a", "b", "cde", "d"],
            (1, 1),
            "3Gllmagg]`",
            "a|b|cde|d",
            (1, 1),
            "",
        )),
        Some(Label("mark:]`")),
        "[count] times to the next lowercase mark, exact column — silent no-op",
    ),
    rm(
        Marks,
        "['",
        "['",
        NotImplemented,
        Silent,
        Some(live(
            &["a", "  b", "c", "d"],
            (1, 1),
            "2GmaG['",
            "a|  b|c|d",
            (4, 1),
            "",
        )),
        Some(Label("mark:['")),
        "backwards form of `]'` — silent no-op",
    ),
    rm(
        Marks,
        "[`",
        "[`",
        NotImplemented,
        Silent,
        Some(live(
            &["a", "bcd", "e"],
            (1, 1),
            "2GllmaG[`",
            "a|bcd|e",
            (3, 1),
            "",
        )),
        Some(Label("mark:[`")),
        "backwards form of `` ]` `` — silent no-op",
    ),
    // ── mark-motions: command modifiers and the mark view ─────────────────
    rm(
        Marks,
        ":loc[kmarks] {command}",
        ":lockmarks",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b"],
            (1, 1),
            ":lockmarks normal x<CR>",
            "a|b",
            (1, 1),
            "Not an editor command: lockmarks normal x",
        )),
        Some(Keys(":lockmarks")),
        "run a command without adjusting marks. Low value without a VimScript \
        runtime — its users are plugins doing line-count-preserving rewrites",
    ),
    rm(
        Marks,
        ":kee[pmarks] {command}",
        ":keepmarks",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b"],
            (1, 1),
            ":keepmarks normal x<CR>",
            "a|b",
            (1, 1),
            "Not an editor command: keepmarks normal x",
        )),
        Some(Keys(":keepmarks")),
        "only affects `:range!` filtering, which vimcode does not have either — \
        low value",
    ),
    rm(
        Marks,
        ":keepj[umps] {command}",
        ":keepjumps",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b", "c"],
            (1, 1),
            ":keepjumps normal G<CR>''",
            "a|b|c",
            (1, 1),
            "No previous jump position",
        )),
        Some(Keys(":keepjumps")),
        "run a command without touching `''`/the jumplist/the changelist — same \
        plugin-facing audience as :lockmarks, low value here",
    ),
    rm(
        Marks,
        "mark-view ('jumpoptions' \"view\")",
        "mark-view",
        NotImplemented,
        Refuses,
        Some(live(
            &["a", "b"],
            (1, 1),
            ":set jumpoptions=view<CR>",
            "a|b",
            (1, 1),
            "Unknown option: jumpoptions",
        )),
        Some(Keys("jumpoptions")),
        "restore the window's topline as well as the cursor when jumping to a \
        mark. Gated on the 'jumpoptions' option, tagged ❌ by the #1225 option \
        slice — low value",
    ),
    // ── mark-motions: marks as operator targets ───────────────────────────
    rm(
        Marks,
        "{operator}'{mark}",
        "mark-motions",
        Implemented,
        Silent,
        Some(live(&["abc", "def"], (2, 2), "magg0d'a", "", (1, 1), "")),
        Some(Label("mark:d'a linewise")),
        "\"Lowercase marks can be used in combination with operators\" — linewise",
    ),
    rm(
        Marks,
        "{operator}`{mark}",
        "mark-motions",
        Implemented,
        Silent,
        Some(live(
            &["abc def"],
            (1, 5),
            "ma0c`aX<Esc>",
            "Xdef",
            (1, 1),
            "",
        )),
        Some(Label("mark:c`a")),
        "the backtick form is exclusive-charwise",
    ),
    rm(
        Marks,
        "y'{mark} cursor placement",
        "mark-motions",
        Implemented,
        Silent,
        Some(live(
            &["a", "b", "c"],
            (3, 1),
            "maggy'a",
            "a|b|c",
            (1, 1),
            "3 lines yanked",
        )),
        Some(Label("mark:y'a cursor")),
        "after a linewise yank to a mark the cursor lands at the start of the range",
    ),
];

/// Oracle cases this slice's rows are not (yet) pinned by — the measurement
/// #1162 starts from. Shrink-only, exactly like [`COVERAGE_EXEMPT`]: adding a
/// case for one of these fails [`regmark_audit_oracle_coverage_is_shrink_only`]
/// until its entry is deleted.
const REGMARK_COVERAGE_EXEMPT: &[&str] = &[
    "\"#",
    "\"*",
    "\"+",
    ":reg[isters]",
    ":reg[isters] {arg}",
    ":di[splay]",
    ":di[splay] {arg}",
    ":[range]pu[t]!",
    ":put ={expr}",
    "i_CTRL-R CTRL-R {register}",
    "i_CTRL-R CTRL-O {register}",
    "i_CTRL-R CTRL-P {register}",
    "c_CTRL-R {register}",
    "c_CTRL-R CTRL-W",
    "m' and m`",
    "m[ and m]",
    "m< and m>",
    ":[range]k{a-zA-Z'}",
    "`{A-Z}",
    "'{0-9} and `{0-9}",
    "lowercase marks restored by undo/redo",
    "g'{mark}",
    "g`{mark}",
    ":marks",
    ":marks {arg}",
    ":delm[arks]!",
    "']",
    "'<",
    "'\"",
    "`\"",
    "'^",
    "'(",
    "`(",
    "')",
    "`)",
    "'{",
    "`{",
    "'}",
    "`}",
    "]'",
    "]`",
    "['",
    "[`",
    ":loc[kmarks] {command}",
    ":kee[pmarks] {command}",
    ":keepj[umps] {command}",
    "mark-view ('jumpoptions' \"view\")",
];

/// Replay one [`Live`] recording against a real engine. Black-box: keys in,
/// rendered buffer + cursor + message out.
fn replay_live(p: &Live) -> (String, (usize, usize), String) {
    let mut engine = engine_with(&p.lines.join("\n"));
    // `Engine::new()` loads the *user's real* command history off disk, and
    // command-line `<C-r>` searches it (that binding conflict is one of this
    // slice's findings). Leaving it populated would make the `c_CTRL-R` rows
    // replay whatever the developer last typed at a `:` prompt — a recording
    // that passes on one machine and executes a random history entry on the
    // next. `engine_with` already resets settings and extension state for the
    // same reason; history is the one it misses.
    engine.history = Default::default();
    engine.settings.shift_width = 4;
    engine.settings.expand_tab = true;
    engine.settings.tabstop = 4;
    engine.set_viewport_lines(24);
    engine.view_mut().cursor.line = p.at.0.saturating_sub(1);
    engine.view_mut().cursor.col = p.at.1.saturating_sub(1);
    engine.ensure_cursor_visible();
    send_keys(&mut engine, p.keys);
    (
        engine.buffer().to_string().replace('\n', "|"),
        (engine.view().cursor.line + 1, engine.view().cursor.col + 1),
        engine.message.clone(),
    )
}

/// Every row whose recorded behaviour no longer matches the live engine.
fn regmark_drift(audit: &'static [RegMarkAudit]) -> Vec<String> {
    let mut drift: Vec<String> = Vec::new();
    for e in audit {
        let Some(p) = e.live.as_ref() else { continue };
        let (buffer, cursor, message) = replay_live(p);
        if buffer != p.buffer || cursor != p.cursor || message != p.message {
            drift.push(format!(
                "  {:?} ({}): recorded buffer={:?} cursor={:?} message={:?}\n\
                 {:width$}   live     buffer={:?} cursor={:?} message={:?}",
                e.item,
                e.help,
                p.buffer,
                p.cursor,
                p.message,
                "",
                buffer,
                cursor,
                message,
                width = 2
            ));
        }
        let measured = measured_report(&message);
        if measured != e.report {
            drift.push(format!(
                "  {:?} ({}): table says {:?}, the live message {:?} is {:?}",
                e.item, e.help, e.report, message, measured
            ));
        }
    }
    drift
}

/// Gate 1 (#1226) — every row's recorded behaviour is a claim about live
/// code, and this replays all 86 of them (the 89 rows minus the 3 ⏭️) against
/// it. Pure: no `nvim`, no subprocess, so it runs on every lane.
#[test]
fn regmark_audit_matches_the_live_engine() {
    if let Ok(path) = std::env::var("CONFORMANCE_DUMP_REGMARK") {
        let mut s = String::new();
        for e in REGMARK_AUDIT {
            let Some(p) = e.live.as_ref() else { continue };
            let (buffer, cursor, message) = replay_live(p);
            // `{:?}` on a `&str` emits a valid Rust string literal, so a
            // message containing a real newline round-trips into the table
            // verbatim instead of being flattened into an ambiguous `\n`.
            s.push_str(&format!(
                "{}\t{:?}\t{}\t{}\t{:?}\n",
                e.item, buffer, cursor.0, cursor.1, message
            ));
        }
        std::fs::write(&path, s).unwrap_or_else(|e| panic!("dump to {path}: {e}"));
        return;
    }

    let drift = regmark_drift(REGMARK_AUDIT);
    assert!(
        drift.is_empty(),
        "\n\n== registers/marks audit drifted from the engine (#1226) ==\n\
         Each row records what vimcode actually did when the slice ran.\n\
         Implementing (or breaking) one of these changes that recording, so\n\
         re-tag the row — that is how the audit stays true instead of rotting\n\
         like a markdown checklist.\n\n{}\n\n\
         A command that gained an implementation also needs its status changed\n\
         from NotImplemented, and a REGMARK_COVERAGE_EXEMPT entry deleted once\n\
         an oracle case pins it.\n",
        drift.join("\n")
    );
}

/// Gate 1b (#1226) — the table describes itself correctly: complete, grouped
/// by `:help` area, unique, no unreviewed row, every ⏭️ carrying a reason from
/// the shared vocabulary, and a live recording + probe on exactly the rows
/// that can have one.
#[test]
fn regmark_audit_is_internally_consistent() {
    use std::collections::HashSet;
    let mut problems: Vec<String> = Vec::new();

    assert_eq!(
        REGMARK_AUDIT.len(),
        89,
        "`:help registers` + `:help mark-motions` walked to 89 entries; the \
         audit must tag all of them"
    );

    let mut seen: HashSet<&str> = HashSet::new();
    let mut in_marks = false;
    for e in REGMARK_AUDIT {
        if !seen.insert(e.item) {
            problems.push(format!("  {:?}: listed twice", e.item));
        }
        match e.area {
            RmArea::Registers if in_marks => problems.push(format!(
                "  {:?}: a `:help registers` row after the marks rows — the table \
                 is grouped by area, in `:help` order within each",
                e.item
            )),
            RmArea::Marks => in_marks = true,
            _ => {}
        }
        if e.note.trim().is_empty() {
            problems.push(format!(
                "  {:?}: empty note — every row is reviewed",
                e.item
            ));
        }
        if e.help.trim().is_empty() {
            problems.push(format!("  {:?}: no `:help` tag", e.item));
        }

        let in_scope = !matches!(e.status, OptStatus::Skipped(_));
        if let OptStatus::Skipped(reason) = e.status {
            if !SKIP_REASONS.contains(&reason) {
                problems.push(format!(
                    "  {:?}: skip reason {reason:?} is not in SKIP_REASONS",
                    e.item
                ));
            }
        }
        if in_scope && e.live.is_none() {
            problems.push(format!(
                "  {:?}: not skipped, so it must carry a live recording",
                e.item
            ));
        }
        if in_scope && e.probe.is_none() {
            problems.push(format!(
                "  {:?}: not skipped, so it must carry an oracle probe",
                e.item
            ));
        }
        if !in_scope && (e.live.is_some() || e.probe.is_some()) {
            problems.push(format!(
                "  {:?}: skipped, so a live recording or probe can never fire",
                e.item
            ));
        }
    }

    // The headline tally, pinned. The module doc quotes these numbers and a
    // PR body quotes the module doc; without this they drift the moment a row
    // is re-tagged, which is the exact rot a markdown checklist suffers from.
    let tally = |want: fn(&RegMarkAudit) -> bool| REGMARK_AUDIT.iter().filter(|e| want(e)).count();
    assert_eq!(
        (
            tally(|e| matches!(e.status, OptStatus::Implemented)),
            tally(|e| matches!(e.status, OptStatus::Partial)),
            tally(|e| matches!(e.status, OptStatus::NotImplemented)),
            tally(|e| matches!(e.status, OptStatus::Skipped(_))),
            tally(|e| matches!(e.area, RmArea::Registers)),
            tally(|e| matches!(e.area, RmArea::Marks)),
            tally(|e| matches!(e.status, OptStatus::NotImplemented) && e.report == Report::Silent),
        ),
        (42, 12, 32, 3, 34, 55, 8),
        "the audit tally moved: (implemented, partial, missing, skipped, \
         registers, marks, missing-and-silent). Update the module doc's table \
         in the same commit."
    );

    assert!(
        problems.is_empty(),
        "\n\n== registers/marks audit table is inconsistent (#1226) ==\n{}\n",
        problems.join("\n")
    );
}

/// `cases` is `(label, keys)` for the whole corpus — the same view
/// [`classify_coverage`] takes.
fn classify_regmark_coverage(
    audit: &'static [RegMarkAudit],
    exempt: &[&'static str],
    cases: &[(&'static str, &'static str)],
) -> OptionCoverage {
    use std::collections::HashSet;
    let exempt_set: HashSet<&str> = exempt.iter().copied().collect();
    let mut v = OptionCoverage::default();
    for e in audit {
        let Some(probe) = e.probe else { continue };
        v.in_scope += 1;
        let covered = cases.iter().any(|(label, keys)| probe.matches(label, keys));
        match (covered, exempt_set.contains(e.item)) {
            (false, false) => v.uncovered.push(e.item),
            (true, true) => v.newly_covered.push(e.item),
            _ => {}
        }
    }
    v.stale = exempt
        .iter()
        .copied()
        .filter(|n| !audit.iter().any(|e| e.item == *n && e.probe.is_some()))
        .collect();
    v
}

/// Gate 2 (#1226) — #1007's ratchet, applied to the audited commands: an
/// in-scope row whose probe matches nothing must be exempt, and an exempt row
/// whose probe now matches must lose its entry. Pure.
#[test]
fn regmark_audit_oracle_coverage_is_shrink_only() {
    use std::collections::HashSet;
    let corpus = all_corpus_cases();
    let exempt: HashSet<&str> = REGMARK_COVERAGE_EXEMPT.iter().copied().collect();
    assert_eq!(
        exempt.len(),
        REGMARK_COVERAGE_EXEMPT.len(),
        "REGMARK_COVERAGE_EXEMPT lists an item twice"
    );

    let v = classify_regmark_coverage(REGMARK_AUDIT, REGMARK_COVERAGE_EXEMPT, &corpus);

    if let Ok(path) = std::env::var("CONFORMANCE_DUMP_REGMARK_COVERAGE") {
        let mut s = String::new();
        for n in &v.uncovered {
            s.push_str(&format!("UNCOVERED\t{n}\n"));
        }
        for n in &v.newly_covered {
            s.push_str(&format!("NEWLY_COVERED\t{n}\n"));
        }
        for n in &v.stale {
            s.push_str(&format!("STALE\t{n}\n"));
        }
        // Every credited row plus the case that credits it — the
        // over-crediting check the section doc demands a human be able to do
        // in one grep (`Keys("g'")` silently matching `magg'a` is exactly
        // the failure mode #1007's "deliberately dumb probe" note warns of).
        for e in REGMARK_AUDIT {
            let Some(probe) = e.probe else { continue };
            if let Some((label, _)) = corpus
                .iter()
                .find(|(label, keys)| probe.matches(label, keys))
            {
                s.push_str(&format!(
                    "COVERED\t{}\t{:?}\t{}\n",
                    e.item,
                    probe.needle(),
                    label
                ));
            }
        }
        std::fs::write(&path, s).unwrap_or_else(|e| panic!("dump to {path}: {e}"));
        return;
    }

    let covered = v.in_scope - REGMARK_COVERAGE_EXEMPT.len();
    println!(
        "\n== registers/marks oracle coverage (#1226) ==\n\
         {covered}/{} audited commands are pinned by an oracle case; {} exempt.\n",
        v.in_scope,
        REGMARK_COVERAGE_EXEMPT.len()
    );

    assert!(
        v.uncovered.is_empty() && v.newly_covered.is_empty() && v.stale.is_empty(),
        "\n\n== registers/marks oracle coverage moved (#1226) ==\n\
         UNCOVERED (probe matches no case — add the case, or exempt it only when \
         seeding a newly-tagged command):\n  {:?}\n\
         NEWLY COVERED (a case now pins it — delete the REGMARK_COVERAGE_EXEMPT \
         entry; that is how the list shrinks):\n  {:?}\n\
         STALE (exempt but not an in-scope audited command):\n  {:?}\n",
        v.uncovered,
        v.newly_covered,
        v.stale
    );
}

/// `"%` and `"#` hold the file name, and this pins the finding the `Live`
/// recordings above cannot reach: with a real file open, both registers paste
/// the **basename**, where Vim pastes the name as it was typed. Black-box —
/// it drives `"%p` and reads the rendered buffer, not `Engine::registers`.
#[test]
fn register_percent_and_hash_paste_basenames_not_paths() {
    // Same temp-dir convention as the multi-file harness above: an explicit
    // `probe_id()`-suffixed directory, canonicalized once, no new dependency.
    let dir = std::env::temp_dir().join(format!("vimcode_regmark_{}", probe_id()));
    let nested = dir.join("src");
    std::fs::create_dir_all(&nested).expect("create temp dir for the \"% probe");
    let nested = nested.canonicalize().unwrap_or(nested);
    let alpha = nested.join("alpha.txt");
    let beta = nested.join("beta.txt");
    std::fs::write(&alpha, "one\n").expect("write alpha");
    std::fs::write(&beta, "two\n").expect("write beta");

    let mut engine = engine_with("");
    engine
        .open_file_with_mode(&alpha, OpenMode::Permanent)
        .expect("open alpha");
    engine
        .open_file_with_mode(&beta, OpenMode::Permanent)
        .expect("open beta");

    // `"%p` on the current buffer, then `"#p` for the alternate one (#1161).
    send_keys(&mut engine, "\"%p");
    let after_percent = engine.buffer().to_string();
    assert!(
        after_percent.contains("beta.txt"),
        "`\"%p` must paste the current file name, got {after_percent:?}"
    );
    assert!(
        !after_percent.contains("src/beta.txt") && !after_percent.contains("src\\beta.txt"),
        "documented divergence (#1226): vimcode pastes the BASENAME, Vim pastes \
         the name as typed. If this now pastes a path, the `\"%` row is no longer \
         Partial — re-tag it. Got {after_percent:?}"
    );

    send_keys(&mut engine, "u\"#p");
    let after_hash = engine.buffer().to_string();
    assert!(
        after_hash.contains("alpha.txt"),
        "`\"#p` must paste the alternate file name, got {after_hash:?}"
    );
    assert!(
        !after_hash.contains("src/alpha.txt") && !after_hash.contains("src\\alpha.txt"),
        "documented divergence (#1226): `\"#` pastes the BASENAME too. Got {after_hash:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// All three gates, observed **failing** — on synthetic input for the shapes
/// a human would otherwise take on trust, and on the **real** table and
/// corpus for the two that matter most. #553 shipped black-box tests that
/// stayed green with the bug reinstated; an audit whose gate cannot go red is
/// the same mistake in table form.
#[test]
fn regmark_audit_gates_are_bidirectional() {
    // ── Gate 1, direction A: a row that under-claims. `'a` works, so
    // recording it as a no-op must be caught.
    static MIS_NOOP: &[RegMarkAudit] = &[rm(
        Marks,
        "'{a-z}",
        "'a",
        Implemented,
        Silent,
        Some(live(
            &["  a", "b", "c"],
            (1, 3),
            "majj'a",
            "  a|b|c",
            (3, 1),
            "",
        )),
        Some(Label("mark:'a first nonblank")),
        "deliberately mis-recorded fixture: the real cursor lands on (1, 3)",
    )];
    assert_eq!(
        regmark_drift(MIS_NOOP).len(),
        1,
        "mis-recording a working command must be caught"
    );

    // ── Gate 1, direction B: a row that over-claims. This is the direction
    // that fires when a NotImplemented row *gains* an implementation: record
    // `'(` as working and the replay must disagree.
    static MIS_WORKS: &[RegMarkAudit] = &[rm(
        Marks,
        "'(",
        "'(",
        Implemented,
        Silent,
        Some(live(
            &["One two. Three four."],
            (1, 12),
            "'(",
            "One two. Three four.",
            (1, 10),
            "",
        )),
        Some(Label("mark:'(")),
        "deliberately mis-recorded fixture: `'(` is not implemented",
    )];
    let drift = regmark_drift(MIS_WORKS);
    assert_eq!(
        drift.len(),
        2,
        "claiming an unimplemented command works must be caught for BOTH the \
         recording and the Report column: {drift:?}"
    );

    // ── Gate 1, direction C: the Report column on its own. `:marks {arg}`
    // really is refused, so tagging it Silent must fail even though the
    // buffer/cursor/message recording is correct.
    static MIS_SILENT: &[RegMarkAudit] = &[rm(
        Marks,
        ":marks {arg}",
        ":marks",
        NotImplemented,
        Silent,
        Some(live(
            &["a", "b"],
            (1, 1),
            "ma:marks a<CR>",
            "a|b",
            (1, 1),
            "Not an editor command: marks a",
        )),
        Some(Label("mark::marks a")),
        "deliberately mis-tagged fixture: vimcode refuses this loudly",
    )];
    assert_eq!(
        regmark_drift(MIS_SILENT)
            .iter()
            .filter(|d| d.contains("table says Silent"))
            .count(),
        1,
        "calling a loud refusal Silent must be caught"
    );

    // ── Gate 1 against the REAL table: every recording is load-bearing, so
    // perturbing one must fail. (Cheap proof that the 86 replays are really
    // compared, not collected and dropped.)
    let victim = REGMARK_AUDIT
        .iter()
        .find(|e| e.item == "]'")
        .expect("fixture drifted — the `]'` row is gone");
    let mut perturbed = *victim.live.as_ref().expect("`]'` has a recording");
    perturbed.cursor = (perturbed.cursor.0 + 1, perturbed.cursor.1);
    let (_, cursor, _) = replay_live(&perturbed);
    assert_ne!(
        cursor, perturbed.cursor,
        "a perturbed recording must disagree with the live engine"
    );

    // ── Gate 2, direction A (synthetic): an in-scope row nothing pins, and
    // nothing exempts.
    static UNPINNED: &[RegMarkAudit] = &[rm(
        Marks,
        "'{a-z}",
        "'a",
        Implemented,
        Silent,
        None,
        Some(Label("mark:'a first nonblank")),
        "fixture",
    )];
    let empty: Vec<(&str, &str)> = Vec::new();
    assert_eq!(
        classify_regmark_coverage(UNPINNED, &[], &empty).uncovered,
        vec!["'{a-z}"]
    );
    // …and exempting it makes the same table clean.
    assert!(classify_regmark_coverage(UNPINNED, &["'{a-z}"], &empty)
        .uncovered
        .is_empty());

    // ── Gate 2, direction B (synthetic): an exempt row a case now pins.
    let pinned = vec![("mark:'a first nonblank", "majj'a")];
    assert_eq!(
        classify_regmark_coverage(UNPINNED, &["'{a-z}"], &pinned).newly_covered,
        vec!["'{a-z}"]
    );

    // ── Gate 2, direction C (synthetic): a stale exemption.
    assert_eq!(
        classify_regmark_coverage(UNPINNED, &["'{a-z}", "`{a-z}"], &pinned).stale,
        vec!["`{a-z}"]
    );

    // ── Gate 2 against the REAL table and corpus: deleting an exempt entry
    // must fail, and adding a case that pins an exempt command must fail.
    // Anything less and the list could grow silently.
    let corpus = all_corpus_cases();
    let victim = "'\"";
    assert!(
        REGMARK_COVERAGE_EXEMPT.contains(&victim),
        "fixture drifted — {victim} is no longer exempt"
    );
    let without: Vec<&str> = REGMARK_COVERAGE_EXEMPT
        .iter()
        .copied()
        .filter(|n| *n != victim)
        .collect();
    assert_eq!(
        classify_regmark_coverage(REGMARK_AUDIT, &without, &corpus).uncovered,
        vec![victim],
        "deleting {victim:?} from REGMARK_COVERAGE_EXEMPT must fail the gate"
    );
    let mut plus = corpus.clone();
    plus.push(("mark:'\" last exit position", "'\""));
    assert_eq!(
        classify_regmark_coverage(REGMARK_AUDIT, REGMARK_COVERAGE_EXEMPT, &plus).newly_covered,
        vec![victim],
        "a case pinning {victim:?} must force its exemption to be deleted"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 5 audit slice — ex commands (#1227)
//
// Slice 3 of 5, and the largest surface in the audit. Walks the pinned fleet
// oracle's own `:help ex-cmd-index` (Neovim v0.12.5's `runtime/doc/index.txt`
// §6, [`DEVIATIONS_ORACLE`]) end to end — **all 553 `:` commands, every one of
// them** — and tags each Implemented / Partial / NotImplemented / Skipped
// against `src/core/engine/execute.rs`, the same way #1225 walked
// `:help option-list` and #1226 walked `:help registers`.
//
// The measurement, as of this slice:
//
//     ✅ Implemented      121
//     🟡 Partial           53
//     ❌ Not implemented  199
//     ⏭️  Intentionally skipped  180   (each with a reason from SKIP_REASONS)
//                         ────
//                          553
//
// The slice landed at 117/203; #1156's undo tree then moved `:earlier`,
// `:later`, `:undojoin` and `:undolist` from ❌ to ✅, which is gate 1 doing
// its job — the branch implemented them, the recorded `ExDispatch` stopped
// matching the live dispatcher, and the rows had to be re-tagged.
//
// ## Why this slice was expected to grow the denominator, and did
//
// `VIM_COMPATIBILITY.md`'s "Core Vim Ex Commands" section lists ~70 rows and
// reads 100%. `:help ex-cmd-index` lists 553. The gap is not that the doc is
// wrong about the 70 — it is that "complete" was being measured against a
// hand-written list of the commands vimcode already had, so a command that
// was never considered could not show up as missing. 553 rows is the
// denominator the ratchet can now count against, and 174 of them (✅ + 🟡)
// are the numerator that oracle cases have to reach.
//
// ## Gate 1 — the recorded dispatch must match the live dispatcher
//
// Every row records [`ExDispatch`]: what `Engine::execute_command` does with
// the **full** command name and with the **minimal abbreviation `:help`
// documents**, measured, never asserted by hand. Both spellings matter
// because they fail independently in vimcode: `normalize_ex_command`'s
// `EX_ABBREVS` table is hand-maintained and first-match-wins, so a command
// can be reachable as `:nmap` and rejected as `:nm` (43 rows), reachable as
// `:tabe` and rejected as `:tabedit` (6 rows), or — twice — have its
// documented abbreviation silently point at a *different* command.
//
// [`ex_audit_matches_the_live_dispatcher`] replays all 551 runnable rows
// (553 minus the two that crash, below) through a real engine and diffs.
// Bidirectional by construction: implementing `:lcd` flips its row from
// `Neither` and fails until it is re-tagged, and a command that stops
// dispatching fails immediately.
//
// It is a black-box observation — an ex line in, `engine.message` out —
// never an engine field. A gate that asserted "`execute.rs` contains the
// string `lgetfile`" would be green for `:lg`, which is precisely the row
// where vimcode runs the wrong command.
//
// ## Gate 2 — oracle coverage, same shrink-only shape as #1007
//
// Every ✅/🟡 row carries a [`Probe`] naming the oracle case that exercises
// it; [`EX_COVERAGE_EXEMPT`] lists the ones no case reaches today. Both
// directions fail, exactly as in `COVERAGE_EXEMPT`. Writing the missing
// cases is #1162's job, not this slice's — the exempt list is the
// measurement it starts from.
//
// ❌ rows carry no probe, deliberately. #1226 gave one to every non-skipped
// row because it had 32 of them; this slice has 199, and 199 permanently
// exempt entries would drown the ~114 that describe a *real* gap in a
// shrink-only list nobody can read. ❌ rows are already gated — harder — by
// gate 1: a command that gains an implementation changes its `ExDispatch`.
//
// ## Findings that are worse than "missing"
//
//   * `:bdelete` and `:bwipeout` **panic** when they delete the last buffer
//     (`active_buffer_state`'s `unwrap` on a buffer that no longer exists),
//     where Vim falls back to an empty [No Name] buffer. Recorded as
//     [`ExDispatch::Crashes`] and pinned by
//     [`bdelete_on_the_last_buffer_panics_instead_of_refusing`].
//   * `:!!` does not repeat the last `:!`; it passes the literal string `!`
//     to the shell.
//   * `:lg`, which `:help` documents as `:lg[etfile]`, runs `:lgrep`, and
//     `:ln`, documented as `:ln[oremap]`, runs `:lnext` — first-match-wins
//     abbreviations pointing at the wrong command.
//   * `:continue`, `:debug`, `:stop` and `:restart` are bound to vimcode's
//     DAP debugger, shadowing four Vim commands.
//   * `:w {file}`, `:wq {file}`, `:x {file}` and `:update {file}` are all
//     rejected — `:saveas` is the only way to write somewhere else.
//   * `:1,2p` and `:1,2#` are rejected although bare `:p`/`:#` work: the
//     print family takes no range.
//   * pressing `:` then Enter answers `Not an editor command: `.
//
// ## Out of scope, deliberately
//
// VimScript (`:let`, `:if`, `:function`, `:autocmd`, `:source`, `:execute`,
// `:call` and the rest of the ⏭️ block) is tagged Skipped per the standing
// decision that vimcode implements Vim *keybindings and editing*, not a
// VimScript runtime. Nothing here implements, fixes or changes any command:
// this slice audits, tags and files. `COVERAGE_PHASE5.md` and
// `VIM_COMPATIBILITY.md` were read, never written.
// ═══════════════════════════════════════════════════════════════════════════

/// What `Engine::execute_command` does with an ex command's two documented
/// spellings — measured by [`measured_ex_dispatch`], never asserted by hand.
///
/// Two spellings rather than one because they fail independently: vimcode's
/// `EX_ABBREVS` table is separate from its dispatch, so "vimcode has this
/// command" and "vimcode has this command under the name `:help` says you can
/// type" are different questions, and 49 of the 553 rows answer them
/// differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExDispatch {
    /// Both the full name and `:help`'s minimal abbreviation reach a handler.
    Both,
    /// The full name reaches a handler; the documented minimal abbreviation
    /// is answered with "Not an editor command" (vimcode's E492).
    FullOnly,
    /// The abbreviation reaches a handler but the full spelling does not.
    AbbrevOnly,
    /// Both spellings are answered with "Not an editor command".
    Neither,
    /// Running it **panics**. Not executed by gate 1 — a panicking probe
    /// would take the whole test binary down rather than report a row — so
    /// the crash is pinned separately, and precisely, by
    /// [`bdelete_on_the_last_buffer_panics_instead_of_refusing`].
    Crashes,
}

/// One `:help ex-cmd-index` entry.
struct ExAudit {
    /// The command exactly as `:help ex-cmd-index` writes it, brackets and
    /// all (`":ab[breviate]"`) — the table's unique key.
    cmd: &'static str,
    /// The `:help` tag it is documented under (`":abbreviate"`).
    help: &'static str,
    status: OptStatus,
    /// Re-measured by gate 1 from `probe_full` / `probe_abbr`.
    dispatch: ExDispatch,
    /// The exact ex line driven for the **full** name. Usually just the name;
    /// commands that need an argument to reach their handler carry one (and
    /// it is always an argument with no side effect — `:make -f /dev/null -q`
    /// reads no Makefile, `:read foo` reads a file that does not exist).
    probe_full: &'static str,
    /// The same, for `:help`'s minimal abbreviation.
    probe_abbr: &'static str,
    /// Oracle probe — `Some` exactly for Implemented/Partial rows; see the
    /// section doc for why ❌ rows carry none.
    probe: Option<Probe>,
    /// For ❌: what Vim does, plus this slice's assessment of whether it is
    /// worth implementing. For 🟡: exactly what is missing.
    note: &'static str,
}

#[allow(clippy::too_many_arguments)]
const fn ex(
    cmd: &'static str,
    help: &'static str,
    status: OptStatus,
    dispatch: ExDispatch,
    probe_full: &'static str,
    probe_abbr: &'static str,
    probe: Option<Probe>,
    note: &'static str,
) -> ExAudit {
    ExAudit {
        cmd,
        help,
        status,
        dispatch,
        probe_full,
        probe_abbr,
        probe,
        note,
    }
}

use crate::ExDispatch::{AbbrevOnly, Both, Crashes, FullOnly, Neither};

/// Every command in `:help ex-cmd-index`, in `:help` order.
///
/// 553 rows, no "TODO" and no unreviewed row: adding one, deleting one,
/// reordering one or leaving one without a note fails
/// [`ex_audit_is_internally_consistent`].
const EX_AUDIT: &[ExAudit] = &[
    ex(":", ":", NotImplemented, Neither, "", "", None,
       "an empty `:` line — Vim does nothing; vimcode answers `Not an editor command: `, which a user sees by pressing `:` then Enter. Worth fixing: one early return"),
    ex(":{range}", ":range", Implemented, Both, "5", "5", Some(Label("ex:5")),
       "`:{N}`, `:{range}{cmd}` and the `.$%'m/pat/?pat?+N;` address grammar all parse"),
    ex(":!", ":!", Implemented, Both, "!", "!", Some(Label("ex:%!sort")),
       "`:!{cmd}` shells out and reports the first output line; `:{range}!{cmd}` filters"),
    ex(":!!", ":!!", NotImplemented, Both, "!!", "!!", None,
       "repeat the last `:!` — vimcode instead passes the literal string `!` to the shell and prints `(no output)`, so the command silently runs garbage; worth implementing (the last command is already stored for `@:`)"),
    ex(":#", ":#", Partial, Both, "#", "#", Some(Keys(":#<CR>")),
       "prints the current line with its number, but only bare: `:1,2#` is rejected, so `:#` has no range"),
    ex(":&", ":&", Implemented, Both, "&", "&", Some(Label("sub:& cmd")),
       "repeats the last `:substitute` on the current line"),
    ex(":*", ":star", Implemented, Neither, "*d", "*d", Some(Label("ex:*d after visual")),
       "`:*` resolves to `'<,'>`, the last Visual area"),
    ex(":<", ":<", Implemented, Both, "<", "<", Some(Label("ex:<")),
       "shifts left, with a count and a range"),
    ex(":=", ":=", Implemented, Both, "=", "=", Some(Keys(":=<CR>")),
       "prints the last line number"),
    ex(":>", ":>", Implemented, Both, ">", ">", Some(Label("ex:>")),
       "shifts right, with a count and a range"),
    ex(":@", ":@", NotImplemented, Neither, "@", "@", None,
       "execute the contents of a register — Normal-mode `@a`/`@:` exist, but the ex form is rejected; cheap to wire to the same code path"),
    ex(":@@", ":@@", NotImplemented, Neither, "@@", "@@", None,
       "repeat the previous `:@`; blocked on `:@`"),
    ex(":2mat[ch]", ":2match", NotImplemented, Neither, "2match", "2mat", None,
       "define a second match to highlight — tree-sitter plus the theme registry replace Vim's syntax/highlight files; low value"),
    ex(":3mat[ch]", ":3match", NotImplemented, Neither, "3match", "3mat", None,
       "define a third match to highlight — tree-sitter plus the theme registry replace Vim's syntax/highlight files; low value"),
    ex(":N[ext]", ":Next", NotImplemented, Neither, "Next", "N", None,
       "go to previous file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":a[ppend]", ":append", Skipped(EXMODE), Neither, "append", "a", None,
       "Ex/Open mode line editing is out of scope"),
    ex(":ab[breviate]", ":abbreviate", Implemented, Both, "abbreviate", "ab", Some(Label("abbrev:expands on CR")),
       "defines and lists abbreviations for both Insert and Command-line mode"),
    ex(":abc[lear]", ":abclear", Implemented, Both, "abclear", "abc", Some(Keys(":abclear<CR>")),
       "clears every abbreviation"),
    ex(":abo[veleft]", ":aboveleft", NotImplemented, Neither, "aboveleft", "abo", None,
       "make split window appear left or above — `:wincmd` and `:split`/`:vsplit` cover the resize and split axes, but the `:{mod} {cmd}` modifier grammar is not parsed at all; moderate value"),
    ex(":al[l]", ":all", NotImplemented, Neither, "all", "al", None,
       "open a window for each file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":am[enu]", ":amenu", Skipped(MENU), Neither, "amenu", "am", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":an[oremenu]", ":anoremenu", Skipped(MENU), Neither, "anoremenu", "an", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":ar[gs]", ":args", NotImplemented, Neither, "args", "ar", None,
       "print the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":arga[dd]", ":argadd", NotImplemented, Neither, "argadd", "arga", None,
       "add items to the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":argded[upe]", ":argdedupe", NotImplemented, Neither, "argdedupe", "argded", None,
       "remove duplicates from the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":argd[elete]", ":argdelete", NotImplemented, Neither, "argdelete", "argd", None,
       "delete items from the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":arge[dit]", ":argedit", NotImplemented, Neither, "argedit", "arge", None,
       "add item to the argument list and edit it — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":argdo", ":argdo", NotImplemented, Neither, "argdo", "argdo", None,
       "do a command on all items in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":argg[lobal]", ":argglobal", NotImplemented, Neither, "argglobal", "argg", None,
       "define the global argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":argl[ocal]", ":arglocal", NotImplemented, Neither, "arglocal", "argl", None,
       "define a local argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":argu[ment]", ":argument", NotImplemented, Neither, "argument", "argu", None,
       "go to specific file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":as[cii]", ":ascii", NotImplemented, Neither, "ascii", "as", None,
       "print ascii value of character under the cursor — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":au[tocmd]", ":autocmd", Skipped(SCRIPTRT), Neither, "autocmd", "au", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":aug[roup]", ":augroup", Skipped(SCRIPTRT), Neither, "augroup", "aug", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":aun[menu]", ":aunmenu", Skipped(MENU), Neither, "aunmenu", "aun", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":b[uffer]", ":buffer", Implemented, Both, "buffer 1", "b 1", Some(Keys(":buffer ")),
       "switches to a buffer by number or name"),
    ex(":bN[ext]", ":bNext", NotImplemented, Neither, "bNext", "bN", None,
       "go to previous buffer in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":ba[ll]", ":ball", NotImplemented, Neither, "ball", "ba", None,
       "open a window for each buffer in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":bad[d]", ":badd", NotImplemented, Neither, "badd", "bad", None,
       "add buffer to the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":balt", ":balt", NotImplemented, Neither, "balt", "balt", None,
       "like \":badd\" but also set the alternate file — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":bd[elete]", ":bdelete", Partial, Crashes, "bdelete", "bd", Some(Keys(":bd<CR>")),
       "unloads a buffer, but **panics** (`active_buffer_state`'s `unwrap`) when it deletes the last one instead of falling back to an empty [No Name] buffer — see `bdelete_on_the_last_buffer_panics_instead_of_refusing`"),
    ex(":bel[owright]", ":belowright", NotImplemented, Neither, "belowright", "bel", None,
       "make split window appear right or below — `:wincmd` and `:split`/`:vsplit` cover the resize and split axes, but the `:{mod} {cmd}` modifier grammar is not parsed at all; moderate value"),
    ex(":bf[irst]", ":bfirst", Implemented, Both, "bfirst", "bf", Some(Label("ex:bfirst noop with one buffer")),
       "jumps to the first buffer"),
    ex(":bl[ast]", ":blast", Implemented, Both, "blast", "bl", Some(Label("ex:blast noop with one buffer")),
       "jumps to the last buffer"),
    ex(":bm[odified]", ":bmodified", NotImplemented, Neither, "bmodified", "bm", None,
       "go to next buffer in the buffer list that has been modified — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":bn[ext]", ":bnext", Implemented, Both, "bnext", "bn", Some(Keys(":bnext<CR>")),
       "cycles to the next buffer"),
    ex(":bo[tright]", ":botright", NotImplemented, Neither, "botright", "bo", None,
       "make split window appear at bottom or far right — `:wincmd` and `:split`/`:vsplit` cover the resize and split axes, but the `:{mod} {cmd}` modifier grammar is not parsed at all; moderate value"),
    ex(":bp[revious]", ":bprevious", Implemented, Both, "bprevious", "bp", Some(Keys(":bprevious<CR>")),
       "cycles to the previous buffer"),
    ex(":br[ewind]", ":brewind", NotImplemented, Neither, "brewind", "br", None,
       "go to first buffer in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":brea[k]", ":break", Skipped(VIMSCRIPT), Neither, "break", "brea", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":breaka[dd]", ":breakadd", Skipped(VIMSCRIPT), Neither, "breakadd", "breaka", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":breakd[el]", ":breakdel", Skipped(VIMSCRIPT), Neither, "breakdel", "breakd", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":breakl[ist]", ":breaklist", Skipped(VIMSCRIPT), Neither, "breaklist", "breakl", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":bro[wse]", ":browse", NotImplemented, Neither, "browse", "bro", None,
       "use file selection dialog — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":bufd[o]", ":bufdo", Implemented, Both, "bufdo s/a/b/", "bufdo s/a/b/", Some(Keys(":bufdo ")),
       "runs a command in every buffer"),
    ex(":buffers", ":buffers", Implemented, Both, "buffers", "buffers", Some(Keys(":buffers<CR>")),
       "lists buffers with the `%a` flags"),
    ex(":bun[load]", ":bunload", NotImplemented, Neither, "bunload", "bun", None,
       "unload a specific buffer — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":bw[ipeout]", ":bwipeout", Partial, Crashes, "bwipeout", "bw", Some(Label("ex:bwipeout refuses on a dirty buffer without a bang")),
       "same code path, and the same last-buffer panic as `:bdelete`"),
    ex(":c[hange]", ":change", Skipped(EXMODE), Neither, "change", "c", None,
       "Ex/Open mode line editing is out of scope"),
    ex(":cN[ext]", ":cNext", Partial, AbbrevOnly, "cNext", "cN", Some(Keys(":cN<CR>")),
       "`:cN` works (it is `:cprevious`), but the full spelling `:cNext` is rejected"),
    ex(":cNf[ile]", ":cNfile", NotImplemented, Neither, "cNfile", "cNf", None,
       "go to last error in previous file — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":ca[bbrev]", ":cabbrev", Implemented, Both, "cabbrev", "ca", Some(Label("abbrev:cabbrev on the command line runs the expanded command")),
       "command-line abbreviations"),
    ex(":cabc[lear]", ":cabclear", NotImplemented, Neither, "cabclear", "cabc", None,
       "clear all abbreviations for Command-line mode — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":cabo[ve]", ":cabove", NotImplemented, Neither, "cabove", "cabo", None,
       "go to error above current line — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cad[dbuffer]", ":caddbuffer", NotImplemented, Neither, "caddbuffer", "cad", None,
       "add errors from buffer — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cadde[xpr]", ":caddexpr", NotImplemented, Neither, "caddexpr", "cadde", None,
       "add errors from expr — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":caddf[ile]", ":caddfile", NotImplemented, Neither, "caddfile", "caddf", None,
       "add error message to current quickfix list — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":caf[ter]", ":cafter", NotImplemented, Neither, "cafter", "caf", None,
       "go to error after current cursor — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cal[l]", ":call", Skipped(VIMSCRIPT), Neither, "call", "cal", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":cat[ch]", ":catch", Skipped(VIMSCRIPT), Neither, "catch", "cat", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":cbe[fore]", ":cbefore", NotImplemented, Neither, "cbefore", "cbe", None,
       "go to error before current cursor — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cbel[ow]", ":cbelow", NotImplemented, Neither, "cbelow", "cbel", None,
       "go to error below current line — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cbo[ttom]", ":cbottom", NotImplemented, Neither, "cbottom", "cbo", None,
       "scroll to the bottom of the quickfix window — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cb[uffer]", ":cbuffer", NotImplemented, Neither, "cbuffer", "cb", None,
       "parse error messages and jump to first error — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cc", ":cc", Implemented, Both, "cc", "cc", Some(Label("ex:cc on empty quickfix list")),
       "jumps to quickfix error N, or re-jumps to the current one"),
    ex(":ccl[ose]", ":cclose", Implemented, Both, "cclose", "ccl", Some(Keys(":cclose<CR>")),
       "closes the quickfix window"),
    ex(":cd", ":cd", Partial, Both, "cd .", "cd .", Some(Keys(":cd ")),
       "changes vimcode's *workspace folder* (and the explorer root), not the process/window working directory; bare `:cd` does not go to $HOME, and `:lcd`/`:tcd` are absent"),
    ex(":cdo", ":cdo", Implemented, Both, "cdo", "cdo", Some(Keys(":cdo ")),
       "runs a command on each quickfix entry"),
    ex(":cfd[o]", ":cfdo", Partial, FullOnly, "cfdo", "cfd", Some(Keys(":cfdo ")),
       "works, but `:cfd` — Vim's documented minimum abbreviation — is rejected (vimcode's table requires 4 characters)"),
    ex(":ce[nter]", ":center", Implemented, Both, "center", "ce", Some(Label("ex:ce 10")),
       "centres lines within 'textwidth' or an explicit width"),
    ex(":cex[pr]", ":cexpr", NotImplemented, Neither, "cexpr", "cex", None,
       "read errors from expr and jump to first — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cf[ile]", ":cfile", NotImplemented, Neither, "cfile", "cf", None,
       "read file with error messages and jump to first — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cfir[st]", ":cfirst", Implemented, Both, "cfirst", "cfir", Some(Keys(":cfirst<CR>")),
       "first quickfix entry"),
    ex(":cgetb[uffer]", ":cgetbuffer", NotImplemented, Neither, "cgetbuffer", "cgetb", None,
       "get errors from buffer — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cgete[xpr]", ":cgetexpr", NotImplemented, Neither, "cgetexpr", "cgete", None,
       "get errors from expr — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cg[etfile]", ":cgetfile", NotImplemented, Neither, "cgetfile", "cg", None,
       "read file with error messages — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":changes", ":changes", Implemented, Both, "changes", "changes", Some(Keys(":changes<CR>")),
       "prints the change list"),
    ex(":chd[ir]", ":chdir", NotImplemented, Neither, "chdir", "chd", None,
       "change directory — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":che[ckhealth]", ":checkhealth", NotImplemented, Neither, "checkhealth", "che", None,
       "run healthchecks — informational commands with no vimcode surface; `:messages` and `:oldfiles` (vimcode already has a recent-files picker) are the two worth adding"),
    ex(":checkp[ath]", ":checkpath", Skipped(CTAGS), Neither, "checkpath", "checkp", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":checkt[ime]", ":checktime", NotImplemented, Neither, "checktime", "checkt", None,
       "check timestamp of loaded buffers — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":chi[story]", ":chistory", NotImplemented, Neither, "chistory", "chi", None,
       "list the error lists — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cla[st]", ":clast", Implemented, Both, "clast", "cla", Some(Keys(":clast<CR>")),
       "last quickfix entry"),
    ex(":cle[arjumps]", ":clearjumps", NotImplemented, Neither, "clearjumps", "cle", None,
       "clear the jump list — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":cl[ist]", ":clist", Implemented, Both, "clist", "cl", Some(Keys(":clist<CR>")),
       "lists quickfix entries"),
    ex(":clo[se]", ":close", Implemented, Both, "close", "clo", Some(Keys(":close<CR>")),
       "closes the window, refusing on the last one"),
    ex(":cm[ap]", ":cmap", Partial, FullOnly, "cmap", "cm", Some(Keys(":cmap ")),
       "works, but `:cm` is rejected"),
    ex(":cmapc[lear]", ":cmapclear", Partial, FullOnly, "cmapclear", "cmapc", Some(Keys(":cmapclear")),
       "works, but `:cmapc` is rejected"),
    ex(":cme[nu]", ":cmenu", Skipped(MENU), Neither, "cmenu", "cme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":cn[ext]", ":cnext", Implemented, Both, "cnext", "cn", Some(Keys(":cnext<CR>")),
       "next quickfix entry"),
    ex(":cnew[er]", ":cnewer", Implemented, Both, "cnewer", "cnew", Some(Keys(":cnewer<CR>")),
       "newer quickfix list"),
    ex(":cnf[ile]", ":cnfile", NotImplemented, Neither, "cnfile", "cnf", None,
       "go to first error in next file — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cno[remap]", ":cnoremap", Partial, FullOnly, "cnoremap", "cno", Some(Keys(":cnoremap ")),
       "works, but `:cno` is rejected"),
    ex(":cnorea[bbrev]", ":cnoreabbrev", Implemented, Both, "cnoreabbrev foo bar", "cnorea foo bar", Some(Keys(":cnoreabbrev ")),
       "defined, and stored alongside `:cabbrev`"),
    ex(":cnoreme[nu]", ":cnoremenu", Skipped(MENU), Neither, "cnoremenu", "cnoreme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":co[py]", ":copy", Implemented, Both, "copy 0", "co 0", Some(Label("ex:1co$")),
       "copies a range below an address"),
    ex(":col[der]", ":colder", Implemented, Both, "colder", "col", Some(Keys(":colder<CR>")),
       "older quickfix list"),
    ex(":colo[rscheme]", ":colorscheme", Implemented, Both, "colorscheme", "colo", Some(Keys(":colorscheme ")),
       "switches themes, and lists them when called bare"),
    ex(":com[mand]", ":command", Skipped(SCRIPTRT), Neither, "command", "com", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":comc[lear]", ":comclear", Skipped(SCRIPTRT), Neither, "comclear", "comc", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":comp[iler]", ":compiler", Skipped(SCRIPTRT), Neither, "compiler", "comp", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":con[tinue]", ":continue", Skipped(VIMSCRIPT), FullOnly, "continue", "con", None,
       "VimScript loop control — and vimcode has taken the name for its DAP debugger's continue, so the two collide"),
    ex(":conf[irm]", ":confirm", NotImplemented, Neither, "confirm", "conf", None,
       "prompt user when confirmation required — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":cons[t]", ":const", Skipped(VIMSCRIPT), Neither, "const", "cons", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":cope[n]", ":copen", Implemented, Both, "copen", "cope", Some(Keys(":copen<CR>")),
       "opens the quickfix window"),
    ex(":cp[revious]", ":cprevious", Implemented, Both, "cprevious", "cp", Some(Keys(":cprevious<CR>")),
       "previous quickfix entry"),
    ex(":cpf[ile]", ":cpfile", NotImplemented, Neither, "cpfile", "cpf", None,
       "go to last error in previous file — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cq[uit]", ":cquit", Implemented, Both, "cquit", "cq", Some(Keys(":cquit<CR>")),
       "quits with a non-zero exit code"),
    ex(":cr[ewind]", ":crewind", NotImplemented, Neither, "crewind", "cr", None,
       "go to the specified error, default first one — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":cu[nmap]", ":cunmap", Partial, FullOnly, "cunmap", "cu", Some(Keys(":cunmap ")),
       "works, but `:cu` is rejected"),
    ex(":cuna[bbrev]", ":cunabbrev", NotImplemented, Neither, "cunabbrev", "cuna", None,
       "like \":unabbrev\" but for Command-line mode — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":cunme[nu]", ":cunmenu", Skipped(MENU), Neither, "cunmenu", "cunme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":cw[indow]", ":cwindow", Implemented, Both, "cwindow", "cw", Some(Keys(":cwindow<CR>")),
       "opens the quickfix window only when it is non-empty"),
    ex(":d[elete]", ":delete", Implemented, Both, "delete", "d", Some(Label("ex:2d")),
       "deletes a range into a register, with a count"),
    ex(":deb[ug]", ":debug", Skipped(VIMSCRIPT), FullOnly, "debug", "deb", None,
       "the VimScript debugger — vimcode has taken the name for starting a DAP session"),
    ex(":debugg[reedy]", ":debuggreedy", Skipped(VIMSCRIPT), Neither, "debuggreedy", "debugg", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":defe[r]", ":defer", Skipped(VIMSCRIPT), Neither, "defer", "defe", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":delc[ommand]", ":delcommand", Skipped(SCRIPTRT), Neither, "delcommand", "delc", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":delf[unction]", ":delfunction", Skipped(VIMSCRIPT), Neither, "delfunction", "delf", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":delm[arks]", ":delmarks", Implemented, Both, "delmarks", "delm", Some(Label("ex:delmarks a")),
       "deletes marks by name and range"),
    ex(":detach", ":detach", Skipped(PLATFORM), Neither, "detach", "detach", None,
       "command of a Vim/Neovim build feature vimcode does not target"),
    ex(":dif[fupdate]", ":diffupdate", NotImplemented, Neither, "diffupdate", "dif", None,
       "update 'diff' buffers — `:diffsplit`/`:diffthis` exist but no hunk transfer or refresh; `:diffget`/`:diffput` are worth implementing"),
    ex(":diffg[et]", ":diffget", NotImplemented, Neither, "diffget", "diffg", None,
       "remove differences in current buffer — `:diffsplit`/`:diffthis` exist but no hunk transfer or refresh; `:diffget`/`:diffput` are worth implementing"),
    ex(":diffo[ff]", ":diffoff", Partial, FullOnly, "diffoff", "diffo", Some(Keys(":diffoff")),
       "turns diff mode off, but `:diffo` is rejected"),
    ex(":diffp[atch]", ":diffpatch", NotImplemented, Neither, "diffpatch", "diffp", None,
       "apply a patch and show differences — `:diffsplit`/`:diffthis` exist but no hunk transfer or refresh; `:diffget`/`:diffput` are worth implementing"),
    ex(":diffpu[t]", ":diffput", NotImplemented, Neither, "diffput", "diffpu", None,
       "remove differences in other buffer — `:diffsplit`/`:diffthis` exist but no hunk transfer or refresh; `:diffget`/`:diffput` are worth implementing"),
    ex(":diffs[plit]", ":diffsplit", Partial, FullOnly, "diffsplit", "diffs", Some(Keys(":diffsplit ")),
       "opens the diff split, but `:diffs` — Vim's documented minimum — is rejected"),
    ex(":difft[his]", ":diffthis", Partial, FullOnly, "diffthis", "difft", Some(Keys(":diffthis")),
       "marks a window for diffing, but `:difft` is rejected"),
    ex(":dig[raphs]", ":digraphs", Implemented, Both, "digraphs", "dig", Some(Keys(":digraphs<CR>")),
       "lists the digraph table and defines user digraphs"),
    ex(":di[splay]", ":display", Implemented, Both, "display", "di", Some(Keys(":display<CR>")),
       "alias of `:registers`"),
    ex(":dj[ump]", ":djump", Skipped(CTAGS), Neither, "djump", "dj", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":dl", ":dl", NotImplemented, Neither, "dl", "dl", None,
       "`:d` with the `l` list flag — same gap as `:dp`; low value"),
    ex(":dli[st]", ":dlist", Skipped(CTAGS), Neither, "dlist", "dli", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":do[autocmd]", ":doautocmd", Skipped(SCRIPTRT), Neither, "doautocmd", "do", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":doautoa[ll]", ":doautoall", Skipped(SCRIPTRT), Neither, "doautoall", "doautoa", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":d[elete]p", ":dp", NotImplemented, Neither, "dp", "dp", None,
       "`:d` with the `p` print flag — vimcode's `:delete` takes a register and a count but no print flags; low value"),
    ex(":dr[op]", ":drop", NotImplemented, Neither, "drop", "dr", None,
       "jump to window editing file or edit file in current window — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":ds[earch]", ":dsearch", Skipped(CTAGS), Neither, "dsearch", "ds", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":dsp[lit]", ":dsplit", Skipped(CTAGS), Neither, "dsplit", "dsp", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":e[dit]", ":edit", Implemented, Both, "edit foo", "e foo", Some(Keys(":edit ")),
       "opens a file, with `!` to discard changes"),
    ex(":ea[rlier]", ":earlier", Implemented, Both, "earlier", "ea", Some(Label("ex:earlier")),
       "go to older change, with a count — landed with the undo tree in #1156, alongside `:later`/`:undolist`/`:undojoin`"),
    ex(":ec[ho]", ":echo", Skipped(VIMSCRIPT), Both, "echo", "ec", None,
       "vimcode accepts `:echo {text}` and echoes it back verbatim, which looks like support but evaluates nothing — the expression half is the VimScript half"),
    ex(":echoe[rr]", ":echoerr", Skipped(VIMSCRIPT), Neither, "echoerr", "echoe", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":echoh[l]", ":echohl", Skipped(VIMSCRIPT), Neither, "echohl", "echoh", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":echom[sg]", ":echomsg", Skipped(VIMSCRIPT), Neither, "echomsg", "echom", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":echon", ":echon", Skipped(VIMSCRIPT), Neither, "echon", "echon", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":el[se]", ":else", Skipped(VIMSCRIPT), Neither, "else", "el", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":elsei[f]", ":elseif", Skipped(VIMSCRIPT), Neither, "elseif", "elsei", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":em[enu]", ":emenu", Skipped(MENU), Neither, "emenu", "em", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":en[dif]", ":endif", Skipped(VIMSCRIPT), Neither, "endif", "en", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":endfo[r]", ":endfor", Skipped(VIMSCRIPT), Neither, "endfor", "endfo", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":endf[unction]", ":endfunction", Skipped(VIMSCRIPT), Neither, "endfunction", "endf", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":endt[ry]", ":endtry", Skipped(VIMSCRIPT), Neither, "endtry", "endt", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":endw[hile]", ":endwhile", Skipped(VIMSCRIPT), Neither, "endwhile", "endw", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":ene[w]", ":enew", Implemented, Both, "enew", "ene", Some(Label("ex:enew abandons a dirty buffer by default ('hidden' is on)")),
       "opens an empty buffer, with `!` to abandon a dirty one"),
    ex(":ev[al]", ":eval", Skipped(VIMSCRIPT), Neither, "eval", "ev", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":ex", ":ex", Skipped(EXMODE), Neither, "ex", "ex", None,
       "Ex/Open mode line editing is out of scope"),
    ex(":exe[cute]", ":execute", Skipped(VIMSCRIPT), Neither, "execute", "exe", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":exi[t]", ":exit", NotImplemented, Neither, "exit", "exi", None,
       "same as \":xit\" — no vimcode equivalent; low value"),
    ex(":exu[sage]", ":exusage", NotImplemented, Neither, "exusage", "exu", None,
       "overview of Ex commands — informational commands with no vimcode surface; `:messages` and `:oldfiles` (vimcode already has a recent-files picker) are the two worth adding"),
    ex(":fc[lose]", ":fclose", NotImplemented, Neither, "fclose", "fc", None,
       "close floating window — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":f[ile]", ":file", Partial, Both, "file", "f", Some(Keys(":file<CR>")),
       "`:file` reports the name/line count/percentage, but `:file {name}` (rename the buffer) is rejected"),
    ex(":files", ":files", Implemented, Both, "files", "files", Some(Keys(":files<CR>")),
       "alias of `:buffers`"),
    ex(":filet[ype]", ":filetype", NotImplemented, Neither, "filetype", "filet", None,
       "switch file type detection on/off — `:set` exists but has no local/global split (vimcode's settings are global), and `:setfiletype`/`:filetype` are absent; `:setfiletype` is worth implementing"),
    ex(":filt[er]", ":filter", Skipped(VIMSCRIPT), Neither, "filter", "filt", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":fin[d]", ":find", NotImplemented, FullOnly, "find", "fin", None,
       "find a file in 'path' and edit it — `:find` bare opens vimcode's Ctrl+F find/replace overlay instead, and `:find {file}` is rejected, so the Vim command is absent behind a name that looks taken; worth implementing"),
    ex(":fina[lly]", ":finally", Skipped(VIMSCRIPT), Neither, "finally", "fina", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":fini[sh]", ":finish", Skipped(SCRIPTRT), Neither, "finish", "fini", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":fir[st]", ":first", NotImplemented, Neither, "first", "fir", None,
       "go to the first file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":fo[ld]", ":fold", Partial, FullOnly, "fold", "fo", Some(Label("fold:ex::fold creates and closes a manual fold")),
       "creates a fold over a range, but `:fo` — Vim's documented minimum — is rejected"),
    ex(":foldc[lose]", ":foldclose", Implemented, Both, "foldclose", "foldc", Some(Label("fold:ex::foldclose! on a nested range closes every level")),
       "closes folds in a range, with `!` for recursive"),
    ex(":foldd[oopen]", ":folddoopen", Implemented, Both, "folddoopen d", "foldd d", Some(Label("fold:ex::folddoopen only touches lines outside the closed fold")),
       "runs a command on every non-folded line"),
    ex(":folddoc[losed]", ":folddoclosed", Implemented, Both, "folddoclosed d", "folddoc d", Some(Label("fold:ex::folddoclosed only touches lines inside the closed fold")),
       "runs a command on every folded line"),
    ex(":foldo[pen]", ":foldopen", Implemented, Both, "foldopen", "foldo", Some(Label("fold:ex::foldopen with a matching range reopens it")),
       "opens folds in a range, with `!` for recursive"),
    ex(":for", ":for", Skipped(VIMSCRIPT), Neither, "for", "for", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":fu[nction]", ":function", Skipped(VIMSCRIPT), Neither, "function", "fu", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":g[lobal]", ":global", Implemented, Both, "global/beta/d", "g/beta/d", Some(Label("ex:cursor after :g/d")),
       "`:g/pat/cmd`, with `:g!` and `:v` for the inverse"),
    ex(":go[to]", ":goto", NotImplemented, FullOnly, "goto", "go", None,
       "go to byte N in the buffer — vimcode recognises the name only to answer \"Use :N to go to line N\"; low value"),
    ex(":gr[ep]", ":grep", Implemented, Both, "grep", "gr", Some(Keys(":grep ")),
       "runs the external grep and fills the quickfix list"),
    ex(":grepa[dd]", ":grepadd", NotImplemented, Neither, "grepadd", "grepa", None,
       "like :grep, but append to current list — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":gu[i]", ":gui", Skipped(MENU), Neither, "gui", "gu", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":gv[im]", ":gvim", Skipped(MENU), Neither, "gvim", "gv", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":h[elp]", ":help", Partial, Both, "help", "h", Some(Keys(":help<CR>")),
       "opens vimcode's own three-topic help buffer; `:help {tag}` into Vim's documentation does not exist"),
    ex(":helpc[lose]", ":helpclose", NotImplemented, Neither, "helpclose", "helpc", None,
       "close one help window — informational commands with no vimcode surface; `:messages` and `:oldfiles` (vimcode already has a recent-files picker) are the two worth adding"),
    ex(":helpg[rep]", ":helpgrep", NotImplemented, Neither, "helpgrep", "helpg", None,
       "like \":grep\" but searches help files — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":helpt[ags]", ":helptags", NotImplemented, Neither, "helptags", "helpt", None,
       "generate help tags for a directory — informational commands with no vimcode surface; `:messages` and `:oldfiles` (vimcode already has a recent-files picker) are the two worth adding"),
    ex(":hi[ghlight]", ":highlight", NotImplemented, Neither, "highlight", "hi", None,
       "specify highlighting methods — tree-sitter plus the theme registry replace Vim's syntax/highlight files; low value"),
    ex(":hid[e]", ":hide", Implemented, Both, "hide", "hid", Some(Label("ex:hide refuses to close the last window")),
       "closes the window, keeping the buffer loaded"),
    ex(":his[tory]", ":history", Implemented, Both, "history", "his", Some(Keys(":history<CR>")),
       "prints the command history"),
    ex(":hor[izontal]", ":horizontal", NotImplemented, Neither, "horizontal", "hor", None,
       "following window command work horizontally — `:wincmd` and `:split`/`:vsplit` cover the resize and split axes, but the `:{mod} {cmd}` modifier grammar is not parsed at all; moderate value"),
    ex(":i[nsert]", ":insert", Skipped(EXMODE), Neither, "insert", "i", None,
       "Ex/Open mode line editing is out of scope"),
    ex(":ia[bbrev]", ":iabbrev", Implemented, Both, "iabbrev", "ia", Some(Label("abbrev:iabbrev does not apply on the command line")),
       "insert-mode abbreviations"),
    ex(":iabc[lear]", ":iabclear", NotImplemented, Neither, "iabclear", "iabc", None,
       "like \":abclear\" but for Insert mode — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":if", ":if", Skipped(VIMSCRIPT), Neither, "if", "if", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":ij[ump]", ":ijump", Skipped(CTAGS), Neither, "ijump", "ij", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":il[ist]", ":ilist", Skipped(CTAGS), Neither, "ilist", "il", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":im[ap]", ":imap", Partial, FullOnly, "imap", "im", Some(Keys(":imap ")),
       "works, but `:im` is rejected"),
    ex(":imapc[lear]", ":imapclear", Partial, FullOnly, "imapclear", "imapc", Some(Keys(":imapclear")),
       "works, but `:imapc` is rejected"),
    ex(":ime[nu]", ":imenu", Skipped(MENU), Neither, "imenu", "ime", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":ino[remap]", ":inoremap", Partial, FullOnly, "inoremap", "ino", Some(Label("map:inoremap_jk_to_escape")),
       "works, but `:ino` is rejected"),
    ex(":inorea[bbrev]", ":inoreabbrev", Implemented, Both, "inoreabbrev foo bar", "inorea foo bar", Some(Keys(":inoreabbrev ")),
       "defined, and stored alongside `:iabbrev`"),
    ex(":inoreme[nu]", ":inoremenu", Skipped(MENU), Neither, "inoremenu", "inoreme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":int[ro]", ":intro", NotImplemented, Neither, "intro", "int", None,
       "print the introductory message — informational commands with no vimcode surface; `:messages` and `:oldfiles` (vimcode already has a recent-files picker) are the two worth adding"),
    ex(":ip[ut]", ":iput", NotImplemented, Neither, "iput", "ip", None,
       "like |:put|, but adjust the indent to the current line — no vimcode equivalent; low value"),
    ex(":is[earch]", ":isearch", Skipped(CTAGS), Neither, "isearch", "is", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":isp[lit]", ":isplit", Skipped(CTAGS), Neither, "isplit", "isp", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":iu[nmap]", ":iunmap", Partial, FullOnly, "iunmap", "iu", Some(Keys(":iunmap ")),
       "works, but `:iu` is rejected"),
    ex(":iuna[bbrev]", ":iunabbrev", NotImplemented, Neither, "iunabbrev", "iuna", None,
       "like \":unabbrev\" but for Insert mode — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":iunme[nu]", ":iunmenu", Skipped(MENU), Neither, "iunmenu", "iunme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":j[oin]", ":join", Implemented, Both, "join", "j", Some(Label("ex:1,3j")),
       "joins a range, with `!` and a count"),
    ex(":ju[mps]", ":jumps", Implemented, Both, "jumps", "ju", Some(Keys(":jumps<CR>")),
       "prints the jump list"),
    ex(":k", ":k", Partial, Both, "2ka", "2ka", Some(Label("ex:2ka 'a")),
       "only the concatenated form with a range works (`:2ka`); Vim's `:k a` and `:1k a` are both rejected"),
    ex(":keepa[lt]", ":keepalt", NotImplemented, Neither, "keepalt", "keepa", None,
       "following command keeps the alternate file — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":kee[pmarks]", ":keepmarks", NotImplemented, Neither, "keepmarks", "kee", None,
       "following command keeps marks where they are — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":keepj[umps]", ":keepjumps", NotImplemented, Neither, "keepjumps", "keepj", None,
       "following command keeps jumplist and marks — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":keepp[atterns]", ":keeppatterns", NotImplemented, Neither, "keeppatterns", "keepp", None,
       "following command keeps search pattern history — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":lN[ext]", ":lNext", Partial, AbbrevOnly, "lNext", "lN", Some(Keys(":lN<CR>")),
       "`:lN` works (it is `:lprevious`), but the full spelling `:lNext` is rejected"),
    ex(":lNf[ile]", ":lNfile", NotImplemented, Neither, "lNfile", "lNf", None,
       "go to last entry in previous file — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":l[ist]", ":list", NotImplemented, Neither, "list", "l", None,
       "print lines — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":lab[ove]", ":labove", NotImplemented, Neither, "labove", "lab", None,
       "go to location above current line — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lad[dexpr]", ":laddexpr", NotImplemented, Neither, "laddexpr", "lad", None,
       "add locations from expr — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":laddb[uffer]", ":laddbuffer", NotImplemented, Neither, "laddbuffer", "laddb", None,
       "add locations from buffer — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":laddf[ile]", ":laddfile", NotImplemented, Neither, "laddfile", "laddf", None,
       "add locations to current location list — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":laf[ter]", ":lafter", NotImplemented, Neither, "lafter", "laf", None,
       "go to location after current cursor — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":la[st]", ":last", NotImplemented, Neither, "last", "la", None,
       "go to the last file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":lan[guage]", ":language", Skipped(ENCODING), Neither, "language", "lan", None,
       "vimcode is UTF-8 only; no encoding-conversion layer"),
    ex(":lat[er]", ":later", Implemented, Both, "later", "lat", Some(Keys(":later")),
       "go to newer change, with a count — landed with the undo tree in #1156; pinned by the `:earlier`/`:later` round-trip case"),
    ex(":lbe[fore]", ":lbefore", NotImplemented, Neither, "lbefore", "lbe", None,
       "go to location before current cursor — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lbel[ow]", ":lbelow", NotImplemented, Neither, "lbelow", "lbel", None,
       "go to location below current line — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lbo[ttom]", ":lbottom", NotImplemented, Neither, "lbottom", "lbo", None,
       "scroll to the bottom of the location window — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lb[uffer]", ":lbuffer", NotImplemented, Neither, "lbuffer", "lb", None,
       "parse locations and jump to first location — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lc[d]", ":lcd", NotImplemented, Neither, "lcd", "lc", None,
       "change directory locally — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":lch[dir]", ":lchdir", NotImplemented, Neither, "lchdir", "lch", None,
       "change directory locally — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":lcl[ose]", ":lclose", Implemented, Both, "lclose", "lcl", Some(Keys(":lclose<CR>")),
       "closes the location-list window"),
    ex(":ld[o]", ":ldo", Partial, FullOnly, "ldo", "ld", Some(Keys(":ldo ")),
       "works, but `:ld` — Vim's documented minimum — is rejected"),
    ex(":lfd[o]", ":lfdo", Partial, FullOnly, "lfdo", "lfd", Some(Keys(":lfdo ")),
       "works, but `:lfd` — Vim's documented minimum — is rejected"),
    ex(":le[ft]", ":left", Implemented, Both, "left", "le", Some(Label("ex:le 4")),
       "left-aligns with an optional indent"),
    ex(":lefta[bove]", ":leftabove", NotImplemented, Neither, "leftabove", "lefta", None,
       "make split window appear left or above — `:wincmd` and `:split`/`:vsplit` cover the resize and split axes, but the `:{mod} {cmd}` modifier grammar is not parsed at all; moderate value"),
    ex(":let", ":let", Skipped(VIMSCRIPT), Neither, "let", "let", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":lex[pr]", ":lexpr", NotImplemented, Neither, "lexpr", "lex", None,
       "read locations from expr and jump to first — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lf[ile]", ":lfile", NotImplemented, Neither, "lfile", "lf", None,
       "read file with locations and jump to first — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lfir[st]", ":lfirst", Implemented, Both, "lfirst", "lfir", Some(Keys(":lfirst<CR>")),
       "first location-list entry"),
    ex(":lgetb[uffer]", ":lgetbuffer", NotImplemented, Neither, "lgetbuffer", "lgetb", None,
       "get locations from buffer — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lgete[xpr]", ":lgetexpr", NotImplemented, Neither, "lgetexpr", "lgete", None,
       "get locations from expr — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lg[etfile]", ":lgetfile", NotImplemented, AbbrevOnly, "lgetfile", "lg", None,
       "read an error file into the location list — and vimcode's abbreviation table gives `:lg` to `:lgrep`, so Vim's documented `:lg[etfile]` silently runs a different command"),
    ex(":lgr[ep]", ":lgrep", Implemented, Both, "lgrep", "lgr", Some(Keys(":lgrep ")),
       "location-list grep"),
    ex(":lgrepa[dd]", ":lgrepadd", NotImplemented, Neither, "lgrepadd", "lgrepa", None,
       "like :grep, but append to current list — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lh[elpgrep]", ":lhelpgrep", NotImplemented, Neither, "lhelpgrep", "lh", None,
       "like \":helpgrep\" but uses location list — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lhi[story]", ":lhistory", NotImplemented, Neither, "lhistory", "lhi", None,
       "list the location lists — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":ll", ":ll", Implemented, Both, "ll", "ll", Some(Keys(":ll<CR>")),
       "jumps to location-list entry N"),
    ex(":lla[st]", ":llast", Implemented, Both, "llast", "lla", Some(Keys(":llast<CR>")),
       "last location-list entry"),
    ex(":lli[st]", ":llist", Implemented, Both, "llist", "lli", Some(Keys(":llist<CR>")),
       "lists location-list entries"),
    ex(":lmak[e]", ":lmake", NotImplemented, Neither, "lmake", "lmak", None,
       "execute external command 'makeprg' and parse error messages — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lm[ap]", ":lmap", Skipped(BIDI), Neither, "lmap", "lm", None,
       "'langmap'/input-method mappings; no right-to-left or IME support planned"),
    ex(":lmapc[lear]", ":lmapclear", Skipped(BIDI), Neither, "lmapclear", "lmapc", None,
       "'langmap'/input-method mappings; no right-to-left or IME support planned"),
    ex(":lne[xt]", ":lnext", Implemented, Both, "lnext", "lne", Some(Keys(":lnext<CR>")),
       "next location-list entry"),
    ex(":lnew[er]", ":lnewer", NotImplemented, Neither, "lnewer", "lnew", None,
       "go to newer location list — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lnf[ile]", ":lnfile", NotImplemented, Neither, "lnfile", "lnf", None,
       "go to first location in next file — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":ln[oremap]", ":lnoremap", Skipped(BIDI), AbbrevOnly, "lnoremap", "ln", None,
       "'langmap' mapping — and vimcode's abbreviation table gives `:ln` to `:lnext`, so Vim's documented `:ln[oremap]` runs a different command"),
    ex(":loadk[eymap]", ":loadkeymap", Skipped(BIDI), Neither, "loadkeymap", "loadk", None,
       "'langmap'/input-method mappings; no right-to-left or IME support planned"),
    ex(":lo[adview]", ":loadview", Skipped(SESSION), Neither, "loadview", "lo", None,
       "no :mksession/:mkview support planned"),
    ex(":loc[kmarks]", ":lockmarks", NotImplemented, Neither, "lockmarks", "loc", None,
       "following command keeps marks where they are — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":lockv[ar]", ":lockvar", Skipped(VIMSCRIPT), Neither, "lockvar", "lockv", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":lol[der]", ":lolder", NotImplemented, Neither, "lolder", "lol", None,
       "go to older location list — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lop[en]", ":lopen", Implemented, Both, "lopen", "lop", Some(Keys(":lopen<CR>")),
       "opens the location-list window"),
    ex(":lp[revious]", ":lprevious", Implemented, Both, "lprevious", "lp", Some(Keys(":lprevious<CR>")),
       "previous location-list entry"),
    ex(":lpf[ile]", ":lpfile", NotImplemented, Neither, "lpfile", "lpf", None,
       "go to last location in previous file — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lr[ewind]", ":lrewind", NotImplemented, Neither, "lrewind", "lr", None,
       "go to the specified location, default first one — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":ls", ":ls", Implemented, Both, "ls", "ls", Some(Keys(":ls<CR>")),
       "alias of `:buffers`"),
    ex(":lsp", ":lsp", NotImplemented, Neither, "lsp", "lsp", None,
       "language server protocol — no vimcode equivalent; low value"),
    ex(":lt[ag]", ":ltag", Skipped(CTAGS), Neither, "ltag", "lt", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":lu[nmap]", ":lunmap", Skipped(BIDI), Neither, "lunmap", "lu", None,
       "'langmap'/input-method mappings; no right-to-left or IME support planned"),
    ex(":lua", ":lua", Skipped(INTERP), Neither, "lua", "lua", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":luad[o]", ":luado", Skipped(INTERP), Neither, "luado", "luad", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":luaf[ile]", ":luafile", Skipped(INTERP), Neither, "luafile", "luaf", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":lv[imgrep]", ":lvimgrep", Partial, FullOnly, "lvimgrep", "lv", Some(Keys(":lvimgrep ")),
       "works, but `:lv` — Vim's documented minimum — is rejected"),
    ex(":lvimgrepa[dd]", ":lvimgrepadd", NotImplemented, Neither, "lvimgrepadd", "lvimgrepa", None,
       "like :vimgrep, but append to current list — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":lw[indow]", ":lwindow", Implemented, Both, "lwindow", "lw", Some(Keys(":lwindow<CR>")),
       "opens the location-list window only when non-empty"),
    ex(":m[ove]", ":move", Implemented, Both, "move 0", "m 0", Some(Label("ex:2,3m$")),
       "moves a range"),
    ex(":ma[rk]", ":mark", Implemented, Both, "mark a", "ma a", Some(Label("ex:2mark a")),
       "sets a mark on a line"),
    ex(":mak[e]", ":make", Implemented, Both, "make -f /dev/null -q", "mak -f /dev/null -q", Some(Keys(":make")),
       "shells out to make and reports the first output line"),
    ex(":map", ":map", Implemented, Both, "map", "map", Some(Keys(":map ")),
       "lists and defines mappings"),
    ex(":mapc[lear]", ":mapclear", Partial, FullOnly, "mapclear", "mapc", Some(Keys(":mapclear")),
       "works, but `:mapc` is rejected"),
    ex(":marks", ":marks", Implemented, Both, "marks", "marks", Some(Keys(":marks<CR>")),
       "lists marks (see #1226 for what it omits)"),
    ex(":mat[ch]", ":match", NotImplemented, Neither, "match", "mat", None,
       "define a match to highlight — tree-sitter plus the theme registry replace Vim's syntax/highlight files; low value"),
    ex(":me[nu]", ":menu", Skipped(MENU), Neither, "menu", "me", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":mes[sages]", ":messages", NotImplemented, Neither, "messages", "mes", None,
       "view previously displayed messages — informational commands with no vimcode surface; `:messages` and `:oldfiles` (vimcode already has a recent-files picker) are the two worth adding"),
    ex(":mk[exrc]", ":mkexrc", Skipped(SESSION), Neither, "mkexrc", "mk", None,
       "no :mksession/:mkview support planned"),
    ex(":mks[ession]", ":mksession", Skipped(SESSION), Neither, "mksession", "mks", None,
       "no :mksession/:mkview support planned"),
    ex(":mksp[ell]", ":mkspell", NotImplemented, Neither, "mkspell", "mksp", None,
       "produce .spl spell file — vimcode ships a real spell checker (`src/core/spell.rs`, #1163) but exposes no `:spell*` ex command for it — the cheapest ❌ family in this slice to close"),
    ex(":mkv[imrc]", ":mkvimrc", Skipped(SESSION), Neither, "mkvimrc", "mkv", None,
       "no :mksession/:mkview support planned"),
    ex(":mkvie[w]", ":mkview", Skipped(SESSION), Neither, "mkview", "mkvie", None,
       "no :mksession/:mkview support planned"),
    ex(":mod[e]", ":mode", Skipped(TERMCAP), Neither, "mode", "mod", None,
       "terminal/redraw control belongs to quadraui; vimcode's backends repaint on their own"),
    ex(":n[ext]", ":next", NotImplemented, Neither, "next", "n", None,
       "go to next file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":new", ":new", Implemented, Both, "new", "new", Some(Keys(":new<CR>")),
       "splits with a new empty buffer"),
    ex(":nm[ap]", ":nmap", Partial, FullOnly, "nmap", "nm", Some(Label("map:nmap_chases_recursively")),
       "works, but `:nm` — Vim's documented minimum — is rejected"),
    ex(":nmapc[lear]", ":nmapclear", Partial, FullOnly, "nmapclear", "nmapc", Some(Keys(":nmapclear")),
       "works, but `:nmapc` is rejected"),
    ex(":nme[nu]", ":nmenu", Skipped(MENU), Neither, "nmenu", "nme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":nn[oremap]", ":nnoremap", Partial, FullOnly, "nnoremap", "nn", Some(Label("map:nnoremap_does_not_chase")),
       "works, but `:nn` is rejected"),
    ex(":nnoreme[nu]", ":nnoremenu", Skipped(MENU), Neither, "nnoremenu", "nnoreme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":noa[utocmd]", ":noautocmd", NotImplemented, Neither, "noautocmd", "noa", None,
       "following commands don't trigger autocommands — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":no[remap]", ":noremap", Partial, FullOnly, "noremap", "no", Some(Keys(":noremap ")),
       "works, but `:no` is rejected"),
    ex(":noh[lsearch]", ":nohlsearch", Implemented, Both, "nohlsearch", "noh", Some(Label("ex:noh no effect")),
       "clears search highlighting"),
    ex(":norea[bbrev]", ":noreabbrev", Implemented, Both, "noreabbrev", "norea", Some(Keys(":noreabbrev ")),
       "non-recursive abbreviation"),
    ex(":noreme[nu]", ":noremenu", Skipped(MENU), Neither, "noremenu", "noreme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":norm[al]", ":normal", Implemented, Both, "normal x", "norm x", Some(Label("ex:normal Ax")),
       "`:normal`/`:normal!` with a range, replaying real keystrokes"),
    ex(":nos[wapfile]", ":noswapfile", NotImplemented, Neither, "noswapfile", "nos", None,
       "following commands don't create a swap file — vimcode has swap files and recovery (`tests/swap_recovery.rs`) but no ex commands for them; `:recover` is worth implementing"),
    ex(":nu[mber]", ":number", Partial, Both, "number", "nu", Some(Keys(":number<CR>")),
       "prints the current line numbered, but `:1,2#`/`:1,2number` is rejected — no range"),
    ex(":nun[map]", ":nunmap", Partial, FullOnly, "nunmap", "nun", Some(Keys(":nunmap ")),
       "works, but `:nun` is rejected"),
    ex(":nunme[nu]", ":nunmenu", Skipped(MENU), Neither, "nunmenu", "nunme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":ol[dfiles]", ":oldfiles", NotImplemented, Neither, "oldfiles", "ol", None,
       "list files that have marks in the |shada| file — informational commands with no vimcode surface; `:messages` and `:oldfiles` (vimcode already has a recent-files picker) are the two worth adding"),
    ex(":om[ap]", ":omap", Partial, FullOnly, "omap", "om", Some(Keys(":omap ")),
       "works, but `:om` is rejected"),
    ex(":omapc[lear]", ":omapclear", Partial, FullOnly, "omapclear", "omapc", Some(Keys(":omapclear")),
       "works, but `:omapc` is rejected"),
    ex(":ome[nu]", ":omenu", Skipped(MENU), Neither, "omenu", "ome", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":on[ly]", ":only", Implemented, Both, "only", "on", Some(Keys(":only<CR>")),
       "closes every other window"),
    ex(":ono[remap]", ":onoremap", Partial, FullOnly, "onoremap", "ono", Some(Label("map:onoremap_extends_a_motion")),
       "works, but `:ono` is rejected"),
    ex(":onoreme[nu]", ":onoremenu", Skipped(MENU), Neither, "onoremenu", "onoreme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":opt[ions]", ":options", Skipped(SCRIPTRT), Neither, "options", "opt", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":ou[nmap]", ":ounmap", Partial, FullOnly, "ounmap", "ou", Some(Keys(":ounmap ")),
       "works, but `:ou` is rejected"),
    ex(":ounme[nu]", ":ounmenu", Skipped(MENU), Neither, "ounmenu", "ounme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":pa[ckadd]", ":packadd", Skipped(SCRIPTRT), Neither, "packadd", "pa", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":packl[oadall]", ":packloadall", Skipped(SCRIPTRT), Neither, "packloadall", "packl", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":pb[uffer]", ":pbuffer", NotImplemented, Neither, "pbuffer", "pb", None,
       "edit buffer in the preview window — no preview window; LSP hover and the peek panel cover the same ground; low value"),
    ex(":pc[lose]", ":pclose", NotImplemented, Neither, "pclose", "pc", None,
       "close preview window — no preview window; LSP hover and the peek panel cover the same ground; low value"),
    ex(":ped[it]", ":pedit", NotImplemented, Neither, "pedit", "ped", None,
       "edit file in the preview window — no preview window; LSP hover and the peek panel cover the same ground; low value"),
    ex(":pe[rl]", ":perl", Skipped(INTERP), Neither, "perl", "pe", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":perld[o]", ":perldo", Skipped(INTERP), Neither, "perldo", "perld", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":perlf[ile]", ":perlfile", Skipped(INTERP), Neither, "perlfile", "perlf", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":p[rint]", ":print", Partial, Both, "print", "p", Some(Keys(":print<CR>")),
       "prints the current line, but `:1,2p` is rejected — no range, and no `l`/`#` flags"),
    ex(":profd[el]", ":profdel", Skipped(VIMSCRIPT), Neither, "profdel", "profd", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":prof[ile]", ":profile", Skipped(VIMSCRIPT), Neither, "profile", "prof", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":po[p]", ":pop", Skipped(CTAGS), Neither, "pop", "po", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":popu[p]", ":popup", Skipped(MENU), Neither, "popup", "popu", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":pp[op]", ":ppop", Skipped(CTAGS), Neither, "ppop", "pp", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":pre[serve]", ":preserve", NotImplemented, Neither, "preserve", "pre", None,
       "write all text to swap file — vimcode has swap files and recovery (`tests/swap_recovery.rs`) but no ex commands for them; `:recover` is worth implementing"),
    ex(":prev[ious]", ":previous", NotImplemented, Neither, "previous", "prev", None,
       "go to previous file in argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":ps[earch]", ":psearch", Skipped(CTAGS), Neither, "psearch", "ps", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":pt[ag]", ":ptag", Skipped(CTAGS), Neither, "ptag", "pt", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":ptN[ext]", ":ptNext", Skipped(CTAGS), Neither, "ptNext", "ptN", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":ptf[irst]", ":ptfirst", Skipped(CTAGS), Neither, "ptfirst", "ptf", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":ptj[ump]", ":ptjump", Skipped(CTAGS), Neither, "ptjump", "ptj", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":ptl[ast]", ":ptlast", Skipped(CTAGS), Neither, "ptlast", "ptl", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":ptn[ext]", ":ptnext", Skipped(CTAGS), Neither, "ptnext", "ptn", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":ptp[revious]", ":ptprevious", Skipped(CTAGS), Neither, "ptprevious", "ptp", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":ptr[ewind]", ":ptrewind", Skipped(CTAGS), Neither, "ptrewind", "ptr", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":pts[elect]", ":ptselect", Skipped(CTAGS), Neither, "ptselect", "pts", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":pu[t]", ":put", Implemented, Both, "put", "pu", Some(Label("ex:put a")),
       "puts a register after a line, with `!` and `:0put`"),
    ex(":pw[d]", ":pwd", Implemented, Both, "pwd", "pw", Some(Keys(":pwd<CR>")),
       "prints the working directory"),
    ex(":py3", ":py3", Skipped(INTERP), Neither, "py3", "py3", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":python3", ":python3", Skipped(INTERP), Neither, "python3", "python3", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":py3d[o]", ":py3do", Skipped(INTERP), Neither, "py3do", "py3d", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":py3f[ile]", ":py3file", Skipped(INTERP), Neither, "py3file", "py3f", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":py[thon]", ":python", Skipped(INTERP), Neither, "python", "py", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":pyd[o]", ":pydo", Skipped(INTERP), Neither, "pydo", "pyd", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":pyf[ile]", ":pyfile", Skipped(INTERP), Neither, "pyfile", "pyf", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":pyx", ":pyx", Skipped(INTERP), Neither, "pyx", "pyx", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":pythonx", ":pythonx", Skipped(INTERP), Neither, "pythonx", "pythonx", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":pyxd[o]", ":pyxdo", Skipped(INTERP), Neither, "pyxdo", "pyxd", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":pyxf[ile]", ":pyxfile", Skipped(INTERP), Neither, "pyxfile", "pyxf", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":q[uit]", ":quit", Implemented, Both, "quit", "q", Some(Keys(":quit<CR>")),
       "closes the window, refusing on unsaved changes without `!`"),
    ex(":quita[ll]", ":quitall", NotImplemented, Neither, "quitall", "quita", None,
       "quit Vim — no vimcode equivalent; low value"),
    ex(":qa[ll]", ":qall", Implemented, Both, "qall", "qa", Some(Keys(":qall<CR>")),
       "quits everything"),
    ex(":r[ead]", ":read", Implemented, Both, "read foo", "r foo", Some(Label("ex:r !echo")),
       "`:r {file}` inserts a file and `:r !{cmd}` inserts a command's stdout"),
    ex(":rec[over]", ":recover", NotImplemented, Neither, "recover", "rec", None,
       "recover a file from a swap file — vimcode has swap files and recovery (`tests/swap_recovery.rs`) but no ex commands for them; `:recover` is worth implementing"),
    ex(":red[o]", ":redo", Implemented, Both, "redo", "red", Some(Label("ex:undo redo")),
       "redo"),
    ex(":redi[r]", ":redir", Skipped(VIMSCRIPT), Neither, "redir", "redi", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":redr[aw]", ":redraw", Skipped(TERMCAP), Neither, "redraw", "redr", None,
       "terminal/redraw control belongs to quadraui; vimcode's backends repaint on their own"),
    ex(":redraws[tatus]", ":redrawstatus", Skipped(TERMCAP), Neither, "redrawstatus", "redraws", None,
       "terminal/redraw control belongs to quadraui; vimcode's backends repaint on their own"),
    ex(":reg[isters]", ":registers", Implemented, Both, "registers", "reg", Some(Keys(":registers<CR>")),
       "lists registers"),
    ex(":res[ize]", ":resize", NotImplemented, Neither, "resize", "res", None,
       "change current window height — `:wincmd` and `:split`/`:vsplit` cover the resize and split axes, but the `:{mod} {cmd}` modifier grammar is not parsed at all; moderate value"),
    ex(":restart", ":restart", NotImplemented, Both, "restart", "restart", None,
       "Vim's restart — vimcode binds the name to its DAP debugger's restart-session, so the Vim command is shadowed rather than missing; low value, but the collision is worth a rename"),
    ex(":ret[ab]", ":retab", Implemented, Both, "retab", "ret", Some(Label("ex:retab")),
       "retabs a range, honouring 'expandtab' and `!`"),
    ex(":retu[rn]", ":return", Skipped(VIMSCRIPT), Neither, "return", "retu", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":rew[ind]", ":rewind", NotImplemented, Neither, "rewind", "rew", None,
       "go to the first file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":ri[ght]", ":right", Implemented, Both, "right", "ri", Some(Label("ex:ri 10")),
       "right-aligns within a width"),
    ex(":rightb[elow]", ":rightbelow", NotImplemented, Neither, "rightbelow", "rightb", None,
       "make split window appear right or below — `:wincmd` and `:split`/`:vsplit` cover the resize and split axes, but the `:{mod} {cmd}` modifier grammar is not parsed at all; moderate value"),
    ex(":rsh[ada]", ":rshada", Skipped(SHADA), Neither, "rshada", "rsh", None,
       "no ShaDa/viminfo file; vimcode persists its own session state (src/core/session.rs)"),
    ex(":rub[y]", ":ruby", Skipped(INTERP), Neither, "ruby", "rub", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":rubyd[o]", ":rubydo", Skipped(INTERP), Neither, "rubydo", "rubyd", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":rubyf[ile]", ":rubyfile", Skipped(INTERP), Neither, "rubyfile", "rubyf", None,
       "language-binding ex command; vimcode's Lua runtime is the extension API, not a `:` command"),
    ex(":rund[o]", ":rundo", NotImplemented, Neither, "rundo", "rund", None,
       "read undo information from a file — the undo-tree surface; tracked by #1156, which is implementing persistent undo and the tree"),
    ex(":ru[ntime]", ":runtime", Skipped(SCRIPTRT), Neither, "runtime", "ru", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":s[ubstitute]", ":substitute", Implemented, Both, "s/alpha/A/", "s/alpha/A/", Some(Label("sub:basic")),
       "the full `:s` surface: flags, ranges, `\\\\v`, `\\\\zs`, `c` confirm, 'gdefault'"),
    ex(":sN[ext]", ":sNext", NotImplemented, Neither, "sNext", "sN", None,
       "split window and go to previous file in argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":san[dbox]", ":sandbox", Skipped(VIMSCRIPT), Neither, "sandbox", "san", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":sa[rgument]", ":sargument", NotImplemented, Neither, "sargument", "sa", None,
       "split window and go to specific file in argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sal[l]", ":sall", NotImplemented, Neither, "sall", "sal", None,
       "open a window for each file in argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sav[eas]", ":saveas", Partial, Both, "saveas", "sav", Some(Keys(":saveas ")),
       "`:saveas {file}` writes and renames, but bare `:saveas` is a silent no-op where Vim reports E471"),
    ex(":sb[uffer]", ":sbuffer", NotImplemented, Neither, "sbuffer", "sb", None,
       "split window and go to specific file in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sbN[ext]", ":sbNext", NotImplemented, Neither, "sbNext", "sbN", None,
       "split window and go to previous file in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sba[ll]", ":sball", NotImplemented, Neither, "sball", "sba", None,
       "open a window for each file in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sbf[irst]", ":sbfirst", NotImplemented, Neither, "sbfirst", "sbf", None,
       "split window and go to first file in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sbl[ast]", ":sblast", NotImplemented, Neither, "sblast", "sbl", None,
       "split window and go to last file in buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sbm[odified]", ":sbmodified", NotImplemented, Neither, "sbmodified", "sbm", None,
       "split window and go to modified file in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sbn[ext]", ":sbnext", NotImplemented, Neither, "sbnext", "sbn", None,
       "split window and go to next file in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sbp[revious]", ":sbprevious", NotImplemented, Neither, "sbprevious", "sbp", None,
       "split window and go to previous file in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sbr[ewind]", ":sbrewind", NotImplemented, Neither, "sbrewind", "sbr", None,
       "split window and go to first file in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":scr[iptnames]", ":scriptnames", Skipped(SCRIPTRT), Neither, "scriptnames", "scr", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":se[t]", ":set", Implemented, Both, "set", "se", Some(Keys(":set ")),
       "the option surface audited in full by #1225"),
    ex(":setf[iletype]", ":setfiletype", NotImplemented, Neither, "setfiletype", "setf", None,
       "set 'filetype', unless it was set already — `:set` exists but has no local/global split (vimcode's settings are global), and `:setfiletype`/`:filetype` are absent; `:setfiletype` is worth implementing"),
    ex(":setg[lobal]", ":setglobal", NotImplemented, Neither, "setglobal", "setg", None,
       "show global values of options — `:set` exists but has no local/global split (vimcode's settings are global), and `:setfiletype`/`:filetype` are absent; `:setfiletype` is worth implementing"),
    ex(":setl[ocal]", ":setlocal", NotImplemented, Neither, "setlocal", "setl", None,
       "show or set options locally — `:set` exists but has no local/global split (vimcode's settings are global), and `:setfiletype`/`:filetype` are absent; `:setfiletype` is worth implementing"),
    ex(":sf[ind]", ":sfind", NotImplemented, Neither, "sfind", "sf", None,
       "split current window and edit file in 'path' — no vimcode equivalent; low value"),
    ex(":sfir[st]", ":sfirst", NotImplemented, Neither, "sfirst", "sfir", None,
       "split window and go to first file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sig[n]", ":sign", NotImplemented, Neither, "sign", "sig", None,
       "manipulate signs — vimcode paints LSP diagnostics in the gutter but exposes no `:sign` API; low value"),
    ex(":sil[ent]", ":silent", Skipped(VIMSCRIPT), Neither, "silent", "sil", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":sl[eep]", ":sleep", NotImplemented, Neither, "sleep", "sl", None,
       "do nothing for a few seconds — only useful inside scripts; low value"),
    ex(":sl[eep]!", ":sleep!", NotImplemented, Neither, "sleep!", "sl!", None,
       "`:sleep` without a visible cursor; blocked on `:sleep`"),
    ex(":sla[st]", ":slast", NotImplemented, Neither, "slast", "sla", None,
       "split window and go to last file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sm[agic]", ":smagic", NotImplemented, Neither, "smagic/alpha/A/", "sm/alpha/A/", None,
       ":substitute with 'magic' — no vimcode equivalent; low value"),
    ex(":smap", ":smap", Skipped(SELECT), Both, "smap", "smap", None,
       "recognised and stored, but vimcode has no Select mode, so the mapping can never fire"),
    ex(":smapc[lear]", ":smapclear", Skipped(SELECT), FullOnly, "smapclear", "smapc", None,
       "recognised, but there is no Select mode for it to apply to"),
    ex(":sme[nu]", ":smenu", Skipped(MENU), Neither, "smenu", "sme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":sn[ext]", ":snext", NotImplemented, Neither, "snext", "sn", None,
       "split window and go to next file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sno[magic]", ":snomagic", NotImplemented, Neither, "snomagic/alpha/A/", "sno/alpha/A/", None,
       ":substitute with 'nomagic' — no vimcode equivalent; low value"),
    ex(":snor[emap]", ":snoremap", Skipped(SELECT), FullOnly, "snoremap", "snor", None,
       "recognised and stored, but there is no Select mode for it to apply to"),
    ex(":snoreme[nu]", ":snoremenu", Skipped(MENU), Neither, "snoremenu", "snoreme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":sor[t]", ":sort", Implemented, Both, "sort", "sor", Some(Label("ex:sort")),
       "sorts with `n`, `i`, `u`, `r`, `!` and a `/pat/`"),
    ex(":so[urce]", ":source", Skipped(SCRIPTRT), Neither, "source", "so", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":spelld[ump]", ":spelldump", NotImplemented, Neither, "spelldump", "spelld", None,
       "split window and fill with all correct words — vimcode ships a real spell checker (`src/core/spell.rs`, #1163) but exposes no `:spell*` ex command for it — the cheapest ❌ family in this slice to close"),
    ex(":spe[llgood]", ":spellgood", NotImplemented, Neither, "spellgood", "spe", None,
       "add good word for spelling — vimcode ships a real spell checker (`src/core/spell.rs`, #1163) but exposes no `:spell*` ex command for it — the cheapest ❌ family in this slice to close"),
    ex(":spelli[nfo]", ":spellinfo", NotImplemented, Neither, "spellinfo", "spelli", None,
       "show info about loaded spell files — vimcode ships a real spell checker (`src/core/spell.rs`, #1163) but exposes no `:spell*` ex command for it — the cheapest ❌ family in this slice to close"),
    ex(":spellra[re]", ":spellrare", NotImplemented, Neither, "spellrare", "spellra", None,
       "add rare word for spelling — vimcode ships a real spell checker (`src/core/spell.rs`, #1163) but exposes no `:spell*` ex command for it — the cheapest ❌ family in this slice to close"),
    ex(":spellr[epall]", ":spellrepall", NotImplemented, Neither, "spellrepall", "spellr", None,
       "replace all bad words like last |z=| — vimcode ships a real spell checker (`src/core/spell.rs`, #1163) but exposes no `:spell*` ex command for it — the cheapest ❌ family in this slice to close"),
    ex(":spellu[ndo]", ":spellundo", NotImplemented, Neither, "spellundo", "spellu", None,
       "remove good or bad word — vimcode ships a real spell checker (`src/core/spell.rs`, #1163) but exposes no `:spell*` ex command for it — the cheapest ❌ family in this slice to close"),
    ex(":spellw[rong]", ":spellwrong", NotImplemented, Neither, "spellwrong", "spellw", None,
       "add spelling mistake — vimcode ships a real spell checker (`src/core/spell.rs`, #1163) but exposes no `:spell*` ex command for it — the cheapest ❌ family in this slice to close"),
    ex(":sp[lit]", ":split", Implemented, Both, "split", "sp", Some(Keys(":split<CR>")),
       "horizontal split"),
    ex(":spr[evious]", ":sprevious", NotImplemented, Neither, "sprevious", "spr", None,
       "split window and go to previous file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sre[wind]", ":srewind", NotImplemented, Neither, "srewind", "sre", None,
       "split window and go to first file in the argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":st[op]", ":stop", NotImplemented, FullOnly, "stop", "st", None,
       "suspend the editor — vimcode binds `:stop` to its DAP debugger instead, so `CTRL-Z`'s ex spelling does nothing a TUI user expects; worth implementing in the TUI backend and renaming the DAP command"),
    ex(":sta[g]", ":stag", Skipped(CTAGS), Neither, "stag", "sta", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":star[tinsert]", ":startinsert", Implemented, Both, "startinsert", "star", Some(Label("ex:startinsert then type")),
       "enters Insert mode, with `!` for end-of-line"),
    ex(":startr[eplace]", ":startreplace", NotImplemented, Neither, "startreplace", "startr", None,
       "start Replace mode — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":stopi[nsert]", ":stopinsert", Implemented, Both, "stopinsert", "stopi", Some(Label("ex:stopinsert noop when already Normal")),
       "leaves Insert mode"),
    ex(":stj[ump]", ":stjump", Skipped(CTAGS), Neither, "stjump", "stj", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":sts[elect]", ":stselect", Skipped(CTAGS), Neither, "stselect", "sts", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":sun[hide]", ":sunhide", NotImplemented, Neither, "sunhide", "sun", None,
       "same as \":unhide\" — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":sunm[ap]", ":sunmap", Skipped(SELECT), FullOnly, "sunmap", "sunm", None,
       "recognised, but there is no Select mode for it to apply to"),
    ex(":sunme[nu]", ":sunmenu", Skipped(MENU), Neither, "sunmenu", "sunme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":sus[pend]", ":suspend", NotImplemented, Neither, "suspend", "sus", None,
       "same as \":stop\" — no vimcode equivalent; low value"),
    ex(":sv[iew]", ":sview", NotImplemented, Neither, "sview", "sv", None,
       "split window and edit file read-only — no vimcode equivalent; low value"),
    ex(":sw[apname]", ":swapname", NotImplemented, Neither, "swapname", "sw", None,
       "show the name of the current swap file — vimcode has swap files and recovery (`tests/swap_recovery.rs`) but no ex commands for them; `:recover` is worth implementing"),
    ex(":sy[ntax]", ":syntax", NotImplemented, Neither, "syntax", "sy", None,
       "syntax highlighting — tree-sitter plus the theme registry replace Vim's syntax/highlight files; low value"),
    ex(":synti[me]", ":syntime", Skipped(VIMSCRIPT), Neither, "syntime", "synti", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":sync[bind]", ":syncbind", NotImplemented, Neither, "syncbind", "sync", None,
       "sync scroll binding — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":t", ":t", Implemented, Both, "t 0", "t 0", Some(Label("ex:1,2t$")),
       "the short form of `:copy`"),
    ex(":tN[ext]", ":tNext", Skipped(CTAGS), Neither, "tNext", "tN", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":tabN[ext]", ":tabNext", NotImplemented, Neither, "tabNext", "tabN", None,
       "go to previous tabpage — no vimcode equivalent; low value"),
    ex(":tabc[lose]", ":tabclose", Implemented, Both, "tabclose", "tabc", Some(Keys(":tabclose<CR>")),
       "closes a tab, refusing on the last one"),
    ex(":tabd[o]", ":tabdo", Implemented, Both, "tabdo s/a/b/", "tabdo s/a/b/", Some(Keys(":tabdo ")),
       "runs a command in every tab"),
    ex(":tabe[dit]", ":tabedit", Partial, AbbrevOnly, "tabedit", "tabe", Some(Keys(":tabe ")),
       "`:tabe`/`:tabnew` work; the full spelling `:tabedit` is rejected"),
    ex(":tabf[ind]", ":tabfind", NotImplemented, Neither, "tabfind", "tabf", None,
       "find file in 'path', edit it in a new tabpage — no vimcode equivalent; low value"),
    ex(":tabfir[st]", ":tabfirst", Implemented, Both, "tabfirst", "tabfir", Some(Label("ex:tabfirst noop with one tab")),
       "first tab"),
    ex(":tabl[ast]", ":tablast", Implemented, Both, "tablast", "tabl", Some(Label("ex:tablast noop with one tab")),
       "last tab"),
    ex(":tabm[ove]", ":tabmove", Implemented, Both, "tabmove", "tabm", Some(Keys(":tabmove")),
       "moves the tab"),
    ex(":tabnew", ":tabnew", Implemented, Both, "tabnew", "tabnew", Some(Keys(":tabnew")),
       "new tab"),
    ex(":tabn[ext]", ":tabnext", Implemented, Both, "tabnext", "tabn", Some(Keys(":tabnext<CR>")),
       "next tab"),
    ex(":tabo[nly]", ":tabonly", Implemented, Both, "tabonly", "tabo", Some(Label("ex:tabonly noop with one tab")),
       "closes every other tab"),
    ex(":tabp[revious]", ":tabprevious", Implemented, Both, "tabprevious", "tabp", Some(Keys(":tabprevious<CR>")),
       "previous tab"),
    ex(":tabr[ewind]", ":tabrewind", NotImplemented, Neither, "tabrewind", "tabr", None,
       "go to first tabpage — no vimcode equivalent; low value"),
    ex(":tabs", ":tabs", Implemented, Both, "tabs", "tabs", Some(Keys(":tabs<CR>")),
       "lists tabs"),
    ex(":tab", ":tab", NotImplemented, Neither, "tab", "tab", None,
       "create new tab when opening new window — `:wincmd` and `:split`/`:vsplit` cover the resize and split axes, but the `:{mod} {cmd}` modifier grammar is not parsed at all; moderate value"),
    ex(":ta[g]", ":tag", Skipped(CTAGS), Neither, "tag", "ta", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":tags", ":tags", Skipped(CTAGS), Neither, "tags", "tags", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":tc[d]", ":tcd", NotImplemented, Neither, "tcd", "tc", None,
       "change directory for tabpage — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":tch[dir]", ":tchdir", NotImplemented, Neither, "tchdir", "tch", None,
       "change directory for tabpage — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":te[rminal]", ":terminal", Implemented, Both, "terminal", "te", Some(Keys(":terminal<CR>")),
       "opens the terminal panel"),
    ex(":tf[irst]", ":tfirst", Skipped(CTAGS), Neither, "tfirst", "tf", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":th[row]", ":throw", Skipped(VIMSCRIPT), Neither, "throw", "th", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":tj[ump]", ":tjump", Skipped(CTAGS), Neither, "tjump", "tj", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":tl[ast]", ":tlast", Skipped(CTAGS), Neither, "tlast", "tl", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":tlm[enu]", ":tlmenu", Skipped(MENU), Neither, "tlmenu", "tlm", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":tln[oremenu]", ":tlnoremenu", Skipped(MENU), Neither, "tlnoremenu", "tln", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":tlu[nmenu]", ":tlunmenu", Skipped(MENU), Neither, "tlunmenu", "tlu", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":tmapc[lear]", ":tmapclear", NotImplemented, Neither, "tmapclear", "tmapc", None,
       "remove all mappings for |Terminal-mode| — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":tma[p]", ":tmap", NotImplemented, Neither, "tmap", "tma", None,
       "like \":map\" but for |Terminal-mode| — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":tm[enu]", ":tmenu", Skipped(MENU), Neither, "tmenu", "tm", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":tn[ext]", ":tnext", Skipped(CTAGS), Neither, "tnext", "tn", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":tno[remap]", ":tnoremap", NotImplemented, Neither, "tnoremap", "tno", None,
       "like \":noremap\" but for |Terminal-mode| — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":to[pleft]", ":topleft", NotImplemented, Neither, "topleft", "to", None,
       "make split window appear at top or far left — `:wincmd` and `:split`/`:vsplit` cover the resize and split axes, but the `:{mod} {cmd}` modifier grammar is not parsed at all; moderate value"),
    ex(":tp[revious]", ":tprevious", Skipped(CTAGS), Neither, "tprevious", "tp", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":tr[ewind]", ":trewind", Skipped(CTAGS), Neither, "trewind", "tr", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":trust", ":trust", Skipped(SCRIPTRT), Neither, "trust", "trust", None,
       "needs a script/plugin runtime vimcode does not have"),
    ex(":try", ":try", Skipped(VIMSCRIPT), Neither, "try", "try", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":ts[elect]", ":tselect", Skipped(CTAGS), Neither, "tselect", "ts", None,
       "tags/'include' file search; vimcode uses LSP go-to-definition and references instead"),
    ex(":tunma[p]", ":tunmap", NotImplemented, Neither, "tunmap", "tunma", None,
       "like \":unmap\" but for |Terminal-mode| — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":tu[nmenu]", ":tunmenu", NotImplemented, Neither, "tunmenu", "tu", None,
       "remove menu tooltip — no vimcode equivalent; low value"),
    ex(":u[ndo]", ":undo", Implemented, Both, "undo", "u", Some(Label("ex:undo")),
       "undo — backed by a real undo tree since #1156, so `g-`/`g+`/`:earlier`/`:later` can reach discarded branches"),
    ex(":undoj[oin]", ":undojoin", Implemented, Both, "undojoin", "undoj", Some(Label("ex:undojoin")),
       "join next change with previous undo block — landed in #1156; the oracle case pins the `E790` no-previous-change path, since Vim documents interactive `:undojoin` as fragile"),
    ex(":undol[ist]", ":undolist", Implemented, Both, "undolist", "undol", Some(Label("ex:undolist")),
       "list leafs of the undo tree — landed with the undo tree in #1156; message-only, so the oracle case pins that it leaves the buffer alone"),
    ex(":una[bbreviate]", ":unabbreviate", Implemented, Both, "unabbreviate", "una", Some(Keys(":unabbreviate ")),
       "removes an abbreviation"),
    ex(":unh[ide]", ":unhide", NotImplemented, Neither, "unhide", "unh", None,
       "open a window for each loaded file in the buffer list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":uni[q]", ":uniq", NotImplemented, Neither, "uniq", "uni", None,
       "uniq lines — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":unl[et]", ":unlet", Skipped(VIMSCRIPT), Neither, "unlet", "unl", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":unlo[ckvar]", ":unlockvar", Skipped(VIMSCRIPT), Neither, "unlockvar", "unlo", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":unm[ap]", ":unmap", Partial, FullOnly, "unmap", "unm", Some(Keys(":unmap ")),
       "works, but `:unm` is rejected"),
    ex(":unme[nu]", ":unmenu", Skipped(MENU), Neither, "unmenu", "unme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":uns[ilent]", ":unsilent", Skipped(VIMSCRIPT), Neither, "unsilent", "uns", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":up[date]", ":update", Partial, Both, "update", "up", Some(Keys(":update<CR>")),
       "writes only when modified, but `:update {file}` is rejected"),
    ex(":v[global]", ":vglobal", Implemented, Both, "vglobal/beta/d", "v/beta/d", Some(Keys(":v/")),
       "the inverse of `:global`"),
    ex(":ve[rsion]", ":version", Implemented, Both, "version", "ve", Some(Keys(":version<CR>")),
       "prints the version"),
    ex(":verb[ose]", ":verbose", NotImplemented, Neither, "verbose", "verb", None,
       "execute command with 'verbose' set — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":vert[ical]", ":vertical", NotImplemented, Neither, "vertical", "vert", None,
       "make following command split vertically — `:wincmd` and `:split`/`:vsplit` cover the resize and split axes, but the `:{mod} {cmd}` modifier grammar is not parsed at all; moderate value"),
    ex(":vim[grep]", ":vimgrep", Implemented, Both, "vimgrep", "vim", Some(Keys(":vimgrep ")),
       "shares `:grep`'s implementation"),
    ex(":vimgrepa[dd]", ":vimgrepadd", NotImplemented, Neither, "vimgrepadd", "vimgrepa", None,
       "like :vimgrep, but append to current list — vimcode's quickfix list is filled by `:grep`/`:make` and LSP diagnostics, never from an error file or expression; low value"),
    ex(":vi[sual]", ":visual", Skipped(EXMODE), Neither, "visual", "vi", None,
       "Ex/Open mode line editing is out of scope"),
    ex(":viu[sage]", ":viusage", NotImplemented, Neither, "viusage", "viu", None,
       "overview of Normal mode commands — informational commands with no vimcode surface; `:messages` and `:oldfiles` (vimcode already has a recent-files picker) are the two worth adding"),
    ex(":vie[w]", ":view", NotImplemented, Neither, "view", "vie", None,
       "edit a file read-only — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":vm[ap]", ":vmap", Partial, FullOnly, "vmap", "vm", Some(Keys(":vmap ")),
       "works, but `:vm` is rejected"),
    ex(":vmapc[lear]", ":vmapclear", Partial, FullOnly, "vmapclear", "vmapc", Some(Keys(":vmapclear")),
       "works, but `:vmapc` is rejected"),
    ex(":vme[nu]", ":vmenu", Skipped(MENU), Neither, "vmenu", "vme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":vne[w]", ":vnew", Implemented, Both, "vnew", "vne", Some(Keys(":vnew<CR>")),
       "vertical split with a new empty buffer"),
    ex(":vn[oremap]", ":vnoremap", Partial, FullOnly, "vnoremap", "vn", Some(Keys(":vnoremap ")),
       "works, but `:vn` is rejected"),
    ex(":vnoreme[nu]", ":vnoremenu", Skipped(MENU), Neither, "vnoremenu", "vnoreme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":vs[plit]", ":vsplit", Implemented, Both, "vsplit", "vs", Some(Keys(":vsplit<CR>")),
       "vertical split"),
    ex(":vu[nmap]", ":vunmap", Partial, FullOnly, "vunmap", "vu", Some(Keys(":vunmap ")),
       "works, but `:vu` is rejected"),
    ex(":vunme[nu]", ":vunmenu", Skipped(MENU), Neither, "vunmenu", "vunme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":wind[o]", ":windo", Implemented, Both, "windo s/a/b/", "windo s/a/b/", Some(Keys(":windo ")),
       "runs a command in every window"),
    ex(":w[rite]", ":write", Partial, Both, "write", "w", Some(Keys(":write<CR>")),
       "writes the current buffer, but `:w {file}`, `:w >>{file}` and `:w !{cmd}` are all rejected — `:saveas` is the only way to write elsewhere"),
    ex(":wN[ext]", ":wNext", NotImplemented, Neither, "wNext", "wN", None,
       "write to a file and go to previous file in argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":wa[ll]", ":wall", Implemented, Both, "wall", "wa", Some(Keys(":wall<CR>")),
       "writes every modified buffer"),
    ex(":wh[ile]", ":while", Skipped(VIMSCRIPT), Neither, "while", "wh", None,
       "VimScript: out of scope per the standing decision (vimcode implements Vim keybindings and editing, not a VimScript runtime)"),
    ex(":wi[nsize]", ":winsize", Skipped(MENU), Neither, "winsize", "wi", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":winc[md]", ":wincmd", Implemented, Both, "wincmd", "winc", Some(Keys(":wincmd ")),
       "the ex form of CTRL-W"),
    ex(":winp[os]", ":winpos", Skipped(MENU), Neither, "winpos", "winp", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":wn[ext]", ":wnext", NotImplemented, Neither, "wnext", "wn", None,
       "write to a file and go to next file in argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":wp[revious]", ":wprevious", NotImplemented, Neither, "wprevious", "wp", None,
       "write to a file and go to previous file in argument list — vimcode has no argument list, and splits+`:buffer` cover the `:s…`/`:sb…` family; moderate value for `:argdo` alone"),
    ex(":wq", ":wq", Partial, Both, "wq", "wq", Some(Keys(":wq<CR>")),
       "writes and quits, but `:wq {file}` is rejected"),
    ex(":wqa[ll]", ":wqall", Implemented, Both, "wqall", "wqa", Some(Keys(":wqall<CR>")),
       "writes everything and quits"),
    ex(":wsh[ada]", ":wshada", Skipped(SHADA), Neither, "wshada", "wsh", None,
       "no ShaDa/viminfo file; vimcode persists its own session state (src/core/session.rs)"),
    ex(":wu[ndo]", ":wundo", NotImplemented, Neither, "wundo", "wu", None,
       "write undo information to a file — the undo-tree surface; tracked by #1156, which is implementing persistent undo and the tree"),
    ex(":x[it]", ":xit", Partial, AbbrevOnly, "xit", "x", Some(Keys(":x<CR>")),
       "`:x` works; the full spelling `:xit` is rejected, and `:x {file}` too"),
    ex(":xa[ll]", ":xall", Implemented, Both, "xall", "xa", Some(Keys(":xall<CR>")),
       "writes the modified buffers and quits"),
    ex(":xmapc[lear]", ":xmapclear", Partial, FullOnly, "xmapclear", "xmapc", Some(Keys(":xmapclear")),
       "works, but `:xmapc` is rejected"),
    ex(":xm[ap]", ":xmap", Partial, FullOnly, "xmap", "xm", Some(Keys(":xmap ")),
       "works, but `:xm` is rejected"),
    ex(":xme[nu]", ":xmenu", Skipped(MENU), Neither, "xmenu", "xme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":xn[oremap]", ":xnoremap", Partial, FullOnly, "xnoremap", "xn", Some(Keys(":xnoremap ")),
       "works, but `:xn` is rejected"),
    ex(":xnoreme[nu]", ":xnoremenu", Skipped(MENU), Neither, "xnoremenu", "xnoreme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":xu[nmap]", ":xunmap", Partial, FullOnly, "xunmap", "xu", Some(Keys(":xunmap ")),
       "works, but `:xu` is rejected"),
    ex(":xunme[nu]", ":xunmenu", Skipped(MENU), Neither, "xunmenu", "xunme", None,
       "Vim's GUI menu/toolkit surface; vimcode's menus are quadraui widgets, not `:menu` entries"),
    ex(":y[ank]", ":yank", Implemented, Both, "yank", "y", Some(Label("ex:y a")),
       "yanks a range into a register, with a count"),
    ex(":z", ":z", NotImplemented, Neither, "z", "z", None,
       "print some lines — one-off commands with no vimcode equivalent; `:z`, `:list`, `:lcd`, `:view`, `:uniq` and the `:keep*`/`:lockmarks` modifiers are the ones a Vim user would actually miss"),
    ex(":~", ":~", Partial, Both, "~", "~", Some(Keys(":~<CR>")),
       "recognised, but it is a straight alias of `:&`: Vim's `:~` reuses the last *replacement string* from any `:s`, which vimcode does not track separately"),
];

/// ✅/🟡 rows that **no** oracle case reaches today. Shrink-only, exactly
/// like `COVERAGE_EXEMPT` and `REGMARK_COVERAGE_EXEMPT`: deleting an entry
/// requires a case that pins it, and a case that starts pinning one forces
/// its entry to be deleted. Writing those cases is #1162.
const EX_COVERAGE_EXEMPT: &[&str] = &[
    ":#",
    ":=",
    ":abc[lear]",
    ":b[uffer]",
    ":bd[elete]",
    ":bn[ext]",
    ":bp[revious]",
    ":bufd[o]",
    ":buffers",
    ":cN[ext]",
    ":ccl[ose]",
    ":cd",
    ":cdo",
    ":cfd[o]",
    ":cfir[st]",
    ":changes",
    ":cla[st]",
    ":cl[ist]",
    ":clo[se]",
    ":cm[ap]",
    ":cmapc[lear]",
    ":cn[ext]",
    ":cnew[er]",
    ":cno[remap]",
    ":cnorea[bbrev]",
    ":col[der]",
    ":colo[rscheme]",
    ":cope[n]",
    ":cp[revious]",
    ":cq[uit]",
    ":cu[nmap]",
    ":cw[indow]",
    ":diffo[ff]",
    ":diffs[plit]",
    ":difft[his]",
    ":dig[raphs]",
    ":di[splay]",
    ":e[dit]",
    ":f[ile]",
    ":files",
    ":gr[ep]",
    ":h[elp]",
    ":his[tory]",
    ":im[ap]",
    ":imapc[lear]",
    ":inorea[bbrev]",
    ":iu[nmap]",
    ":ju[mps]",
    ":lN[ext]",
    ":lcl[ose]",
    ":ld[o]",
    ":lfd[o]",
    ":lfir[st]",
    ":lgr[ep]",
    ":ll",
    ":lla[st]",
    ":lli[st]",
    ":lne[xt]",
    ":lop[en]",
    ":lp[revious]",
    ":ls",
    ":lv[imgrep]",
    ":lw[indow]",
    ":mak[e]",
    ":map",
    ":mapc[lear]",
    ":marks",
    ":new",
    ":nmapc[lear]",
    ":no[remap]",
    ":norea[bbrev]",
    ":nu[mber]",
    ":nun[map]",
    ":om[ap]",
    ":omapc[lear]",
    ":on[ly]",
    ":ou[nmap]",
    ":p[rint]",
    ":pw[d]",
    ":q[uit]",
    ":qa[ll]",
    ":reg[isters]",
    ":sav[eas]",
    ":sp[lit]",
    ":tabc[lose]",
    ":tabd[o]",
    ":tabe[dit]",
    ":tabm[ove]",
    ":tabn[ext]",
    ":tabp[revious]",
    ":tabs",
    ":te[rminal]",
    ":una[bbreviate]",
    ":unm[ap]",
    ":up[date]",
    ":ve[rsion]",
    ":vim[grep]",
    ":vm[ap]",
    ":vmapc[lear]",
    ":vne[w]",
    ":vn[oremap]",
    ":vs[plit]",
    ":vu[nmap]",
    ":wind[o]",
    ":w[rite]",
    ":wa[ll]",
    ":winc[md]",
    ":wq",
    ":wqa[ll]",
    ":x[it]",
    ":xa[ll]",
    ":xmapc[lear]",
    ":xm[ap]",
    ":xn[oremap]",
    ":xu[nmap]",
    ":~",
];

/// Rows whose `status` and `dispatch` disagree for a reason gate 1b must be
/// told about, rather than silently tolerating the shape.
///
/// `(cmd, reason)`. Kept tiny on purpose: every entry is a place where the
/// cross-check would otherwise fire, so an unexplained one is a bug in the
/// table, not in the rule.
const EX_STATUS_DISPATCH_EXCEPTIONS: &[(&str, &str)] = &[(
    ":*",
    "`:*` is the `'<,'>` range, so the probe — an ex line run with no prior \
     Visual selection — hits unset `'<`/`'>` marks and is refused. The corpus \
     case `ex:*d after visual` selects first and passes, which is what makes \
     the row ✅ despite a `Neither` measurement.",
)];

/// Drive one ex line through a real engine and report whether the dispatcher
/// recognised it. Black-box: an ex line in, `engine.message` out.
fn ex_line_is_recognised(line: &str) -> bool {
    let mut engine = engine_with("alpha\nbeta\ngamma\ndelta\n");
    // Same hermeticity fix as `replay_live` (#1226): `Engine::new()` loads
    // the user's real command history off disk.
    engine.history = Default::default();
    engine.set_viewport_lines(24);
    engine.execute_command(line);
    !engine.message.contains("Not an editor command")
}

/// Re-measure one row's [`ExDispatch`].
fn measured_ex_dispatch(probe_full: &str, probe_abbr: &str) -> ExDispatch {
    match (
        ex_line_is_recognised(probe_full),
        ex_line_is_recognised(probe_abbr),
    ) {
        (true, true) => ExDispatch::Both,
        (true, false) => ExDispatch::FullOnly,
        (false, true) => ExDispatch::AbbrevOnly,
        (false, false) => ExDispatch::Neither,
    }
}

/// Every row whose recorded dispatch no longer matches the live dispatcher.
fn ex_dispatch_drift(audit: &'static [ExAudit]) -> Vec<String> {
    let mut drift: Vec<String> = Vec::new();
    for e in audit {
        if e.dispatch == ExDispatch::Crashes {
            continue;
        }
        let measured = measured_ex_dispatch(e.probe_full, e.probe_abbr);
        if measured != e.dispatch {
            drift.push(format!(
                "  {} ({}): table says {:?}, driving {:?} / {:?} measures {:?}",
                e.cmd, e.help, e.dispatch, e.probe_full, e.probe_abbr, measured
            ));
        }
    }
    drift
}

/// Gate 1 (#1227) — every row's `ExDispatch` is a claim about live code, and
/// this replays all 551 runnable rows against it. Pure: no `nvim`, no
/// subprocess except the one `:make` probe, so it runs on every lane.
#[test]
fn ex_audit_matches_the_live_dispatcher() {
    if let Ok(path) = std::env::var("CONFORMANCE_DUMP_EX") {
        let mut s = String::new();
        for e in EX_AUDIT {
            if e.dispatch == ExDispatch::Crashes {
                s.push_str(&format!("{}\tCrashes\n", e.cmd));
                continue;
            }
            s.push_str(&format!(
                "{}\t{:?}\n",
                e.cmd,
                measured_ex_dispatch(e.probe_full, e.probe_abbr)
            ));
        }
        std::fs::write(&path, s).unwrap_or_else(|e| panic!("dump to {path}: {e}"));
        return;
    }

    let drift = ex_dispatch_drift(EX_AUDIT);
    assert!(
        drift.is_empty(),
        "\n\n== ex-command audit drifted from execute.rs (#1227) ==\n\
         Each row records what `Engine::execute_command` actually did with the\n\
         full name and with `:help`'s minimal abbreviation when the slice ran.\n\
         Implementing (or breaking) one of these changes that measurement, so\n\
         re-tag the row — that is how the audit stays true instead of rotting\n\
         like a markdown checklist.\n\n{}\n\n\
         A command that gained an implementation also needs its status changed\n\
         from NotImplemented, a probe added, and an EX_COVERAGE_EXEMPT entry\n\
         until an oracle case pins it.\n",
        drift.join("\n")
    );
}

/// Gate 1b (#1227) — the table describes itself correctly: complete, in
/// `:help` order, unique, no unreviewed row, every ⏭️ carrying a reason from
/// the shared vocabulary, and a probe on exactly the rows that can have one.
#[test]
fn ex_audit_is_internally_consistent() {
    use std::collections::HashSet;
    let mut problems: Vec<String> = Vec::new();

    assert_eq!(
        EX_AUDIT.len(),
        553,
        "`:help ex-cmd-index` walked to 553 entries; the audit must tag all of them"
    );

    let excepted: HashSet<&str> = EX_STATUS_DISPATCH_EXCEPTIONS
        .iter()
        .map(|(c, _)| *c)
        .collect();
    let mut seen: HashSet<&str> = HashSet::new();
    for e in EX_AUDIT {
        if !seen.insert(e.cmd) {
            problems.push(format!("  {}: listed twice", e.cmd));
        }
        if e.note.trim().is_empty() {
            problems.push(format!("  {}: empty note — every row is reviewed", e.cmd));
        }
        if e.help.trim().is_empty() {
            problems.push(format!("  {}: no `:help` tag", e.cmd));
        }
        if e.note.contains("TODO") {
            problems.push(format!("  {}: TODO note — every row is reviewed", e.cmd));
        }

        if let OptStatus::Skipped(reason) = e.status {
            if !SKIP_REASONS.contains(&reason) {
                problems.push(format!(
                    "  {}: skip reason {reason:?} is not in SKIP_REASONS",
                    e.cmd
                ));
            }
        }

        let in_scope = matches!(e.status, OptStatus::Implemented | OptStatus::Partial);
        if in_scope && e.probe.is_none() {
            problems.push(format!(
                "  {}: Implemented/Partial, so it must name the oracle case that covers it",
                e.cmd
            ));
        }
        if !in_scope && e.probe.is_some() {
            problems.push(format!(
                "  {}: only Implemented/Partial rows carry an oracle probe",
                e.cmd
            ));
        }
        if in_scope && e.dispatch == ExDispatch::Neither && !excepted.contains(e.cmd) {
            problems.push(format!(
                "  {}: tagged {:?} but the dispatcher rejects both spellings — \
                 either the tag is wrong or it needs an EX_STATUS_DISPATCH_EXCEPTIONS entry",
                e.cmd, e.status
            ));
        }
        if e.probe_full.is_empty() && e.probe_abbr.is_empty() && e.cmd != ":" {
            problems.push(format!("  {}: no probe line to re-measure", e.cmd));
        }
    }

    for (cmd, reason) in EX_STATUS_DISPATCH_EXCEPTIONS {
        if !EX_AUDIT.iter().any(|e| e.cmd == *cmd) {
            problems.push(format!("  {cmd}: exception names no audited command"));
        }
        if reason.trim().is_empty() {
            problems.push(format!("  {cmd}: exception with no reason is a shrug"));
        }
    }

    // The headline tally, pinned. The module doc quotes these numbers and a
    // PR body quotes the module doc; without this they drift the moment a row
    // is re-tagged, which is the exact rot a markdown checklist suffers from.
    let tally = |want: fn(&ExAudit) -> bool| EX_AUDIT.iter().filter(|e| want(e)).count();
    assert_eq!(
        (
            tally(|e| matches!(e.status, OptStatus::Implemented)),
            tally(|e| matches!(e.status, OptStatus::Partial)),
            tally(|e| matches!(e.status, OptStatus::NotImplemented)),
            tally(|e| matches!(e.status, OptStatus::Skipped(_))),
        ),
        (121, 53, 199, 180),
        "the audit tally moved: (implemented, partial, missing, skipped). \
         Update the module doc's table in the same commit."
    );

    // The two shapes the findings section calls out, pinned so they cannot
    // regress silently: commands vimcode *recognises* while not implementing
    // them, and commands reachable under only one of their two documented
    // spellings.
    assert_eq!(
        (
            tally(|e| matches!(e.status, OptStatus::NotImplemented)
                && e.dispatch != ExDispatch::Neither),
            tally(
                |e| matches!(e.status, OptStatus::Skipped(_)) && e.dispatch != ExDispatch::Neither
            ),
            tally(|e| e.dispatch == ExDispatch::FullOnly),
            tally(|e| e.dispatch == ExDispatch::AbbrevOnly),
            tally(|e| e.dispatch == ExDispatch::Crashes),
        ),
        (6, 8, 43, 6, 2),
        "the (recognised-but-❌, recognised-but-⏭️, full-name-only, \
         abbreviation-only, crashing) shape moved"
    );

    assert!(
        problems.is_empty(),
        "\n\n== ex-command audit table is inconsistent (#1227) ==\n{}\n",
        problems.join("\n")
    );
}

/// `cases` is `(label, keys)` for the whole corpus — the same view
/// [`classify_coverage`] takes.
fn classify_ex_coverage(
    audit: &'static [ExAudit],
    exempt: &[&'static str],
    cases: &[(&'static str, &'static str)],
) -> OptionCoverage {
    use std::collections::HashSet;
    let exempt_set: HashSet<&str> = exempt.iter().copied().collect();
    let mut v = OptionCoverage::default();
    for e in audit {
        let Some(probe) = e.probe else { continue };
        v.in_scope += 1;
        let covered = cases.iter().any(|(label, keys)| probe.matches(label, keys));
        match (covered, exempt_set.contains(e.cmd)) {
            (false, false) => v.uncovered.push(e.cmd),
            (true, true) => v.newly_covered.push(e.cmd),
            _ => {}
        }
    }
    v.stale = exempt
        .iter()
        .copied()
        .filter(|n| !audit.iter().any(|e| e.cmd == *n && e.probe.is_some()))
        .collect();
    v
}

/// Gate 2 (#1227) — #1007's ratchet, applied to the audited ex commands: an
/// in-scope row whose probe matches nothing must be exempt, and an exempt row
/// whose probe now matches must lose its entry. Pure.
#[test]
fn ex_audit_oracle_coverage_is_shrink_only() {
    use std::collections::HashSet;
    let corpus = all_corpus_cases();
    let exempt: HashSet<&str> = EX_COVERAGE_EXEMPT.iter().copied().collect();
    assert_eq!(
        exempt.len(),
        EX_COVERAGE_EXEMPT.len(),
        "EX_COVERAGE_EXEMPT lists a command twice"
    );

    let v = classify_ex_coverage(EX_AUDIT, EX_COVERAGE_EXEMPT, &corpus);

    if let Ok(path) = std::env::var("CONFORMANCE_DUMP_EX_COVERAGE") {
        let mut s = String::new();
        for n in &v.uncovered {
            s.push_str(&format!("UNCOVERED\t{n}\n"));
        }
        for n in &v.newly_covered {
            s.push_str(&format!("NEWLY_COVERED\t{n}\n"));
        }
        for n in &v.stale {
            s.push_str(&format!("STALE\t{n}\n"));
        }
        // Every credited row plus the case that credits it — the
        // over-crediting check #1007's "deliberately dumb probe" note demands
        // a human be able to do in one grep.
        for e in EX_AUDIT {
            let Some(probe) = e.probe else { continue };
            if let Some((label, _)) = corpus
                .iter()
                .find(|(label, keys)| probe.matches(label, keys))
            {
                s.push_str(&format!(
                    "COVERED\t{}\t{:?}\t{}\n",
                    e.cmd,
                    probe.needle(),
                    label
                ));
            }
        }
        std::fs::write(&path, s).unwrap_or_else(|e| panic!("dump to {path}: {e}"));
        return;
    }

    let covered = v.in_scope - EX_COVERAGE_EXEMPT.len();
    println!(
        "\n== ex-command oracle coverage (#1227) ==\n\
         {covered}/{} audited commands are pinned by an oracle case; {} exempt.\n",
        v.in_scope,
        EX_COVERAGE_EXEMPT.len()
    );

    assert!(
        v.uncovered.is_empty() && v.newly_covered.is_empty() && v.stale.is_empty(),
        "\n\n== ex-command oracle coverage moved (#1227) ==\n\
         UNCOVERED (probe matches no case — add the case, or exempt it only when \
         seeding a newly-tagged command):\n  {:?}\n\
         NEWLY COVERED (a case now pins it — delete the EX_COVERAGE_EXEMPT \
         entry; that is how the list shrinks):\n  {:?}\n\
         STALE (exempt but not an in-scope audited command):\n  {:?}\n",
        v.uncovered,
        v.newly_covered,
        v.stale
    );
}

/// The finding behind [`ExDispatch::Crashes`], pinned precisely rather than
/// left as a status letter: `:bdelete` on the **last** buffer panics.
///
/// Vim replaces it with an empty [No Name] buffer; vimcode unloads it and
/// then dereferences the window's now-dangling `buffer_id` in
/// `Engine::active_buffer_state`. Black-box — an ex line in, a crash out —
/// and it is a live user path: a one-file session plus `:bd` takes the editor
/// down. `:bwipeout` shares the code path and the crash.
///
/// `#[should_panic]`, not `#[ignore]`, so it is the *fix* that has to touch
/// this test: whoever makes `:bd` fall back to an empty buffer will see this
/// go red and rewrite it into the assertion the behaviour deserves.
#[test]
#[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
fn bdelete_on_the_last_buffer_panics_instead_of_refusing() {
    let mut engine = engine_with("alpha\nbeta\n");
    engine.history = Default::default();
    engine.execute_command("bdelete");
    // Unreachable today. When `:bd` learns Vim's fallback this line runs, the
    // `should_panic` fails, and the row's `ExDispatch::Crashes` has to change.
    let _ = engine.buffer().to_string();
}

/// Neither ex-audit gate is one nobody has seen fail. Drives all three RED on
/// synthetic input and on the real table/corpus.
#[test]
fn ex_audit_gates_are_bidirectional() {
    // ── gate 1: a perturbed dispatch is caught ──────────────────────────────
    const FAKE: &[ExAudit] = &[
        ex(
            ":se[t]",
            ":set",
            OptStatus::Implemented,
            // The lie: `:set` plainly dispatches.
            ExDispatch::Neither,
            "set",
            "se",
            Some(Keys(":set ")),
            "perturbed on purpose",
        ),
        ex(
            ":lcd",
            ":lcd",
            OptStatus::NotImplemented,
            // The other lie: `:lcd` plainly does not.
            ExDispatch::Both,
            "lcd foo",
            "lcd foo",
            None,
            "perturbed on purpose",
        ),
    ];
    let drift = ex_dispatch_drift(FAKE);
    assert_eq!(
        drift.len(),
        2,
        "both perturbed rows must be reported, got:\n{}",
        drift.join("\n")
    );
    assert!(drift[0].contains(":se[t]") && drift[0].contains("Both"));
    assert!(drift[1].contains(":lcd") && drift[1].contains("Neither"));

    // The real table must, of course, be clean.
    assert!(
        ex_dispatch_drift(EX_AUDIT).is_empty(),
        "the real EX_AUDIT must match the live dispatcher"
    );

    // ── gate 2: both directions, against the real corpus ────────────────────
    let corpus = all_corpus_cases();
    let victim = ":ret[ab]";
    assert!(
        EX_AUDIT
            .iter()
            .any(|e| e.cmd == victim && e.probe.is_some()),
        "fixture drifted — {victim} is no longer an in-scope audited command"
    );
    assert!(
        !EX_COVERAGE_EXEMPT.contains(&victim),
        "fixture drifted — {victim} is covered, so it must not be exempt"
    );
    let plus_exempt: Vec<&str> = EX_COVERAGE_EXEMPT
        .iter()
        .copied()
        .chain(std::iter::once(victim))
        .collect();
    assert_eq!(
        classify_ex_coverage(EX_AUDIT, &plus_exempt, &corpus).newly_covered,
        vec![victim],
        "exempting a covered command must fail the gate"
    );

    let exempt_victim = ":lgr[ep]";
    assert!(
        EX_COVERAGE_EXEMPT.contains(&exempt_victim),
        "fixture drifted — {exempt_victim} is no longer exempt"
    );
    let without: Vec<&str> = EX_COVERAGE_EXEMPT
        .iter()
        .copied()
        .filter(|n| *n != exempt_victim)
        .collect();
    assert_eq!(
        classify_ex_coverage(EX_AUDIT, &without, &corpus).uncovered,
        vec![exempt_victim],
        "deleting {exempt_victim:?} from EX_COVERAGE_EXEMPT must fail the gate"
    );
    let mut plus_case = corpus.clone();
    plus_case.push(("ex:lgrep fills the location list", ":lgrep foo<CR>"));
    assert_eq!(
        classify_ex_coverage(EX_AUDIT, EX_COVERAGE_EXEMPT, &plus_case).newly_covered,
        vec![exempt_victim],
        "a case pinning {exempt_victim:?} must force its exemption to be deleted"
    );

    // A stale exemption — one naming no in-scope row — is reported too.
    let stale: Vec<&str> = EX_COVERAGE_EXEMPT
        .iter()
        .copied()
        .chain(std::iter::once(":let"))
        .collect();
    assert_eq!(
        classify_ex_coverage(EX_AUDIT, &stale, &corpus).stale,
        vec![":let"],
        "an exemption naming a ⏭️ row must be reported as stale"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// #1228 — Phase 5 audit slice 4/5: `:help visual-index`
//
// The fourth `:help`-area walk in the #26 audit, tagging every Visual-mode
// command Implemented / Partial / NotImplemented / Skipped against the live
// engine. Source: the **pinned fleet oracle's own** documentation — Neovim
// v0.12.5's `runtime/doc/index.txt` §3, the same Vim every other gate in this
// file compares against — walked end to end, all **84** rows:
//
//     ✅ Implemented       65
//     🟡 Partial            6
//     ❌ Not implemented   11
//     ⏭️  Skipped            2   (each carrying a reason from SKIP_REASONS)
//                          ──
//                          84
//
// Unlike #1227's slice this one does *not* grow the denominator much —
// `VIM_COMPATIBILITY.md`'s "Visual Mode" section and the operator/text-object
// corpus already cover this ground well, and the tally above says so: 77% of
// the area matches Neovim keystroke-for-keystroke. The value here is the 17
// rows that do not, and in particular the four where vimcode does something
// **destructive** rather than nothing (see the module-level findings in the
// PR body): `CTRL-C`, `CTRL-\ CTRL-N`, `a>`/`i>` and `!{filter}`.
//
// ## Gate 1 — the recorded behaviour must match the live engine
//
// Every non-Skipped row carries a [`VisLive`] recording: real keystrokes over
// a real buffer, plus the buffer, cursor, **mode indicator** and
// `engine.message` vimcode produced when the slice ran.
// [`visual_audit_matches_the_live_engine`] replays all 82 and diffs. That is
// the bidirectional half — implementing `a<` or fixing `CTRL-C` changes its
// recording and fails the gate until the row is re-tagged, and a row claiming
// Implemented for something that regresses fails immediately.
//
// The recording is a black-box observation — keys in, rendered buffer +
// cursor + the status line's own mode string (`Engine::mode_str`, what both
// backends paint) + message out. Never an engine field: a gate that asserted
// `engine.visual_start` was `Some` would have passed throughout every one of
// the ❌ rows below, because vimcode *is* in Visual mode for all of them; what
// is wrong is what the keystroke then does.
//
// Mode is recorded because most of this area is about mode transitions, and
// buffer+cursor cannot see them. `v_v`'s whole content is "stop Visual mode";
// without the mode column its recording is indistinguishable from a no-op.
//
// ## Why several recordings end in an operator
//
// A text object in Visual mode only *extends the selection* — the buffer does
// not change, so a recording of `va(` alone would pin nothing. Each
// text-object row therefore records `v{object}d`: the region the `d` removes
// **is** the selection the object made, read back out of the rendered buffer.
// Same reason `v_P`/`v_p` end in a second paste (it is the only way to see
// whether the unnamed register was clobbered) and `v_V`/`v_CTRL-V` end in an
// `x` (it is the only way to see that the *second* `V`/`CTRL-V` stopped
// Visual mode rather than re-entering it).
//
// ## [`Touch`] — "silently mangles the buffer" is worse than "does nothing"
//
// Same role [`Report`] plays in #1226. For a ❌ row, a silent no-op leaves the
// user's text intact; a row that *modifies the buffer* while doing the wrong
// thing is strictly worse, because the user's next keystroke lands on text
// they did not expect. Six of this slice's 11 ❌ rows are `Modified`, and
// [`visual_audit_is_internally_consistent`] pins that count, so a future
// change that quietly adds a seventh has to say so.
//
// ## Gate 2 — oracle coverage, same shrink-only shape as #1007
//
// Every non-Skipped row also carries a [`Probe`] naming the oracle case that
// exercises it; [`VISUAL_COVERAGE_EXEMPT`] lists the 39 no case reaches
// today. Both directions fail, exactly as in `COVERAGE_EXEMPT`. Writing the
// missing cases is #1162's job, not this slice's.
//
// ## Out of scope, deliberately
//
// Select mode itself (`gH`, `gV`, `g CTRL-H`) belongs to the g-prefix slice
// and is parked as **#1193**; the four visual-index rows that reach into it
// (`CTRL-G`, `CTRL-O`, `<BS>`, `CTRL-H`) cite #1193 rather than filing a
// duplicate. `:help visual-index`'s opening sentence — "most commands in
// Visual mode are the same as in Normal mode" — means the motions are *not*
// here: they are `:help motion.txt`, i.e. another slice.
// ═══════════════════════════════════════════════════════════════════════════

/// Whether the recorded run left the buffer changed.
///
/// A recorded, re-measured column rather than a derived bool, for the same
/// reason [`Report`] is one: for a ❌ row, `Modified` means vimcode mangled
/// the user's text on its way to doing the wrong thing, which is strictly
/// worse than refusing. `Modified` on an ✅ row is just "the command edits" —
/// the column describes the recording, the verdict is `status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Touch {
    /// The recorded run changed the buffer.
    Modified,
    /// The recorded run left the buffer byte-identical to the fixture.
    Unchanged,
}

use crate::Touch::{Modified, Unchanged};

fn measured_touch(lines: &[&str], buffer: &str) -> Touch {
    if buffer == lines.join("|") {
        Touch::Unchanged
    } else {
        Touch::Modified
    }
}

/// One black-box observation of what vimcode does **today** in Visual mode:
/// keys in, rendered buffer + cursor + painted mode string + message out.
/// Replayed by gate 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VisLive {
    /// Starting buffer, one `&str` per line.
    lines: &'static [&'static str],
    /// Starting cursor, 1-indexed `(line, col)`.
    at: (usize, usize),
    /// Keys to send, in the corpus's `<C-v>`/`<Esc>`/`<CR>` notation.
    keys: &'static str,
    /// Resulting buffer, lines joined with `|`.
    buffer: &'static str,
    /// Resulting cursor, 1-indexed `(line, col)`.
    cursor: (usize, usize),
    /// `Engine::mode_str()` afterwards — the string both backends paint in
    /// the status line ("NORMAL", "VISUAL", "VISUAL LINE", "VISUAL BLOCK",
    /// "INSERT", …).
    mode: &'static str,
    /// `engine.message` afterwards, verbatim; `""` when vimcode said nothing.
    message: &'static str,
}

const fn vlive(
    lines: &'static [&'static str],
    at: (usize, usize),
    keys: &'static str,
    buffer: &'static str,
    cursor: (usize, usize),
    mode: &'static str,
    message: &'static str,
) -> VisLive {
    VisLive {
        lines,
        at,
        keys,
        buffer,
        cursor,
        mode,
        message,
    }
}

struct VisualAudit {
    /// The command as `:help visual-index` writes it — the table's unique key.
    item: &'static str,
    /// The `:help` tag it is documented under (`v_…`).
    help: &'static str,
    status: OptStatus,
    /// Whether the recording changed the buffer; re-measured by gate 1.
    touch: Touch,
    /// `Some` for every non-Skipped row.
    live: Option<VisLive>,
    /// Oracle probe; `Some` for every non-Skipped row.
    probe: Option<Probe>,
    /// For ❌: what Vim does, plus this slice's assessment of whether it is
    /// worth implementing. For 🟡: exactly what is missing.
    note: &'static str,
}

const fn vis(
    item: &'static str,
    help: &'static str,
    status: OptStatus,
    touch: Touch,
    live: Option<VisLive>,
    probe: Option<Probe>,
    note: &'static str,
) -> VisualAudit {
    VisualAudit {
        item,
        help,
        status,
        touch,
        live,
        probe,
        note,
    }
}

const VA_TXT: &[&str] = &["alpha beta gamma", "delta epsilon zeta", "eta theta iota"];
const VA_UP: &[&str] = &["ALPHA beta", "GAMMA delta"];
const VA_NUM: &[&str] = &["count 7 here", "next 11 line"];
const VA_NUMS: &[&str] = &["1 a", "1 b", "1 c"];
const VA_Q: &[&str] = &[
    "say \"hello there\" ok",
    "it's 'a b' fine",
    "run `cmd arg` now",
];
const VA_BR: &[&str] = &[
    "foo(bar baz) qux",
    "arr[one two] end",
    "map{k v} tail",
    "lt<a b> gt",
];
const VA_TAG: &[&str] = &["<div>hello there</div>"];
const VA_PARA: &[&str] = &["alpha one", "beta two", "", "gamma three", "delta four"];
const VA_SENT: &[&str] = &["One two. Three four. Five six."];
const VA_IND: &[&str] = &["    alpha", "    beta", "    gamma"];

/// Every command in `:help visual-index`, in `:help` order.
///
/// 84 rows, no "TODO" and no unreviewed row: adding one, deleting one, or
/// leaving one without a note fails [`visual_audit_is_internally_consistent`].
const VISUAL_AUDIT: &[VisualAudit] = &[
    vis(
        "CTRL-\\ CTRL-N",
        "v_CTRL-\\_CTRL-N",
        NotImplemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vll<C-\\><C-n>d",
            "ha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:<C-\\><C-n> stops visual")),
        concat!(
            "Vim leaves Visual mode for Normal; vimcode ignores both keys",
            " and stays in Visual, so the recording's trailing `d` deletes",
            " the selection the user believed was already cancelled. Worth",
            " implementing: this is the canonical \"get me to Normal mode",
            " from anywhere\" escape hatch.",
        ),
    ),
    vis(
        "CTRL-\\ CTRL-G",
        "v_CTRL-\\_CTRL-G",
        NotImplemented,
        Unchanged,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vll<C-\\><C-g>d",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 3),
            "VISUAL",
            "",
        )),
        Some(Label("vis:<C-\\><C-g> goes to normal")),
        concat!(
            "Vim goes to Normal mode; vimcode ignores both keys and stays",
            " in Visual (and swallows the following `d`). Low value on its",
            " own, but it shares the `CTRL-\\` prefix with the escape hatch",
            " above.",
        ),
    ),
    vis(
        "CTRL-A",
        "v_CTRL-A",
        Partial,
        Modified,
        Some(vlive(
            VA_NUM,
            (1, 1),
            "v$<C-a>",
            "count 8 here|next 11 line",
            (1, 7),
            "NORMAL",
            "",
        )),
        Some(Label("num:v C-a partial")),
        concat!(
            "the increment itself matches; the cursor is left on the last",
            " digit of the number changed, where Vim puts it at the start",
            " of the Visual area. The corpus cannot see this today —",
            " `num:V C-a cursor`'s number is at column 1, so both answers",
            " coincide there.",
        ),
    ),
    vis(
        "CTRL-C",
        "v_CTRL-C",
        NotImplemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vll<C-c>d",
            "dha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "INSERT",
            "",
        )),
        Some(Label("vis:<C-c> stops visual")),
        concat!(
            "Vim stops Visual mode. vimcode treats `CTRL-C` as `c`: it",
            " **deletes the selection and enters Insert mode**, so the one",
            " key a user presses to cancel silently destroys the selected",
            " text. Pinned by `ctrl_c_in_visual_mode_changes_the_selection",
            " _instead_of_stopping_visual_mode`; the highest-severity",
            " finding in this slice.",
        ),
    ),
    vis(
        "CTRL-G",
        "v_CTRL-G",
        NotImplemented,
        Unchanged,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vll<C-g>d",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 3),
            "VISUAL",
            "",
        )),
        Some(Label("vis:<C-g> toggles select mode")),
        concat!(
            "toggles Visual <-> Select. vimcode has no Select mode",
            " (parked as #1193), so `CTRL-G` is a silent no-op that also",
            " swallows the next key — the recording's `d` never runs.",
        ),
    ),
    vis(
        "<BS>",
        "v_<BS>",
        Partial,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 6),
            "vll<BS>d",
            "alphaeta gamma|delta epsilon zeta|eta theta iota",
            (1, 6),
            "NORMAL",
            "",
        )),
        Some(Label("vis:<BS> in select mode deletes")),
        concat!(
            "`:help` documents the Select-mode meaning (delete the",
            " highlighted area). In *Visual* mode `<BS>` is the `h`",
            " motion, which is exactly what vimcode does and what the",
            " oracle does — the recording agrees with Neovim. Only the",
            " Select-mode half is missing, with #1193.",
        ),
    ),
    vis(
        "CTRL-H",
        "v_CTRL-H",
        Partial,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 6),
            "vll<C-h>d",
            "alphaeta gamma|delta epsilon zeta|eta theta iota",
            (1, 6),
            "NORMAL",
            "",
        )),
        Some(Label("vis:<C-h> in select mode deletes")),
        concat!(
            "same as `<BS>`: the Visual-mode `h` motion matches the",
            " oracle, the Select-mode delete needs #1193.",
        ),
    ),
    vis(
        "CTRL-O",
        "v_CTRL-O",
        NotImplemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vll<C-o>d",
            "ha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:<C-o> select one visual command")),
        concat!(
            "Select -> Visual for one command. vimcode has no Select mode",
            " (#1193), and `CTRL-O` in Visual instead runs the jumplist's",
            " older-position jump, moving the cursor out of the selection.",
        ),
    ),
    vis(
        "CTRL-V",
        "v_CTRL-V",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "v<C-v>jd<C-v>l<C-v>x",
            "lha beta gamma|elta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
        )),
        Some(Label("vb:v then C-v switch")),
        concat!(
            "both halves recorded: `v<C-v>` switches a charwise selection",
            " to blockwise (the block delete matches the oracle) and a",
            " second `<C-v>` while already blockwise stops Visual mode, so",
            " the trailing `x` deletes one character rather than a block.",
        ),
    ),
    vis(
        "CTRL-X",
        "v_CTRL-X",
        Partial,
        Modified,
        Some(vlive(
            VA_NUM,
            (1, 1),
            "v$<C-x>",
            "count 6 here|next 11 line",
            (1, 7),
            "NORMAL",
            "",
        )),
        Some(Label("vis:v$<C-x> cursor")),
        concat!(
            "same cursor deviation as `CTRL-A`: the decrement is right,",
            " the cursor ends on the last digit instead of at the start of",
            " the Visual area.",
        ),
    ),
    vis(
        "<Esc>",
        "v_<Esc>",
        Implemented,
        Unchanged,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vll<Esc>d",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 3),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vll Esc cursor")),
        concat!(
            "stops Visual mode and leaves the cursor where Vim leaves it;",
            " the trailing `d` is then an incomplete operator and changes",
            " nothing.",
        ),
    ),
    vis(
        "CTRL-]",
        "v_CTRL-]",
        Skipped(CTAGS),
        Unchanged,
        None,
        None,
        concat!(
            "jump to the highlighted tag. vimcode ships no tags file",
            " support at all; go-to-definition is LSP (`gd`). Measured for",
            " the record: `<C-]>` in Visual is a silent no-op that leaves",
            " the selection up.",
        ),
    ),
    vis(
        "!{filter}",
        "v_!",
        NotImplemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "Vj!tr a-z A-Z<CR>",
            " -z A-Z| |                  |eta theta iota",
            (2, 2),
            "INSERT",
            "",
        )),
        Some(Label("vis:Vj! filters through an external command")),
        concat!(
            "filter the highlighted lines through an external command.",
            " vimcode's `!` is a silent no-op in Visual mode, and —",
            " because it does not open a command line — every character of",
            " the filter the user then types is executed as a Visual-mode",
            " command. The recording shows the damage: `Vj!tr a-z A-Z<CR>`",
            " mangles the buffer and lands in Insert mode. Worth",
            " implementing, or at minimum refusing.",
        ),
    ),
    vis(
        ":",
        "v_:",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "Vj:d<CR>",
            "eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vj: shows range")),
        concat!(
            "starts a command line pre-filled with `'<,'>`: the",
            " recording's `:d<CR>` deletes both selected lines, not just",
            " the cursor line.",
        ),
    ),
    vis(
        "<",
        "v_<",
        Implemented,
        Modified,
        Some(vlive(
            VA_IND,
            (1, 5),
            "Vj<",
            "alpha|beta|    gamma",
            (1, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vj<")),
        "shifts the selected lines one 'shiftwidth' left, cursor included.",
    ),
    vis(
        "=",
        "v_=",
        Partial,
        Modified,
        Some(vlive(
            VA_IND,
            (1, 5),
            "Vj=",
            "alpha|beta|    gamma",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:Vj=")),
        concat!(
            "the reindent matches the oracle, but the cursor is moved to",
            " column 1 where Neovim (`nostartofline`, its default) keeps",
            " the column — and vimcode's own `<` and `>` *do* keep it, so",
            " this is an internal inconsistency as much as a Vim",
            " deviation.",
        ),
    ),
    vis(
        ">",
        "v_>",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "Vj>",
            "    alpha beta gamma|    delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vj>")),
        "shifts the selected lines one 'shiftwidth' right.",
    ),
    vis(
        "A",
        "v_b_A",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "<C-v>jjAX<Esc>",
            "aXlpha beta gamma|dXelta epsilon zeta|eXta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vb:jjAx")),
        concat!(
            "blockwise append: the same text is inserted after the block",
            " on every line.",
        ),
    ),
    vis(
        "C",
        "v_C",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vlCxy<Esc>",
            "xy|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjC")),
        concat!(
            "deletes the selected lines and starts Insert, linewise even",
            " from a charwise selection.",
        ),
    ),
    vis(
        "D",
        "v_D",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vlD",
            "delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjD")),
        "deletes the selected lines linewise.",
    ),
    vis(
        "I",
        "v_b_I",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 2),
            "<C-v>jjIX<Esc>",
            "aXlpha beta gamma|dXelta epsilon zeta|eXta theta iota",
            (1, 2),
            "NORMAL",
            "",
        )),
        Some(Label("vb:jjIx")),
        concat!(
            "blockwise insert: the same text is inserted before the block",
            " on every line.",
        ),
    ),
    vis(
        "J",
        "v_J",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "VjJ",
            "alpha beta gamma delta epsilon zeta|eta theta iota",
            (1, 17),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjJ")),
        "joins the selected lines, inserting a space.",
    ),
    vis(
        "K",
        "v_K",
        NotImplemented,
        Unchanged,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "veK",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 5),
            "VISUAL",
            "",
        )),
        Some(Label("vis:veK runs keywordprg")),
        concat!(
            "run 'keywordprg' on the highlighted area. vimcode has no",
            " 'keywordprg' option and Visual `K` is a silent no-op. Low",
            " value: LSP hover covers the same intent, and vimcode has no",
            " `:Man`.",
        ),
    ),
    vis(
        "O",
        "v_O",
        Implemented,
        Unchanged,
        Some(vlive(
            VA_TXT,
            (1, 3),
            "<C-v>jllO",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 3),
            "VISUAL BLOCK",
            "",
        )),
        Some(Label("vb:O")),
        concat!(
            "moves the cursor horizontally to the other corner of a",
            " blockwise selection.",
        ),
    ),
    vis(
        "P",
        "v_P",
        Partial,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "yiwwvePjp",
            "alpha alpha gamma|delta epsilbetaon zeta|eta theta iota",
            (2, 15),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vly$P")),
        concat!(
            "the replacement itself is right, but vimcode clobbers the",
            " unnamed register with the replaced text — `P` is a straight",
            " alias of `p`. The recording proves it: after `veP` the",
            " following `p` pastes `beta` (the text `P` replaced) where",
            " Vim pastes `alpha` (the register, unchanged).",
        ),
    ),
    vis(
        "Q",
        "v_Q",
        Skipped(EXMODE),
        Unchanged,
        None,
        None,
        concat!(
            "the row documents a *negative* — Vim's `Q` does not start Ex",
            " mode from Visual. vimcode has no Ex mode at all (`gQ` was",
            " tagged the same way by the g-prefix slice), so there is",
            " nothing to not-start; `Q` in Visual is ignored.",
        ),
    ),
    vis(
        "R",
        "v_R",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vlRxy<Esc>",
            "xy|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjR")),
        "deletes the selected lines and starts Insert, like `S`/`C`.",
    ),
    vis(
        "S",
        "v_S",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vlSxy<Esc>",
            "xy|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjS")),
        "deletes the selected lines and starts Insert.",
    ),
    vis(
        "U",
        "v_U",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "veU",
            "ALPHA beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjU")),
        "uppercases the highlighted area.",
    ),
    vis(
        "V",
        "v_V",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vVdVVx",
            "elta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vb:C-v then V")),
        concat!(
            "both halves recorded: `vV` promotes a charwise selection to",
            " linewise (the delete takes the whole line) and a second `V`",
            " while already linewise stops Visual mode, so the trailing",
            " `x` deletes one character.",
        ),
    ),
    vis(
        "X",
        "v_X",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vlX",
            "delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjX")),
        "deletes the selected lines linewise, like `D`.",
    ),
    vis(
        "Y",
        "v_Y",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "veYjp",
            "alpha beta gamma|delta epsilon zeta|alpha beta gamma|eta theta iota",
            (3, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjY p")),
        concat!(
            "yanks the selected lines linewise; the recording's `p` puts",
            " a whole line back.",
        ),
    ),
    vis(
        "a\"",
        "v_aquote",
        Implemented,
        Modified,
        Some(vlive(
            VA_Q,
            (1, 8),
            "va\"d",
            "say ok|it's 'a b' fine|run `cmd arg` now",
            (1, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:va\"d")),
        "a double-quoted string including the quotes.",
    ),
    vis(
        "a'",
        "v_a'",
        Implemented,
        Modified,
        Some(vlive(
            VA_Q,
            (2, 7),
            "va'd",
            "say \"hello there\" ok|it's fine|run `cmd arg` now",
            (2, 6),
            "NORMAL",
            "",
        )),
        Some(Label("vis:va' extends with a quoted string")),
        concat!(
            "a single-quoted string including the quotes; the apostrophe",
            " in `it's` is correctly not treated as an opening quote.",
        ),
    ),
    vis(
        "a(",
        "v_a(",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (1, 6),
            "va(d",
            "foo qux|arr[one two] end|map{k v} tail|lt<a b> gt",
            (1, 4),
            "NORMAL",
            "",
        )),
        Some(Label("vis:va(d")),
        "a parenthesised block including the parentheses.",
    ),
    vis(
        "a)",
        "v_a)",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (1, 6),
            "va)d",
            "foo qux|arr[one two] end|map{k v} tail|lt<a b> gt",
            (1, 4),
            "NORMAL",
            "",
        )),
        Some(Label("vis:va) same as ab")),
        concat!(
            "the `a)` spelling of `ab`; vimcode accepts it and produces",
            " the same selection as `a(`.",
        ),
    ),
    vis(
        "a<",
        "v_a<",
        NotImplemented,
        Unchanged,
        Some(vlive(
            VA_BR,
            (4, 5),
            "va<d",
            "foo(bar baz) qux|arr[one two] end|map{k v} tail|lt<a b> gt",
            (4, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:va< extends with an angle block")),
        concat!(
            "extend with a `<>` block. vimcode does not recognise `a<` at",
            " all — the recording is a silent no-op where Vim deletes `<a",
            " b>`. Worth implementing: `<>` is the one bracket pair",
            " missing from an otherwise complete text-object set, and it",
            " is the common case in HTML/JSX and Rust generics.",
        ),
    ),
    vis(
        "a>",
        "v_a>",
        NotImplemented,
        Modified,
        Some(vlive(
            VA_BR,
            (4, 5),
            "va>d",
            "foo(bar baz) qux|arr[one two] end|map{k v} tail|    lt<a b> gt",
            (4, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:va> same as a<")),
        concat!(
            "the `a>` spelling of `a<`. Worse than `a<`: because `a` is",
            " not consumed as a text-object prefix, the `>` runs as the",
            " Visual shift command, so the recording **indents the line**",
            " instead of selecting anything.",
        ),
    ),
    vis(
        "aB",
        "v_aB",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (3, 6),
            "vaBd",
            "foo(bar baz) qux|arr[one two] end|map tail|lt<a b> gt",
            (3, 4),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vaB extends with a brace block")),
        "a `{}` block including the braces.",
    ),
    vis(
        "aW",
        "v_aW",
        Implemented,
        Modified,
        Some(vlive(
            VA_Q,
            (2, 4),
            "vaWd",
            "say \"hello there\" ok|'a b' fine|run `cmd arg` now",
            (2, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vaW extends with a WORD")),
        "a WORD plus its trailing whitespace.",
    ),
    vis(
        "a[",
        "v_a[",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (2, 6),
            "va[d",
            "foo(bar baz) qux|arr end|map{k v} tail|lt<a b> gt",
            (2, 4),
            "NORMAL",
            "",
        )),
        Some(Label("vis:va[ extends with a bracket block")),
        "a `[]` block including the brackets.",
    ),
    vis(
        "a]",
        "v_a]",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (2, 6),
            "va]d",
            "foo(bar baz) qux|arr end|map{k v} tail|lt<a b> gt",
            (2, 4),
            "NORMAL",
            "",
        )),
        Some(Label("vis:va] same as a[")),
        "the `a]` spelling of `a[`.",
    ),
    vis(
        "a`",
        "v_a`",
        Implemented,
        Modified,
        Some(vlive(
            VA_Q,
            (3, 7),
            "va`d",
            "say \"hello there\" ok|it's 'a b' fine|run now",
            (3, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:va` extends with a backtick string")),
        "a backtick-quoted string including the backticks.",
    ),
    vis(
        "ab",
        "v_ab",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (1, 6),
            "vabd",
            "foo qux|arr[one two] end|map{k v} tail|lt<a b> gt",
            (1, 4),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vab extends with a paren block")),
        "the `ab` spelling of `a(`.",
    ),
    vis(
        "ap",
        "v_ap",
        Implemented,
        Modified,
        Some(vlive(
            VA_PARA,
            (1, 1),
            "vapd",
            "gamma three|delta four",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vapd")),
        "a paragraph plus its trailing blank lines.",
    ),
    vis(
        "as",
        "v_as",
        Implemented,
        Modified,
        Some(vlive(
            VA_SENT,
            (1, 10),
            "vasd",
            "One two. Five six.",
            (1, 10),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vas extends with a sentence")),
        "a sentence plus its trailing whitespace.",
    ),
    vis(
        "at",
        "v_at",
        Implemented,
        Modified,
        Some(vlive(VA_TAG, (1, 7), "vatd", "", (1, 1), "NORMAL", "")),
        Some(Label("vis:vat extends with a tag block")),
        "a tag block including both tags.",
    ),
    vis(
        "aw",
        "v_aw",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 8),
            "vawd",
            "alpha gamma|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "",
        )),
        Some(Label("vis:v2awd")),
        "a word plus its trailing whitespace.",
    ),
    vis(
        "a{",
        "v_a{",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (3, 6),
            "va{d",
            "foo(bar baz) qux|arr[one two] end|map tail|lt<a b> gt",
            (3, 4),
            "NORMAL",
            "",
        )),
        Some(Label("vis:va{ same as aB")),
        "the `a{` spelling of `aB`.",
    ),
    vis(
        "a}",
        "v_a}",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (3, 6),
            "va}d",
            "foo(bar baz) qux|arr[one two] end|map tail|lt<a b> gt",
            (3, 4),
            "NORMAL",
            "",
        )),
        Some(Label("vis:va} same as aB")),
        "the `a}` spelling of `aB`.",
    ),
    vis(
        "c",
        "v_c",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vecXY<Esc>",
            "XY beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vec")),
        "deletes the highlighted area and starts Insert.",
    ),
    vis(
        "d",
        "v_d",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "ved",
            " beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjd")),
        "deletes the highlighted area.",
    ),
    vis(
        "g CTRL-A",
        "v_g_CTRL-A",
        Implemented,
        Modified,
        Some(vlive(
            VA_NUMS,
            (1, 1),
            "Vjjg<C-a>",
            "2 a|3 b|4 c",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("num:V g C-a")),
        concat!(
            "progressively increments the numbers on the selected lines",
            " (1/2/3, not 2/2/2).",
        ),
    ),
    vis(
        "g CTRL-X",
        "v_g_CTRL-X",
        Implemented,
        Modified,
        Some(vlive(
            VA_NUMS,
            (1, 1),
            "Vjjg<C-x>",
            "0 a|-1 b|-2 c",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:V g C-x decrements progressively")),
        "progressively decrements the numbers on the selected lines.",
    ),
    vis(
        "gJ",
        "v_gJ",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "VjgJ",
            "alpha beta gammadelta epsilon zeta|eta theta iota",
            (1, 17),
            "NORMAL",
            "",
        )),
        Some(Label("vis:v_gJ")),
        "joins the selected lines without inserting a space.",
    ),
    vis(
        "gq",
        "v_gq",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "Vjgq",
            "alpha beta gamma delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjgq")),
        "formats the selected lines to 'textwidth'.",
    ),
    vis(
        "gv",
        "v_gv",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "ve<Esc>gvd",
            " beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:gv")),
        "reselects the previous Visual area after it was left with `<Esc>`.",
    ),
    vis(
        "i\"",
        "v_iquote",
        Implemented,
        Modified,
        Some(vlive(
            VA_Q,
            (1, 8),
            "vi\"d",
            "say \"\" ok|it's 'a b' fine|run `cmd arg` now",
            (1, 6),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vi\"d")),
        "the inside of a double-quoted string, quotes excluded.",
    ),
    vis(
        "i'",
        "v_i'",
        Implemented,
        Modified,
        Some(vlive(
            VA_Q,
            (2, 7),
            "vi'd",
            "say \"hello there\" ok|it's '' fine|run `cmd arg` now",
            (2, 7),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vi' extends with inner quoted string")),
        "the inside of a single-quoted string.",
    ),
    vis(
        "i(",
        "v_i(",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (1, 6),
            "vi(d",
            "foo() qux|arr[one two] end|map{k v} tail|lt<a b> gt",
            (1, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vi(d")),
        "the inside of a parenthesised block.",
    ),
    vis(
        "i)",
        "v_i)",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (1, 6),
            "vi)d",
            "foo() qux|arr[one two] end|map{k v} tail|lt<a b> gt",
            (1, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vi) same as ib")),
        "the `i)` spelling of `ib`.",
    ),
    vis(
        "i<",
        "v_i<",
        NotImplemented,
        Unchanged,
        Some(vlive(
            VA_BR,
            (4, 5),
            "vi<d",
            "foo(bar baz) qux|arr[one two] end|map{k v} tail|lt<a b> gt",
            (4, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vi< extends with inner angle block")),
        concat!(
            "the inside of a `<>` block. Not recognised: a silent no-op,",
            " same gap as `a<`.",
        ),
    ),
    vis(
        "i>",
        "v_i>",
        NotImplemented,
        Modified,
        Some(vlive(
            VA_BR,
            (4, 5),
            "vi>d",
            "foo(bar baz) qux|arr[one two] end|map{k v} tail|    lt<a b> gt",
            (4, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vi> same as i<")),
        concat!(
            "the `i>` spelling of `i<`. Same damage as `a>`: the",
            " unconsumed `>` runs as the Visual shift and indents the",
            " line.",
        ),
    ),
    vis(
        "iB",
        "v_iB",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (3, 6),
            "viBd",
            "foo(bar baz) qux|arr[one two] end|map{} tail|lt<a b> gt",
            (3, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:viB extends with inner brace block")),
        "the inside of a `{}` block.",
    ),
    vis(
        "iW",
        "v_iW",
        Implemented,
        Modified,
        Some(vlive(
            VA_Q,
            (2, 4),
            "viWd",
            "say \"hello there\" ok| 'a b' fine|run `cmd arg` now",
            (2, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:viW extends with inner WORD")),
        "a WORD without surrounding whitespace.",
    ),
    vis(
        "i[",
        "v_i[",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (2, 6),
            "vi[d",
            "foo(bar baz) qux|arr[] end|map{k v} tail|lt<a b> gt",
            (2, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vi[ extends with inner bracket block")),
        "the inside of a `[]` block.",
    ),
    vis(
        "i]",
        "v_i]",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (2, 6),
            "vi]d",
            "foo(bar baz) qux|arr[] end|map{k v} tail|lt<a b> gt",
            (2, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vi] same as i[")),
        "the `i]` spelling of `i[`.",
    ),
    vis(
        "i`",
        "v_i`",
        Implemented,
        Modified,
        Some(vlive(
            VA_Q,
            (3, 7),
            "vi`d",
            "say \"hello there\" ok|it's 'a b' fine|run `` now",
            (3, 6),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vi` extends with inner backtick string")),
        "the inside of a backtick-quoted string.",
    ),
    vis(
        "ib",
        "v_ib",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (1, 6),
            "vibd",
            "foo() qux|arr[one two] end|map{k v} tail|lt<a b> gt",
            (1, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vib extends with inner paren block")),
        "the `ib` spelling of `i(`.",
    ),
    vis(
        "ip",
        "v_ip",
        Implemented,
        Modified,
        Some(vlive(
            VA_PARA,
            (1, 1),
            "vipd",
            "|gamma three|delta four",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vipd")),
        "a paragraph without its trailing blank lines.",
    ),
    vis(
        "is",
        "v_is",
        Implemented,
        Modified,
        Some(vlive(
            VA_SENT,
            (1, 10),
            "visd",
            "One two.  Five six.",
            (1, 10),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vis extends with inner sentence")),
        "a sentence without its trailing whitespace.",
    ),
    vis(
        "it",
        "v_it",
        Implemented,
        Modified,
        Some(vlive(
            VA_TAG,
            (1, 7),
            "vitd",
            "<div></div>",
            (1, 6),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vit extends with inner tag block")),
        "the contents of a tag block, tags excluded.",
    ),
    vis(
        "iw",
        "v_iw",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 8),
            "viwd",
            "alpha  gamma|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "",
        )),
        Some(Label("vis:viwiwiwd")),
        "a word without surrounding whitespace.",
    ),
    vis(
        "i{",
        "v_i{",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (3, 6),
            "vi{d",
            "foo(bar baz) qux|arr[one two] end|map{} tail|lt<a b> gt",
            (3, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vi{ same as iB")),
        "the `i{` spelling of `iB`.",
    ),
    vis(
        "i}",
        "v_i}",
        Implemented,
        Modified,
        Some(vlive(
            VA_BR,
            (3, 6),
            "vi}d",
            "foo(bar baz) qux|arr[one two] end|map{} tail|lt<a b> gt",
            (3, 5),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vi} same as iB")),
        "the `i}` spelling of `iB`.",
    ),
    vis(
        "o",
        "v_o",
        Implemented,
        Unchanged,
        Some(vlive(
            VA_TXT,
            (1, 3),
            "vllo",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 3),
            "VISUAL",
            "",
        )),
        Some(Label("vis:vllohd")),
        concat!(
            "moves the cursor to the other end of the selection, leaving",
            " Visual mode active.",
        ),
    ),
    vis(
        "p",
        "v_p",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "yiwwvepjp",
            "alpha alpha gamma|delta epsilbetaon zeta|eta theta iota",
            (2, 15),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vlp linewise reg")),
        concat!(
            "replaces the highlighted area with the register, and the",
            " replaced text does land in the unnamed register — the",
            " recording's second `p` pastes `beta`, matching the oracle.",
            " It is `P` (above) that shares this behaviour and should not.",
        ),
    ),
    vis(
        "r",
        "v_r",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "verX",
            "XXXXX beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjr-")),
        concat!(
            "replaces every character of the highlighted area with the",
            " typed character.",
        ),
    ),
    vis(
        "s",
        "v_s",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vesXY<Esc>",
            "XY beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
        )),
        Some(Label("vis:v s")),
        "deletes the highlighted area and starts Insert, like `c`.",
    ),
    vis(
        "u",
        "v_u",
        Implemented,
        Modified,
        Some(vlive(
            VA_UP,
            (1, 1),
            "veu",
            "alpha beta|GAMMA delta",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:Vju")),
        "lowercases the highlighted area.",
    ),
    vis(
        "v",
        "v_v",
        Implemented,
        Unchanged,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vlvd",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
        )),
        Some(Label("vb:C-v then v switch")),
        concat!(
            "the recording covers the *stop* half — `v` pressed while",
            " already charwise leaves Visual mode, so the trailing `d` is",
            " an incomplete operator. The `make charwise` half is pinned",
            " by the oracle case this row's probe names, which switches a",
            " blockwise selection to charwise with `v`.",
        ),
    ),
    vis(
        "x",
        "v_x",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vex",
            " beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:v x")),
        "deletes the highlighted area, like `d`.",
    ),
    vis(
        "y",
        "v_y",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "veyjp",
            "alpha beta gamma|dalphaelta epsilon zeta|eta theta iota",
            (2, 6),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjy cursor")),
        concat!(
            "yanks the highlighted area charwise; the recording's `p`",
            " puts it back inline.",
        ),
    ),
    vis(
        "~",
        "v_~",
        Implemented,
        Modified,
        Some(vlive(
            VA_UP,
            (1, 1),
            "ve~",
            "alpha beta|GAMMA delta",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vj~")),
        "swaps the case of the highlighted area.",
    ),
];

const VISUAL_COVERAGE_EXEMPT: &[&str] = &[
    "CTRL-\\ CTRL-N",
    "CTRL-\\ CTRL-G",
    "CTRL-C",
    "CTRL-G",
    "<BS>",
    "CTRL-H",
    "CTRL-O",
    "CTRL-X",
    "!{filter}",
    "K",
    "a'",
    "a)",
    "a<",
    "a>",
    "aB",
    "aW",
    "a[",
    "a]",
    "a`",
    "ab",
    "as",
    "at",
    "a{",
    "a}",
    "g CTRL-X",
    "i'",
    "i)",
    "i<",
    "i>",
    "iB",
    "iW",
    "i[",
    "i]",
    "i`",
    "ib",
    "is",
    "it",
    "i{",
    "i}",
];

/// Replay one [`VisLive`] recording against a real engine. Black-box: keys
/// in, rendered buffer + cursor + painted mode string + message out.
fn replay_vis_live(p: &VisLive) -> (String, (usize, usize), String, String) {
    let mut engine = engine_with(&p.lines.join("\n"));
    // Same reason as `replay_live` (#1226): `Engine::new()` loads the *user's
    // real* command history off disk, and `v_:`'s recording types at a `:`
    // prompt. Leaving it populated would make that row replay whatever the
    // developer last typed.
    engine.history = Default::default();
    engine.settings.shift_width = 4;
    engine.settings.expand_tab = true;
    engine.settings.tabstop = 4;
    engine.set_viewport_lines(24);
    engine.view_mut().cursor.line = p.at.0.saturating_sub(1);
    engine.view_mut().cursor.col = p.at.1.saturating_sub(1);
    engine.ensure_cursor_visible();
    send_keys(&mut engine, p.keys);
    (
        engine.buffer().to_string().replace('\n', "|"),
        (engine.view().cursor.line + 1, engine.view().cursor.col + 1),
        engine.mode_str().to_string(),
        engine.message.clone(),
    )
}

/// Every row whose recorded behaviour no longer matches the live engine.
fn visual_drift(audit: &[VisualAudit]) -> Vec<String> {
    let mut drift: Vec<String> = Vec::new();
    for e in audit {
        let Some(p) = e.live.as_ref() else { continue };
        let (buffer, cursor, mode, message) = replay_vis_live(p);
        if buffer != p.buffer || cursor != p.cursor || mode != p.mode || message != p.message {
            drift.push(format!(
                "  {:?} ({}): recorded buffer={:?} cursor={:?} mode={:?} message={:?}\n\
                 \x20  live     buffer={:?} cursor={:?} mode={:?} message={:?}",
                e.item,
                e.help,
                p.buffer,
                p.cursor,
                p.mode,
                p.message,
                buffer,
                cursor,
                mode,
                message
            ));
        }
        let measured = measured_touch(p.lines, &buffer);
        if measured != e.touch {
            drift.push(format!(
                "  {:?} ({}): table says {:?}, the live run is {:?}",
                e.item, e.help, e.touch, measured
            ));
        }
    }
    drift
}

/// Gate 1 (#1228) — every row's recorded behaviour is a claim about live
/// code, and this replays all 82 of them (the 84 rows minus the 2 ⏭️) against
/// it. Pure: no `nvim`, no subprocess, so it runs on every lane.
#[test]
fn visual_audit_matches_the_live_engine() {
    if let Ok(path) = std::env::var("CONFORMANCE_DUMP_VISUAL") {
        let mut s = String::new();
        for e in VISUAL_AUDIT {
            let Some(p) = e.live.as_ref() else { continue };
            let (buffer, cursor, mode, message) = replay_vis_live(p);
            s.push_str(&format!(
                "{}\t{:?}\t{}\t{}\t{}\t{:?}\n",
                e.item, buffer, cursor.0, cursor.1, mode, message
            ));
        }
        std::fs::write(&path, s).unwrap_or_else(|e| panic!("dump to {path}: {e}"));
        return;
    }

    let drift = visual_drift(VISUAL_AUDIT);
    assert!(
        drift.is_empty(),
        "\n\n== visual-mode audit drifted from the engine (#1228) ==\n\
         Each row records what vimcode actually did when the slice ran.\n\
         Implementing (or breaking) one of these changes that recording, so\n\
         re-tag the row — that is how the audit stays true instead of rotting\n\
         like a markdown checklist.\n\n{}\n\n\
         A command that gained an implementation also needs its status changed\n\
         from NotImplemented, and a VISUAL_COVERAGE_EXEMPT entry deleted once\n\
         an oracle case pins it.\n",
        drift.join("\n")
    );
}

/// Gate 1b (#1228) — the table describes itself correctly: complete, unique,
/// no unreviewed row, every ⏭️ carrying a reason from the shared vocabulary,
/// and a live recording + probe on exactly the rows that can have one.
#[test]
fn visual_audit_is_internally_consistent() {
    use std::collections::HashSet;
    let mut problems: Vec<String> = Vec::new();

    assert_eq!(
        VISUAL_AUDIT.len(),
        84,
        "`:help visual-index` walked to 84 entries; the audit must tag all of them"
    );

    let mut seen: HashSet<&str> = HashSet::new();
    let mut seen_help: HashSet<&str> = HashSet::new();
    for e in VISUAL_AUDIT {
        if !seen.insert(e.item) {
            problems.push(format!("  {:?}: listed twice", e.item));
        }
        if !seen_help.insert(e.help) {
            problems.push(format!(
                "  {:?}: `:help` tag {:?} used twice",
                e.item, e.help
            ));
        }
        if e.note.trim().is_empty() {
            problems.push(format!(
                "  {:?}: empty note — every row is reviewed",
                e.item
            ));
        }
        if !e.help.starts_with("v_") {
            problems.push(format!(
                "  {:?}: `:help` tag {:?} is not a `v_…` Visual-mode tag",
                e.item, e.help
            ));
        }

        let in_scope = !matches!(e.status, OptStatus::Skipped(_));
        if let OptStatus::Skipped(reason) = e.status {
            if !SKIP_REASONS.contains(&reason) {
                problems.push(format!(
                    "  {:?}: skip reason {reason:?} is not in SKIP_REASONS",
                    e.item
                ));
            }
        }
        if in_scope && e.live.is_none() {
            problems.push(format!(
                "  {:?}: not skipped, so it must carry a live recording",
                e.item
            ));
        }
        if in_scope && e.probe.is_none() {
            problems.push(format!(
                "  {:?}: not skipped, so it must carry an oracle probe",
                e.item
            ));
        }
        if !in_scope && (e.live.is_some() || e.probe.is_some()) {
            problems.push(format!(
                "  {:?}: skipped, so a live recording or probe can never fire",
                e.item
            ));
        }
    }

    // The headline tally, pinned. The section doc quotes these numbers and a
    // PR body quotes the section doc; without this they drift the moment a
    // row is re-tagged, which is the exact rot a markdown checklist suffers.
    let tally = |want: fn(&VisualAudit) -> bool| VISUAL_AUDIT.iter().filter(|e| want(e)).count();
    assert_eq!(
        (
            tally(|e| matches!(e.status, OptStatus::Implemented)),
            tally(|e| matches!(e.status, OptStatus::Partial)),
            tally(|e| matches!(e.status, OptStatus::NotImplemented)),
            tally(|e| matches!(e.status, OptStatus::Skipped(_))),
            tally(|e| matches!(e.status, OptStatus::NotImplemented) && e.touch == Touch::Modified),
        ),
        (65, 6, 11, 2, 6),
        "the audit tally moved: (implemented, partial, missing, skipped, \
         missing-and-buffer-modified). Update the section doc's table in the \
         same commit."
    );

    assert!(
        problems.is_empty(),
        "\n\n== visual-mode audit table is inconsistent (#1228) ==\n{}\n",
        problems.join("\n")
    );
}

/// `cases` is `(label, keys)` for the whole corpus — the same view
/// [`classify_coverage`] takes.
fn classify_visual_coverage(
    audit: &[VisualAudit],
    exempt: &[&'static str],
    cases: &[(&'static str, &'static str)],
) -> OptionCoverage {
    use std::collections::HashSet;
    let exempt_set: HashSet<&str> = exempt.iter().copied().collect();
    let mut v = OptionCoverage::default();
    for e in audit {
        let Some(probe) = e.probe else { continue };
        v.in_scope += 1;
        let covered = cases.iter().any(|(label, keys)| probe.matches(label, keys));
        match (covered, exempt_set.contains(e.item)) {
            (false, false) => v.uncovered.push(e.item),
            (true, true) => v.newly_covered.push(e.item),
            _ => {}
        }
    }
    v.stale = exempt
        .iter()
        .copied()
        .filter(|n| !audit.iter().any(|e| e.item == *n && e.probe.is_some()))
        .collect();
    v
}

/// Gate 2 (#1228) — #1007's ratchet, applied to the audited commands: an
/// in-scope row whose probe matches nothing must be exempt, and an exempt row
/// whose probe now matches must lose its entry. Pure.
#[test]
fn visual_audit_oracle_coverage_is_shrink_only() {
    use std::collections::HashSet;
    let corpus = all_corpus_cases();
    let exempt: HashSet<&str> = VISUAL_COVERAGE_EXEMPT.iter().copied().collect();
    assert_eq!(
        exempt.len(),
        VISUAL_COVERAGE_EXEMPT.len(),
        "VISUAL_COVERAGE_EXEMPT lists an item twice"
    );

    let v = classify_visual_coverage(VISUAL_AUDIT, VISUAL_COVERAGE_EXEMPT, &corpus);

    if let Ok(path) = std::env::var("CONFORMANCE_DUMP_VISUAL_COVERAGE") {
        let mut s = String::new();
        for n in &v.uncovered {
            s.push_str(&format!("UNCOVERED\t{n}\n"));
        }
        for n in &v.newly_covered {
            s.push_str(&format!("NEWLY_COVERED\t{n}\n"));
        }
        for n in &v.stale {
            s.push_str(&format!("STALE\t{n}\n"));
        }
        // Every credited row plus the case that credits it — the
        // over-crediting check #1007's "deliberately dumb probe" note demands
        // a human be able to do in one grep.
        for e in VISUAL_AUDIT {
            let Some(probe) = e.probe else { continue };
            if let Some((label, _)) = corpus
                .iter()
                .find(|(label, keys)| probe.matches(label, keys))
            {
                s.push_str(&format!(
                    "COVERED\t{}\t{:?}\t{}\n",
                    e.item,
                    probe.needle(),
                    label
                ));
            }
        }
        std::fs::write(&path, s).unwrap_or_else(|e| panic!("dump to {path}: {e}"));
        return;
    }

    let covered = v.in_scope - VISUAL_COVERAGE_EXEMPT.len();
    println!(
        "\n== visual-mode oracle coverage (#1228) ==\n\
         {covered}/{} audited commands are pinned by an oracle case; {} exempt.\n",
        v.in_scope,
        VISUAL_COVERAGE_EXEMPT.len()
    );

    assert!(
        v.uncovered.is_empty() && v.newly_covered.is_empty() && v.stale.is_empty(),
        "\n\n== visual-mode oracle coverage moved (#1228) ==\n\
         UNCOVERED (probe matches no case — add the case, or exempt it only when \
         seeding a newly-tagged command):\n  {:?}\n\
         NEWLY COVERED (a case now pins it — delete the VISUAL_COVERAGE_EXEMPT \
         entry; that is how the list shrinks):\n  {:?}\n\
         STALE (exempt but not an in-scope audited command):\n  {:?}\n",
        v.uncovered,
        v.newly_covered,
        v.stale
    );
}

/// The worst finding in this slice, pinned on its own rather than left as a
/// status letter in the table: `CTRL-C` — the one key a user presses to
/// *cancel* — is wired to `c` in Visual mode, so it deletes the highlighted
/// text and drops into Insert. Black-box: it reads the rendered buffer and
/// the status line's own mode string, not an engine flag.
///
/// Delete this test when `CTRL-C` is fixed; `v_CTRL-C`'s row and recording in
/// [`VISUAL_AUDIT`] have to change in the same commit, and gate 1 enforces it.
#[test]
fn ctrl_c_in_visual_mode_deletes_the_selection_instead_of_stopping_visual_mode() {
    let mut engine = engine_with("alpha beta gamma");
    engine.history = Default::default();
    engine.set_viewport_lines(24);
    send_keys(&mut engine, "vll<C-c>");

    assert_eq!(
        engine.buffer().to_string(),
        "ha beta gamma",
        "CTRL-C deleted the three highlighted characters; Vim stops Visual \
         mode and leaves the buffer alone"
    );
    assert_eq!(
        engine.mode_str(),
        "INSERT",
        "CTRL-C also dropped into Insert mode, so the user's next keystroke \
         is typed into the buffer"
    );
}

/// Gate 3 (#1228) — the gates above are observed **RED**, so none of them is
/// a test nobody has seen fail. Synthetic tables for the drift/consistency
/// directions, and the real corpus for the coverage ones.
#[test]
fn visual_audit_gates_are_bidirectional() {
    // ── Gate 1, direction A: a recording that no longer matches ──────────
    let wrong_buffer = vec![vis(
        "d",
        "v_d",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "ved",
            "this is not what vimcode does",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjd")),
        "synthetic",
    )];
    assert!(
        !visual_drift(&wrong_buffer).is_empty(),
        "a recording whose buffer no longer matches the engine must drift"
    );

    // ── Gate 1, direction B: a recording whose *mode* no longer matches ──
    // The column the buffer/cursor pair cannot see, and the reason it exists.
    let wrong_mode = vec![vis(
        "v",
        "v_v",
        Implemented,
        Unchanged,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "vlvd",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "VISUAL",
            "",
        )),
        Some(Label("vb:C-v then v switch")),
        "synthetic",
    )];
    assert!(
        !visual_drift(&wrong_mode).is_empty(),
        "a recording that claims the engine stayed in VISUAL when it left \
         must drift — otherwise the mode column is decoration"
    );

    // ── Gate 1, direction C: a mis-tagged [`Touch`] ──────────────────────
    let wrong_touch = vec![vis(
        "d",
        "v_d",
        Implemented,
        Unchanged,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "ved",
            " beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjd")),
        "synthetic",
    )];
    assert!(
        !visual_drift(&wrong_touch).is_empty(),
        "a row tagged Unchanged whose recording edits the buffer must drift"
    );

    // ── Gate 1, direction D: the unperturbed row is green ────────────────
    let good = vec![vis(
        "d",
        "v_d",
        Implemented,
        Modified,
        Some(vlive(
            VA_TXT,
            (1, 1),
            "ved",
            " beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
        )),
        Some(Label("vis:vjd")),
        "synthetic",
    )];
    assert!(
        visual_drift(&good).is_empty(),
        "the unperturbed recording must be green, or the gate is red for an \
         unrelated reason"
    );

    // ── Gate 2, against the REAL corpus and the REAL table ───────────────
    let corpus = all_corpus_cases();

    // A real exemption deleted must fail: the command is still uncovered.
    let victim = "a<";
    assert!(
        VISUAL_COVERAGE_EXEMPT.contains(&victim),
        "fixture drifted — {victim:?} is no longer exempt"
    );
    let without: Vec<&str> = VISUAL_COVERAGE_EXEMPT
        .iter()
        .copied()
        .filter(|n| *n != victim)
        .collect();
    let deleted = classify_visual_coverage(VISUAL_AUDIT, &without, &corpus);
    assert!(
        deleted.uncovered.contains(&victim),
        "deleting {victim:?} from VISUAL_COVERAGE_EXEMPT must fail the gate: {deleted:?}"
    );

    // A case that pins an exempt command must fail until the entry goes.
    let mut plus = corpus.clone();
    plus.push(("vis:va< extends with an angle block", "va<d"));
    let improved = classify_visual_coverage(VISUAL_AUDIT, VISUAL_COVERAGE_EXEMPT, &plus);
    assert!(
        improved.newly_covered.contains(&victim),
        "an oracle case for an exempt command must fail until its \
         VISUAL_COVERAGE_EXEMPT entry is deleted: {improved:?}"
    );

    // A stale exemption — a name that is not an in-scope audited command.
    let mut stale: Vec<&str> = VISUAL_COVERAGE_EXEMPT.to_vec();
    stale.push("gH");
    let with_stale = classify_visual_coverage(VISUAL_AUDIT, &stale, &corpus);
    assert!(
        with_stale.stale.contains(&"gH"),
        "an exemption naming something outside the audit must be reported \
         stale: {with_stale:?}"
    );

    // And the real pair is green, so the reds above are the perturbations.
    let real = classify_visual_coverage(VISUAL_AUDIT, VISUAL_COVERAGE_EXEMPT, &corpus);
    assert!(
        real.uncovered.is_empty() && real.newly_covered.is_empty() && real.stale.is_empty(),
        "the shipped table/exemption pair must be green: {real:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// #1229 — Phase 5 audit slice 5/5: `:help normal-index`, everything outside `g`
//
// The last `:help`-area walk of the #26 audit. Source: the **pinned fleet
// oracle's own** documentation — Neovim v0.12.5's `runtime/doc/index.txt` §2,
// the same Vim every other gate in this file compares against — walked end to
// end. Every row of the Normal-mode index outside the `g`-prefix cluster is
// tagged Implemented / Partial / NotImplemented / Skipped against the live
// engine:
//
//     §2    Normal mode                200 rows
//     §2.2  Window commands (CTRL-W)    75 rows
//     §2.3  Square-bracket commands     46 rows
//     §2.5  Commands starting with 'z'  52 rows (this section is *not*
//                                               g-prefixed, so it is in scope)
//     §2.6  Operator-pending            3 rows
//                                      ───
//                                      376 index rows, tagged as 370 entries
//
//     ✅ Implemented       208
//     🟡 Partial            20
//     ❌ Not implemented   118
//     ⏭️  Skipped            24   (each carrying a reason from SKIP_REASONS)
//                          ───
//                          370
//
// ## What is *not* here, and why
//
// * §2.4 (`g{char}`) is slice 1 of #26, already landed. The dispatcher rows
//   that only forward to another section — `g{char}`, `z{char}`, `[{char}`,
//   `]{char}`, `CTRL-W {char}` — are excluded with them: they are pointers,
//   and their targets are the subsections above.
// * §2.1 (text objects) carries `v_a"`-style tags and was tagged by #1228's
//   `VISUAL_AUDIT`; re-tagging it here would duplicate 36 rows.
// * The index's untagged "not used" rows (`CTRL-K`, `CTRL-Q`, `CTRL-S`,
//   `CTRL-_`, `\`, `CTRL-W CTRL-G`, …) have no `:help` tag and no action to
//   conform to.
// * The nine `|count|` rows (`1` … `9`) are one command and are tagged once,
//   as `1 - 9`.
//
// The index reuses a tag twice on purpose (`q` for both "record" and "stop
// recording"; `~` for the 'tildeop' variant), so — unlike #1228's table —
// this one keys on `item` and allows a repeated `help` tag.
//
// ## Gate 1 — the recorded behaviour must match the live engine
//
// Every row that a keyboard can reach carries a [`NormLive`] recording: real
// keystrokes over a real buffer, plus **eight** rendered observations vimcode
// produced when the slice ran — buffer, cursor, `Engine::mode_str()`,
// `engine.message`, the first rendered line, the first rendered column, the
// rendered line ranges (what folding hides) and the window/tab layout.
// [`normal_audit_matches_the_live_engine`] replays all 323 and diffs.
//
// The extra columns are not decoration; without them most of this `:help`
// area is invisible. A third of §2.5 is *only* about which lines are
// rendered (`zc`, `zR`, `zv`), the CTRL-W section is *only* about the window
// tree, and `zh`/`zL`/`ze`/`zs` move nothing but the first rendered column.
// All eight are black-box observations of rendered output — never an engine
// flag: a gate that asserted `view.folds` was non-empty would pass for `zE`,
// whose whole bug is that the fold it failed to delete is **still rendered as
// hidden**.
//
// ## Gate 1b — rows a keyboard cannot reach
//
// 25 index rows are mouse, wheel or shifted keys. `Engine::handle_key(name,
// unicode, ctrl)` has no Shift bit and no pointer events, so neither backend
// can deliver them through the path this harness drives — they are listed in
// [`NORM_NO_REPLAY`] with the reason, carry no recording and no probe, and
// are still tagged. That is a finding in itself: eight index rows are
// unreachable *because of an engine API gap*, not because nobody wrote them.
//
// ## Gate 2 — oracle coverage, same shrink-only shape as #1007
//
// Every replayable, non-Skipped row carries a [`Probe`] naming the oracle
// case that exercises it; [`NORM_COVERAGE_EXEMPT`] lists the 230 that no case
// reaches today (93 of 323 are covered). Both directions fail, exactly as in
// `COVERAGE_EXEMPT`. Writing the missing cases — including all 75 CTRL-W
// rows, which no case in the corpus presses — is **#1162's** job, not this
// slice's.
//
// ## What the pinned oracle could and could not settle
//
// Every row was replayed through vimcode *and* through `nvim --headless -l`
// v0.12.5 on the identical fixture, and the two were diffed before a status
// was written. That is how rows like `CTRL-M`, `<Del>` and `<C-Left>` were
// caught: they look plausible until Vim's answer is next to them.
//
// Four columns the `-l` oracle cannot answer, and which were therefore judged
// against `:help` plus the corpus's own UI-attached cases (`scroll:*`,
// `fold:*`): the first rendered line and column (no UI is attached, so
// `line('w0')` does not move), fold state, and macro recording (`q` behaves
// differently under `feedkeys`). Those rows say so in their notes.
// ═══════════════════════════════════════════════════════════════════════════

/// Added by this slice for the `CTRL-W P`/`CTRL-W z`/`CTRL-W }`/`CTRL-W g }`
/// family. Vim's preview window is a window *kind* with its own commands;
/// vimcode has splits and LSP hover popups, and nothing a `:ptag` could
/// target.
const PREVIEW: &str = "no preview window (:h preview-window) in vimcode";

/// One black-box observation of what vimcode does **today** in Normal mode.
///
/// Keys in; rendered buffer, cursor, painted mode string, message, first
/// rendered line, first rendered column, rendered line ranges and window
/// layout out. Replayed by gate 1. Deliberately a superset of #1228's
/// `VisLive`: Visual mode needed buffer+cursor+mode, and most of
/// `:help normal-index` does not touch any of the three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NormLive {
    /// Starting buffer, one `&str` per line.
    lines: &'static [&'static str],
    /// Starting cursor, 1-indexed `(line, col)`.
    at: (usize, usize),
    /// Viewport height in lines. 10 for the scrolling rows — a 24-line
    /// viewport over a 30-line fixture makes `CTRL-D` and `zt`
    /// indistinguishable from a no-op.
    view: usize,
    /// Keys to send, in the corpus's `<C-v>`/`<Esc>`/`<CR>` notation.
    keys: &'static str,
    /// Resulting buffer, lines joined with `|`.
    buffer: &'static str,
    /// Resulting cursor, 1-indexed `(line, col)`.
    cursor: (usize, usize),
    /// `Engine::mode_str()` afterwards — the string both backends paint.
    mode: &'static str,
    /// `engine.message` afterwards, verbatim; `""` when vimcode said nothing.
    message: &'static str,
    /// First rendered buffer line, 1-indexed (`view.scroll_top + 1`).
    top: usize,
    /// First rendered column, 1-indexed (`view.scroll_left + 1`).
    left: usize,
    /// The rendered line ranges, e.g. `"1,3-30"` when a fold hides line 2;
    /// `""` when every line is rendered.
    folded: &'static str,
    /// The window tree and tab counter, e.g. `"(h0.50 2* 1) tabs=1/2"`:
    /// `h`/`v` split, its ratio, the leaves in layout order with `*` on the
    /// focused one, and `tabs=<active>/<count>`.
    windows: &'static str,
}

#[allow(clippy::too_many_arguments)]
const fn nlive(
    lines: &'static [&'static str],
    at: (usize, usize),
    view: usize,
    keys: &'static str,
    buffer: &'static str,
    cursor: (usize, usize),
    mode: &'static str,
    message: &'static str,
    top: usize,
    left: usize,
    folded: &'static str,
    windows: &'static str,
) -> NormLive {
    NormLive {
        lines,
        at,
        view,
        keys,
        buffer,
        cursor,
        mode,
        message,
        top,
        left,
        folded,
        windows,
    }
}

struct NormAudit {
    /// The command as `:help normal-index` writes its Char column — the
    /// table's unique key. The three operator-pending rows carry an
    /// `(operator-pending)` suffix because `v`/`V`/`CTRL-V` also name
    /// Normal-mode commands.
    item: &'static str,
    /// The `:help` tag it is documented under. Not unique: the index gives
    /// `q` and `~` two rows each.
    help: &'static str,
    status: OptStatus,
    /// `Some` for every row that is neither Skipped nor in
    /// [`NORM_NO_REPLAY`].
    live: Option<NormLive>,
    /// Oracle probe, on exactly the rows that carry a recording.
    probe: Option<Probe>,
    /// For ❌: what Vim does, what vimcode does instead, and whether it is
    /// worth implementing. For 🟡: exactly what is missing.
    note: &'static str,
}

const fn na(
    item: &'static str,
    help: &'static str,
    status: OptStatus,
    live: Option<NormLive>,
    probe: Option<Probe>,
    note: &'static str,
) -> NormAudit {
    NormAudit {
        item,
        help,
        status,
        live,
        probe,
        note,
    }
}

const NA_BR: &[&str] = &["foo(bar baz) qux", "arr[one two] end", "map{k v} tail"];
const NA_CODE: &[&str] = &[
    "#if FOO",
    "int one(void)",
    "{",
    "    return 1;",
    "}",
    "#else",
    "/* note */",
    "int two(void)",
    "{",
    "    return 2;",
    "}",
    "#endif",
];
const NA_FILE: &[&str] = &["README.md", "second line"];
const NA_IND: &[&str] = &["    alpha beta", "    gamma delta", "    eps zeta"];
const NA_LONG: &[&str] = &[
    "line 01", "line 02", "line 03", "line 04", "line 05", "line 06", "line 07", "line 08",
    "line 09", "line 10", "line 11", "line 12", "line 13", "line 14", "line 15", "line 16",
    "line 17", "line 18", "line 19", "line 20", "line 21", "line 22", "line 23", "line 24",
    "line 25", "line 26", "line 27", "line 28", "line 29", "line 30",
];
const NA_MIX: &[&str] = &["abc DEF ghi"];
const NA_NUM: &[&str] = &["count 7 here", "next 11 line"];
const NA_PARA: &[&str] = &["alpha one", "beta two", "", "gamma three", "delta four"];
const NA_SENT: &[&str] = &["One two. Three four. Five six."];
const NA_SPELL: &[&str] = &["teh quick brwn fox"];
const NA_TXT: &[&str] = &["alpha beta gamma", "delta epsilon zeta", "eta theta iota"];
const NA_WIDE: &[&str] = &["001 002 003 004 005 006 007 008 009 010 011 012 013 014 015 016 017 018 019 020 021 022 023 024 025 026 027 028 029 030 031 032 033 034 035 036 037 038 039 040 041 042 043 044 045 046 047 048 049 050"];
const NA_WORDS: &[&str] = &["alpha beta", "gamma alpha", "beta alpha"];

const NORMAL_AUDIT: &[NormAudit] = &[
    na(
        "CTRL-A",
        "CTRL-A",
        Implemented,
        Some(nlive(
            NA_NUM,
            (1, 7),
            24,
            "<C-a>",
            "count 8 here|next 11 line",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("num:C-a on number")),
        "matches Vim: increments the first number at or after the cursor.",
    ),
    na(
        "CTRL-B",
        "CTRL-B",
        Implemented,
        Some(nlive(
            NA_LONG,
            (20, 1),
            10,
            "<C-b>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (12, 1),
            "NORMAL",
            "",
            3,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:C-b")),
        "matches Vim: pages backwards with Vim's two-line overlap (#805).",
    ),
    na(
        "CTRL-C",
        "CTRL-C",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "/bet<C-c>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Pattern not found: betc",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("misc:C-c aborts the search prompt")),
        concat!(
            "Vim interrupts the current (search) command. vimcode's search ",
            "prompt takes the `c` as a literal pattern character — the ",
            "recording ends with the pattern \"betc\" still in the command ",
            "line. Worth implementing: <C-c> is the universal \"get me out of ",
            "here\" key and typing into a prompt is the worst possible answer.",
        ),
    ),
    na(
        "CTRL-D",
        "CTRL-D",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "<C-d>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (6, 1),
            "NORMAL",
            "",
            6,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:C-d")),
        concat!(
            "matches Vim: scrolls down half a screen; an explicit count sets ",
            "the sticky 'scroll' value (#805).",
        ),
    ),
    na(
        "CTRL-E",
        "CTRL-E",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "<C-e>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (2, 1),
            "NORMAL",
            "",
            2,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:C-e pushes cursor")),
        concat!(
            "matches Vim: scrolls the text up one line and pushes the cursor ",
            "with it.",
        ),
    ),
    na(
        "CTRL-F",
        "CTRL-F",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "<C-f>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (9, 1),
            "NORMAL",
            "",
            9,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:C-f")),
        "matches Vim: pages forward with the two-line overlap.",
    ),
    na(
        "CTRL-G",
        "CTRL-G",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-g>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "\"[No Name]\" line 1 of 3 --33%-- col 1",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("misc:C-g file info")),
        concat!(
            "matches Vim's shape: prints name, line, percentage and column — ",
            "recorded as `\"[No Name]\" line 1 of 3 --33%-- col 1`.",
        ),
    ),
    na(
        "<BS>",
        "<BS>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 3),
            24,
            "<BS>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<BS> is h")),
        "matches Vim: same as \"h\".",
    ),
    na(
        "CTRL-H",
        "CTRL-H",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 3),
            24,
            "<C-h>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:C-h is h")),
        "matches Vim: same as \"h\".",
    ),
    na(
        "<Tab>",
        "<Tab>",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "G<C-o><Tab>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (30, 1),
            "NORMAL",
            "",
            21,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("jump:<Tab> newer entry")),
        "matches Vim: goes to the newer jump-list entry.",
    ),
    na(
        "CTRL-I",
        "CTRL-I",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "G<C-o><C-i>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (30, 1),
            "NORMAL",
            "",
            21,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("jump:C-i newer entry")),
        "matches Vim: same as <Tab>.",
    ),
    na(
        "<NL>",
        "<NL>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-j>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:C-j is j")),
        "matches Vim: <NL> is CTRL-J, which moves down a line.",
    ),
    na(
        "<S-NL>",
        "<S-NL>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim makes Shift-<NL> a synonym for CTRL-F. `Engine::handle_key` ",
            "has no Shift parameter at all, so both backends deliver plain ",
            "\"Return\"/CTRL-J here and the Shift is lost before the engine ",
            "sees it. Not worth implementing on its own — it is one of eight ",
            "rows blocked by the same missing modifier bit.",
        ),
    ),
    na(
        "CTRL-J",
        "CTRL-J",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-j>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:C-j is j")),
        "matches Vim: same as \"j\".",
    ),
    na(
        "CTRL-L",
        "CTRL-L",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-l>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("misc:C-l noop")),
        concat!(
            "matches Vim: a redraw the user cannot observe; the corpus pins ",
            "it as a no-op.",
        ),
    ),
    na(
        "<CR>",
        "<CR>",
        Implemented,
        Some(nlive(
            NA_IND,
            (1, 1),
            24,
            "<CR>",
            "    alpha beta|    gamma delta|    eps zeta",
            (2, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<CR>")),
        "matches Vim: down a line, cursor on the first non-blank.",
    ),
    na(
        "<S-CR>",
        "<S-CR>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim makes Shift-<CR> a synonym for CTRL-F; the Shift bit never ",
            "reaches the engine (see <S-NL>).",
        ),
    ),
    na(
        "CTRL-M",
        "CTRL-M",
        NotImplemented,
        Some(nlive(
            NA_IND,
            (1, 1),
            24,
            "<C-m>",
            "    alpha beta|    gamma delta|    eps zeta",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:C-m is <CR>")),
        concat!(
            "Vim's CTRL-M is <CR> — down a line, first non-blank. vimcode ",
            "ignores it: the recording shows the cursor still at (1,1). Worth ",
            "implementing: a three-line alias next to the existing <CR> arm.",
        ),
    ),
    na(
        "CTRL-N",
        "CTRL-N",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-n>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:C-n is j")),
        concat!(
            "Vim's CTRL-N is \"j\". vimcode ignores it (cursor unmoved). Worth ",
            "implementing, with the same caveat as CTRL-P: check first that ",
            "nothing else wants <C-n>.",
        ),
    ),
    na(
        "CTRL-O",
        "CTRL-O",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "G<C-o>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("jump:C-o at start")),
        "matches Vim: goes to the older jump-list entry.",
    ),
    na(
        "CTRL-P",
        "CTRL-P",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (2, 1),
            24,
            "<C-p>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:C-p is k")),
        concat!(
            "Vim's CTRL-P is \"k\". vimcode ignores it (the recording starts on ",
            "line 2 and stays there). Worth implementing.",
        ),
    ),
    na(
        "CTRL-R",
        "CTRL-R",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "xu<C-r>",
            "lpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("undo:C-r redoes")),
        "matches Vim: redoes the change \"u\" undid.",
    ),
    na(
        "CTRL-T",
        "CTRL-T",
        Skipped(CTAGS),
        None,
        None,
        "Vim pops the tag stack. vimcode has no tag stack at all.",
    ),
    na(
        "CTRL-U",
        "CTRL-U",
        Implemented,
        Some(nlive(
            NA_LONG,
            (20, 1),
            10,
            "<C-u>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (15, 1),
            "NORMAL",
            "",
            6,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:C-u")),
        "matches Vim: scrolls up half a screen, mirroring CTRL-D (#805).",
    ),
    na(
        "CTRL-V",
        "CTRL-V",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-v>jl",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 2),
            "VISUAL BLOCK",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("visual:C-v block")),
        concat!(
            "matches Vim: starts blockwise Visual mode (the recording ends in ",
            "\"VISUAL BLOCK\").",
        ),
    ),
    na(
        "CTRL-X",
        "CTRL-X",
        Implemented,
        Some(nlive(
            NA_NUM,
            (1, 7),
            24,
            "<C-x>",
            "count 6 here|next 11 line",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("num:C-x to negative")),
        "matches Vim: decrements the number at or after the cursor.",
    ),
    na(
        "CTRL-Y",
        "CTRL-Y",
        Implemented,
        Some(nlive(
            NA_LONG,
            (20, 1),
            10,
            "<C-y>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (19, 1),
            "NORMAL",
            "",
            10,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:C-y pulls cursor")),
        concat!(
            "matches Vim: scrolls the text down one line, pulling the cursor ",
            "with it.",
        ),
    ),
    na(
        "CTRL-Z",
        "CTRL-Z",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-z>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "z: a/o/c=fold  M=closeAll  R=openAll  d/D=del  f=create  j/k=nav  z/t/b=scroll  h/l=hscroll",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("misc:C-z suspends")),
        concat!(
            "Vim suspends the editor (or starts a shell). vimcode prints an ",
            "unrelated fold-command hint (\"z: a/o/c=fold ...\") — the ",
            "keystroke is bound to something else entirely. Only the TUI ",
            "backend could ever implement this (SIGTSTP); the GUI cannot, so ",
            "it needs a quadraui-level answer before vimcode can have one.",
        ),
    ),
    na(
        "CTRL-\\ CTRL-N",
        "CTRL-\\_CTRL-N",
        Partial,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-\\><C-n>x",
            "lpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("misc:C-bslash C-n from Normal")),
        concat!(
            "In Normal mode the documented action is \"go to Normal mode ",
            "(no-op)\", and the recording is indeed a no-op — but only because ",
            "vimcode drops both keys on the floor. #1228 measured the same ",
            "chord from Visual mode and it fails to leave Visual there, so ",
            "this row is \"accidentally right\", not implemented. Fixing it is ",
            "one arm in `handle_normal_key`.",
        ),
    ),
    na(
        "CTRL-\\ CTRL-G",
        "CTRL-\\_CTRL-G",
        Partial,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-\\><C-g>x",
            "lpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("misc:C-bslash C-g from Normal")),
        concat!(
            "Same as CTRL-\\\\ CTRL-N: the Normal-mode no-op is ",
            "indistinguishable from the keys being ignored, and #1228 shows ",
            "the chord does nothing from Visual either.",
        ),
    ),
    na(
        "CTRL-]",
        "CTRL-]",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "Vim runs `:ta` on the identifier under the cursor. vimcode has ",
            "no ctags; go-to-definition is the LSP-backed `gd`/`gD` of the ",
            "g-prefix slice.",
        ),
    ),
    na(
        "CTRL-^",
        "CTRL-^",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-^>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("buf:C-^ alternate file")),
        concat!(
            "Vim edits the alternate file (`:e #`). vimcode keeps a buffer ",
            "list but has no alternate-file register and no binding: the ",
            "recording does nothing and says nothing. Worth implementing — ",
            "\"flip to the last file\" is high-frequency muscle memory.",
        ),
    ),
    na(
        "CTRL-<Tab>",
        "CTRL-<Tab>",
        Partial,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-Tab>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("tab:C-Tab last accessed")),
        concat!(
            "Vim goes straight to the last accessed tab page. vimcode opens ",
            "an MRU *tab switcher overlay* instead (`keys.rs` intercepts ",
            "ctrl+\"Tab\" before the Normal-mode dispatch), so the keystroke is ",
            "bound but its effect is a different interaction. The recording ",
            "cannot see the overlay — it is neither buffer, cursor, mode nor ",
            "message.",
        ),
    ),
    na(
        "<Space>",
        "<Space>",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<Space>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<Space> is l")),
        concat!(
            "Vim's <Space> is \"l\". In vimcode <Space> is the **leader key** ",
            "(`default_leader()` returns ' '), so it opens leader-pending ",
            "state instead and the cursor does not move. Not worth \"fixing\" ",
            "blindly — it is a deliberate trade, but it is a real conformance ",
            "gap and should be documented as one.",
        ),
    ),
    na(
        "!{motion}{filter}",
        "!",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "!jsort<CR>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "2 lines filtered",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:!Gsort")),
        concat!(
            "matches Vim: filters the Nmove lines through the external ",
            "command (\"2 lines filtered\").",
        ),
    ),
    na(
        "!!{filter}",
        "!!",
        Implemented,
        Some(nlive(
            NA_TXT,
            (2, 1),
            24,
            "!!tr a-z A-Z<CR>",
            "alpha beta gamma|DELTA EPSILON ZETA|eta theta iota",
            (2, 1),
            "NORMAL",
            "1 lines filtered",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:!!tr")),
        "matches Vim: filters N lines through the external command.",
    ),
    na(
        "\"{register}",
        "quote",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "\"ayyj\"aP",
            "alpha beta gamma|alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("reg:\"ayy \"ap")),
        concat!(
            "matches Vim: `\"{register}` selects the register for the next ",
            "delete, yank or put.",
        ),
    ),
    na(
        "#",
        "#",
        Implemented,
        Some(nlive(
            NA_WORDS,
            (3, 7),
            24,
            "#",
            "alpha beta|gamma alpha|beta alpha",
            (2, 7),
            "NORMAL",
            "match 2 of 3",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("search:#")),
        concat!(
            "matches Vim: searches backwards for the identifier under the ",
            "cursor.",
        ),
    ),
    na(
        "$",
        "$",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "$",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 16),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:2$")),
        "matches Vim: end of the Nth next line.",
    ),
    na(
        "%",
        "%",
        Implemented,
        Some(nlive(
            NA_BR,
            (1, 4),
            24,
            "%",
            "foo(bar baz) qux|arr[one two] end|map{k v} tail",
            (1, 12),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:% on (")),
        "matches Vim: jumps to the matching bracket.",
    ),
    na(
        "{count}%",
        "N%",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "50%",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (15, 1),
            "NORMAL",
            "",
            6,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:N% percentage")),
        "matches Vim: `50%` on a 30-line buffer lands on line 15.",
    ),
    na(
        "&",
        "&",
        Implemented,
        Some(nlive(
            NA_WORDS,
            (1, 1),
            24,
            ":s/alpha/X/<CR>j&",
            "X beta|gamma X|beta alpha",
            (2, 1),
            "NORMAL",
            "1 substitution on 1 line",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("ex:& repeats last substitute")),
        "matches Vim: repeats the last `:s` on the current line.",
    ),
    na(
        "'{a-zA-Z0-9}",
        "'",
        Implemented,
        Some(nlive(
            NA_IND,
            (1, 7),
            24,
            "majj'a",
            "    alpha beta|    gamma delta|    eps zeta",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:'a line")),
        "matches Vim: to the first non-blank of the marked line.",
    ),
    na(
        "''",
        "''",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "G''",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:'' back")),
        concat!(
            "matches Vim: back to the line of the position before the latest ",
            "jump.",
        ),
    ),
    na(
        "'(",
        "'(",
        NotImplemented,
        Some(nlive(
            NA_SENT,
            (1, 20),
            24,
            "'(",
            "One two. Three four. Five six.",
            (1, 20),
            "NORMAL",
            "Marks must be a letter or special char",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:'( sentence start")),
        concat!(
            "Vim jumps to the line holding the start of the current sentence. ",
            "vimcode rejects the mark outright — \"Marks must be a letter or ",
            "special char\". Worth implementing together with `'`)`, `` `( `` ",
            "and `` `) ``: four rows, one sentence-boundary helper that ",
            "`(`/`)` already have.",
        ),
    ),
    na(
        "')",
        "')",
        NotImplemented,
        Some(nlive(
            NA_SENT,
            (1, 1),
            24,
            "')",
            "One two. Three four. Five six.",
            (1, 1),
            "NORMAL",
            "Marks must be a letter or special char",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:') sentence end")),
        concat!(
            "Same as `'(`: rejected with \"Marks must be a letter or special ",
            "char\".",
        ),
    ),
    na(
        "'<",
        "'<",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "vjl<Esc>gg'<",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:'< visual start")),
        "matches Vim: first line of the last Visual area.",
    ),
    na(
        "'>",
        "'>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "vjl<Esc>gg'>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:'> visual end")),
        "matches Vim: last line of the last Visual area.",
    ),
    na(
        "'[",
        "'[",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "yyjpgg'[",
            "alpha beta gamma|delta epsilon zeta|alpha beta gamma|eta theta iota",
            (1, 1),
            "NORMAL",
            "No previous change",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:'[ change start")),
        concat!(
            "Vim goes to the line of the start of the last change or put; ",
            "vimcode answers \"No previous change\" even directly after a `p`, ",
            "so the `'[`/`']`/`` `[ ``/`` `] `` family is unset. Worth ",
            "implementing — `]p`-style workflows and `gp` lean on it.",
        ),
    ),
    na(
        "']",
        "']",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "yyjpgg']",
            "alpha beta gamma|delta epsilon zeta|alpha beta gamma|eta theta iota",
            (1, 1),
            "NORMAL",
            "No previous change",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:'] change end")),
        concat!(
            "Same as `'[`: \"No previous change\" after a put that plainly ",
            "changed the buffer.",
        ),
    ),
    na(
        "'{",
        "'{",
        NotImplemented,
        Some(nlive(
            NA_PARA,
            (4, 1),
            24,
            "'{",
            "alpha one|beta two||gamma three|delta four",
            (4, 1),
            "NORMAL",
            "Marks must be a letter or special char",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:'{ paragraph start")),
        concat!(
            "Vim goes to the line of the start of the current paragraph; ",
            "vimcode rejects the mark (\"Marks must be a letter or special ",
            "char\"). Cheap to implement: `{`/`}` already compute the ",
            "boundary.",
        ),
    ),
    na(
        "'}",
        "'}",
        NotImplemented,
        Some(nlive(
            NA_PARA,
            (1, 1),
            24,
            "'}",
            "alpha one|beta two||gamma three|delta four",
            (1, 1),
            "NORMAL",
            "Marks must be a letter or special char",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:'} paragraph end")),
        "Same as `'{`.",
    ),
    na(
        "(",
        "(",
        Implemented,
        Some(nlive(
            NA_SENT,
            (1, 20),
            24,
            "(",
            "One two. Three four. Five six.",
            (1, 10),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:( sentence")),
        "matches Vim: N sentences backwards.",
    ),
    na(
        ")",
        ")",
        Implemented,
        Some(nlive(
            NA_SENT,
            (1, 1),
            24,
            ")",
            "One two. Three four. Five six.",
            (1, 10),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:) sentences")),
        "matches Vim: N sentences forwards.",
    ),
    na(
        "*",
        "star",
        Implemented,
        Some(nlive(
            NA_WORDS,
            (1, 1),
            24,
            "*",
            "alpha beta|gamma alpha|beta alpha",
            (2, 7),
            "NORMAL",
            "match 2 of 3",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("search:*")),
        concat!(
            "matches Vim: searches forward for the identifier under the ",
            "cursor.",
        ),
    ),
    na(
        "+",
        "+",
        Implemented,
        Some(nlive(
            NA_IND,
            (1, 7),
            24,
            "+",
            "    alpha beta|    gamma delta|    eps zeta",
            (2, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:+")),
        "matches Vim: down a line, first non-blank.",
    ),
    na(
        "<S-+>",
        "<S-Plus>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim makes Shift-+ a synonym for CTRL-F; the Shift bit never ",
            "reaches the engine (see <S-NL>).",
        ),
    ),
    na(
        ",",
        ",",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "$Fa,",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 16),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:F, F, then ,")),
        "matches Vim: repeats the last f/t/F/T in the opposite direction.",
    ),
    na(
        "-",
        "-",
        Implemented,
        Some(nlive(
            NA_IND,
            (2, 7),
            24,
            "-",
            "    alpha beta|    gamma delta|    eps zeta",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:-")),
        "matches Vim: up a line, first non-blank.",
    ),
    na(
        "<S-->",
        "<S-Minus>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim makes Shift-- a synonym for CTRL-B; the Shift bit never ",
            "reaches the engine (see <S-NL>).",
        ),
    ),
    na(
        ".",
        ".",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "x.",
            "pha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("dot:x then .")),
        "matches Vim: repeats the last change.",
    ),
    na(
        "/{pattern}<CR>",
        "/",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "/eta<CR>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 8),
            "NORMAL",
            "match 1 of 4",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("search:/pattern")),
        concat!(
            "matches Vim: searches forward for the Nth occurrence, and ",
            "reports \"match 1 of 4\".",
        ),
    ),
    na(
        "/<CR>",
        "/<CR>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "/eta<CR>gg/<CR>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 16),
            "NORMAL",
            "match 2 of 4",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("search:/<CR> reuses last")),
        "matches Vim: a bare `/<CR>` re-runs the previous pattern.",
    ),
    na(
        "0",
        "0",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 5),
            24,
            "0",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:0")),
        "matches Vim: to the first character of the line.",
    ),
    na(
        "1 - 9",
        "count",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "3l",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 4),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:3l count")),
        concat!(
            "matches Vim: digits 1-9 prepend a count (the nine `|count|` rows ",
            "of `:help normal-index` are one command and are tagged once).",
        ),
    ),
    na(
        ":",
        ":",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            ":",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "COMMAND",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("ex:: opens the command line")),
        concat!(
            "matches Vim: enters command-line mode (the recording ends in ",
            "vimcode's \"COMMAND\" mode string).",
        ),
    ),
    na(
        "{count}:",
        "N:",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "3:",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "COMMAND",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("ex:3: prefills a range")),
        concat!(
            "matches Vim: `3:` opens the command line prefilled with the ",
            "range `.,.+2`.",
        ),
    ),
    na(
        ";",
        ";",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "fa;",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 10),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:f, then ;")),
        "matches Vim: repeats the last f/t/F/T.",
    ),
    na(
        "<{motion}",
        "<",
        Partial,
        Some(nlive(
            NA_IND,
            (1, 5),
            24,
            "<j",
            "alpha beta|gamma delta|    eps zeta",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:< with motion")),
        concat!(
            "The buffer matches Vim exactly, but the cursor does not: for ",
            "`<j` over a 4-space-indented block the oracle leaves the cursor ",
            "in column 5 and vimcode in column 1. Worth fixing as one change ",
            "with `=`/`>` — same operator plumbing, same one-column error.",
        ),
    ),
    na(
        "<<",
        "<<",
        Implemented,
        Some(nlive(
            NA_IND,
            (1, 5),
            24,
            "<<",
            "alpha beta|    gamma delta|    eps zeta",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:<< partial indent")),
        concat!(
            "matches Vim: shifts N lines one 'shiftwidth' left, cursor ",
            "included.",
        ),
    ),
    na(
        "={motion}",
        "=",
        Partial,
        Some(nlive(
            NA_IND,
            (1, 5),
            24,
            "=j",
            "alpha beta|gamma delta|    eps zeta",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:= with motion")),
        concat!(
            "Same one-column cursor difference as `<{motion}`; the ",
            "re-indented buffer itself matches.",
        ),
    ),
    na(
        "==",
        "==",
        Implemented,
        Some(nlive(
            NA_IND,
            (1, 5),
            24,
            "==",
            "alpha beta|    gamma delta|    eps zeta",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:== single")),
        "matches Vim: re-indents the current line.",
    ),
    na(
        ">{motion}",
        ">",
        Partial,
        Some(nlive(
            NA_IND,
            (1, 5),
            24,
            ">j",
            "        alpha beta|        gamma delta|    eps zeta",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:> with motion")),
        concat!(
            "Same one-column cursor difference as `<{motion}`; the shifted ",
            "buffer itself matches.",
        ),
    ),
    na(
        ">>",
        ">>",
        Implemented,
        Some(nlive(
            NA_IND,
            (1, 5),
            24,
            ">>",
            "        alpha beta|    gamma delta|    eps zeta",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:>>")),
        "matches Vim: shifts N lines one 'shiftwidth' right.",
    ),
    na(
        "?{pattern}<CR>",
        "?",
        Implemented,
        Some(nlive(
            NA_TXT,
            (3, 1),
            24,
            "?beta<CR>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "match 1 of 1",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("search:?pattern")),
        "matches Vim: searches backwards for the Nth previous occurrence.",
    ),
    na(
        "?<CR>",
        "?<CR>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (3, 1),
            24,
            "?beta<CR>G?<CR>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "match 1 of 1",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("search:?<CR> reuses last")),
        concat!(
            "matches Vim: a bare `?<CR>` re-runs the previous pattern ",
            "backwards.",
        ),
    ),
    na(
        "@{a-z}",
        "@",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "qaxqj@a",
            "lpha beta gamma|elta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mac:qaxjq @a")),
        "matches Vim: executes the contents of the named register.",
    ),
    na(
        "@:",
        "@:",
        Implemented,
        Some(nlive(
            NA_WORDS,
            (1, 1),
            24,
            ":s/a/X/<CR>j@:",
            "Xlpha beta|gXmma alpha|beta alpha",
            (2, 1),
            "NORMAL",
            "1 substitution on 1 line",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("ex:@: repeats last ex")),
        concat!(
            "matches Vim: repeats the previous `:` command (the recording's ",
            "second substitution lands on line 2).",
        ),
    ),
    na(
        "@@",
        "@@",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "qaxqj@aj@@",
            "lpha beta gamma|elta epsilon zeta|ta theta iota",
            (3, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mac:@@ repeats")),
        "matches Vim: repeats the previous `@{a-z}`.",
    ),
    na(
        "A",
        "A",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "AX<Esc>",
            "alpha beta gammaX|delta epsilon zeta|eta theta iota",
            (1, 17),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:A esc cursor")),
        "matches Vim: appends after the end of the line.",
    ),
    na(
        "B",
        "B",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 12),
            24,
            "B",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:B")),
        "matches Vim: N WORDS backwards.",
    ),
    na(
        "[\"x]C",
        "C",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 7),
            24,
            "CX<Esc>",
            "alpha X|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:C")),
        "matches Vim: changes to end of line (\"c$\").",
    ),
    na(
        "[\"x]D",
        "D",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 7),
            24,
            "D",
            "alpha |delta epsilon zeta|eta theta iota",
            (1, 6),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:3D")),
        "matches Vim: deletes to end of line (\"d$\").",
    ),
    na(
        "E",
        "E",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "E",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:E")),
        "matches Vim: forward to the end of WORD N.",
    ),
    na(
        "F{char}",
        "F",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 12),
            24,
            "Fb",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:F at col1")),
        "matches Vim: to the Nth occurrence of {char} leftwards.",
    ),
    na(
        "G",
        "G",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "G",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (30, 1),
            "NORMAL",
            "",
            21,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:5G")),
        "matches Vim: to line N, last line by default.",
    ),
    na(
        "H",
        "H",
        Implemented,
        Some(nlive(
            NA_LONG,
            (20, 1),
            10,
            "H",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (11, 1),
            "NORMAL",
            "",
            11,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:H from 30")),
        concat!(
            "matches Vim: to line N from the top of the window. (The recorded ",
            "top differs from the `-l` oracle's because that oracle has no ",
            "attached UI; the corpus's UI-attached `scroll:H` cases are the ",
            "real proof.)",
        ),
    ),
    na(
        "I",
        "I",
        Implemented,
        Some(nlive(
            NA_IND,
            (1, 8),
            24,
            "IX<Esc>",
            "    Xalpha beta|    gamma delta|    eps zeta",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:I indented")),
        "matches Vim: inserts before the first non-blank.",
    ),
    na(
        "J",
        "J",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "J",
            "alpha beta gamma delta epsilon zeta|eta theta iota",
            (1, 17),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:J basic")),
        "matches Vim: joins N lines with a space.",
    ),
    na(
        "K",
        "K",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "K",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("lsp:K keyword lookup")),
        concat!(
            "Vim looks the keyword under the cursor up with 'keywordprg'. ",
            "vimcode does nothing and says nothing. Worth implementing as LSP ",
            "hover — vimcode already has hover, it is simply not on `K`.",
        ),
    ),
    na(
        "L",
        "L",
        Implemented,
        Some(nlive(
            NA_LONG,
            (20, 1),
            10,
            "L",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (20, 1),
            "NORMAL",
            "",
            11,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:L from 30")),
        concat!(
            "matches Vim: to line N from the bottom of the window (see H on ",
            "the oracle's viewport).",
        ),
    ),
    na(
        "M",
        "M",
        Implemented,
        Some(nlive(
            NA_LONG,
            (20, 1),
            10,
            "M",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (15, 1),
            "NORMAL",
            "",
            11,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:M from 30")),
        "matches Vim: to the middle line of the window.",
    ),
    na(
        "N",
        "N",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "/eta<CR>N",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (3, 7),
            "NORMAL",
            "match 4 of 4",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("search:N reverse")),
        concat!(
            "matches Vim: repeats the last search in the opposite direction ",
            "(\"match 4 of 4\").",
        ),
    ),
    na(
        "O",
        "O",
        Implemented,
        Some(nlive(
            NA_TXT,
            (2, 1),
            24,
            "OX<Esc>",
            "alpha beta gamma|X|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:O first line")),
        "matches Vim: opens a line above and inserts.",
    ),
    na(
        "[\"x]P",
        "P",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "yyjP",
            "alpha beta gamma|alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:P charwise multiline")),
        "matches Vim: puts the register before the cursor.",
    ),
    na(
        "R",
        "R",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "RXY<Esc>",
            "XYpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:R")),
        "matches Vim: Replace mode overtypes existing characters.",
    ),
    na(
        "[\"x]S",
        "S",
        Implemented,
        Some(nlive(
            NA_TXT,
            (2, 1),
            24,
            "SX<Esc>",
            "alpha beta gamma|X|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:S keeps indent")),
        "matches Vim: deletes N lines and starts insert (\"cc\").",
    ),
    na(
        "T{char}",
        "T",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 12),
            24,
            "Tb",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 8),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:T, then ;")),
        "matches Vim: till after the Nth occurrence of {char} leftwards.",
    ),
    na(
        "U",
        "U",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "xxU",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Line restored",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("undo:U line")),
        concat!(
            "matches Vim: undoes all latest changes on one line (the ",
            "recording reports \"Line restored\").",
        ),
    ),
    na(
        "V",
        "V",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "Vj",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "VISUAL LINE",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:V2>")),
        "matches Vim: starts linewise Visual mode.",
    ),
    na(
        "W",
        "W",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "W",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:W")),
        "matches Vim: N WORDS forwards.",
    ),
    na(
        "[\"x]X",
        "X",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 3),
            24,
            "X",
            "apha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:X at col1")),
        "matches Vim: deletes N characters before the cursor.",
    ),
    na(
        "[\"x]Y",
        "Y",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "Yjp",
            "alpha beta gamma|delta epsilon zeta|alpha beta gamma|eta theta iota",
            (3, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:Y is linewise")),
        concat!(
            "matches Vim's documented `Y` (a synonym for \"yy\"), i.e. the ",
            "pre-'default-mappings' behaviour the whole suite compares ",
            "against.",
        ),
    ),
    na(
        "ZZ",
        "ZZ",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "ZZ",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("ex:ZZ writes and closes")),
        concat!(
            "Vim writes the buffer if changed and closes the window. vimcode ",
            "has **no `Z` handler at all** — the recording is a complete ",
            "no-op, no message, no EngineAction. Worth implementing: ",
            "`ZZ`/`ZQ` are the shortest way out of an editor and their ",
            "absence is silent.",
        ),
    ),
    na(
        "ZQ",
        "ZQ",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "ZQ",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("ex:ZQ closes without writing")),
        "Same as ZZ: no handler, no message.",
    ),
    na(
        "^",
        "^",
        Implemented,
        Some(nlive(
            NA_IND,
            (1, 9),
            24,
            "^",
            "    alpha beta|    gamma delta|    eps zeta",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:^")),
        "matches Vim: to the first non-blank of the line.",
    ),
    na(
        "_",
        "_",
        Implemented,
        Some(nlive(
            NA_IND,
            (1, 7),
            24,
            "2_",
            "    alpha beta|    gamma delta|    eps zeta",
            (2, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:2_")),
        "matches Vim: first non-blank, N-1 lines lower.",
    ),
    na(
        "`{a-zA-Z0-9}",
        "`",
        Implemented,
        Some(nlive(
            NA_IND,
            (1, 7),
            24,
            "majj`a",
            "    alpha beta|    gamma delta|    eps zeta",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:`a exact")),
        "matches Vim: to the exact position of the mark.",
    ),
    na(
        "`(",
        "`(",
        NotImplemented,
        Some(nlive(
            NA_SENT,
            (1, 20),
            24,
            "`(",
            "One two. Three four. Five six.",
            (1, 20),
            "NORMAL",
            "Marks must be a letter or special char",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:`( sentence start")),
        concat!(
            "Vim goes to the start of the current sentence; vimcode rejects ",
            "the mark (\"Marks must be a letter or special char\"). Same ",
            "one-line fix as `'(`.",
        ),
    ),
    na(
        "`)",
        "`)",
        NotImplemented,
        Some(nlive(
            NA_SENT,
            (1, 1),
            24,
            "`)",
            "One two. Three four. Five six.",
            (1, 1),
            "NORMAL",
            "Marks must be a letter or special char",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:`) sentence end")),
        "Same as `` `( ``.",
    ),
    na(
        "`<",
        "`<",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "vjl<Esc>gg`<",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:`< visual start")),
        "matches Vim: to the start of the last Visual area.",
    ),
    na(
        "`>",
        "`>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "vjl<Esc>gg`>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:`> visual end")),
        "matches Vim: to the end of the last Visual area.",
    ),
    na(
        "`[",
        "`[",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "yyjpgg`[",
            "alpha beta gamma|delta epsilon zeta|alpha beta gamma|eta theta iota",
            (1, 1),
            "NORMAL",
            "No previous change",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:`[ change start")),
        concat!(
            "Vim goes to the start of the last change or put; vimcode answers ",
            "\"No previous change\" (see `'[`).",
        ),
    ),
    na(
        "`]",
        "`]",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "yyjpgg`]",
            "alpha beta gamma|delta epsilon zeta|alpha beta gamma|eta theta iota",
            (1, 1),
            "NORMAL",
            "No previous change",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:`] change end")),
        concat!(
            "Vim goes to the end of the last change or put; vimcode answers ",
            "\"No previous change\".",
        ),
    ),
    na(
        "``",
        "``",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "G``",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:`` back")),
        "matches Vim: to the position before the latest jump.",
    ),
    na(
        "`{",
        "`{",
        NotImplemented,
        Some(nlive(
            NA_PARA,
            (4, 1),
            24,
            "`{",
            "alpha one|beta two||gamma three|delta four",
            (4, 1),
            "NORMAL",
            "Marks must be a letter or special char",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:`{ paragraph start")),
        concat!(
            "Vim goes to the start of the current paragraph; vimcode rejects ",
            "the mark.",
        ),
    ),
    na(
        "`}",
        "`}",
        NotImplemented,
        Some(nlive(
            NA_PARA,
            (1, 1),
            24,
            "`}",
            "alpha one|beta two||gamma three|delta four",
            (1, 1),
            "NORMAL",
            "Marks must be a letter or special char",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:`} paragraph end")),
        "Same as `` `{ ``.",
    ),
    na(
        "a",
        "a",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "aX<Esc>",
            "aXlpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:a at eol")),
        "matches Vim: appends after the cursor.",
    ),
    na(
        "b",
        "b",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 12),
            24,
            "b",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:b from mid word")),
        "matches Vim: N words backwards.",
    ),
    na(
        "[\"x]c{motion}",
        "c",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "cwX<Esc>",
            "X beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:cw on word")),
        "matches Vim: deletes Nmove text and starts insert.",
    ),
    na(
        "[\"x]cc",
        "cc",
        Implemented,
        Some(nlive(
            NA_IND,
            (1, 7),
            24,
            "ccX<Esc>",
            "    X|    gamma delta|    eps zeta",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:cc keeps indent")),
        concat!(
            "matches Vim: deletes N lines and starts insert, keeping the ",
            "indent.",
        ),
    ),
    na(
        "[\"x]d{motion}",
        "d",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "dw",
            "beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:dw last word of buffer")),
        "matches Vim: deletes Nmove text.",
    ),
    na(
        "[\"x]dd",
        "dd",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "dd",
            "delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:dd last line cursor")),
        "matches Vim: deletes N lines.",
    ),
    na(
        "do",
        "do",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "do",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Not in diff mode",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("diff:do obtains a line")),
        concat!(
            "implemented as `Engine::diff_obtain` ",
            "(`src/core/engine/windows.rs`); the recording shows its guard ",
            "(\"Not in diff mode\") because a diff pair needs two windows onto ",
            "two files, which this harness's single scratch buffer cannot set ",
            "up.",
        ),
    ),
    na(
        "dp",
        "dp",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "dp",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Not in diff mode",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("diff:dp puts a line")),
        concat!(
            "implemented as `Engine::diff_put`; recorded through the same ",
            "\"Not in diff mode\" guard as `do`.",
        ),
    ),
    na(
        "e",
        "e",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "e",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:e")),
        "matches Vim: forward to the end of word N.",
    ),
    na(
        "f{char}",
        "f",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "fb",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:f, then ;")),
        "matches Vim: to the Nth occurrence of {char} rightwards.",
    ),
    na(
        "h",
        "h",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 3),
            24,
            "h",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:h at start")),
        "matches Vim: N characters left.",
    ),
    na(
        "i",
        "i",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "iX<Esc>",
            "Xalpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:i col1 esc")),
        "matches Vim: inserts before the cursor.",
    ),
    na(
        "j",
        "j",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "j",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:j col memory short")),
        "matches Vim: N lines down, with column memory.",
    ),
    na(
        "k",
        "k",
        Implemented,
        Some(nlive(
            NA_TXT,
            (2, 1),
            24,
            "k",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:10k beyond")),
        "matches Vim: N lines up.",
    ),
    na(
        "l",
        "l",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "l",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:l right")),
        "matches Vim: N characters right.",
    ),
    na(
        "m{A-Za-z}",
        "m",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 7),
            24,
            "majj`a",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:d`a")),
        concat!(
            "matches Vim: sets the named mark at the cursor (the recording ",
            "jumps back to it with `` `a ``).",
        ),
    ),
    na(
        "n",
        "n",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "/eta<CR>ggn",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 16),
            "NORMAL",
            "match 2 of 4",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("search:n forward")),
        "matches Vim: repeats the last search (\"match 2 of 4\").",
    ),
    na(
        "o",
        "o",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "oX<Esc>",
            "alpha beta gamma|X|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:o autoindent")),
        "matches Vim: opens a line below and inserts.",
    ),
    na(
        "[\"x]p",
        "p",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "yyp",
            "alpha beta gamma|alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:p linewise cursor first nonblank")),
        "matches Vim: puts the register after the cursor.",
    ),
    na(
        "q{0-9a-zA-Z\"}",
        "q",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "qaxx",
            "pha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mac:qaxjq @a")),
        "matches Vim: records typed characters into the named register.",
    ),
    na(
        "q (while recording)",
        "q",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "qaxqj@a",
            "lpha beta gamma|elta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mac:q stops recording")),
        concat!(
            "matches Vim: a second `q` stops recording, and the register ",
            "replays (the recording's `@a` deletes a character on line 2).",
        ),
    ),
    na(
        "Q",
        "Q",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "qaxqjQ",
            "lpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mac:Q replays last register")),
        concat!(
            "Neovim's `Q` replays the last recorded register. vimcode does ",
            "nothing: after `qaxq` and a `j`, `Q` leaves line 2 untouched. ",
            "Cheap to implement on top of the existing macro playback queue.",
        ),
    ),
    na(
        "q:",
        "q:",
        Partial,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "q:",
            "|",
            (1, 1),
            "NORMAL",
            "Press Enter to execute, q to close",
            1,
            1,
            "",
            "1* tabs=2/2",
        )),
        Some(Label("cmdwin:q: opens the history window")),
        concat!(
            "Vim opens the command-line window *in the current tab*, filled ",
            "with the `:` history, and `:h cmdwin` semantics apply. vimcode ",
            "opens an empty command-line buffer **in a new tab** (\"Press ",
            "Enter to execute, q to close\"), so the keystroke is bound but ",
            "the window is not Vim's.",
        ),
    ),
    na(
        "q/",
        "q/",
        Partial,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "q/",
            "|",
            (1, 1),
            "NORMAL",
            "Press Enter to execute, q to close",
            1,
            1,
            "",
            "1* tabs=2/2",
        )),
        Some(Label("cmdwin:q/ opens the search history window")),
        concat!(
            "Same as `q:` — and the recording is byte-identical to it, i.e. ",
            "vimcode does not distinguish the `/` history from the `:` ",
            "history.",
        ),
    ),
    na(
        "q?",
        "q?",
        Partial,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "q?",
            "|",
            (1, 1),
            "NORMAL",
            "Press Enter to execute, q to close",
            1,
            1,
            "",
            "1* tabs=2/2",
        )),
        Some(Label("cmdwin:q? opens the reverse-search history window")),
        "Same as `q/`.",
    ),
    na(
        "r{char}",
        "r",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "rZ",
            "Zlpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:r")),
        "matches Vim: replaces N characters with {char}.",
    ),
    na(
        "[\"x]s",
        "s",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "sZ<Esc>",
            "Zlpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:s")),
        "matches Vim: deletes N characters and starts insert.",
    ),
    na(
        "t{char}",
        "t",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "tb",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 6),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:t; then ; repeat")),
        concat!(
            "matches Vim: till before the Nth occurrence of {char} ",
            "rightwards.",
        ),
    ),
    na(
        "u",
        "u",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "xu",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("undo:u on unchanged")),
        "matches Vim: undoes the last change.",
    ),
    na(
        "v",
        "v",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "vl",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "VISUAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("visual:v charwise")),
        "matches Vim: starts charwise Visual mode.",
    ),
    na(
        "w",
        "w",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "w",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:w punctuation")),
        "matches Vim: N words forwards.",
    ),
    na(
        "[\"x]x",
        "x",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "x",
            "lpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:x at eol")),
        "matches Vim: deletes N characters under and after the cursor.",
    ),
    na(
        "[\"x]y{motion}",
        "y",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "ywP",
            "alpha alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 6),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:yiw cursor")),
        "matches Vim: yanks Nmove text.",
    ),
    na(
        "[\"x]yy",
        "yy",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "yyp",
            "alpha beta gamma|alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:yy 3p")),
        "matches Vim: yanks N lines.",
    ),
    na(
        "{",
        "{",
        Implemented,
        Some(nlive(
            NA_PARA,
            (5, 1),
            24,
            "{",
            "alpha one|beta two||gamma three|delta four",
            (3, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:{")),
        "matches Vim: N paragraphs backwards.",
    ),
    na(
        "|",
        "bar",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "8|",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 8),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:4|")),
        "matches Vim: to column N.",
    ),
    na(
        "}",
        "}",
        Implemented,
        Some(nlive(
            NA_PARA,
            (1, 1),
            24,
            "}",
            "alpha one|beta two||gamma three|delta four",
            (3, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:2}")),
        "matches Vim: N paragraphs forwards.",
    ),
    na(
        "~",
        "~",
        Implemented,
        Some(nlive(
            NA_MIX,
            (1, 1),
            24,
            "3~",
            "ABC DEF ghi",
            (1, 4),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:3~")),
        concat!(
            "matches Vim with 'tildeop' off: switches the case of N ",
            "characters and moves right.",
        ),
    ),
    na(
        "~{motion} ('tildeop')",
        "~",
        NotImplemented,
        Some(nlive(
            NA_MIX,
            (1, 1),
            24,
            ":set tildeop<CR>~w",
            "Abc DEF ghi",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("opt:tildeop makes ~ an operator")),
        concat!(
            "Vim with 'tildeop' on makes `~` take a motion. vimcode has no ",
            "'tildeop' option — `:set tildeop` is an error and `~w` switches ",
            "a single character and then moves a word. Low value: 'tildeop' ",
            "is off by default and rarely turned on.",
        ),
    ),
    na(
        "<C-End>",
        "<C-End>",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "<C-End>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 7),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<C-End> is G")),
        concat!(
            "Vim's <C-End> is \"G\" (last line). vimcode routes it to plain ",
            "<End> — the recording ends at the end of *line 1*, not on line ",
            "30. Worth implementing; it is a one-line alias and the wrong ",
            "answer is a *jump*, which is easy to mistake for a bug in G.",
        ),
    ),
    na(
        "<C-Home>",
        "<C-Home>",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (20, 1),
            10,
            "<C-Home>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (20, 1),
            "NORMAL",
            "",
            11,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<C-Home> is gg")),
        concat!(
            "Vim's <C-Home> is \"gg\". vimcode ignores it entirely (cursor ",
            "unmoved on line 20).",
        ),
    ),
    na(
        "<C-Left>",
        "<C-Left>",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 12),
            24,
            "<C-Left>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 11),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<C-Left> is b")),
        concat!(
            "Vim's <C-Left> is \"b\" (word left). vimcode moves a single ",
            "character left, i.e. it treats it as plain <Left>. Worth ",
            "implementing: every other editor's Ctrl+Arrow is word-wise and ",
            "the current behaviour is quietly wrong rather than absent.",
        ),
    ),
    na(
        "<C-LeftMouse>",
        "<C-LeftMouse>",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "Vim runs `:ta` on the keyword at the click. vimcode has no tag ",
            "stack (see CTRL-]).",
        ),
    ),
    na(
        "<C-Right>",
        "<C-Right>",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-Right>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<C-Right> is w")),
        concat!(
            "Vim's <C-Right> is \"w\". vimcode treats it as plain <Right> (one ",
            "character).",
        ),
    ),
    na(
        "<C-RightMouse>",
        "<C-RightMouse>",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "Vim pops the tag stack (CTRL-T) at the click position; vimcode ",
            "has no tag stack.",
        ),
    ),
    na(
        "<C-Tab>",
        "<C-Tab>",
        Partial,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-Tab>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("tab:<C-Tab> last accessed")),
        concat!(
            "The doc lists this twice (`|CTRL-<Tab>|` and `|<C-Tab>|`); both ",
            "are \"go to the last accessed tab page\". vimcode binds ctrl+Tab ",
            "to its MRU **tab switcher overlay** instead — bound, but a ",
            "different interaction, and invisible to a ",
            "buffer/cursor/mode/message recording.",
        ),
    ),
    na(
        "[\"x]<Del>",
        "<Del>",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<Del>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:<Del> is x")),
        concat!(
            "Vim's <Del> is \"x\". vimcode ignores it in Normal mode — the ",
            "recording leaves the buffer untouched. Worth implementing: it is ",
            "one arm, and users who reach for <Del> get silence.",
        ),
    ),
    na(
        "{count}<Del>",
        "N<Del>",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "12<Del>l",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 13),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:{count}<Del> drops a digit")),
        concat!(
            "Vim's `{count}<Del>` removes the last digit of the count. ",
            "vimcode ignores the <Del> and applies the whole count — ",
            "`12<Del>l` moves twelve columns instead of one. Very low value; ",
            "nobody types this deliberately.",
        ),
    ),
    na(
        "<Down>",
        "<Down>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<Down>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<Down> is j")),
        "matches Vim: same as \"j\".",
    ),
    na(
        "<End>",
        "<End>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<End>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 16),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<End> is $")),
        "matches Vim: same as \"$\".",
    ),
    na(
        "<F1>",
        "<F1>",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<F1>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("help:<F1> opens help")),
        concat!(
            "Vim opens a help window. vimcode does nothing. Worth ",
            "implementing only alongside a real in-app help surface; `:help` ",
            "is itself not a vimcode command.",
        ),
    ),
    na(
        "<Help>",
        "<Help>",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<Help>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("help:<Help> opens help")),
        "Same as <F1>.",
    ),
    na(
        "<Home>",
        "<Home>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 5),
            24,
            "<Home>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<Home> is 0")),
        "matches Vim: same as \"0\".",
    ),
    na(
        "<Insert>",
        "<Insert>",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<Insert>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:<Insert> is i")),
        concat!(
            "Vim's <Insert> is \"i\". vimcode ignores it — the recording stays ",
            "in NORMAL. Worth implementing: one arm, and it is what a non-Vim ",
            "user presses first.",
        ),
    ),
    na(
        "<Left>",
        "<Left>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 3),
            24,
            "<Left>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<Left> is h")),
        "matches Vim: same as \"h\".",
    ),
    na(
        "<LeftMouse>",
        "<LeftMouse>",
        Implemented,
        None,
        None,
        concat!(
            "matches Vim: a click moves the cursor to the clicked position. ",
            "No key-replay is possible — mouse events arrive as backend ",
            "events, never as `handle_key` names — but both backends ",
            "implement it and #1104's hit-testing test covers the GTK path.",
        ),
    ),
    na(
        "<MiddleMouse>",
        "<MiddleMouse>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim pastes (\"gP\") at the click position. vimcode's middle click ",
            "does not paste. No key-replay possible (mouse event).",
        ),
    ),
    na(
        "<PageDown>",
        "<PageDown>",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "<PageDown>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:<PageDown> is C-f")),
        concat!(
            "Vim's <PageDown> is CTRL-F. vimcode ignores the key name ",
            "entirely — the recording never leaves line 1. Worth ",
            "implementing: PageUp/PageDown are the two keys a mouse-first ",
            "user reaches for, and both are dead.",
        ),
    ),
    na(
        "<PageUp>",
        "<PageUp>",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (20, 1),
            10,
            "<PageUp>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (20, 1),
            "NORMAL",
            "",
            11,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:<PageUp> is C-b")),
        "Same as <PageDown>: ignored.",
    ),
    na(
        "<Right>",
        "<Right>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<Right>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<Right> is l")),
        "matches Vim: same as \"l\".",
    ),
    na(
        "<RightMouse>",
        "<RightMouse>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim starts Visual mode at the click position; vimcode opens a ",
            "context menu instead (a deliberate IDE-shaped choice, but a ",
            "conformance gap). No key-replay possible.",
        ),
    ),
    na(
        "<S-Down>",
        "<S-Down>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim's <S-Down> is CTRL-F. `Engine::handle_key` has no Shift ",
            "parameter, so the backends cannot deliver this at all (see ",
            "<S-NL>).",
        ),
    ),
    na(
        "<S-Left>",
        "<S-Left>",
        NotImplemented,
        None,
        None,
        "Vim's <S-Left> is \"b\"; the Shift bit never reaches the engine.",
    ),
    na(
        "<S-LeftMouse>",
        "<S-LeftMouse>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim searches forward for the word at the click (\"*\"); vimcode ",
            "does not. No key-replay possible.",
        ),
    ),
    na(
        "<S-Right>",
        "<S-Right>",
        NotImplemented,
        None,
        None,
        "Vim's <S-Right> is \"w\"; the Shift bit never reaches the engine.",
    ),
    na(
        "<S-RightMouse>",
        "<S-RightMouse>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim searches backwards for the word at the click (\"#\"); vimcode ",
            "does not.",
        ),
    ),
    na(
        "<S-Up>",
        "<S-Up>",
        NotImplemented,
        None,
        None,
        "Vim's <S-Up> is CTRL-B; the Shift bit never reaches the engine.",
    ),
    na(
        "<Undo>",
        "<Undo>",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "x<Undo>",
            "lpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("undo:<Undo> is u")),
        concat!(
            "Vim's <Undo> key is \"u\". vimcode ignores it — the recording's ",
            "`x` is still deleted afterwards.",
        ),
    ),
    na(
        "<Up>",
        "<Up>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (2, 1),
            24,
            "<Up>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:<Up> is k")),
        "matches Vim: same as \"k\".",
    ),
    na(
        "<ScrollWheelDown>",
        "<ScrollWheelDown>",
        Partial,
        None,
        None,
        concat!(
            "Both backends scroll the window on a wheel event ",
            "(tests/terminal_wheel.rs), so the gesture works — but whether it ",
            "is Vim's *three lines per notch* is pinned by nothing, and the ",
            "event never reaches `Engine::handle_key`, so this slice cannot ",
            "measure it.",
        ),
    ),
    na(
        "<S-ScrollWheelDown>",
        "<S-ScrollWheelDown>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim scrolls a whole page for a shifted wheel notch; vimcode has ",
            "no Shift-aware wheel path.",
        ),
    ),
    na(
        "<ScrollWheelUp>",
        "<ScrollWheelUp>",
        Partial,
        None,
        None,
        "See <ScrollWheelDown>.",
    ),
    na(
        "<S-ScrollWheelUp>",
        "<S-ScrollWheelUp>",
        NotImplemented,
        None,
        None,
        "See <S-ScrollWheelDown>.",
    ),
    na(
        "<ScrollWheelLeft>",
        "<ScrollWheelLeft>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim scrolls six columns left; vimcode has no horizontal wheel ",
            "handling.",
        ),
    ),
    na(
        "<S-ScrollWheelLeft>",
        "<S-ScrollWheelLeft>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim scrolls a page left; vimcode has no horizontal wheel ",
            "handling.",
        ),
    ),
    na(
        "<ScrollWheelRight>",
        "<ScrollWheelRight>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim scrolls six columns right; vimcode has no horizontal wheel ",
            "handling.",
        ),
    ),
    na(
        "<S-ScrollWheelRight>",
        "<S-ScrollWheelRight>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim scrolls a page right; vimcode has no horizontal wheel ",
            "handling.",
        ),
    ),
    na(
        "CTRL-W CTRL-B",
        "CTRL-W_CTRL-B",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w><C-b>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w C-b goes to the bottom window")),
        concat!(
            "Vim's alias for \"CTRL-W b\". vimcode leaves the focus where it ",
            "was (the recording ends focused on the top window). One entry in ",
            "the wincmd alias table.",
        ),
    ),
    na(
        "CTRL-W CTRL-C",
        "CTRL-W_CTRL-C",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w><C-c>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Cannot close last window",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w C-c is a no-op")),
        concat!(
            "`:help CTRL-W_CTRL-C` is explicit: **no-op**. vimcode routes it ",
            "to \"close window\" — the recording only survives because there is ",
            "a single window (\"Cannot close last window\"); with a split it ",
            "destroys one. Worth fixing precisely because the failure is ",
            "destructive and silent.",
        ),
    ),
    na(
        "CTRL-W CTRL-D",
        "CTRL-W_CTRL-D",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w><C-d>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (3, 1),
            "NORMAL",
            "",
            3,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w C-d splits to the definition")),
        concat!(
            "Vim's alias for \"CTRL-W d\". vimcode falls through to plain ",
            "CTRL-D and scrolls half a page instead — no split at all.",
        ),
    ),
    na(
        "CTRL-W CTRL-F",
        "CTRL-W_CTRL-F",
        NotImplemented,
        Some(nlive(
            NA_FILE,
            (1, 1),
            24,
            "<C-w><C-f>",
            "README.md|second line",
            (2, 1),
            "NORMAL",
            "",
            2,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w C-f splits to the file under the cursor")),
        concat!(
            "Vim's alias for \"CTRL-W f\". vimcode falls through to plain ",
            "CTRL-F and pages down.",
        ),
    ),
    na(
        "CTRL-W CTRL-H",
        "CTRL-W_CTRL-H",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v<C-w>l<C-w><C-h>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(v0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w C-h goes left")),
        "matches Vim: the alias moves focus to the window on the left.",
    ),
    na(
        "CTRL-W CTRL-I",
        "CTRL-W_CTRL-I",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w><C-i>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Already at newest position in jump list",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w C-i splits to the declaration")),
        concat!(
            "Vim's alias for \"CTRL-W i\". vimcode falls through to plain ",
            "CTRL-I and walks the jump list (\"Already at newest position in ",
            "jump list\").",
        ),
    ),
    na(
        "CTRL-W CTRL-J",
        "CTRL-W_CTRL-J",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w><C-j>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2 1*) tabs=1/1",
        )),
        Some(Label("win:C-w C-j goes down")),
        "matches Vim: the alias moves focus to the window below.",
    ),
    na(
        "CTRL-W CTRL-K",
        "CTRL-W_CTRL-K",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>j<C-w><C-k>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w C-k goes up")),
        "matches Vim: the alias moves focus to the window above.",
    ),
    na(
        "CTRL-W CTRL-L",
        "CTRL-W_CTRL-L",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v<C-w>h<C-w><C-l>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(v0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w C-l goes right")),
        concat!(
            "Vim's alias for \"CTRL-W l\". vimcode does not move the focus, ",
            "even though the unprefixed `CTRL-W l` works — the alias table ",
            "has h/j/k but not l.",
        ),
    ),
    na(
        "CTRL-W CTRL-N",
        "CTRL-W_CTRL-N",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w><C-n>",
            "",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w C-n opens a new window")),
        "matches Vim: opens a new window on an empty buffer.",
    ),
    na(
        "CTRL-W CTRL-O",
        "CTRL-W_CTRL-O",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w><C-o>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Already at oldest position in jump list",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w C-o closes the others")),
        concat!(
            "Vim's alias for \"CTRL-W o\". vimcode falls through to plain ",
            "CTRL-O and walks the jump list, leaving both windows open.",
        ),
    ),
    na(
        "CTRL-W CTRL-P",
        "CTRL-W_CTRL-P",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w><C-p>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w C-p goes to the previous window")),
        concat!(
            "Vim's alias for \"CTRL-W p\". vimcode leaves the focus where it ",
            "was — and so does the unprefixed `CTRL-W p`, so the gap is the ",
            "last-accessed-window bookkeeping, not the alias.",
        ),
    ),
    na(
        "CTRL-W CTRL-Q",
        "CTRL-W_CTRL-Q",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w><C-q>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w C-q quits the window")),
        "matches Vim: quits the current window.",
    ),
    na(
        "CTRL-W CTRL-R",
        "CTRL-W_CTRL-R",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w><C-r>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Already at newest change",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w C-r rotates")),
        concat!(
            "Vim's alias for \"CTRL-W r\". vimcode falls through to plain ",
            "CTRL-R and tries to redo (\"Already at newest change\").",
        ),
    ),
    na(
        "CTRL-W CTRL-S",
        "CTRL-W_CTRL-S",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w><C-s>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Save failed: No file name",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w C-s splits")),
        concat!(
            "Vim's alias for \"CTRL-W s\". vimcode falls through to its save ",
            "binding (\"Save failed: No file name\") and never splits — the ",
            "most surprising of the fall-throughs, because it can write a ",
            "file.",
        ),
    ),
    na(
        "CTRL-W CTRL-T",
        "CTRL-W_CTRL-T",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>j<C-w><C-t>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2 1*) tabs=1/1",
        )),
        Some(Label("win:C-w C-t goes to the top window")),
        concat!(
            "Vim's alias for \"CTRL-W t\". Focus unchanged; the unprefixed ",
            "`CTRL-W t` is broken too.",
        ),
    ),
    na(
        "CTRL-W CTRL-V",
        "CTRL-W_CTRL-V",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w><C-v>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "VISUAL BLOCK",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w C-v splits vertically")),
        concat!(
            "Vim's alias for \"CTRL-W v\". vimcode falls through to plain ",
            "CTRL-V and **enters Visual Block mode** instead of splitting.",
        ),
    ),
    na(
        "CTRL-W CTRL-W",
        "CTRL-W_CTRL-W",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w><C-w>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w C-w cycles windows")),
        concat!(
            "Vim's alias for \"CTRL-W w\". Focus unchanged, although the ",
            "unprefixed `CTRL-W w` cycles correctly.",
        ),
    ),
    na(
        "CTRL-W CTRL-X",
        "CTRL-W_CTRL-X",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w><C-x>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "No number under cursor",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w C-x exchanges windows")),
        concat!(
            "Vim's alias for \"CTRL-W x\". vimcode falls through to plain ",
            "CTRL-X and tries to decrement a number (\"No number under ",
            "cursor\").",
        ),
    ),
    na(
        "CTRL-W CTRL-Z",
        "CTRL-W_CTRL-Z",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w><C-z>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Unknown wincmd: z",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w C-z closes the preview window")),
        concat!(
            "Vim's alias for \"CTRL-W z\". vimcode answers \"Unknown wincmd: z\" ",
            "— at least it is loud.",
        ),
    ),
    na(
        "CTRL-W CTRL-]",
        "CTRL-W_CTRL-]",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "Vim splits and jumps to the tag under the cursor; vimcode has no ",
            "tag stack.",
        ),
    ),
    na(
        "CTRL-W CTRL-^",
        "CTRL-W_CTRL-^",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w><C-^>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Unknown wincmd: ^",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w C-^ splits to the alternate file")),
        concat!(
            "Vim's alias for \"CTRL-W ^\". vimcode answers \"Unknown wincmd: ^\" ",
            "— it has no alternate file at all (see CTRL-^).",
        ),
    ),
    na(
        "CTRL-W CTRL-_",
        "CTRL-W_CTRL-_",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w><C-_>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.90 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w C-_ maximises the height")),
        concat!(
            "matches Vim: the alias sets the window height to its maximum ",
            "(the recorded split ratio goes to 0.90).",
        ),
    ),
    na(
        "CTRL-W +",
        "CTRL-W_+",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>+",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.55 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w + grows the height")),
        concat!(
            "matches Vim: increases the current window's height (ratio 0.50 ",
            "to 0.55).",
        ),
    ),
    na(
        "CTRL-W -",
        "CTRL-W_-",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>-",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.45 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w - shrinks the height")),
        concat!(
            "matches Vim: decreases the current window's height (ratio 0.50 ",
            "to 0.45).",
        ),
    ),
    na(
        "CTRL-W <",
        "CTRL-W_<",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v<C-w><",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(v0.45 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w < shrinks the width")),
        "matches Vim: decreases the current window's width.",
    ),
    na(
        "CTRL-W =",
        "CTRL-W_=",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>+<C-w>=",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w = equalises")),
        concat!(
            "matches Vim: returns the split to equal shares (0.55 back to ",
            "0.50).",
        ),
    ),
    na(
        "CTRL-W >",
        "CTRL-W_>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v<C-w>>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(v0.55 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w > grows the width")),
        "matches Vim: increases the current window's width.",
    ),
    na(
        "CTRL-W H",
        "CTRL-W_H",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>H",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w H moves the window far left")),
        concat!(
            "Vim moves the current window to the far left, keeping both ",
            "windows. vimcode **destroys the layout**: the recording ends ",
            "with a single, brand-new window (id 3) and both original windows ",
            "gone. The worst row in this slice — an unimplemented command ",
            "that silently discards the user's other window.",
        ),
    ),
    na(
        "CTRL-W J",
        "CTRL-W_J",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v<C-w>J",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w J moves the window to the bottom")),
        "Same destructive collapse as CTRL-W H.",
    ),
    na(
        "CTRL-W K",
        "CTRL-W_K",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v<C-w>K",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w K moves the window to the top")),
        "Same destructive collapse as CTRL-W H.",
    ),
    na(
        "CTRL-W L",
        "CTRL-W_L",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>L",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w L moves the window far right")),
        "Same destructive collapse as CTRL-W H.",
    ),
    na(
        "CTRL-W P",
        "CTRL-W_P",
        Skipped(PREVIEW),
        None,
        None,
        "Vim goes to the preview window; vimcode has no preview window.",
    ),
    na(
        "CTRL-W R",
        "CTRL-W_R",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>R",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w R rotates upwards")),
        concat!(
            "Vim rotates the windows upwards; vimcode's layout is ",
            "byte-identical afterwards (window ids in the same slots).",
        ),
    ),
    na(
        "CTRL-W S",
        "CTRL-W_S",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>S",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w S splits")),
        concat!(
            "matches Vim: the uppercase synonym for \"CTRL-W s\" splits the ",
            "window.",
        ),
    ),
    na(
        "CTRL-W T",
        "CTRL-W_T",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>T",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w T moves the window to a new tab")),
        concat!(
            "Vim moves the current window into a new tab page. vimcode ends ",
            "with one tab and, as with CTRL-W H, a single brand-new window — ",
            "the split is destroyed rather than moved.",
        ),
    ),
    na(
        "CTRL-W W",
        "CTRL-W_W",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>W",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2 1*) tabs=1/1",
        )),
        Some(Label("win:C-w W goes to the previous window")),
        "matches Vim: cycles to the previous window, wrapping.",
    ),
    na(
        "CTRL-W ]",
        "CTRL-W_]",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "Vim splits and jumps to the tag under the cursor; no tag stack ",
            "in vimcode (\"Unknown wincmd: ]\").",
        ),
    ),
    na(
        "CTRL-W ^",
        "CTRL-W_^",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>^",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Unknown wincmd: ^",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w ^ splits to the alternate file")),
        concat!(
            "Vim splits and edits the alternate file; vimcode answers ",
            "\"Unknown wincmd: ^\" (see CTRL-^).",
        ),
    ),
    na(
        "CTRL-W _",
        "CTRL-W__",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>_",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.90 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w _ maximises the height")),
        concat!(
            "matches Vim: sets the current window height to the maximum ",
            "(ratio 0.90).",
        ),
    ),
    na(
        "CTRL-W b",
        "CTRL-W_b",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>b",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w b goes to the bottom window")),
        concat!(
            "Vim goes to the bottom window; vimcode leaves the focus on the ",
            "top one. Worth implementing with `CTRL-W t` — one pair, one ",
            "traversal helper.",
        ),
    ),
    na(
        "CTRL-W c",
        "CTRL-W_c",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>c",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w c closes the window")),
        "matches Vim: closes the current window like `:close`.",
    ),
    na(
        "CTRL-W d",
        "CTRL-W_d",
        Partial,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>d",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w d splits to the definition")),
        concat!(
            "Vim splits and jumps to the *tag* definition. vimcode does ",
            "split, then asks the LSP for the definition — the right shape ",
            "through a different mechanism, and with no LSP attached the ",
            "recording shows the split and no jump.",
        ),
    ),
    na(
        "CTRL-W f",
        "CTRL-W_f",
        Partial,
        Some(nlive(
            NA_FILE,
            (1, 1),
            24,
            "<C-w>f",
            "README.md|second line",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w f splits to the file under the cursor")),
        concat!(
            "Vim splits and edits the file whose name is under the cursor. ",
            "vimcode splits, but the new window still shows the current ",
            "buffer — the file under the cursor is never opened.",
        ),
    ),
    na(
        "CTRL-W F",
        "CTRL-W_F",
        NotImplemented,
        Some(nlive(
            NA_FILE,
            (1, 1),
            24,
            "<C-w>F",
            "README.md|second line",
            (1, 1),
            "NORMAL",
            "Unknown wincmd: F",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w F splits to the file and line under the cursor")),
        concat!(
            "Vim splits, edits the file under the cursor and jumps to the ",
            "line number after it; vimcode answers \"Unknown wincmd: F\".",
        ),
    ),
    na(
        "CTRL-W g CTRL-]",
        "CTRL-W_g_CTRL-]",
        Skipped(CTAGS),
        None,
        None,
        "Vim splits and does `:tjump`; no tag stack in vimcode.",
    ),
    na(
        "CTRL-W g ]",
        "CTRL-W_g]",
        Skipped(CTAGS),
        None,
        None,
        "Vim splits and does `:tselect`; no tag stack in vimcode.",
    ),
    na(
        "CTRL-W g }",
        "CTRL-W_g}",
        Skipped(PREVIEW),
        None,
        None,
        concat!(
            "Vim does a `:ptjump` into the preview window; vimcode has ",
            "neither tags nor a preview window.",
        ),
    ),
    na(
        "CTRL-W g f",
        "CTRL-W_gf",
        NotImplemented,
        Some(nlive(
            NA_FILE,
            (1, 1),
            24,
            "<C-w>gf",
            "README.md|second line",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w gf opens the file under the cursor in a tab")),
        concat!(
            "Vim opens the file under the cursor in a new tab page; vimcode ",
            "does nothing at all (no tab, no message).",
        ),
    ),
    na(
        "CTRL-W g F",
        "CTRL-W_gF",
        NotImplemented,
        Some(nlive(
            NA_FILE,
            (1, 1),
            24,
            "<C-w>gF",
            "README.md|second line",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w gF opens the file and line under the cursor in a tab")),
        "Same as `CTRL-W gf`, plus the line number; vimcode does nothing.",
    ),
    na(
        "CTRL-W g t",
        "CTRL-W_gt",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            ":tabnew<CR><C-w>gt",
            "",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=2/2",
        )),
        Some(Label("win:C-w gt goes to the next tab")),
        concat!(
            "Vim's `CTRL-W g t` is `gt`. With two tabs open the recording ",
            "stays on tab 2, so the prefixed form is unbound even though the ",
            "g-prefix slice found plain `gt` implemented.",
        ),
    ),
    na(
        "CTRL-W g T",
        "CTRL-W_gT",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            ":tabnew<CR><C-w>gT",
            "",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=2/2",
        )),
        Some(Label("win:C-w gT goes to the previous tab")),
        "Same as `CTRL-W g t`: the prefixed form does not reach `gT`.",
    ),
    na(
        "CTRL-W g <Tab>",
        "CTRL-W_g<Tab>",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            ":tabnew<CR><C-w>g<Tab>",
            "",
            (1, 1),
            "NORMAL",
            "Already at newest position in jump list",
            1,
            1,
            "",
            "1* tabs=2/2",
        )),
        Some(Label("win:C-w g<Tab> goes to the last accessed tab")),
        concat!(
            "Vim's `CTRL-W g <Tab>` is `g<Tab>`. vimcode walks the jump list ",
            "instead (\"Already at newest position in jump list\") and stays on ",
            "tab 2.",
        ),
    ),
    na(
        "CTRL-W h",
        "CTRL-W_h",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v<C-w>l<C-w>h",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(v0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w h goes left")),
        "matches Vim: focus to the window on the left.",
    ),
    na(
        "CTRL-W i",
        "CTRL-W_i",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>i",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "Unknown wincmd: i",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w i splits to the declaration")),
        concat!(
            "Vim splits and jumps to the declaration of the identifier under ",
            "the cursor (an 'include'-path search); vimcode answers \"Unknown ",
            "wincmd: i\". Close relative of the CTAGS family, but listed as ❌ ",
            "rather than skipped because vimcode's LSP could answer it, ",
            "exactly as it already does for `CTRL-W d`.",
        ),
    ),
    na(
        "CTRL-W j",
        "CTRL-W_j",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>j",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2 1*) tabs=1/1",
        )),
        Some(Label("win:C-w j goes down")),
        "matches Vim: focus to the window below.",
    ),
    na(
        "CTRL-W k",
        "CTRL-W_k",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>j<C-w>k",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w k goes up")),
        "matches Vim: focus to the window above.",
    ),
    na(
        "CTRL-W l",
        "CTRL-W_l",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v<C-w>h<C-w>l",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(v0.50 2 1*) tabs=1/1",
        )),
        Some(Label("win:C-w l goes right")),
        "matches Vim: focus to the window on the right.",
    ),
    na(
        "CTRL-W n",
        "CTRL-W_n",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>n",
            "",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w n opens a new window")),
        "matches Vim: opens a new window on an empty buffer.",
    ),
    na(
        "CTRL-W o",
        "CTRL-W_o",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>o",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w o closes the others")),
        "matches Vim: closes every window but the current one.",
    ),
    na(
        "CTRL-W p",
        "CTRL-W_p",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>p",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w p goes to the previous window")),
        concat!(
            "Vim goes to the last accessed window. vimcode leaves the focus ",
            "alone — it keeps no last-accessed-window record. Worth ",
            "implementing: `CTRL-W p` is the fastest two-window toggle there ",
            "is.",
        ),
    ),
    na(
        "CTRL-W q",
        "CTRL-W_q",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>q",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:C-w q quits the window")),
        "matches Vim: quits the current window like `:quit`.",
    ),
    na(
        "CTRL-W r",
        "CTRL-W_r",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>r",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w r rotates downwards")),
        concat!(
            "Vim rotates the windows downwards; vimcode's layout is unchanged ",
            "afterwards.",
        ),
    ),
    na(
        "CTRL-W s",
        "CTRL-W_s",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w s splits")),
        concat!(
            "matches Vim: splits the window horizontally, focus in the new ",
            "one.",
        ),
    ),
    na(
        "CTRL-W t",
        "CTRL-W_t",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>j<C-w>t",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2 1*) tabs=1/1",
        )),
        Some(Label("win:C-w t goes to the top window")),
        concat!(
            "Vim goes to the top window; vimcode leaves the focus on the ",
            "bottom one (see `CTRL-W b`).",
        ),
    ),
    na(
        "CTRL-W v",
        "CTRL-W_v",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(v0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w v splits vertically")),
        "matches Vim: splits the window vertically, focus in the new one.",
    ),
    na(
        "CTRL-W w",
        "CTRL-W_w",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>w",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2 1*) tabs=1/1",
        )),
        Some(Label("win:C-w w cycles windows")),
        "matches Vim: to the next window, wrapping.",
    ),
    na(
        "CTRL-W x",
        "CTRL-W_x",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>x",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w x exchanges windows")),
        concat!(
            "Vim exchanges the current window with the next one; vimcode's ",
            "layout is unchanged afterwards (both windows keep their slots).",
        ),
    ),
    na(
        "CTRL-W z",
        "CTRL-W_z",
        Skipped(PREVIEW),
        None,
        None,
        concat!(
            "Vim closes the preview window; vimcode has no preview window ",
            "(\"Unknown wincmd: z\").",
        ),
    ),
    na(
        "CTRL-W |",
        "CTRL-W_bar",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v<C-w>|",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(v0.90 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w bar maximises the width")),
        "matches Vim: sets the window width to the maximum (ratio 0.90).",
    ),
    na(
        "CTRL-W }",
        "CTRL-W_}",
        Skipped(PREVIEW),
        None,
        None,
        concat!(
            "Vim shows the tag under the cursor in the preview window; ",
            "vimcode has neither.",
        ),
    ),
    na(
        "CTRL-W <Down>",
        "CTRL-W_<Down>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w><Down>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2 1*) tabs=1/1",
        )),
        Some(Label("win:C-w <Down> goes down")),
        "matches Vim: same as \"CTRL-W j\".",
    ),
    na(
        "CTRL-W <Up>",
        "CTRL-W_<Up>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>s<C-w>j<C-w><Up>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(h0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w <Up> goes up")),
        "matches Vim: same as \"CTRL-W k\".",
    ),
    na(
        "CTRL-W <Left>",
        "CTRL-W_<Left>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v<C-w>l<C-w><Left>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(v0.50 2* 1) tabs=1/1",
        )),
        Some(Label("win:C-w <Left> goes left")),
        "matches Vim: same as \"CTRL-W h\".",
    ),
    na(
        "CTRL-W <Right>",
        "CTRL-W_<Right>",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-w>v<C-w>h<C-w><Right>",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "(v0.50 2 1*) tabs=1/1",
        )),
        Some(Label("win:C-w <Right> goes right")),
        "matches Vim: same as \"CTRL-W l\".",
    ),
    na(
        "[ CTRL-D",
        "[_CTRL-D",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "Vim searches the 'include' path for the first matching #define; ",
            "vimcode has no tag or include search.",
        ),
    ),
    na(
        "[ CTRL-I",
        "[_CTRL-I",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "Vim searches the 'include' path for the first matching line; ",
            "vimcode has no include search.",
        ),
    ),
    na(
        "[#",
        "[#",
        Implemented,
        Some(nlive(
            NA_CODE,
            (7, 1),
            24,
            "[#",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (6, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:[# previous unmatched if")),
        "matches Vim: back to the previous unmatched #if/#else/#ifdef.",
    ),
    na(
        "['",
        "['",
        NotImplemented,
        Some(nlive(
            NA_CODE,
            (2, 1),
            24,
            "majjj['",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (5, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:[' previous mark line")),
        concat!(
            "Vim goes to the previous lowercase mark, on the first non-blank. ",
            "vimcode does not move at all. Worth implementing with `` [` ",
            "``/`` ]` ``/`]'` — one sorted walk over the existing mark table.",
        ),
    ),
    na(
        "[(",
        "[(",
        Implemented,
        Some(nlive(
            NA_BR,
            (1, 8),
            24,
            "[(",
            "foo(bar baz) qux|arr[one two] end|map{k v} tail",
            (1, 4),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:[(")),
        "matches Vim: back to the unmatched '('.",
    ),
    na(
        "[*",
        "[star",
        Implemented,
        Some(nlive(
            NA_CODE,
            (9, 1),
            24,
            "[*",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (7, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:[* previous comment start")),
        "matches Vim: back to the previous start of a C comment.",
    ),
    na(
        "[`",
        "[`",
        NotImplemented,
        Some(nlive(
            NA_CODE,
            (2, 3),
            24,
            "majjj[`",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (5, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:[` previous mark")),
        "Same as `['`: no movement.",
    ),
    na(
        "[/",
        "[/",
        Implemented,
        Some(nlive(
            NA_CODE,
            (9, 1),
            24,
            "[/",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (7, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:[/ previous comment start")),
        "matches Vim: same as \"[*\".",
    ),
    na(
        "[D",
        "[D",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "Vim lists every matching #define from the included files; no ",
            "include search in vimcode.",
        ),
    ),
    na(
        "[I",
        "[I",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "Vim lists every matching line from the included files; no ",
            "include search in vimcode.",
        ),
    ),
    na(
        "[P",
        "[P",
        NotImplemented,
        Some(nlive(
            NA_IND,
            (1, 1),
            24,
            "yyj[P",
            "    alpha beta|    gamma delta|    eps zeta",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:[P puts with indent")),
        concat!(
            "Vim's `[P` is `[p` — put linewise with the indent adjusted to ",
            "the current line. vimcode does nothing at all: the recording's ",
            "buffer is byte-identical to the fixture, while `[p` (which it ",
            "does implement) pastes. Worth implementing: it is an alias of a ",
            "command that already exists.",
        ),
    ),
    na(
        "[[",
        "[[",
        Implemented,
        Some(nlive(
            NA_CODE,
            (10, 1),
            24,
            "[[",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (9, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:[[ previous section")),
        "matches Vim: N sections backwards.",
    ),
    na(
        "[]",
        "[]",
        Implemented,
        Some(nlive(
            NA_CODE,
            (10, 1),
            24,
            "[]",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (5, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:[] previous SECTION end")),
        "matches Vim: N SECTIONS backwards.",
    ),
    na(
        "[c",
        "[c",
        Partial,
        Some(nlive(
            NA_CODE,
            (10, 1),
            24,
            "[c",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (10, 1),
            "NORMAL",
            "No more hunks",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("git:[c previous hunk")),
        concat!(
            "Vim moves to the previous change **in diff mode**. vimcode binds ",
            "it to git-hunk navigation instead (\"No more hunks\"), which is ",
            "the same gesture over a different source of truth — useful, but ",
            "it is not `:help [c`.",
        ),
    ),
    na(
        "[d",
        "[d",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "Vim shows the first matching #define from the included files; no ",
            "include search in vimcode.",
        ),
    ),
    na(
        "[f",
        "[f",
        NotImplemented,
        Some(nlive(
            NA_FILE,
            (1, 1),
            24,
            "[f",
            "README.md|second line",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("file:[f opens the file under the cursor")),
        concat!(
            "Vim's `[f` is `gf` — edit the file whose name is under the ",
            "cursor. vimcode does nothing (the buffer and cursor are ",
            "untouched with `README.md` under the cursor).",
        ),
    ),
    na(
        "[i",
        "[i",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "Vim shows the first matching line from the included files; no ",
            "include search in vimcode.",
        ),
    ),
    na(
        "[m",
        "[m",
        Implemented,
        Some(nlive(
            NA_CODE,
            (10, 5),
            24,
            "[m",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (9, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:[m previous member start")),
        "matches Vim: back to the start of the previous member function.",
    ),
    na(
        "[p",
        "[p",
        Partial,
        Some(nlive(
            NA_IND,
            (1, 1),
            24,
            "yyj[p",
            "    alpha beta|    alpha beta|    gamma delta|    eps zeta",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:[p puts with indent")),
        concat!(
            "The pasted buffer matches Vim exactly — the indent really is ",
            "adjusted to the current line — but the cursor lands in column 1 ",
            "where Vim leaves it on the first non-blank (column 5 in the ",
            "recording). Same one-column class as `<{motion}`/`]p`.",
        ),
    ),
    na(
        "[s",
        "[s",
        Implemented,
        Some(nlive(
            NA_SPELL,
            (1, 14),
            24,
            "[s",
            "teh quick brwn fox",
            (1, 14),
            "NORMAL",
            "Spell checking is off (use :set spell)",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("spell:[s previous misspelling")),
        concat!(
            "implemented in `src/core/spell.rs` (#1163) and gated on 'spell' ",
            "exactly as Vim is: with spelling off the recording answers ",
            "\"Spell checking is off (use :set spell)\".",
        ),
    ),
    na(
        "[z",
        "[z",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zf5jzo3G[z",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:[z start of open fold")),
        "matches Vim: to the start of the current open fold.",
    ),
    na(
        "[{",
        "[{",
        Implemented,
        Some(nlive(
            NA_CODE,
            (4, 9),
            24,
            "[{",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (3, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:[{")),
        "matches Vim: back to the unmatched '{'.",
    ),
    na(
        "[<MiddleMouse>",
        "[<MiddleMouse>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim does \"[p\" at the click position; vimcode's middle click does ",
            "not paste at all. No key-replay possible (mouse event).",
        ),
    ),
    na(
        "] CTRL-D",
        "]_CTRL-D",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "As `[ CTRL-D`, searching from the cursor instead of the start of ",
            "the file.",
        ),
    ),
    na(
        "] CTRL-I",
        "]_CTRL-I",
        Skipped(CTAGS),
        None,
        None,
        concat!(
            "As `[ CTRL-I`, searching from the cursor instead of the start of ",
            "the file.",
        ),
    ),
    na(
        "]#",
        "]#",
        Implemented,
        Some(nlive(
            NA_CODE,
            (2, 1),
            24,
            "]#",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (6, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:]# next unmatched endif")),
        "matches Vim: forward to the next unmatched #endif/#else.",
    ),
    na(
        "]'",
        "]'",
        NotImplemented,
        Some(nlive(
            NA_CODE,
            (4, 1),
            24,
            "maggj]'",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:]' next mark line")),
        concat!(
            "Vim goes to the next lowercase mark, on the first non-blank; ",
            "vimcode does not move (see `['`).",
        ),
    ),
    na(
        "])",
        "])",
        Implemented,
        Some(nlive(
            NA_BR,
            (1, 8),
            24,
            "])",
            "foo(bar baz) qux|arr[one two] end|map{k v} tail",
            (1, 12),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:])")),
        "matches Vim: forward to the unmatched ')'.",
    ),
    na(
        "]*",
        "]star",
        Implemented,
        Some(nlive(
            NA_CODE,
            (8, 1),
            24,
            "]*",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (8, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:]* next comment end")),
        "matches Vim: forward to the next end of a C comment.",
    ),
    na(
        "]`",
        "]`",
        NotImplemented,
        Some(nlive(
            NA_CODE,
            (4, 3),
            24,
            "maggj]`",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (2, 3),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("mark:]` next mark")),
        "Same as `` [` ``: no movement.",
    ),
    na(
        "]/",
        "]/",
        Partial,
        Some(nlive(
            NA_CODE,
            (7, 1),
            24,
            "]/",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (7, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:]/ next comment end")),
        concat!(
            "vimcode finds the right *line* (the `/* note */` line) but stops ",
            "in column 1; Vim lands on the `*/` itself (column 10 in the ",
            "recording).",
        ),
    ),
    na(
        "]D",
        "]D",
        Skipped(CTAGS),
        None,
        None,
        "As `[D`, searching from the cursor.",
    ),
    na(
        "]I",
        "]I",
        Skipped(CTAGS),
        None,
        None,
        "As `[I`, searching from the cursor.",
    ),
    na(
        "]P",
        "]P",
        NotImplemented,
        Some(nlive(
            NA_IND,
            (1, 1),
            24,
            "yyj]P",
            "    alpha beta|    gamma delta|    eps zeta",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:]P puts with indent")),
        "Vim's `]P` is `[p`; vimcode does nothing (see `[P`).",
    ),
    na(
        "][",
        "][",
        Implemented,
        Some(nlive(
            NA_CODE,
            (2, 1),
            24,
            "][",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (5, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:][ next SECTION end")),
        "matches Vim: N SECTIONS forwards.",
    ),
    na(
        "]]",
        "]]",
        Implemented,
        Some(nlive(
            NA_CODE,
            (2, 1),
            24,
            "]]",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (3, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:]] next section")),
        "matches Vim: N sections forwards.",
    ),
    na(
        "]c",
        "]c",
        Partial,
        Some(nlive(
            NA_CODE,
            (2, 1),
            24,
            "]c",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (2, 1),
            "NORMAL",
            "No more hunks",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("git:]c next hunk")),
        concat!(
            "Same as `[c`: bound to git-hunk navigation rather than diff-mode ",
            "changes.",
        ),
    ),
    na(
        "]d",
        "]d",
        Skipped(CTAGS),
        None,
        None,
        "As `[d`, searching from the cursor.",
    ),
    na(
        "]f",
        "]f",
        NotImplemented,
        Some(nlive(
            NA_FILE,
            (1, 1),
            24,
            "]f",
            "README.md|second line",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("file:]f opens the file under the cursor")),
        "Same as `[f`: vimcode does nothing.",
    ),
    na(
        "]i",
        "]i",
        Skipped(CTAGS),
        None,
        None,
        "As `[i`, searching from the cursor.",
    ),
    na(
        "]m",
        "]m",
        Implemented,
        Some(nlive(
            NA_CODE,
            (2, 1),
            24,
            "]m",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (3, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("bracket:]m next member end")),
        "matches Vim: forward to the end of the next member function.",
    ),
    na(
        "]p",
        "]p",
        Partial,
        Some(nlive(
            NA_IND,
            (1, 1),
            24,
            "yyj]p",
            "    alpha beta|    gamma delta|    alpha beta|    eps zeta",
            (3, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:]p puts with indent")),
        concat!(
            "The pasted buffer matches Vim exactly; the cursor lands in ",
            "column 1 instead of on the first non-blank (see `[p`).",
        ),
    ),
    na(
        "]s",
        "]s",
        Implemented,
        Some(nlive(
            NA_SPELL,
            (1, 1),
            24,
            "]s",
            "teh quick brwn fox",
            (1, 1),
            "NORMAL",
            "Spell checking is off (use :set spell)",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("spell:]s next misspelling")),
        concat!(
            "implemented in `src/core/spell.rs` (#1163) and gated on 'spell' ",
            "like Vim.",
        ),
    ),
    na(
        "]z",
        "]z",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zf5jzo3G]z",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (6, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:]z end of open fold")),
        "matches Vim: to the end of the current open fold.",
    ),
    na(
        "]}",
        "]}",
        Implemented,
        Some(nlive(
            NA_CODE,
            (4, 9),
            24,
            "]}",
            "#if FOO|int one(void)|{|    return 1;|}|#else|/* note */|int two(void)|{|    return 2;|}|#endif",
            (5, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("word:]}")),
        "matches Vim: forward to the unmatched '}'.",
    ),
    na(
        "]<MiddleMouse>",
        "]<MiddleMouse>",
        NotImplemented,
        None,
        None,
        concat!(
            "Vim does \"]p\" at the click position; vimcode's middle click does ",
            "not paste. No key-replay possible (mouse event).",
        ),
    ),
    na(
        "z<CR>",
        "z<CR>",
        Implemented,
        Some(nlive(
            NA_LONG,
            (20, 3),
            10,
            "z<CR>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (20, 1),
            "NORMAL",
            "",
            20,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:z<CR> col first nonblank")),
        concat!(
            "matches Vim: redraws with the cursor line at the top of the ",
            "window, cursor on the first non-blank.",
        ),
    ),
    na(
        "z{height}<CR>",
        "zN<CR>",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (20, 1),
            10,
            "5z<CR>",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (20, 1),
            "NORMAL",
            "",
            20,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("win:z{height}<CR> resizes the window")),
        concat!(
            "Vim's `z{height}<CR>` makes the window {height} lines high. ",
            "vimcode ignores the count and behaves like a plain `z<CR>`. Low ",
            "value — `CTRL-W _` and `:resize` cover the same ground, and ",
            "neither exists as a *count-carrying* form here.",
        ),
    ),
    na(
        "z+",
        "z+",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (5, 1),
            10,
            "z+",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (5, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:z+ next screenful")),
        concat!(
            "Vim puts line N (default: the line below the window) at the top ",
            "of the window. vimcode does nothing at all — the recording's ",
            "cursor and top are both unchanged.",
        ),
    ),
    na(
        "z-",
        "z-",
        Implemented,
        Some(nlive(
            NA_LONG,
            (15, 3),
            10,
            "z-",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (15, 1),
            "NORMAL",
            "",
            6,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:z- bottom first nonblank")),
        concat!(
            "matches Vim: cursor line to the bottom of the window, cursor on ",
            "the first non-blank.",
        ),
    ),
    na(
        "z.",
        "z.",
        Implemented,
        Some(nlive(
            NA_LONG,
            (15, 3),
            10,
            "z.",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (15, 1),
            "NORMAL",
            "",
            11,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:z. centre first nonblank")),
        "matches Vim: cursor line centred, cursor on the first non-blank.",
    ),
    na(
        "z=",
        "z=",
        Implemented,
        Some(nlive(
            NA_SPELL,
            (1, 1),
            24,
            "z=",
            "teh quick brwn fox",
            (1, 1),
            "NORMAL",
            "Spell checking is off (use :set spell)",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("spell:z= suggestions")),
        concat!(
            "implemented in `src/core/spell.rs` (#1163); gated on 'spell' ",
            "exactly as Vim is.",
        ),
    ),
    na(
        "zA",
        "zA",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzA",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zA toggles recursively")),
        concat!(
            "matches Vim: opens the closed fold recursively (the recorded ",
            "fold is gone from the rendered line set).",
        ),
    ),
    na(
        "zC",
        "zC",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzC",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "1,3-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zC closes recursively")),
        concat!(
            "matches Vim: closes the fold recursively (line 2 is no longer ",
            "rendered).",
        ),
    ),
    na(
        "zD",
        "zD",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzD",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zD deletes recursively")),
        concat!(
            "matches Vim: deletes the fold recursively; every line is ",
            "rendered again.",
        ),
    ),
    na(
        "zE",
        "zE",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzE",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "1,3-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zE eliminates all folds")),
        concat!(
            "Vim eliminates *all* folds. vimcode leaves the fold standing — ",
            "line 2 is still unrendered after `zE`. Worth implementing: `zD` ",
            "already does the hard part for one fold.",
        ),
    ),
    na(
        "zF",
        "zF",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "3zF",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "3 lines folded",
            1,
            1,
            "1,5-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zF folds N lines")),
        "matches Vim: creates a fold for N lines (\"3 lines folded\").",
    ),
    na(
        "zG",
        "zG",
        Implemented,
        Some(nlive(
            NA_SPELL,
            (1, 1),
            24,
            "zG",
            "teh quick brwn fox",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("spell:zG marks good temporarily")),
        concat!(
            "implemented in `src/core/spell.rs` (#1163); the recording is ",
            "silent because the word list is not rendered.",
        ),
    ),
    na(
        "zH",
        "zH",
        Implemented,
        Some(nlive(
            NA_WIDE,
            (1, 150),
            10,
            "zH",
            "001 002 003 004 005 006 007 008 009 010 011 012 013 014 015 016 017 018 019 020 021 022 023 024 025 026 027 028 029 030 031 032 033 034 035 036 037 038 039 040 041 042 043 044 045 046 047 048 049 050",
            (1, 110),
            "NORMAL",
            "",
            1,
            31,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:zH half screen right")),
        concat!(
            "matches Vim: scrolls half a screen-width; the recorded first ",
            "rendered column moves from 71 to 31 and the cursor follows.",
        ),
    ),
    na(
        "zL",
        "zL",
        Implemented,
        Some(nlive(
            NA_WIDE,
            (1, 150),
            10,
            "zL",
            "001 002 003 004 005 006 007 008 009 010 011 012 013 014 015 016 017 018 019 020 021 022 023 024 025 026 027 028 029 030 031 032 033 034 035 036 037 038 039 040 041 042 043 044 045 046 047 048 049 050",
            (1, 150),
            "NORMAL",
            "",
            1,
            111,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:zL half screen left")),
        concat!(
            "matches Vim: scrolls half a screen-width the other way (first ",
            "rendered column 71 to 111).",
        ),
    ),
    na(
        "zM",
        "zM",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzozM",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "1,3-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zM closes all")),
        concat!(
            "matches Vim: sets 'foldlevel' to zero, re-closing the fold the ",
            "recording had opened.",
        ),
    ),
    na(
        "zN",
        "zN",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzozN",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zN sets foldenable")),
        concat!(
            "Vim sets 'foldenable', so the open fold re-closes. vimcode ",
            "leaves it open — the whole 'foldenable' switch (`zn`/`zN`/`zi`) ",
            "is missing.",
        ),
    ),
    na(
        "zO",
        "zO",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzczO",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zO opens recursively")),
        "matches Vim: opens the fold recursively.",
    ),
    na(
        "zR",
        "zR",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzczR",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zR opens all")),
        concat!(
            "matches Vim: sets 'foldlevel' to the deepest fold, opening ",
            "everything.",
        ),
    ),
    na(
        "zW",
        "zW",
        Implemented,
        Some(nlive(
            NA_SPELL,
            (1, 1),
            24,
            "zW",
            "teh quick brwn fox",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("spell:zW marks bad temporarily")),
        concat!(
            "implemented in `src/core/spell.rs` (#1163); silent for the same ",
            "reason as `zG`.",
        ),
    ),
    na(
        "zX",
        "zX",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzozX",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zX reapplies foldlevel")),
        concat!(
            "Vim re-applies 'foldlevel', re-closing folds the user opened. ",
            "vimcode leaves them open. (Its sibling `zx` *does* re-close — so ",
            "the two halves of the same pair disagree.)",
        ),
    ),
    na(
        "z^",
        "z^",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (15, 1),
            10,
            "z^",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (15, 1),
            "NORMAL",
            "",
            6,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:z^ previous screenful")),
        concat!(
            "Vim puts line N (default: the line above the window) at the ",
            "bottom of the window. vimcode's recording is identical to `z-`, ",
            "i.e. it treats `z^` as \"current line to the bottom\" and never ",
            "pages.",
        ),
    ),
    na(
        "za",
        "za",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjza",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:za toggles")),
        "matches Vim: opens a closed fold, closes an open one.",
    ),
    na(
        "zb",
        "zb",
        Implemented,
        Some(nlive(
            NA_LONG,
            (20, 3),
            10,
            "zb",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (20, 3),
            "NORMAL",
            "",
            11,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:zbH")),
        concat!(
            "matches Vim: redraws with the cursor line at the bottom of the ",
            "window.",
        ),
    ),
    na(
        "zc",
        "zc",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzozc",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "1,3-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zc closes")),
        "matches Vim: closes the fold under the cursor.",
    ),
    na(
        "zd",
        "zd",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzd",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zd deletes")),
        "matches Vim: deletes the fold under the cursor.",
    ),
    na(
        "ze",
        "ze",
        Implemented,
        Some(nlive(
            NA_WIDE,
            (1, 150),
            10,
            "ze",
            "001 002 003 004 005 006 007 008 009 010 011 012 013 014 015 016 017 018 019 020 021 022 023 024 025 026 027 028 029 030 031 032 033 034 035 036 037 038 039 040 041 042 043 044 045 046 047 048 049 050",
            (1, 150),
            "NORMAL",
            "",
            1,
            71,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:ze cursor at right edge")),
        concat!(
            "matches Vim: scrolls horizontally so the cursor sits at the ",
            "right edge (first rendered column 71 for a cursor in column 150 ",
            "of an 80-column window).",
        ),
    ),
    na(
        "zf{motion}",
        "zf",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfj",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "1 lines folded",
            1,
            1,
            "1,3-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zfj hides one line")),
        concat!(
            "matches Vim: creates a fold over the Nmove text (\"1 lines ",
            "folded\").",
        ),
    ),
    na(
        "zg",
        "zg",
        Implemented,
        Some(nlive(
            NA_SPELL,
            (1, 1),
            24,
            "zg",
            "teh quick brwn fox",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("spell:zg marks good")),
        "implemented in `src/core/spell.rs` (#1163).",
    ),
    na(
        "zh",
        "zh",
        Implemented,
        Some(nlive(
            NA_WIDE,
            (1, 150),
            10,
            "zh",
            "001 002 003 004 005 006 007 008 009 010 011 012 013 014 015 016 017 018 019 020 021 022 023 024 025 026 027 028 029 030 031 032 033 034 035 036 037 038 039 040 041 042 043 044 045 046 047 048 049 050",
            (1, 149),
            "NORMAL",
            "",
            1,
            70,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:zh one column right")),
        concat!(
            "matches Vim: scrolls the screen one character right and pushes ",
            "the cursor with it.",
        ),
    ),
    na(
        "zi",
        "zi",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzczi",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "1,3-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zi toggles foldenable")),
        concat!(
            "Vim toggles 'foldenable', so a closed fold opens. vimcode leaves ",
            "it closed (see `zN`).",
        ),
    ),
    na(
        "zj",
        "zj",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "3Gzfjggzj",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (3, 1),
            "NORMAL",
            "",
            1,
            1,
            "1-3,5-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zj next fold start")),
        "matches Vim: moves to the start of the next fold.",
    ),
    na(
        "zk",
        "zk",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjGzk",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (2, 1),
            "NORMAL",
            "",
            2,
            1,
            "1,3-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zk previous fold end")),
        "matches Vim: moves to the end of the previous fold.",
    ),
    na(
        "zl",
        "zl",
        Implemented,
        Some(nlive(
            NA_WIDE,
            (1, 150),
            10,
            "zl",
            "001 002 003 004 005 006 007 008 009 010 011 012 013 014 015 016 017 018 019 020 021 022 023 024 025 026 027 028 029 030 031 032 033 034 035 036 037 038 039 040 041 042 043 044 045 046 047 048 049 050",
            (1, 150),
            "NORMAL",
            "",
            1,
            72,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:zl one column left")),
        "matches Vim: scrolls the screen one character left.",
    ),
    na(
        "zm",
        "zm",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzozm",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zm lowers foldlevel")),
        concat!(
            "Vim subtracts one from 'foldlevel', closing the fold the ",
            "recording had opened; vimcode leaves it open. (`zM` — \"all the ",
            "way to zero\" — does work, so only the incremental form is ",
            "missing.)",
        ),
    ),
    na(
        "zn",
        "zn",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzn",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "1,3-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zn resets foldenable")),
        concat!(
            "Vim resets 'foldenable', so every fold shows its lines again; ",
            "vimcode's fold stays closed.",
        ),
    ),
    na(
        "zo",
        "zo",
        Implemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzo",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("fold:indent:zo opens the level-1 fold, inner stays closed")),
        "matches Vim: opens the fold under the cursor.",
    ),
    na(
        "zp",
        "zp",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-v>jlyjzp",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:zp pastes a block without trailing spaces")),
        concat!(
            "Vim pastes blockwise without the trailing whitespace. vimcode ",
            "does nothing at all — the recording's buffer is byte-identical ",
            "to the fixture while Vim's is not. Worth implementing with ",
            "`zP`/`zy`, which have the same gap.",
        ),
    ),
    na(
        "zP",
        "zP",
        NotImplemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-v>jlyjzP",
            "alpha beta gamma|delta epsilon zeta|eta theta iota",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:zP pastes a block without trailing spaces")),
        "Same as `zp`: no paste at all.",
    ),
    na(
        "zr",
        "zr",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzczr",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "1,3-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zr raises foldlevel")),
        concat!(
            "Vim adds one to 'foldlevel', opening a level of folds; vimcode's ",
            "fold stays closed (see `zm`).",
        ),
    ),
    na(
        "zs",
        "zs",
        Implemented,
        Some(nlive(
            NA_WIDE,
            (1, 150),
            10,
            "zs",
            "001 002 003 004 005 006 007 008 009 010 011 012 013 014 015 016 017 018 019 020 021 022 023 024 025 026 027 028 029 030 031 032 033 034 035 036 037 038 039 040 041 042 043 044 045 046 047 048 049 050",
            (1, 150),
            "NORMAL",
            "",
            1,
            150,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:zs cursor at left edge")),
        concat!(
            "matches Vim: scrolls horizontally so the cursor sits at the left ",
            "edge (first rendered column becomes the cursor's column).",
        ),
    ),
    na(
        "zt",
        "zt",
        Implemented,
        Some(nlive(
            NA_LONG,
            (15, 3),
            10,
            "zt",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (15, 3),
            "NORMAL",
            "",
            15,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:zt col kept")),
        concat!(
            "matches Vim: redraws with the cursor line at the top of the ",
            "window.",
        ),
    ),
    na(
        "zuw",
        "zuw",
        NotImplemented,
        Some(nlive(
            NA_SPELL,
            (1, 1),
            24,
            "zwzuw",
            "teh quick brwn fox",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("spell:zuw undoes zw")),
        concat!(
            "Vim undoes a `zw`. vimcode does not recognise `zu` at all: the ",
            "recording's cursor ends a word to the right, i.e. the trailing ",
            "`w` ran as a motion. Worth implementing — `zg`/`zw`/`zG`/`zW` ",
            "all exist, so only their undo half is missing, and the current ",
            "behaviour silently *moves the cursor* instead.",
        ),
    ),
    na(
        "zug",
        "zug",
        NotImplemented,
        Some(nlive(
            NA_SPELL,
            (1, 1),
            24,
            "zgzug",
            "teh quick brwn fox",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("spell:zug undoes zg")),
        concat!(
            "Same as `zuw`; `g` is swallowed rather than executed, so this ",
            "one is at least a silent no-op.",
        ),
    ),
    na(
        "zuW",
        "zuW",
        NotImplemented,
        Some(nlive(
            NA_SPELL,
            (1, 1),
            24,
            "zWzuW",
            "teh quick brwn fox",
            (1, 5),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("spell:zuW undoes zW")),
        "Same as `zuw`, including the stray `W` motion.",
    ),
    na(
        "zuG",
        "zuG",
        NotImplemented,
        Some(nlive(
            NA_SPELL,
            (1, 1),
            24,
            "zGzuG",
            "teh quick brwn fox",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("spell:zuG undoes zG")),
        "Same as `zug`.",
    ),
    na(
        "zv",
        "zv",
        NotImplemented,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "jzfjzcggjzv",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (2, 1),
            "NORMAL",
            "",
            1,
            1,
            "1-2,4-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zv opens enough to view the cursor")),
        concat!(
            "Vim opens exactly enough folds to make the cursor line visible. ",
            "vimcode leaves the fold closed with the cursor on its header ",
            "line.",
        ),
    ),
    na(
        "zw",
        "zw",
        Implemented,
        Some(nlive(
            NA_SPELL,
            (1, 1),
            24,
            "zw",
            "teh quick brwn fox",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("spell:zw marks bad")),
        "implemented in `src/core/spell.rs` (#1163).",
    ),
    na(
        "zx",
        "zx",
        Partial,
        Some(nlive(
            NA_LONG,
            (1, 1),
            10,
            "zfjzozx",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "1,3-30",
            "1* tabs=1/1",
        )),
        Some(Label("fold:zx reapplies foldlevel and opens to the cursor")),
        concat!(
            "The 'foldlevel' half works — the recording re-closes a fold the ",
            "user had opened — but the `zv` half cannot, because `zv` itself ",
            "is unimplemented. Its sibling `zX` does not even do the first ",
            "half.",
        ),
    ),
    na(
        "zy",
        "zy",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "<C-v>jlzyjp",
            "alpha beta gamma|dalelta epsilon zeta|edeta theta iota",
            (2, 2),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:zy yanks a block without trailing spaces")),
        concat!(
            "matches Vim: yanks the block and the recorded paste puts it back ",
            "without trailing whitespace. (The one member of the ",
            "`zp`/`zP`/`zy` trio that works.)",
        ),
    ),
    na(
        "zz",
        "zz",
        Implemented,
        Some(nlive(
            NA_LONG,
            (15, 3),
            10,
            "zz",
            "line 01|line 02|line 03|line 04|line 05|line 06|line 07|line 08|line 09|line 10|line 11|line 12|line 13|line 14|line 15|line 16|line 17|line 18|line 19|line 20|line 21|line 22|line 23|line 24|line 25|line 26|line 27|line 28|line 29|line 30",
            (15, 3),
            "NORMAL",
            "",
            11,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:zzH")),
        "matches Vim: redraws with the cursor line centred.",
    ),
    na(
        "z<Left>",
        "z<Left>",
        NotImplemented,
        Some(nlive(
            NA_WIDE,
            (1, 150),
            10,
            "z<Left>",
            "001 002 003 004 005 006 007 008 009 010 011 012 013 014 015 016 017 018 019 020 021 022 023 024 025 026 027 028 029 030 031 032 033 034 035 036 037 038 039 040 041 042 043 044 045 046 047 048 049 050",
            (1, 150),
            "NORMAL",
            "",
            1,
            71,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:z<Left> is zh")),
        concat!(
            "Vim's `z<Left>` is `zh`. vimcode does nothing — the recorded ",
            "first rendered column and cursor are both unchanged, while `zh` ",
            "itself works.",
        ),
    ),
    na(
        "z<Right>",
        "z<Right>",
        NotImplemented,
        Some(nlive(
            NA_WIDE,
            (1, 150),
            10,
            "z<Right>",
            "001 002 003 004 005 006 007 008 009 010 011 012 013 014 015 016 017 018 019 020 021 022 023 024 025 026 027 028 029 030 031 032 033 034 035 036 037 038 039 040 041 042 043 044 045 046 047 048 049 050",
            (1, 150),
            "NORMAL",
            "",
            1,
            71,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("scroll:z<Right> is zl")),
        "Vim's `z<Right>` is `zl`; vimcode does nothing (see `z<Left>`).",
    ),
    na(
        "v (operator-pending)",
        "o_v",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "dvj",
            "delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:o_v forces charwise")),
        concat!(
            "matches Vim: forces the pending operator to work charwise (`dvj` ",
            "deletes to the start of the next line rather than both lines).",
        ),
    ),
    na(
        "V (operator-pending)",
        "o_V",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "dVl",
            "delta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:o_V forces linewise")),
        concat!(
            "matches Vim: forces the pending operator to work linewise (`dVl` ",
            "deletes the whole line).",
        ),
    ),
    na(
        "CTRL-V (operator-pending)",
        "o_CTRL-V",
        Implemented,
        Some(nlive(
            NA_TXT,
            (1, 1),
            24,
            "d<C-v>j",
            "lpha beta gamma|elta epsilon zeta|eta theta iota",
            (1, 1),
            "NORMAL",
            "",
            1,
            1,
            "",
            "1* tabs=1/1",
        )),
        Some(Label("op:o_C-v forces blockwise")),
        concat!(
            "matches Vim: forces the pending operator to work blockwise ",
            "(`d<C-v>j` deletes one column from two lines).",
        ),
    ),
];

const NORM_NO_REPLAY: &[(&str, &str)] = &[
    ("<S-NL>", "`Engine::handle_key` takes no Shift modifier, so neither backend can deliver a shifted key distinctly"),
    ("<S-CR>", "`Engine::handle_key` takes no Shift modifier, so neither backend can deliver a shifted key distinctly"),
    ("<S-+>", "`Engine::handle_key` takes no Shift modifier, so neither backend can deliver a shifted key distinctly"),
    ("<S-->", "`Engine::handle_key` takes no Shift modifier, so neither backend can deliver a shifted key distinctly"),
    ("<C-LeftMouse>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<C-RightMouse>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<LeftMouse>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<MiddleMouse>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<RightMouse>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<S-Down>", "`Engine::handle_key` takes no Shift modifier, so neither backend can deliver a shifted key distinctly"),
    ("<S-Left>", "`Engine::handle_key` takes no Shift modifier, so neither backend can deliver a shifted key distinctly"),
    ("<S-LeftMouse>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<S-Right>", "`Engine::handle_key` takes no Shift modifier, so neither backend can deliver a shifted key distinctly"),
    ("<S-RightMouse>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<S-Up>", "`Engine::handle_key` takes no Shift modifier, so neither backend can deliver a shifted key distinctly"),
    ("<ScrollWheelDown>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<S-ScrollWheelDown>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<ScrollWheelUp>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<S-ScrollWheelUp>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<ScrollWheelLeft>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<S-ScrollWheelLeft>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<ScrollWheelRight>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("<S-ScrollWheelRight>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("[<MiddleMouse>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
    ("]<MiddleMouse>", "mouse and wheel events reach the backends as pointer events, never as a key name `Engine::handle_key` could be given"),
];

const NORM_COVERAGE_EXEMPT: &[&str] = &[
    "CTRL-C",
    "CTRL-G",
    "<BS>",
    "CTRL-H",
    "<Tab>",
    "CTRL-I",
    "<NL>",
    "CTRL-J",
    "CTRL-M",
    "CTRL-N",
    "CTRL-P",
    "CTRL-R",
    "CTRL-V",
    "CTRL-Y",
    "CTRL-Z",
    "CTRL-\\ CTRL-N",
    "CTRL-\\ CTRL-G",
    "CTRL-^",
    "CTRL-<Tab>",
    "<Space>",
    "{count}%",
    "&",
    "'{a-zA-Z0-9}",
    "''",
    "'(",
    "')",
    "'<",
    "'>",
    "'[",
    "']",
    "'{",
    "'}",
    ".",
    "/{pattern}<CR>",
    "/<CR>",
    "1 - 9",
    ":",
    "{count}:",
    "<{motion}",
    "={motion}",
    ">{motion}",
    "?{pattern}<CR>",
    "?<CR>",
    "@:",
    "@@",
    "K",
    "N",
    "U",
    "ZZ",
    "ZQ",
    "`(",
    "`)",
    "`<",
    "`>",
    "`[",
    "`]",
    "``",
    "`{",
    "`}",
    "do",
    "dp",
    "l",
    "n",
    "q (while recording)",
    "Q",
    "q:",
    "q/",
    "q?",
    "v",
    "~{motion} ('tildeop')",
    "<C-End>",
    "<C-Home>",
    "<C-Left>",
    "<C-Right>",
    "<C-Tab>",
    "[\"x]<Del>",
    "{count}<Del>",
    "<Down>",
    "<End>",
    "<F1>",
    "<Help>",
    "<Home>",
    "<Insert>",
    "<Left>",
    "<PageDown>",
    "<PageUp>",
    "<Right>",
    "<Undo>",
    "<Up>",
    "CTRL-W CTRL-B",
    "CTRL-W CTRL-C",
    "CTRL-W CTRL-D",
    "CTRL-W CTRL-F",
    "CTRL-W CTRL-H",
    "CTRL-W CTRL-I",
    "CTRL-W CTRL-J",
    "CTRL-W CTRL-K",
    "CTRL-W CTRL-L",
    "CTRL-W CTRL-N",
    "CTRL-W CTRL-O",
    "CTRL-W CTRL-P",
    "CTRL-W CTRL-Q",
    "CTRL-W CTRL-R",
    "CTRL-W CTRL-S",
    "CTRL-W CTRL-T",
    "CTRL-W CTRL-V",
    "CTRL-W CTRL-W",
    "CTRL-W CTRL-X",
    "CTRL-W CTRL-Z",
    "CTRL-W CTRL-^",
    "CTRL-W CTRL-_",
    "CTRL-W +",
    "CTRL-W -",
    "CTRL-W <",
    "CTRL-W =",
    "CTRL-W >",
    "CTRL-W H",
    "CTRL-W J",
    "CTRL-W K",
    "CTRL-W L",
    "CTRL-W R",
    "CTRL-W S",
    "CTRL-W T",
    "CTRL-W W",
    "CTRL-W ^",
    "CTRL-W _",
    "CTRL-W b",
    "CTRL-W c",
    "CTRL-W d",
    "CTRL-W f",
    "CTRL-W F",
    "CTRL-W g f",
    "CTRL-W g F",
    "CTRL-W g t",
    "CTRL-W g T",
    "CTRL-W g <Tab>",
    "CTRL-W h",
    "CTRL-W i",
    "CTRL-W j",
    "CTRL-W k",
    "CTRL-W l",
    "CTRL-W n",
    "CTRL-W o",
    "CTRL-W p",
    "CTRL-W q",
    "CTRL-W r",
    "CTRL-W s",
    "CTRL-W t",
    "CTRL-W v",
    "CTRL-W w",
    "CTRL-W x",
    "CTRL-W |",
    "CTRL-W <Down>",
    "CTRL-W <Up>",
    "CTRL-W <Left>",
    "CTRL-W <Right>",
    "[#",
    "['",
    "[*",
    "[`",
    "[/",
    "[P",
    "[[",
    "[]",
    "[c",
    "[f",
    "[m",
    "[p",
    "[s",
    "[z",
    "]#",
    "]'",
    "]*",
    "]`",
    "]/",
    "]P",
    "][",
    "]]",
    "]c",
    "]f",
    "]m",
    "]p",
    "]s",
    "]z",
    "z{height}<CR>",
    "z+",
    "z-",
    "z.",
    "z=",
    "zA",
    "zC",
    "zD",
    "zE",
    "zF",
    "zG",
    "zH",
    "zL",
    "zM",
    "zN",
    "zW",
    "zX",
    "z^",
    "za",
    "zc",
    "ze",
    "zg",
    "zh",
    "zi",
    "zj",
    "zk",
    "zl",
    "zm",
    "zn",
    "zp",
    "zP",
    "zr",
    "zs",
    "zuw",
    "zug",
    "zuW",
    "zuG",
    "zv",
    "zw",
    "zx",
    "zy",
    "z<Left>",
    "z<Right>",
    "v (operator-pending)",
    "V (operator-pending)",
    "CTRL-V (operator-pending)",
];

/// Send one key sequence to the engine, the way a backend would.
///
/// Deliberately **not** [`send_keys`]: that tokenizer maps every `<C-…>` name
/// to `Ctrl(<third char>)`, so `<C-Left>` would arrive as `Ctrl('L')` and
/// three index rows (`<C-Left>`, `<C-Right>`, `<C-Home>`/`<C-End>`) would be
/// silently mis-measured. Here a multi-character `<C-Name>` is delivered the
/// way GTK and the TUI deliver it: the key *name* with `ctrl = true`.
fn send_norm_keys(engine: &mut Engine, keys: &str) {
    let mut chars = keys.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let rest: String = chars.clone().collect();
            let starts_special = chars
                .peek()
                .map(|&c| c.is_ascii_uppercase() || c == 'C')
                .unwrap_or(false);
            if rest.contains('>') && starts_special {
                let name: String = chars.by_ref().take_while(|&c| c != '>').collect();
                if let Some(sub) = name.strip_prefix("C-") {
                    if sub.chars().count() == 1 {
                        let c = sub.chars().next().unwrap_or('?');
                        press_ctrl(engine, c);
                    } else {
                        engine.handle_key(sub, None, true);
                        pump(engine);
                    }
                } else {
                    let mapped = match name.as_str() {
                        "Esc" => "Escape",
                        "CR" | "Enter" => "Return",
                        "BS" => "BackSpace",
                        "Del" => "Delete",
                        "PageDown" => "Page_Down",
                        "PageUp" => "Page_Up",
                        other => other,
                    };
                    if mapped == "Space" {
                        press_char(engine, ' ');
                    } else {
                        press_special(engine, mapped);
                    }
                }
                continue;
            }
            press_char(engine, '<');
        } else {
            press_char(engine, ch);
        }
    }
}

/// The rendered line ranges, 1-indexed — `"1,3-30"` when a fold hides line 2.
/// `""` means "every line is rendered", which is the common case and keeps
/// the table readable.
fn norm_folded(engine: &Engine) -> String {
    let total = engine.buffer().len_lines();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for i in 0..total {
        if engine.view().is_line_hidden(i) {
            continue;
        }
        match ranges.last_mut() {
            Some(r) if r.1 == i => r.1 = i + 1,
            _ => ranges.push((i + 1, i + 1)),
        }
    }
    if ranges.len() == 1 && ranges[0] == (1, total) {
        return String::new();
    }
    ranges
        .iter()
        .map(|(a, b)| {
            if a == b {
                format!("{a}")
            } else {
                format!("{a}-{b}")
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The window tree and tab counter, e.g. `"(h0.50 2* 1) tabs=1/2"`.
///
/// Leaves are numbered by **ascending window id**, not by position, so a
/// rotation (`CTRL-W r`) or an exchange (`CTRL-W x`) changes the string —
/// that is the only way those two rows can be observed at all. Numbering by
/// position would print the same signature before and after.
fn norm_windows(engine: &Engine) -> String {
    use vimcode_core::core::window::{WindowId, WindowLayout};

    fn walk(l: &WindowLayout, active: WindowId, ids: &[usize], out: &mut String) {
        let idx = |w: WindowId| ids.iter().position(|x| *x == w.0).unwrap_or(98) + 1;
        match l {
            WindowLayout::Leaf(w) => {
                out.push_str(&format!(
                    "{}{}",
                    idx(*w),
                    if *w == active { "*" } else { "" }
                ));
            }
            WindowLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                out.push('(');
                out.push_str(&format!("{direction:?}").to_lowercase()[..1]);
                out.push_str(&format!("{ratio:.2} "));
                walk(first, active, ids, out);
                out.push(' ');
                walk(second, active, ids, out);
                out.push(')');
            }
        }
    }

    let tab = engine.active_tab();
    let mut ids: Vec<usize> = tab.layout.window_ids().iter().map(|w| w.0).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut s = String::new();
    walk(&tab.layout, tab.active_window, &ids, &mut s);
    format!(
        "{s} tabs={}/{}",
        engine.active_group().active_tab + 1,
        engine.active_group().tabs.len()
    )
}

/// What one [`NormLive`] recording claims, re-measured.
type NormSeen = (
    String,
    (usize, usize),
    String,
    String,
    usize,
    usize,
    String,
    String,
);

/// Replay one recording against a real engine. Black-box throughout: keys in,
/// rendered output out.
fn replay_norm_live(p: &NormLive) -> NormSeen {
    let mut engine = engine_with(&p.lines.join("\n"));
    // Same reason as #1226/#1228: `Engine::new()` loads the *user's real*
    // command history off disk, and several rows here type at a `:` prompt.
    engine.history = Default::default();
    engine.settings.shift_width = 4;
    engine.settings.expand_tab = true;
    engine.settings.tabstop = 4;
    engine.set_viewport_lines(p.view);
    // 80 columns, matching the oracle's default screen: the `zh`/`zl`/`ze`/
    // `zs`/`zH`/`zL` rows are *about* the horizontal viewport.
    engine.view_mut().viewport_cols = 80;
    engine.view_mut().cursor.line = p.at.0.saturating_sub(1);
    engine.view_mut().cursor.col = p.at.1.saturating_sub(1);
    engine.ensure_cursor_visible();
    send_norm_keys(&mut engine, p.keys);
    (
        engine.buffer().to_string().replace('\n', "|"),
        (engine.view().cursor.line + 1, engine.view().cursor.col + 1),
        engine.mode_str().to_string(),
        engine.message.replace('\n', "\\n"),
        engine.view().scroll_top + 1,
        engine.view().scroll_left + 1,
        norm_folded(&engine),
        norm_windows(&engine),
    )
}

/// Every row whose recorded behaviour no longer matches the live engine.
fn norm_drift(audit: &[NormAudit]) -> Vec<String> {
    let mut drift: Vec<String> = Vec::new();
    for e in audit {
        let Some(p) = e.live.as_ref() else { continue };
        let (buffer, cursor, mode, message, top, left, folded, windows) = replay_norm_live(p);
        let recorded = (
            p.buffer.to_string(),
            p.cursor,
            p.mode.to_string(),
            p.message.to_string(),
            p.top,
            p.left,
            p.folded.to_string(),
            p.windows.to_string(),
        );
        let live = (buffer, cursor, mode, message, top, left, folded, windows);
        if recorded != live {
            drift.push(format!(
                "  {:?} ({}) keys={:?}\n\
                 \x20   recorded {:?}\n\
                 \x20   live     {:?}",
                e.item, e.help, p.keys, recorded, live
            ));
        }
    }
    drift
}

/// Gate 1 (#1229) — every row's recorded behaviour is a claim about live
/// code, and this replays all 323 of them against it. Pure: no `nvim`, no
/// subprocess, so it runs on every lane.
#[test]
fn normal_audit_matches_the_live_engine() {
    if let Ok(path) = std::env::var("CONFORMANCE_DUMP_NORMAL") {
        let mut s = String::new();
        for e in NORMAL_AUDIT {
            let Some(p) = e.live.as_ref() else { continue };
            let (buffer, cursor, mode, message, top, left, folded, windows) = replay_norm_live(p);
            s.push_str(&format!(
                "{}\t{:?}\t{}\t{}\t{}\t{:?}\t{}\t{}\t{}\t{}\n",
                e.item, buffer, cursor.0, cursor.1, mode, message, top, left, folded, windows
            ));
        }
        std::fs::write(&path, s).unwrap_or_else(|e| panic!("dump to {path}: {e}"));
        return;
    }

    let drift = norm_drift(NORMAL_AUDIT);
    assert!(
        drift.is_empty(),
        "\n\n== normal-mode audit drifted from the engine (#1229) ==\n\
         Each row records what vimcode actually did when the slice ran.\n\
         Implementing (or breaking) one of these changes that recording, so\n\
         re-tag the row — that is how the audit stays true instead of rotting\n\
         like a markdown checklist.\n\n{}\n\n\
         A command that gained an implementation also needs its status changed\n\
         from NotImplemented, and a NORM_COVERAGE_EXEMPT entry deleted once an\n\
         oracle case pins it.\n",
        drift.join("\n")
    );
}

/// Gate 1b (#1229) — the table describes itself correctly: complete, unique,
/// no unreviewed row, every ⏭️ carrying a reason from the shared vocabulary,
/// and a recording + probe on exactly the rows that can have one.
#[test]
fn normal_audit_is_internally_consistent() {
    use std::collections::HashSet;
    let mut problems: Vec<String> = Vec::new();

    assert_eq!(
        NORMAL_AUDIT.len(),
        370,
        "`:help normal-index` outside the g-prefix walked to 370 entries; \
         the audit must tag all of them"
    );

    let no_replay: HashSet<&str> = NORM_NO_REPLAY.iter().map(|(item, _)| *item).collect();
    assert_eq!(
        no_replay.len(),
        NORM_NO_REPLAY.len(),
        "NORM_NO_REPLAY lists an item twice"
    );
    for (item, reason) in NORM_NO_REPLAY {
        if !NORMAL_AUDIT.iter().any(|e| e.item == *item) {
            problems.push(format!("  NORM_NO_REPLAY {item:?}: not a row of the audit"));
        }
        if reason.trim().is_empty() {
            problems.push(format!("  NORM_NO_REPLAY {item:?}: needs a reason"));
        }
    }

    let mut seen: HashSet<&str> = HashSet::new();
    for e in NORMAL_AUDIT {
        if !seen.insert(e.item) {
            problems.push(format!("  {:?}: listed twice", e.item));
        }
        if e.note.trim().is_empty() {
            problems.push(format!(
                "  {:?}: empty note — every row is reviewed",
                e.item
            ));
        }
        if e.help.trim().is_empty() {
            problems.push(format!("  {:?}: no `:help` tag", e.item));
        }

        let skipped = matches!(e.status, OptStatus::Skipped(_));
        if let OptStatus::Skipped(reason) = e.status {
            if !SKIP_REASONS.contains(&reason) {
                problems.push(format!(
                    "  {:?}: skip reason {reason:?} is not in SKIP_REASONS",
                    e.item
                ));
            }
        }
        let replayable = !skipped && !no_replay.contains(e.item);
        if replayable && e.live.is_none() {
            problems.push(format!(
                "  {:?}: reachable from a keyboard, so it must carry a live recording",
                e.item
            ));
        }
        if replayable && e.probe.is_none() {
            problems.push(format!(
                "  {:?}: reachable from a keyboard, so it must carry an oracle probe",
                e.item
            ));
        }
        if !replayable && (e.live.is_some() || e.probe.is_some()) {
            problems.push(format!(
                "  {:?}: skipped or unreachable, so a recording or probe can never fire",
                e.item
            ));
        }
    }

    // The headline tally, pinned. The section doc quotes these numbers and a
    // PR body quotes the section doc; without this they drift the moment a
    // row is re-tagged, which is the exact rot a markdown checklist suffers.
    let tally = |want: fn(&NormAudit) -> bool| NORMAL_AUDIT.iter().filter(|e| want(e)).count();
    assert_eq!(
        (
            tally(|e| matches!(e.status, OptStatus::Implemented)),
            tally(|e| matches!(e.status, OptStatus::Partial)),
            tally(|e| matches!(e.status, OptStatus::NotImplemented)),
            tally(|e| matches!(e.status, OptStatus::Skipped(_))),
            tally(|e| e.live.is_some()),
        ),
        (208, 20, 118, 24, 323),
        "the audit tally moved: (implemented, partial, missing, skipped, \
         replayed). Update the section doc's table in the same commit."
    );

    assert!(
        problems.is_empty(),
        "\n\n== normal-mode audit table is inconsistent (#1229) ==\n{}\n",
        problems.join("\n")
    );
}

/// `cases` is `(label, keys)` for the whole corpus — the same view
/// [`classify_coverage`] takes.
fn classify_norm_coverage(
    audit: &[NormAudit],
    exempt: &[&'static str],
    cases: &[(&'static str, &'static str)],
) -> OptionCoverage {
    use std::collections::HashSet;
    let exempt_set: HashSet<&str> = exempt.iter().copied().collect();
    let mut v = OptionCoverage::default();
    for e in audit {
        let Some(probe) = e.probe else { continue };
        v.in_scope += 1;
        let covered = cases.iter().any(|(label, keys)| probe.matches(label, keys));
        match (covered, exempt_set.contains(e.item)) {
            (false, false) => v.uncovered.push(e.item),
            (true, true) => v.newly_covered.push(e.item),
            _ => {}
        }
    }
    v.stale = exempt
        .iter()
        .copied()
        .filter(|n| !audit.iter().any(|e| e.item == *n && e.probe.is_some()))
        .collect();
    v
}

/// Gate 2 (#1229) — #1007's ratchet, applied to the audited commands: an
/// in-scope row whose probe matches nothing must be exempt, and an exempt row
/// whose probe now matches must lose its entry. Pure.
#[test]
fn normal_audit_oracle_coverage_is_shrink_only() {
    use std::collections::HashSet;
    let corpus = all_corpus_cases();
    let exempt: HashSet<&str> = NORM_COVERAGE_EXEMPT.iter().copied().collect();
    assert_eq!(
        exempt.len(),
        NORM_COVERAGE_EXEMPT.len(),
        "NORM_COVERAGE_EXEMPT lists an item twice"
    );

    let v = classify_norm_coverage(NORMAL_AUDIT, NORM_COVERAGE_EXEMPT, &corpus);

    if let Ok(path) = std::env::var("CONFORMANCE_DUMP_NORMAL_COVERAGE") {
        let mut s = String::new();
        for n in &v.uncovered {
            s.push_str(&format!("UNCOVERED\t{n}\n"));
        }
        for n in &v.newly_covered {
            s.push_str(&format!("NEWLY_COVERED\t{n}\n"));
        }
        for n in &v.stale {
            s.push_str(&format!("STALE\t{n}\n"));
        }
        for e in NORMAL_AUDIT {
            let Some(probe) = e.probe else { continue };
            if let Some((label, _)) = corpus
                .iter()
                .find(|(label, keys)| probe.matches(label, keys))
            {
                s.push_str(&format!(
                    "COVERED\t{}\t{:?}\t{}\n",
                    e.item,
                    probe.needle(),
                    label
                ));
            }
        }
        std::fs::write(&path, s).unwrap_or_else(|e| panic!("dump to {path}: {e}"));
        return;
    }

    let covered = v.in_scope - NORM_COVERAGE_EXEMPT.len();
    println!(
        "\n== normal-mode oracle coverage (#1229) ==\n\
         {covered}/{} audited commands are pinned by an oracle case; {} exempt.\n\
         Writing the missing cases is #1162.\n",
        v.in_scope,
        NORM_COVERAGE_EXEMPT.len()
    );

    assert!(
        v.uncovered.is_empty() && v.newly_covered.is_empty() && v.stale.is_empty(),
        "\n\n== normal-mode audit oracle coverage moved (#1229) ==\n\
         no case exercises these, and they are not exempt:\n  {:?}\n\
         a case now exercises these — delete them from NORM_COVERAGE_EXEMPT:\n  {:?}\n\
         these exempt entries name no probed row — stale:\n  {:?}\n",
        v.uncovered,
        v.newly_covered,
        v.stale
    );
}

/// Gate 2b (#1229) — the gate above can actually fail.
///
/// #553's lesson, applied to a ratchet: a shrink-only list is worthless if
/// nothing proves the three failure directions fire. Deleting a real exempt
/// entry must report it uncovered, adding a case that matches an exempt row
/// must report it newly covered, and an exempt entry naming nothing must be
/// reported stale.
#[test]
fn normal_audit_ratchet_can_fail() {
    let corpus = all_corpus_cases();

    let victim = "CTRL-W s";
    assert!(
        NORM_COVERAGE_EXEMPT.contains(&victim),
        "fixture drifted: {victim} is no longer exempt"
    );
    let without: Vec<&str> = NORM_COVERAGE_EXEMPT
        .iter()
        .copied()
        .filter(|n| *n != victim)
        .collect();
    let deleted = classify_norm_coverage(NORMAL_AUDIT, &without, &corpus);
    assert_eq!(
        deleted.uncovered,
        vec![victim],
        "deleting {victim:?} from NORM_COVERAGE_EXEMPT must fail the gate"
    );

    let mut plus = corpus.clone();
    plus.push(("win:C-w s splits", "<C-w>s"));
    let improved = classify_norm_coverage(NORMAL_AUDIT, NORM_COVERAGE_EXEMPT, &plus);
    assert_eq!(
        improved.newly_covered,
        vec![victim],
        "a case that matches an exempt row must fail the gate until the entry goes"
    );

    let stale: Vec<&str> = NORM_COVERAGE_EXEMPT
        .iter()
        .copied()
        .chain(std::iter::once("CTRL-W definitely-not-a-command"))
        .collect();
    let with_stale = classify_norm_coverage(NORMAL_AUDIT, &stale, &corpus);
    assert_eq!(
        with_stale.stale,
        vec!["CTRL-W definitely-not-a-command"],
        "an exempt entry naming no audited row must be reported stale"
    );

    let real = classify_norm_coverage(NORMAL_AUDIT, NORM_COVERAGE_EXEMPT, &corpus);
    assert!(
        real.uncovered.is_empty() && real.newly_covered.is_empty() && real.stale.is_empty(),
        "the real lists must be clean once the three injections are removed"
    );
}

/// The `g`-prefix slice's stale header, pinned from the *read-only* doc.
///
/// `COVERAGE_PHASE5.md`'s "Slice 1" header still reads
/// `✅ 36 · 🟡 2 · ❌ 14 · ⏭️ 6`, but its own "Coverage summary so far" table
/// reads `41 / 2 / 9 / 6` for the same 58 commands. The summary is the
/// correct one: #120–#123 were implemented and struck through in the gap
/// list (`gF`, `g@`, `g<Tab>` and the five screen-line motions `g$` / `g0` /
/// `g^` / `g<End>` / `g<Home>` — five rows moved from ❌ to ✅), and only the
/// header was left behind.
///
/// #1229 was asked to carry that correction in **the table and the PR body**;
/// editing `COVERAGE_PHASE5.md` is a coordinator follow-up, so this slice
/// pins the right numbers here instead and reads the doc without writing it.
/// The gate is deliberately asserted against the *summary row*, so "fixing"
/// the contradiction by editing the summary down to the stale header's
/// numbers fails here.
const G_PREFIX_CORRECTED_TALLY: (usize, usize, usize, usize) = (41, 2, 9, 6);

/// Gate 3 (#1229) — the `g`-prefix slice's corrected tally still matches the
/// summary table of `COVERAGE_PHASE5.md`, and still sums to its 58 commands.
#[test]
fn g_prefix_slice_tally_is_the_summary_not_the_stale_header() {
    let doc = include_str!("../COVERAGE_PHASE5.md");
    let row = doc
        .lines()
        .find(|l| l.starts_with("| `g`-prefix |"))
        .unwrap_or_else(|| panic!("COVERAGE_PHASE5.md has no `g`-prefix summary row"));
    let cells: Vec<&str> = row
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect::<Vec<_>>();
    let num = |i: usize| -> usize {
        cells
            .get(i)
            .and_then(|c| c.parse().ok())
            .unwrap_or_else(|| panic!("summary row cell {i} is not a number: {row:?}"))
    };
    let (total, implemented, partial, missing, skipped) = (num(1), num(2), num(3), num(4), num(5));

    assert_eq!(
        (implemented, partial, missing, skipped),
        G_PREFIX_CORRECTED_TALLY,
        "COVERAGE_PHASE5.md's `g`-prefix summary row moved. The stale header \
         above it says ❌ 14; the correct count is ❌ 9 (#120–#123 landed). \
         Fix the header, not the summary."
    );
    assert_eq!(
        implemented + partial + missing + skipped,
        total,
        "the `g`-prefix tally must still add up to its {total} commands"
    );
}
