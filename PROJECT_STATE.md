# VimCode Project State

**Last updated:** October 7, 2026 (#1828 — TUI minimap on real terminals:
no visible thumb on Windows Terminal; drag behaves like the scrollbar on
macOS). **Split into a shared-code fix (macOS drag mapping) + a quadraui
gap (Windows Terminal thumb contrast) — the two reported symptoms had two
different, independent root causes.**

1. **Drag-mapping fix, shipped directly in vimcode (no quadraui change
   needed):** the real root cause of "dragging the minimap feels like the
   scrollbar" was that the **default** (no-modifier) minimap thumb drag
   was already #1187's intentional file-wide/scrollbar-equivalent
   mapping, with #1271's minimap-own-scale ("fine") mapping gated behind
   Alt — the opposite of VS Code, where the plain drag is the
   minimap-scale one. Both backends already shared one resolver
   (`click::pixel_to_click_target` -> `render::minimap_press`), so this
   was a pure shared-code fix: flipped the boolean vimcode passes in
   (`!alt` instead of `alt`), so a plain drag now gets #1271's
   minimap-own-scale mapping and **Alt-held** gets the old #1187
   file-wide one (kept as a power-user "fast scroll" affordance rather
   than deleted). Also fixed a genuine, independent latent bug this
   flip exposed: `render::fine_seek_geometry` re-derived its own,
   unclamped `thumb_length` (`Sh * viewport_lines / span`) instead of
   reusing the real, already-painted `viewport_highlight` band height —
   diverging from it whenever a file is short enough that quadraui's
   `fit_thumb` minimum-thumb-length floor is in play (caught by the
   pre-existing `minimap_drag_keeps_seeking_while_the_button_is_held`
   GTK test going from ~50% to ~19% once "fine" became the default).
   Fixed by threading the real band height straight through — the
   virtual track's endpoint math is provably independent of which
   `thumb_length` is used (it cancels out of dispatch's own
   `track_length - thumb_length`), so this costs nothing.

   **Black-box coverage, both backends, RED-verified:** four new/updated
   driver tests — GTK's
   `dragging_the_minimap_viewport_highlight_scrolls_the_whole_file_on_gtk`
   (now Alt-held) + new sibling
   `..._scrolls_within_its_own_scale_by_default_on_gtk`, and the TUI
   twins `..._scrolls_the_whole_file_with_alt_held` +
   `..._scrolls_within_its_own_scale_by_default` in
   `tui_main/app_on_tui_tests.rs`. Confirmed RED against the pre-fix
   mapping by hand (temporarily reverting `click.rs`'s `!alt` back to
   `alt` and re-running all four — all four failed, the two new
   "default" tests landing past 90% of a 200,000-line file instead of
   under 10%). The pre-existing `minimap_drag_keeps_seeking_while_the_button_is_held`
   GTK test was updated to hold Alt (its point — drag-continuation keeps
   re-seeking — is orthogonal to which of the two mappings is active;
   its specific "~50% of the whole file" assertion only holds under the
   file-wide one).

2. **Windows Terminal thumb invisibility — quadraui gap, drafted, not
   fixable in vimcode.** Traced the viewport-highlight band's paint
   colour to `quadraui::tui::minimap::draw_minimap_with_scale`'s
   `theme.background.blend(theme.accent_bg, 0.25)` — a fixed, no-floor
   25% blend entirely inside quadraui; vimcode supplies only the two
   source colours (`to_quadraui_theme`) and wires no terminal
   colour-depth/`COLORTERM` detection anywhere in `src/tui_main/`
   (confirmed by grep — there is no vimcode-side seam to intervene at
   without adding new per-backend logic, which `CLAUDE.md`'s
   Platform-Neutrality Rule forbids). Drafted as a new entry in
   `docs/PENDING_QUADRAUI_ISSUES.md` for the coordinator to file;
   `ISSUE_RESOLUTION: partial` on #1828 — the macOS drag-mapping half is
   fixed, the Windows Terminal thumb-visibility half still needs the
   quadraui fix and a real Windows Terminal re-check per the issue's own
   acceptance criteria.

**Last updated:** October 7, 2026 (#1786 — bugbash: Alt-M back to Vim mode
leaves the VSCode-mode menu bar permanently visible). **Duplicate, already
fixed — no new code.** #1786 is the identical bug (same repro: Alt-M,
Alt-M, row 0 still shows the File/Edit/.../Help menu bar) as #1780, whose
fix (`ad939abf`/`6bb1fbcf`, gating `Engine::toggle_editor_mode`'s
VSCode->Vim arm to clear `menu_bar_visible` when `menu_bar_toggleable`) is
already merged to `develop` — confirmed by `git fetch origin develop`
landing on the exact same SHA (`e2466e97`) this branch was created from,
i.e. this branch's diff against `develop` is empty. The acceptance
criterion in #1786 ("must add a Tier-1 shared conformance scenario or
Tier-2 smoke-spec step that fails first") is also already satisfied by
#1780's own fix: `tests/smoke-spec/catalogue.yaml`'s
`modeswitch-alt-m-back-to-vim` entry, the TUI `TuiDriver` test
(`alt_m_round_trip_hides_the_menu_bar_row_again_via_shell_app_1780` in
`src/tui_main/app_on_tui_tests.rs`), and the GTK mirror
(`mod alt_rung_1744` in `src/gtk/testing.rs`) were all added/tightened in
#1780's round 2. Re-ran the TUI test this session
(`cargo test --no-default-features --lib
alt_m_round_trip_hides_the_menu_bar_row_again_via_shell_app_1780`) — passes
clean against current `develop`. `ISSUE_RESOLUTION: resolved` — #1786
should close as a duplicate of #1780, not as new work merged.

**Last updated:** October 6, 2026 (#1824 — macOS native real-screen smoke: Dock icon, window activation, icon font, text quality — investigation, no production fix). **Investigated on real macmini hardware this session; no vimcode-repo fix exists for any of the three confirmed/partially-confirmed findings.** Built `cargo build --release --bin vimcode --no-default-features --features macos` and drove it live (real `screencapture`, `osascript`/`System Events`, `lsappinfo`) rather than reading code alone. Findings, most to least confirmed:

1. **Window activation (regression 2 of 4) — reproduced cleanly.** Launched from inside a real, frontmost Terminal.app window (not a detached automation harness — that path did *not* reproduce it, which mattered). Result: `lsappinfo` reports vimcode `(in front)`/`Foreground`, but `System Events`'s frontmost-process query still names `Terminal`, and the composited screenshot shows Terminal's window literally covering vimcode's. Root-caused to quadraui, not vimcode: `quadraui::macos::run::run_with` (`src/macos/run.rs:2082`) calls the *deprecated* `-[NSApplication activateIgnoringOtherApps:]` (Apple's own SDK note, visible in the pinned `objc2-app-kit` 0.3.2 binding, says "Use NSApp.activate instead" — and the non-deprecated `NSApplication::activate()` already exists in that same pinned crate, unused). `src/macos/mod.rs` is confirmed thin wiring with no activation seam of its own. Drafted as a quadraui issue in `docs/PENDING_QUADRAUI_ISSUES.md` (new entry, directly below the pre-existing #1825 GTK-activation draft — this finding is stronger evidence than that draft had, since the native backend already calls an activation API and still fails, where GTK calls none at all).
2. **Icon-font glyphs (regression 3 of 4) — reproduced cleanly, root cause narrowed but not found.** Real painted activity bar: Explorer/Source Control/Run&Debug show a generic `?`-box placeholder; Search/Extensions/AI Chat show their correct Nerd-Font glyphs, same frame, same font registration. Hand-parsed `data/fonts/vimcode-icons.ttf`'s raw `cmap`(fmt 4 + fmt 12)/`glyf` tables (no `fontTools` — not installed, can't `pip install` per this session's tooling policy) for all six codepoints: **every one** has a valid GID in both cmap formats and non-empty `glyf` outline data (216-350 bytes) — the font asset is not the bug, contradicting the plausible "missing glyph in the subset" hypothesis before it could cause anyone to chase it. This is new, stronger evidence than vimcode#937 (closed) had — that issue's own test documented it *could not verify* real-hardware glyph resolution at all. Drafted as a quadraui issue (`MacBackend`'s Core Text fallback cascade, `macos/text.rs::font_with_fallback`) with the full per-codepoint table; root cause of *why* exactly these three and not the other three is left open for the next pass (candidates listed in the draft, none confirmed).
3. **Dock icon (regression 1 of 4) — not reproduced on a clean launch.** Polled the Dock's UI-element list for 20+ seconds after a fresh launch; the tile was present throughout with the correct embedded icon. Did observe it absent once, but only right after this session's own `killall Dock` (used to defeat autohide for screenshotting) restarted Dock out from under an *already-running*, older instance — a plausible Dock-restart reconnection artifact, not evidence of #1824's reported "used to show, now doesn't" on an ordinary launch. Not drafted upstream; needs a clean re-test (fresh launch, no Dock-process interference) before concluding anything.
4. **Text quality (regression 4 of 4, long-standing) — not investigated further.** `sips -z` nearest-neighbour upscaling (the only inspection tool available this session) makes any crop look blocky regardless of real on-screen rendering quality, so no credible new evidence either way. #1824 itself frames this as longstanding ("has always been poor"), not a new regression; #1069/#1542 already closed against the same complaint. Needs a native-resolution, non-upscaled capture method to make progress.

System left exactly as found: Dock `autohide` restored to `true` (`killall Dock` to apply), no stray vimcode processes left running. No `src/` changes in this PR — per CLAUDE.md's Platform-Neutrality Rule, none of these three findings has a legitimate vimcode-repo fix (all three root causes, where found, point inside `quadraui::macos`, which this repo's workers may not edit directly). `ISSUE_RESOLUTION: investigation` on #1824 — ships two new quadraui issue drafts plus the Dock/text-quality notes in `docs/PENDING_QUADRAUI_ISSUES.md` for the coordinator to file; #1824 should stay open behind those two upstream issues per `GOALS.md`'s milestone-discipline rule.

**Last updated:** October 6, 2026 (#1829 — Windows TUI: `:term` opens a
blank panel with no PowerShell prompt). **Investigation only — no
production change, real-hardware verification still required.** Traced
every vimcode-side hop between the `:term` ex-command and the PTY write and
confirmed each one is already shared, platform-neutral code with no
Windows-specific branch: `Engine::execute_command`'s `"terminal"` arm ->
`EngineAction::OpenTerminal` (`src/core/engine/execute.rs`) ->
`render::handle_action`'s `OpenTerminal` arm (`src/render.rs`) ->
`Engine::terminal_new_tab` (`src/core/engine/terminal_ops.rs`, identical on
every backend) -> once
`terminal_has_focus` is set, every subsequent keystroke is forwarded by
`render::route_terminal_key`'s `TerminalKeyAction::SendToPty` arm
(`src/render.rs`, also shared). `TerminalSession::spawn`/`poll` themselves
are `quadraui::terminal_engine` — not vimcode source at all, confirmed
against the pinned rev (`a5360532e297deecece65103df8ce61f53230bda`). So
there is **no per-backend vimcode-side fix available** here, per the
Platform-Neutrality Rule — this is a quadraui/`portable_pty` ConPTY
question, and confirmed to have essentially zero existing coverage there:
`quadraui/src/terminal_engine.rs`'s own `#[cfg(test)] mod tests` is almost
entirely `#[cfg(unix)]`-gated (grep finds dozens of hits, zero
Windows-gated spawn/poll tests), so the exact path this bug lives in has
never been exercised automatically on the platform it was reported on.

Also confirmed, narrowly: the tick re-arm (`5401a24e`/`f321ac37`,
`App::tick_dispatch`'s `terminal_poll_rearm_delay`) that #1668 landed for
Win-GUI cannot be **this** TUI symptom's cause — it's explicitly a no-op on
TUI (`src/app.rs`'s own comment: "Harmless on GTK/TUI/macOS (they already
tick regardless, via their own `IDLE_POLL_CEILING` fallback)"), and
`terminal_poll_rearm_delay`'s own doc already flags the one thing that fix
could not verify (whether a real `WM_TIMER` re-fires on real Windows
hardware). That narrow mechanical point does **not**, on its own, mean the
issue's "likely the same root cause as #1668" premise is dead — the issue
body notes #1668 was **reopened** the same day this session ran, i.e. the
re-arm did not actually resolve the Win-GUI symptom on real hardware, which
is evidence *for* a shared root cause, not against it. This session's own
destination (quadraui's untested Windows ConPTY leg of the shared
`terminal_engine`) is exactly the kind of shared cause the issue predicted:
both #1668 and #1829 drive the same `quadraui::terminal_engine::
TerminalSession` on Windows, just from different front ends (Win-GUI vs.
Windows TUI). So: the Win-GUI tick re-arm is not the TUI bug's cause — yes;
"the two issues share nothing but symptom + dependency" — not established,
and arguably contradicted by #1668's reopening. Confirming this needs the
same real-hardware check named below: once a fix lands and is verified
against vimcode's new ConPTY test on `dell64`, re-check #1668 against it
too, per the issue's own acceptance step 3.

Added `tests/conpty_term_opens_shell_1829.rs` — a new, `#[cfg(windows)]`-
gated real-ConPTY regression test following the exact precedent of
`tests/conpty_idle_flicker.rs` (#1634) and
`tests/conpty_activity_bar_click.rs` (#1636): spawns `vcd.exe` cross-
compiled for `x86_64-pc-windows-msvc` under a real Win32 ConPTY, types the
literal keystrokes `:term` + Enter, waits, then types the issue's own
acceptance probe (`echo <marker> > "<path>"` + Enter) and polls the
**filesystem** (not the screen — the whole point is proving the shell
actually ran a command) for the marker file to appear. Verified to compile
cleanly both ways from this Linux session (no Windows host attached here):
`cargo xwin check --target x86_64-pc-windows-msvc --test
conpty_term_opens_shell_1829` and plain `cargo check --test
conpty_term_opens_shell_1829` (compiles to an empty, no-op crate on
non-Windows via its `#![cfg(windows)]`) both succeed; `cargo fmt --check`
and the normal host-target `cargo clippy -D warnings` lane are clean. **Not
yet run** — that needs `cargo xwin test --target x86_64-pc-windows-msvc`
executed on real Windows hardware (`dell64`), which this session has no
access to. (A Windows-cross-target `cargo xwin clippy` run surfaced 4
pre-existing lint failures in unrelated `src/core/paths.rs`/`src/core/
swap.rs` Windows-only code — not part of this diff, not part of this
repo's mandated host-target pre-commit gate, left alone.)

Also confirmed, reading the issue body against the pinned quadraui source:
the separately-named latent bug is real — `quadraui::terminal_engine::
default_shell()` reads `$SHELL` **before** checking `target_os`, so a
Windows host with `$SHELL` set (a Git Bash or WSL tab, per the issue's own
note) hands a Unix shell path straight to `TerminalSession::spawn`'s
`CommandBuilder`, which would plausibly also produce a dead/blank panel —
distinct from dell64's own repro (confirmed `$SHELL` unset there) but
explicitly in-scope per the issue body ("the fix should cover it"). This
is quadraui code (`quadraui/src/terminal_engine.rs:2175`), not reachable
from vimcode source — no vimcode-side fix available for this half either.

**Quadraui issue drafted (review round 1):** the file-overlap fence that
blocked this in the first pass has cleared — #1825's own branch (which was
editing `docs/PENDING_QUADRAUI_ISSUES.md` concurrently) has since landed on
`develop` (`58edb6d1`) — so the draft now lives in its designated home,
`docs/PENDING_QUADRAUI_ISSUES.md`'s "`quadraui::terminal_engine`'s Windows
ConPTY spawn/poll path has essentially no automated test coverage..."
entry, rather than buried in this chronological log. See that file for the
full title/body/ask/test/blocks text; filing it on `JDonaghy/quadraui` is
still coordinator/human work per that file's own process.

**CI wiring added (review round 1):** `.github/workflows/ci.yml`'s
`build-windows-tui` job now runs `conpty_term_opens_shell_1829` as a
`continue-on-error: true` step, same pattern #1636's step documents for a
test "not yet promotable to a hard gate" — this was the blocking review
finding: `windows-latest` is a real ConPTY host already building `vcd.exe`
and already running the two sibling ConPTY tests, so it is a host capable
of producing this test's RED/GREEN signal today, at zero added human cost,
and leaving the new test wired into nothing meant 380 lines of this PR's
only executable deliverable ran nowhere, ever.

**What remains before #1829 can be considered resolved, let alone closed**
(explicitly per the issue's own acceptance criteria): (1) the quadraui
issue above needs to actually be filed (coordinator/human action) and a
fix landed + pin bumped; (2) the new CI step needs to actually run once on
`windows-latest` and its RED/GREEN result read, to confirm the test itself
behaves as designed before anyone promotes it off `continue-on-error`; (3)
a person with `dell64` access needs to actually launch `vcd.exe` from a
PowerShell tab, run `:term`, and visually confirm whether a real prompt
appears — the acceptance criteria's own "a person confirms on dell64's
screen" step, which no CI run (real-ConPTY or otherwise) substitutes for.
No `tests/smoke-spec/win-terminal.yaml` step was added for this: that
file's own step vocabulary is interpreted by `coord`'s
`win_native_driver.py` (a different repo this worker cannot see or edit),
which has no filesystem-assertion step type today — the same class of
testing-infrastructure gap `win-terminal.yaml`'s own header already
documents for #1635/#1636 (UIA can only see Windows Terminal's single
opaque `terminal` element, not step content). Inventing an unsupported
YAML key here would silently no-op rather than gate anything, so none was
added; the new Tier-1 `conpty_term_opens_shell_1829.rs` test, now actually
wired into CI, is the closest automatable substitute, same reasoning
`win-terminal.yaml`'s own #1751 section gives for the identical situation.

ISSUE_RESOLUTION: investigation — root cause isolated to quadraui's
Windows ConPTY leg (untested at the pinned rev) and a separate latent
`default_shell()` ordering bug, both outside vimcode's own source per the
Platform-Neutrality Rule. The quadraui-side issue is **drafted** in its
designated home, `docs/PENDING_QUADRAUI_ISSUES.md` (the file-overlap fence
with #1825 that blocked that in the first pass has cleared — see "Quadraui
issue drafted" above), but **not yet filed** as a real GitHub issue on
`JDonaghy/quadraui`; a drafted entry is not a filed issue. A new
real-ConPTY regression test is added and is now wired into CI's
`windows-latest` job, but has not yet produced a readable RED/GREEN result
anywhere, and the issue's own acceptance step 3 — a person confirming a
PowerShell prompt on `dell64`'s screen — is still outstanding.

**Review round 1 fixes:** the quadraui draft has moved to its designated
home (`docs/PENDING_QUADRAUI_ISSUES.md`, #1825's conflicting branch has
since landed); `.github/workflows/ci.yml`'s `build-windows-tui` job now
actually runs `conpty_term_opens_shell_1829` (`continue-on-error: true`,
per #1829's own blocking review finding that the test previously executed
nowhere); the test itself replaced its fixed `AFTER_OPEN_SETTLE` sleep and
O(n²) re-parse with a persistent-parser `wait_for_screen_contains(...)`
poll (mirroring `conpty_activity_bar_click.rs`'s own documented
reasoning for why a fixed quiet-window is flaky on real hardware), gained
an RAII `TermTestGuard` so the child/temp-home/probe-file are cleaned up on
every exit path including a failed assertion before the old explicit
teardown block ran (mirroring the #1822 `OrphanChildGuard` pattern already
landed in this repo), and the PROJECT_STATE #1668 paragraph above was
softened to stop overstating "the premise does not hold" against an issue
the bug report itself says was reopened the same day. See this session's
commit for the full list (probe path now single-quoted in the PowerShell
command line, `&Path` instead of `&PathBuf`, UTF-16BE doc note).

**Review round 2 fixes:** round 1's new "the panel opened" precondition
waited on the needle `"TERMINAL"` (uppercase), which **can never paint in
any state that test can reach** — it is the terminal toolbar tab strip's
`if tabs.is_empty()` fallback label (`src/render.rs`), so it needs
`TerminalPanel::tab_count == 0`, but `tab_count` is
`engine.terminal_panes.len()` and `render::terminal_panel_desc`
early-returns `None` (painting no panel at all) when there are no panes;
on the `TerminalSession::spawn` failure path `terminal_new_tab_at` never
sets `terminal_open`, so nothing paints there either. The uppercase
assumption was carried over by false analogy from
`conpty_activity_bar_click.rs`'s sidebar needles, which come from
`fixed_panel_title_tooltip` (sidebar panels only, no terminal entry). As
written it would have burned the 20 s `SETTLE_TIMEOUT` and failed
identically on a working build and a broken one, never reaching the
filesystem probe — i.e. it neutralised the exact CI signal round 1 added
the step to produce, and made that step's own promotion condition
("observed GREEN at least once") unsatisfiable. Fixed: the needle is now a
documented `PANEL_OPEN_NEEDLE` constant = `"[1]"`, the toolbar's first
per-tab label (`format!("[{}]", i + 1)`), which is painted exactly when a
terminal pane exists. Title-case `"Terminal"` was rejected as a substitute
because it is also a permanent top-level menu-bar title, so it is on
screen before `:term` runs and the gate would be vacuous. **The needle is
now verified on Linux with no Windows host**, which is what would have
caught this in round 1:
`src/tui_main/app_on_tui_tests.rs`'s
`term_ex_command_paints_bracketed_tab_label_not_uppercase_terminal` drives
the real `:term` ex-command through `TuiDriver` and asserts both halves —
`"[1]"` absent before and painted after, `"TERMINAL"` absent in both — so
"the needle is wrong" can no longer masquerade as "ConPTY is broken".
Also: `TermTestGuard::drop` now takes every lock with
`unwrap_or_else(|e| e.into_inner())` (a panic inside `Drop` during an
assertion unwind aborts the process and destroys the captured-screen
diagnostics the panic message exists to deliver); dropped the no-op
`let _ = self.child.try_wait();` that preceded an unconditional
sleep-then-kill; corrected `Drop`'s "graceful shutdown" comment, which
claimed a `:qa!` that `render::route_terminal_key` actually forwards to
the nested shell once `terminal_has_focus` is set (it only reaches
vimcode's ex line on the early-failure paths, which is now what the
comment says); and the test's "CI wiring" module doc now spells out what a
red result means *today* (expected, carries no new information beyond the
failure's shape) versus after the quadraui pin bump (a real regression
signal, and the thing that promotes the step off `continue-on-error`). The
contradictory `ISSUE_RESOLUTION` paragraph above — which still claimed the
quadraui draft "could not be drafted into
`docs/PENDING_QUADRAUI_ISSUES.md`" that the same commit had in fact landed
there — was reconciled, and now states plainly that the draft exists but
the GitHub issue is still unfiled.

**Last updated:** October 6, 2026 (#1798 — bugbash:win-native "Tab bar label doesn't update when switching the active file via the Explorer; only one tab ever appears"). **Fixed.** Root cause was never the Explorer dispatch path — `Engine::open_file_in_tab` always appended the second tab, which is why breadcrumb, content and status bar all followed the new buffer. The second tab had nowhere to **paint**: `App::shell_config` (`src/app.rs`) set `min_sidebar_width`/`max_sidebar_width` but left `ShellConfig::default_sidebar_width` at quadraui's generic `20.0`, and `AppShell::compute_layout` multiplies that by `line_height` on *every* backend. On TUI that is 20 terminal columns (correct); on a GUI backend it is ~460 device pixels, so beside the 48px activity bar the bugbash's 800x480 window had ~290px left for the editor *and* its tab bar — room for exactly one tab, hence "only one tab ever appears". Fix is one number made per-*unit* rather than per-backend, in the sanctioned shared place: `render::UnitProfile::sidebar_width_lh` (`src/render.rs`), joining `activity_bar_width_px`/`title_bar_lh` — `ALT_SIDEBAR_WIDTH_MIN`'s 15 (~345px, in the same range as `Session::sidebar_width`'s persisted 260 default and VS Code's ~300px) on the `px` profile, 20 cells unchanged on `cell`. No per-backend code, no new quadraui knob needed — the earlier rounds' claim that this needed a pixel-valued `ShellConfig::default_sidebar_width_px` upstream was wrong, and the review that pushed back on it was right. It deliberately stops *at* the shared Alt rung's floor rather than going narrower (~10 lh / ~230px would be closer to 260): `alt_resized_sidebar_width` clamps to `ALT_SIDEBAR_WIDTH_MIN..=ALT_SIDEBAR_WIDTH_MAX` on both backends, so an opening width below that floor makes the user's first Alt+Right jump discontinuously to it with no way back (measured on GTK at 10.0: painted sidebar 230px -> 345px on Alt+Right, then stuck at 345px on Alt+Left), and `compute_layout` would clamp it back up anyway. Making that floor per-unit too is a change to the shared Alt rung's cross-backend contract (#759) and wants its own issue. Coverage, fail-first per the acceptance bar: `gtk::testing`'s `explorer_double_click_opens_second_file_in_a_second_tab_1798` now runs at the reported **800x480** (it had been widened to 1600x480 in an earlier round to dodge this very bug) and is RED against unfixed `develop` — restoring `sidebar_width_lh` to `20.0` makes `tab_center(&bar, 1)` `None` because the tab never paints — plus `render::unit_profiles_scale_the_sidebar_width_per_unit_1798` pinning the three invariants, and the `shell_config` assertions in `src/app.rs`. Tab-bar assertions are geometrically scoped via a new `painted_label_is_in_tab_slot` helper (the matched run's rect must fall inside that tab's own slot in the cached `TabBarLayout`), with a negative control asserting the helper rejects the Explorer row's bare `"main.rs"` run — a bare `screen_contains` would pass under the reported bug. The two TUI twins are green both before and after, correctly: the `cell` profile was never mis-scaled, so they cover the shared dispatch path and guard against the fix narrowing the TUI sidebar as a side effect. Full `cargo test` green (4299 passed); the narrower GUI sidebar required no fixture updates at 15 lh.

**Last updated:** October 5, 2026 (#1797 — bugbash:win-native "Source Control panel CHANGES section never lists modified/untracked files"). **Fixed.** Root cause: `Engine::startup_inner`'s file-opening branch (as opposed to the folder-opening branch, which `open_folder` already repoints) never touched `cwd`/`workspace_root` at all, so `cwd` stayed whatever the process's actual working directory happened to be at `Engine::new()` time — correct by accident on a terminal launch (`cd workspace && vimcode main.rs`, since the shell already `cd`ed there first), but wrong on any native-GUI launch that hands over an absolute file path without first changing the process's directory (a desktop shortcut with no "Start in" folder, a file-association "Open with" launch). With `cwd` pointing somewhere unrelated, `git::find_repo_root(&self.cwd)` returns `None`, `sc_refresh`'s `git status` queries the wrong directory, and the CHANGES section (which only ever reads `sc_file_statuses`) stays empty no matter how many times the panel is refreshed — exactly the report. This is shared, platform-neutral `Engine`/`core` code (no backend touches `cwd` differently), so the fix lives entirely in `src/core/engine/mod.rs`: a new `adopt_cwd_for_startup_file` resolves the file's own git repo root (falling back to its immediate parent directory) and adopts it as `cwd`/`workspace_root`, but only when the file is not already reachable from the existing `cwd` — so the ordinary terminal-launch case (including a file nested several directories into the workspace) is untouched, and only the "file lives somewhere `cwd` has no path to" case is repointed. RED-verified: `src/core/engine/tests.rs`'s `startup_on_a_file_outside_cwd_still_finds_its_repo_for_source_control_1797` builds a real git repo (one modified tracked file, one untracked file) in one temp dir, points the engine's `cwd` at a second, unrelated temp dir, calls the real `Engine::startup_without_session_restore(Some(&file))`, and asserts `sc_section_file_count(SC_SECTION_CHANGES) == 2` — observed `0` before the fix (confirmed by reverting `adopt_cwd_for_startup_file`'s call site), `2` after.

**Review round 1 fixes (same day):** the first version's `TuiDriver` test hand-set `engine.cwd` to the file's own repo and called `sc_refresh()` directly, bypassing `adopt_cwd_for_startup_file` entirely — it could not fail against unfixed `develop`, so it was not coverage (CLAUDE.md rule 2 / #553). Rewrote it (`src/tui_main/app_on_tui_tests.rs`'s `sc_panel_changes_section_shows_status_badges_for_modified_and_untracked_files_1797`) to start `cwd` at a second, unrelated, repo-less temp dir and drive the real `startup_without_session_restore`, then to trigger the refresh by *clicking* the sync/refresh toolbar icon (`sc:sync`) through the driver — the report's own repro gesture — rather than calling `sc_refresh()` directly; widened the sidebar via the real `Alt+Right` resize chord first, since the default ~20-column sidebar truncates the toolbar down to the Commit button alone. RED-verified by hand (stubbing `adopt_cwd_for_startup_file` to a no-op fails both this test and the `core::engine::tests` one above). Added two unit tests pinning the fix's two no-op guarantees (`adopt_cwd_for_startup_file_is_a_no_op_when_the_file_is_already_under_cwd_1797`, `..._when_cwd_already_has_its_own_repo_1797` — the latter backing a new, narrower gate: `adopt_cwd_for_startup_file` is now also a no-op when `cwd` already has *any* git repo of its own, not just when the file is reachable from it, so `cd ~/myrepo && vimcode ~/.gitconfig` no longer silently re-roots to `$HOME`). Mirrored `open_folder`'s `explorer_expanded.clear()`/`insert(canonical)` into the new code path too, closing a canonicalisation-asymmetry gap `explorer_reveal_path`'s `strip_prefix` could otherwise silently fail on (Windows short paths, symlinked launch paths, macOS `/tmp` vs `/private/tmp`). Also added a `backend_conformance!` (`gtk`+`tui`+`tui_prod`) scenario in `src/harness.rs` (`sc_panel_changes_section_shows_status_badges_after_startup_cwd_fix_1797`) so the fix has cross-backend render coverage, not just TUI — GTK paints the status badge and filename as separate coloured Pango labels, so the TUI-only idiom of `screen_has("M main.rs")` doesn't transfer; added a small `badge_precedes_file` helper built on `quadraui::testing::FrameInventory`'s relational vocabulary (`left_of`/`same_row`, quadraui#490) instead. All three arms RED-verified the same way. Satisfies the issue's "Tier-1 shared conformance scenario... that fails first" acceptance bar through the one render path GTK, TUI, and win-native all share (confirmed no `src/win/` file touches `sc_*`/`source_control` at all). No GTK- or Win-specific *production* code added, per the Platform-Neutrality Rule — only tests.

**Last updated:** October 5, 2026 (#1796 — bugbash:win-native "dd leaves cursor at the previous column instead of column 0 of the line that moves up"). **Not a bug — false report, now with Tier-1 coverage closing the gap that produced it.** The issue's own oracle check placed the cursor at column 0 before `dd`, a degenerate starting point where "reset to 0" and "preserve the column, clamped" produce the same answer — it can't actually distinguish the two. Re-ran real `nvim --headless` 0.12.5 with the cursor explicitly on a non-zero column (`call cursor(2,3)` on `aaa/bbb/ccc` then `normal! dd`): buffer becomes `aaa`/`ccc`, cursor lands at `(2,2)` 0-indexed — column *preserved*, not reset. VimCode's `delete_lines` (`src/core/engine/motions.rs`) already implements this (added in #805). Real Vim 9.1 genuinely disagrees and does reset to column 0 (`'startofline'` defaults on in Vim, off in Neovim) — but this project's documented oracle is Neovim (`reference: nvim` in `tests/smoke-spec/catalogue.yaml`), so vimcode's current behaviour is correct against the oracle that matters here. Corrected the `vim-dd-deletes-line` catalogue entry, which had stated the degenerate-case result as a general rule — the likely source of the false report — and reworded the `delete_lines` comment that repeated the same conflation. Closed the mechanical gap that let this degeneracy exist in the first place: every other `op:dd` case in `tests/nvim_conformance.rs`'s oracle-backed `CASES_OP` started at column 1; added `"op:dd nonzero col 1796"` (column 3 start), oracle-verified green. Coverage, both tiers per the acceptance bar: a Tier-1 engine test (`test_nvim_dd_preserves_column_not_reset_to_zero_1796` in `src/core/engine/tests.rs`, pinning `view().cursor.{line,col}`) plus a Tier-1 `TuiDriver` test asserting the actual **painted** status bar (`dd_preserves_painted_status_bar_column_1796` in `src/tui_main/app_on_tui_tests.rs`, asserting `screen.contains("Ln 2, Col 3")`) — the issue's own evidence was a status-bar screenshot, not an engine field, so the rendered-output assertion is the one that actually answers the report. No production change in this PR; `ISSUE_RESOLUTION: investigation` — the issue stays open until a reviewer confirms the coverage closes it as "not a bug."

**Last updated:** October 5, 2026 (#1783 — bugbash:mac-native "viewport scroll position incorrectly snaps to cursor's line after multi-line insert/paste"). **Already fixed by #1779 — duplicate root cause, new regression coverage only, no production change.** #1783's own two repros (`i`/`aaa`/Enter/`bbb`/Enter/`ccc`/Escape; and `dd` then `p` on `aaa/bbb/ccc`) are the exact same `run_shared_tick_chores` mechanism #1779 fixed the same day (see that entry below) via a different trigger (Insert-mode typing and delete-then-paste, instead of #1779's `o<text><Esc>`) — the mac-native bugbash lane drives the real `App`/`AppShell` the same shared `src/app.rs`/`src/render.rs` code path every backend (TUI, GTK, and `src/macos/mod.rs`'s AppKit wrapper) shares, so #1779's fix (`RenderedWindow::visible_line_capacity` instead of `rw.lines.len()`) already covers it. Verified, not assumed: added two permanent `TuiDriver`-based tests (`src/tui_main/app_on_tui_tests.rs`'s `multiline_insert_does_not_hide_earlier_lines_1783` and `dd_then_p_does_not_hide_earlier_lines_1783`), RED-verified by hand against this file's own pre-#1779 shape (temporarily reverting `run_shared_tick_chores`'s `rw.visible_line_capacity.max(1)` back to `rw.lines.len().max(1)` reproduces both repros' exact symptom — the earlier line(s) scroll out of view), GREEN again with the fix restored (`src/render.rs` is unchanged from `develop` in this commit — test-only diff). No Tier-2 `mac-native` smoke-spec step added: `tests/smoke-spec/mac-gui.yaml`'s own header notes that driver's step vocabulary (`launch`/`key`/`click`/`wait`/`capture`/`expect_a11y`/`expect_closed`) has no pixel-content or screen-text assertion primitive at all — `capture` alone can't fail (#553's "a test that cannot fail is not coverage" rule already rules it out, same reasoning that file gives for omitting the two "All GUI specs" seed checks) — so the Tier-1 `TuiDriver` route (the issue's own stated alternative) is the one that can actually assert on rendered content here.

**Last updated:** October 5, 2026 (#1779 — TUI "line 1 vanishes" on `o` / yy+j+p when the edit grows the buffer's last line). **Fixed.** Root cause: `src/render.rs`'s `run_shared_tick_chores` fed `Engine::set_viewport_for_window` the *previous frame's painted line count* (`RenderedWindow.lines.len()`), not the window's actual row *capacity* — on a buffer shorter than the viewport (e.g. a freshly opened 1-line file), those two differ, so `view.viewport_lines` got pinned to the buffer's current length. The real TUI runner calls that tick between every input batch (including idle ones), so by the time the next keystroke grew the buffer, `Engine::ensure_cursor_visible` believed the viewport was exactly as tall as the old content and scrolled line 0 out of view to keep the cursor's new line "on screen". This *is* reachable in-process — `quadraui::tui::testing::TuiDriver::tick()` is public and routes straight to `App::tick` -> `run_shared_tick_chores`, so calling it between the initial render and the edit keystroke reproduces the bug with no pty at all (`src/tui_main/app_on_tui_tests.rs`'s `opening_a_line_below_the_last_line_paints_every_line_in_order_1779`, RED-verified pre-fix). `quadraui::tui::vt_testing::TuiVtDriver` has no equivalent public `tick()` at this repo's pinned quadraui rev, so its twin test genuinely can't be fixed the same way and stays as ANSI-diff-path evidence only. Also RED-verified end-to-end over a real Unix pty (`tests/pty_open_line_below_paints_all_lines.rs`, new). Fix: added `RenderedWindow::visible_line_capacity` (the window's real row count from `rect`/`line_height`, independent of how much buffer content exists) and pointed `run_shared_tick_chores` at it instead of `lines.len()`. Coverage: the new real-pty test (RED-verified pre-fix, GREEN post-fix), the `TuiDriver`-based in-process test above (also RED-verified pre-fix, GREEN post-fix), a `src/render.rs` unit test pinning the new field's value against `lines.len()` on a short buffer, and the `TuiVtDriver` twin kept as supplementary evidence the ANSI-diff/vt100 path isn't itself where the bug lives. No GTK twin: unlike `TuiDriver`, `quadraui::gtk::testing::GtkDriver` (at this repo's pinned rev) exposes neither a `tick()` nor a mutable backend accessor, so there is no way to invoke `App::tick` -> `run_shared_tick_chores` on it from outside the driver at all (confirmed while investigating, not assumed) — a real quadraui testing-API gap worth filing upstream (`GtkDriver::tick()`, mirroring `TuiDriver::tick()`), not a vimcode-side fix. The fix itself lives in shared `src/render.rs`, so GTK gets the production fix for free regardless.

**Last updated:** October 5, 2026 (#1762 fix iteration 1 — activity bar got
stuck on "Run and Debug" after visiting Extensions, and a misrouted
right-click launched a failing debug session). **Status: partial — see
below.** This entry was corrected in fix iteration 1 after review found two
of its original factual claims contradicted by the code/spec they cited;
read the "What is NOT demonstrated" bullet before treating this as closing
#1762.

- Confirmed real bug (independent of whether it explains #1762's reported
  run): traced to the pinned quadraui rev
  (`quadraui::dispatch::DoubleClickDetector`): `DOUBLE_CLICK_RADIUS` is 1.5
  TUI cells and the activity bar's icon rows are exactly 1.0 cell apart, so
  two genuinely distinct real clicks on *adjacent* activity-bar icons
  (Source Control then Debug, Debug then Extensions, …) landing within the
  400ms `DOUBLE_CLICK_MS` window fold into one synthesized
  `UiEvent::DoubleClick`. `quadraui::compose::app_shell::AppShell::handle`
  only has a hit-test arm for a plain `MouseDown` — every other event,
  `DoubleClick` included, falls through its own `_ => Ignored` arm — so the
  second click was silently dropped and the sidebar stayed on whatever
  panel was already active until the next real click.
- Fix (`src/app.rs`, `App::handle_dispatch`): the real fix belongs in
  quadraui (`AppShell::handle` growing a `DoubleClick` arm identical to its
  `MouseDown` one for the activity-bar band — a double-click on an
  activity-bar icon has no distinct meaning from a single click there, for
  every consumer, not just vimcode). That gap is now drafted in
  `docs/PENDING_QUADRAUI_ISSUES.md` for the coordinator to file verbatim
  (not yet filed/landed as of this commit — this repo's workers don't run
  `gh`). Until it lands, a new rung in the shared `App` (not
  `src/gtk/`/`src/tui_main/`, so both backends pick it up from one place)
  catches a `DoubleClick` landing inside the activity bar's freshly
  recomputed layout bounds, re-synthesizes it as the plain `MouseDown` it
  was always meant to be, and feeds it back through `AppShell`'s own
  public `handle()` — the exact dispatch a real single click takes — then
  through the existing `ShellApp::on_shell_event_ctx` pipeline, same as a
  real click would. This also changes same-icon double-click behaviour on
  every backend (GTK/mac too, not just TUI) — see that comment block and
  `src/gtk/testing.rs`'s
  `activity_bar_double_click_on_active_icon_reopens_sidebar_via_gtk_driver`.
- Test (Tier-1): `tui_main::app_on_tui_tests::tests::activity_bar::
  activity_bar_adjacent_clicks_are_not_dropped_by_double_click_fold_1762`
  — drives the same six rows `tests/smoke-spec/tui.yaml`'s activity-bar
  section clicks, in order, but with **no simulated time between clicks**
  (the worst case for the 400ms fold window) to isolate the adjacency-fold
  bug on its own; it is not a timing-faithful replay of the spec (the
  spec's own `wait_idle` pacing is ≥500ms between clicks, over the 400ms
  fold window). RED-verified by hand with the `App::handle_dispatch` rung
  reverted: only the *first* assertion that exercises the fold ("clicking
  Source Control right after Debug") is ever observed to fail — `assert!`
  panics there, so later assertions in the same test are never reached in
  that run. It fails finding "RUN AND DEBUG" where "SOURCE CONTROL" was
  expected, confirming the click was dropped.
- GTK coverage (non-blocking review finding): `src/gtk/testing.rs`'s
  `activity_bar_double_click_on_active_icon_reopens_sidebar_via_gtk_driver`
  — the fix is shared code, so it changes behaviour on GTK too (a genuine
  double-click on an already-active icon now re-shows the sidebar instead
  of leaving it hidden). RED-verified by hand the same way.
- **What is NOT demonstrated: that this fold is #1762's reported
  mechanism.** Two claims in the original version of this entry did not
  survive a check against the code/spec they cited: (1) the pacing claim —
  `tui.yaml`'s own activity-bar steps pace every click with a confirmed
  `expect_within` *and then* a 500ms `wait_idle`, comfortably over the
  400ms fold window, so the reported lane should not hit this fold under
  normal timing; no measurement to the contrary is offered. (2) the
  symptom claim — the issue's own
  `activity-bar-explorer-reselect-1636` failure is row 5 (Extensions) to
  row 1 (Explorer), a 4.0-cell gap far outside the 1.5-cell radius, and
  `DoubleClickDetector::process` resets `last_click_time` to `None` the
  instant it folds a pair, so the click immediately following any fold is
  never itself eligible to be folded — under no timing can that specific
  re-click be dropped by this mechanism. #1762 therefore plausibly still
  reproduces after this fix; something else is holding the sidebar on
  Run&Debug in the reported run, not yet identified. The second half of
  the issue title (the misrouted right-click launching a failing debug
  session) is a pure downstream consequence of whichever cause turns out
  to be real, so it is equally unconfirmed by this fix.
- Filed in `docs/PENDING_QUADRAUI_ISSUES.md`, not yet filed on GitHub: the
  quadraui-side issue (`AppShell::handle`'s missing `DoubleClick` arm) —
  this session cannot run `gh`; the coordinator should file it against
  `JDonaghy/quadraui` using that entry verbatim.

**Last updated:** October 4, 2026 (#1761 — the automatic startup
extension-registry refresh no longer breaks the idle-silence guarantee):

- Root cause: `Engine::startup_inner`'s unconditional, ambient
  `ext_refresh()` call at launch always surfaced a
  `"Extension registry updated (N extensions)"` status message (and the
  repaint that comes with it) whenever its background fetch completed —
  and that completion time is bounded only by `registry::fetch_registry`'s
  `curl --max-time 15`, i.e. anywhere from under a second to ~15s after
  the first frame paints, entirely outside the user's control. #1702/
  #1737/#1741 had already traced this exact mechanism and worked around
  it with a wider Tier-2 YAML settle margin (`tests/smoke-spec/tui.yaml`'s
  `settle-ext-registry-fetch-1741`), but a real-pty bugbash run still
  caught it: a fixed settle margin cannot bound an unbounded network
  delay.
- Fix (`src/core/engine/lsp_ops.rs`): split `ext_refresh` into a shared
  `ext_refresh_inner(quiet: bool)`, with the public `ext_refresh()` (every
  explicit, user-initiated call site — Extensions panel open/refresh,
  `:ExtRefresh`) keeping its status message, and a new `pub(crate)
  ext_refresh_quiet()` — used only by `Engine::startup_inner`'s automatic
  call — that never touches `self.message` on either the success or
  failure branch, and reports "no redraw needed" to `poll_idle`. New
  `Engine::ext_registry_quiet` field (`src/core/engine/mod.rs`) threads the
  quiet/non-quiet flag through the async fetch's `mpsc` channel round
  trip.
- Test:
  `tui_main::app_on_tui_tests::tests::quiet_startup_registry_refresh_1761::startup_registry_refresh_never_shows_a_message_or_forces_a_repaint`
  (`src/tui_main/app_on_tui_tests.rs`) — Tier-1, drives the
  real `Engine::ext_refresh_quiet()` production entry point (not a
  hand-rolled channel) through a `TuiDriver`, asserts the status message
  never paints and no repaint fires across a 2s poll window, and confirms
  the fetch genuinely completed (not vacuously silent because it never
  ran). RED-verified against pre-fix behavior (manually reverting
  `ext_refresh_quiet` to the old unconditional-message path reproduces
  the exact failure the bugbash found).
- Manually reproduced the pre-fix bug directly against the real compiled
  `vcd` binary under a `tmux` pty with a fresh `$HOME` (no
  `registry_cache.json`) before writing the fix, to ground-truth the
  mechanism rather than relying on the existing (already deeply-explored)
  theories in #1702/#1737/#1741's own comments.
- Not touched: `tests/smoke-spec/tui.yaml`'s existing `wait_idle`/
  `expect_silent` settle margins from #1741/#1737 — they're now
  redundant insurance (the message they were waiting out no longer
  fires) rather than wrong, and removing them wasn't necessary to satisfy
  this issue's acceptance bar (a new Tier-1 regression test).
- Review fix-iteration 1: `ext_refresh_inner`'s "already in progress" dedupe
  (`src/core/engine/lsp_ops.rs`) now upgrades `ext_registry_quiet` to
  `false` when a non-quiet caller dedupes against an in-flight quiet fetch
  — otherwise a user's explicit `r` refresh pressed during the startup
  fetch's window silently inherited startup's silence policy and produced
  no feedback at all. `poll_ext_registry`'s redraw verdict is now
  `!quiet || self.active_panel_is(PANEL_EXTENSIONS)` — a quiet fetch
  landing while the Extensions panel happens to be open still redraws, since
  `sidebar.rs`'s panel-open handler deliberately doesn't re-arm its own
  refresh when one is already in flight and relies on this fetch's own
  completion to paint the list. Four new unit tests
  (`core::engine::lsp_ops::tests`) cover both fixes directly against
  `ext_refresh`/`ext_refresh_quiet`/`poll_ext_registry` (RED-verified
  against the pre-fix code). A second Tier-1 test,
  `tui_main::app_on_tui_tests::tests::quiet_startup_registry_refresh_1761::public_startup_entry_point_uses_the_quiet_refresh`,
  drives the real public `Engine::startup()` entry point (not just
  `ext_refresh_quiet()` directly) so a regression at `startup_inner`'s own
  call site is caught black-box too — RED-verified the same way.
- Review round 2: the four new unit tests drive the real
  `poll_ext_registry()`, whose success branch calls `registry::save_cache`
  unconditionally, so their shared `poll_with` helper now holds a
  `core::paths::TestHomeGuard` across that call (and asserts the cache
  landed under the temp home). Without it `cargo test` truncated the
  developer's real `~/.config/vimcode/registry_cache.json` to `[]`, which
  is sticky: `load_cache()` then returns `Some([])`, defeating
  `sidebar.rs`'s `ext_registry.is_none()` guard so the Extensions panel
  paints empty until a manual refresh. Same convention as
  `lsp_ops.rs`'s existing guard use and the #1741 review fix.
  The dedupe fix also gained a driver-tier guard that asserts on *painted*
  output rather than the `ext_registry_quiet` flag:
  `tui_main::app_on_tui_tests::tests::quiet_startup_registry_refresh_1761::explicit_refresh_during_the_quiet_startup_fetch_still_paints_its_message`
  (RED-verified: fails with the dedupe upgrade reverted).

**Last updated:** October 4, 2026 (#1760 — status bar drops the Ln/Col
cursor-position segment once other optional segments compete for width).
Root cause: `build_window_status_line` (`src/render.rs`, shared by both
backends) pushed `cursor_seg` (`Ln N, Col N`) **first** into the window
status bar's `right_segments`, to match VS Code's own left-to-right visual
order (#1690). quadraui's `StatusBar::layout`/`fit_right_start`
priority-drop removes `right_segments` from the *front* of the vector and
always preserves only the *last* one — so pushing the ruler first made it
the first segment *dropped* the moment a dirty marker (`[+]`), a git
branch, or VS Code mode's `EDIT  F1:cmd  Alt-M:vim` hint ate into the left
side's width budget, while lower-value segments (layout toggles, LSP
status) survived instead.

- Fix: `cursor_seg` is now pushed **last** into `right`, unconditionally
  the bar's right-most segment, which is the only position quadraui's
  "always keep the last right segment" rule can guarantee survives any
  priority-drop. The rest of `right`'s push order was re-ranked
  least-important-first (LSP status, notifications, layout toggles, then
  VS Code's filetype/line-ending/encoding/indent cluster, then `showcmd`)
  so something sensible is sacrificed before the ruler ever could be.
  `sidebar_toggle_seg`'s text regained its trailing space (quadraui#1155)
  since it's no longer the bar's default right-most segment.
- This is a genuine quadraui `StatusBar` primitive limitation, not a
  per-backend bug: a flat `right_segments` vector conflates visual
  left-to-right position with drop priority, so VS Code's own visual
  order (ruler leftmost of the right cluster) and "never disappears"
  (ruler must be last) cannot both be satisfied with the primitive's
  current shape — #164 had already flagged this coupling as unresolved.
  The existing #1690 GTK test asserting the old visual order was updated
  to assert the new order instead (language → LF → UTF-8 → Spaces →
  Ln/Col), and two render.rs unit tests (`test_status_bar_toggle_and_
  bell_glyphs_come_from_icons_rs_constants`, `test_window_status_line_
  right_most_segment_has_no_trailing_space`) were updated for the same
  reason.
- Tests: new Tier-1 TuiDriver test `status_bar_1760_keeps_cursor_
  position_once_other_segments_compete` (`src/tui_main/app_on_tui_tests.
  rs`) — dirty marker + git branch + VS Code EDIT hint all competing on an
  80-column bar, asserting `Ln 1,` still paints. Verified RED against
  unfixed `develop` (reverting just the `cursor_seg` push-order hunk and
  re-running prints a status row with no `Ln ` anywhere).

**Last updated:** October 3, 2026 (#1719, partial — detect extension
prerequisites before install, for LSP and DAP). Fixes the half of #1719
that lives in this repo without touching the registry (vimcode-ext):

- `src/core/engine/lsp_ops.rs`'s `ext_install_from_registry_with_runtime_
  check` now checks `manifest.lsp.dependencies` **before** dispatching the
  legacy terminal-install command (previously only checked at LSP
  server-start time, well after the install pane had already failed with
  a bare `command not found: npm`), and likewise for the new
  `manifest.dap.dependencies` (`DapConfig`, `src/core/extensions.rs` — did
  not exist before this issue) before the DAP leg's install.
- `src/core/dap_manager.rs`'s new `adapter_dependencies(adapter, platform)`
  declares the built-in codelldb/debugpy/delve/netcoredbg installers' own
  prerequisites (curl+unzip, python3, go, curl+tar respectively — fewer on
  Windows, which uses PowerShell's built-in `Expand-Archive` instead of
  `unzip`/`tar`) — these ship inside vimcode itself, so nothing in the
  registry could ever have declared them. Checked only when the manifest
  doesn't override the install command itself (a manifest-declared
  `dap.dependencies` always wins over the built-in guess).
- `PREREQ_INSTALLS` (`src/core/extensions.rs`) gained `brew` (the Homebrew
  installer one-liner — several registry manifests' `install_macos` shells
  to `brew install …` with nothing declaring Homebrew itself), `java`
  (JDK), `curl`, `unzip`, `tar`. `go`'s Linux hint changed from `sudo apt
  install golang-go` (Ubuntu 24.04 ships 1.22, too old for `gopls@latest`,
  and pins `GOTOOLCHAIN=local`) to `sudo snap install go --classic`.
- New headless CLI entry point: `vimcode --ext-install <name> [--json]`
  (`src/main.rs`) — runs the same prerequisite check with no GUI/TUI
  backend, reports a JSON or plain-text verdict, exits non-zero and
  dispatches **no** install when a prerequisite is missing. Exists so an
  external driver (vimcode-ext#17's CI matrix) can exercise vimcode's real
  detect/instruct logic instead of re-implementing it.
- Tests: `every_prereq_install_entry_is_runnable_on_every_platform` +
  `prereq_install_cmd_covers_the_1719_additions` (`extensions.rs`),
  `adapter_dependencies_*` (`dap_manager.rs`), five new engine-level tests
  in `lsp_ops.rs` (`lsp_legacy_install_blocked_when_declared_dependency_
  missing` and four DAP siblings), `ext_install_flag_and_value_parse_and_
  json_is_recognised` + `ext_install_value_is_not_mistaken_for_the_file_
  path` (`main.rs`), and the black-box `tests/ext_install_cli.rs` (spawns
  the real compiled binary with a throwaway `$HOME`/local extension
  manifest and a `PATH` that can't resolve `npm`). All RED-verified against
  the pre-fix code.
- **Review fix (2026-10-03):** the engine-internal-state tests above were
  flagged as necessary-but-not-sufficient per CLAUDE.md's black-box
  coverage rule (the same gap #1346's review caught for the closely
  analogous "missing runtime" fallback in this same function). Added
  driver-tier coverage for both backends, asserting on rendered output
  instead of engine state: `tui_main::app_on_tui_tests::tests::issue_1719_
  prerequisite_detect_before_install` (`TuiDriver`, two tests — the LSP
  legacy leg and the built-in delve DAP leg) and its GTK twin
  `gtk::testing::issue_1719_missing_prereq_blocks_install` (`GtkDriver`,
  same two scenarios). Both RED-verified against a reintroduced regression
  (`if true || missing.is_empty()` in `lsp_ops.rs`'s legacy-install
  branch) before being restored to pass against the real fix.

**Not done (see `docs/PENDING_VIMCODE_ISSUES.md`'s "#1719 remainder"
entry for the full writeup):** no minimum-version checking (a *present but
too old* `go`/etc. still passes detection); the headless entry point
checks prerequisites and runs the install but does not yet speak LSP/DAP
`initialize` to confirm "working" (contract item 3) — left to
vimcode-ext#17's own matrix, which has the per-language client fixtures
for it. The npm-global-prefix and dotnet/csharp-ls version-pin issues the
GitHub issue also names are explicitly vimcode-ext's own fixes (#14/#15),
not this repo's.

**Last updated:** September 24, 2026 (#523, Track A Phase 0b — wire Board
actions to provider-declared commands, on top of #521/#522's generic Board
host and #524's document buffers). Makes the board actionable: right-click
(or a provider-declared stage keybinding) runs a provider-declared named
action against a card, with confirmation for irreversible/metered actions
and results surfaced to the status line — all through the #522 seam, with
**no coordinator vocabulary anywhere in `src/core/`**
(`tests/no_coord_vocabulary_in_core.rs` still passes).

`src/core/extensions.rs`: `BoardProviderConfig::actions` changed from
`HashMap<String, Vec<String>>` (unused in production — #521/#522 shipped it
as the seam, #523 is the first consumer) to `Vec<BoardActionDef>` — each
entry names an action, an argv `command` (`{id}` substituted), the `stages`
(column ids) it's valid in (empty = every stage), an optional single-key
`key` binding, and a `confirm` flag. New `BoardProviderConfig::action_by_
name`/`actions_for_stage`/`action_for_key` lookups replace the old
`action_argv`. Also gained opt-in freshness: `tick_command`/
`tick_interval_secs` — a fire-and-forget nudge for a daemon-less provider's
pipeline, run only when `Settings::board_tick_enabled` (new, **default
off** — a passive viewer must not silently dispatch metered work) is set.

`src/core/engine/board_ops.rs`: `open_board_context_menu(card_id, x, y)`
lists the provider's declared actions valid for the card's current stage
(its column) through the *existing* generic `Engine::context_menu`/
`ContextMenuState` machinery already used by Explorer/Tab/Editor — a new
`ContextMenuTarget::Board { card_id }` variant, no bespoke menu widget.
`run_board_action_by_name(name, card_id)` resolves the argv and either
dispatches immediately (`dispatch_board_action_command` — background
thread via the same `ToolClient` seam `board_refresh` uses,
`poll_board_action` surfaces exit status/stdout to `Engine::message` and
triggers a fresh `board_refresh`) or, when the provider marked the action
`confirm`, opens a Yes/No dialog first via the existing generic
`show_dialog`/`process_dialog_result` machinery (new `"confirm_board_
action"` tag, new `PendingBoardAction` engine field) — reusing established
infra end-to-end rather than building a parallel one, per the Platform-
Neutrality Rule. `dispatch_board_key_unified` checks a provider-declared
stage keybinding (the coord-tui-parity `P`/`S`/`F` style) *before* falling
to `quadraui::BoardModel::handle_key`'s generic nav, so a provider is free
to bind letters `handle_key` doesn't already claim. `OpenIssue`/
`OpenReview` (`quadraui::BoardAction` variants) now fall back to a
provider-declared action of the same name when no more specific handling
applies (a `[document]` provider still wins for `OpenIssue`, #524) —
`apply_board_action`'s `ContextMenu` arm stays a no-op, since nothing in
quadraui's `Board` primitive constructs that variant itself (no mouse
handler on `BoardModel`, unlike `TreeController`); right-click resolves the
card straight from the cached `BoardLayout` at the click site instead.
`tick_board_provider_freshness` (new, called unconditionally from
`poll_idle`, *not* gated on the Board panel being visible — the opposite of
`tick_board`'s read-refresh cadence) fires `tick_command` on its own
interval when enabled.

**Backends** — GTK's `App::route_board_sidebar_event` and TUI's
`mouse::handle_mouse` (`SidebarOwner::Board` right-click arm) both do the
same 1-3 lines: resolve the right-clicked card via new `render::
board_right_click_card` (mirrors `route_board_click`'s hit-test), convert
pixel→cell (GTK only — TUI's board paints in cell units already, same
asymmetry `handle_tab_right_click`/`handle_editor_right_click` already
have), call `Engine::open_board_context_menu`. Confirming a menu item
(Enter, or a click) reuses the *existing* generic `route_modal_key`/
`ContextMenuRoute` machinery unchanged — no new backend wiring needed for
that half, since `ContextMenuTarget::Board`'s dispatch is entirely engine-
side (`context_menu_confirm`'s new match arm in `windows.rs`).

**Settings**: new `board_tick_enabled: bool` (`Settings`, default off, no
vim-abbreviation precedent) wired through `get_value_str`/`set_value_str`
and a new `SettingDef` (Workspace category, "Board Auto-Tick") so it's
toggleable from the Settings sidebar or `:set board_tick_enabled=true`.

**Tests**: extensive `board_ops.rs` unit coverage (context-menu contents
filtered by stage, dispatch + confirm + cancel + result-surfacing, stage
keybinding dispatch falling back to nav on an unbound key, `OpenIssue`/
`OpenReview` provider-action fallback, freshness-tick default-off/enabled/
interval-respecting/no-tick-command cases) using the new `RecordingToolClient`-
based assertions (argv sent, not just "something ran"). Driver-tier black-
box coverage on both backends — TUI (`tui_main/shell_app.rs`, `TuiDriver`:
`board_right_click_opens_context_menu_with_provider_actions_via_shell_app`,
`board_context_menu_action_dispatches_provider_command_via_shell_app`, the
latter using `driver.tick()`/`poll_until_screen` to reach `Engine::poll_
board_action` exactly as the live loop does) and GTK (`gtk/testing.rs`,
`GtkDriver`: same two scenarios, polling `Engine::poll_board_action`
directly since the harness keeps a live `Rc<RefCell<Engine>>`) — a real
right-click through the actual event-dispatch pipeline opens the menu
(asserted on painted text, not `Engine::context_menu.is_some()`,
#587/#592's lesson) and a real Enter-confirm dispatches the provider's argv
end-to-end with a mock provider, proving no coordinator is needed to
exercise the path (#523's acceptance bar). All four new driver tests
RED-verified (temporarily short-circuited `open_board_context_menu`,
confirmed both fail, restored). `cargo build`/`clippy -D warnings`/`fmt`
clean on both feature lanes.

Out of scope (per the issue): the coordinator extension bundle itself (the
parity-matrix mapping of `assign`/`test`/`pr`/`merge`/`backlog` to real
`coord` argv) — this ships the generic dispatch mechanism only, provable
end-to-end with a mock provider.

Rebased onto #525/#528/#530 (Track A Phases 2/3/5 landed on `develop` while
this branch was open and rewrote `board_ops.rs` around the same
`"OpenReview"` action name). Both features survive, with one deliberate
precedence rule now pinned by tests: **`"OpenReview"` is the one
provider-declared action that does *not* go through #523's generic
fire-and-forget dispatcher.** Its command's stdout is a review *target* to
resolve into a local diff (`Engine::open_review_card` ->
`fetch_branch_review_target` -> `open_branch_review`), so routing it through
`run_board_action_by_name` would silently downgrade "open the review" to
"print that JSON on the status line". For the same reason the host's `R`
keybinding is checked *before* provider-declared stage keybindings, so a
provider cannot rebind `R` out from under the review. #525's
`action_argv(name, id)` helper folded into #523's richer
`action_by_name(name)` + `BoardActionDef::resolve_argv(id)` (the `actions`
field is a `Vec<BoardActionDef>` now, not a `HashMap<String, Vec<String>>`).
RED-verified: reinstating the generic dispatch for `OpenReview` turns
`board_review_key_paints_branch_diff_via_shell_app` (driver-tier),
`apply_open_review_resolves_a_target_instead_of_dispatching_the_action` and
two of #525's engine tests red; restored, all green.

**Last updated:** September 24, 2026 (#530, Track A Phase 5 — the fleet
review seat). "vimcode-over-ssh" needs **no vimcode-side ssh/transport
code at all** — the moat is that vimcode already works correctly when it
is the process running (via a plain `ssh <host>`) on a worker's box, in
that worktree's own checkout, so #525's `Engine::open_branch_review`
already handles the "review-where-the-code-is" mode unmodified: both it
and "pull-local" (#525's original default) resolve `target.branch`/
`.base` as local git revisions either way. The only real gap was
**provenance** — "which worktree" (already shown by the #528 footer) stops
uniquely identifying "which checkout" once vimcode itself can *be* the
process on a different machine. `src/core/tool_client.rs::
BranchReviewTarget` gained `host: Option<String>` (`#[serde(default)]` —
every #525-era provider response still parses unchanged), a purely
descriptive label a fleet provider's roster fills in per card/assignment
(picking which machine+assignment to review is just picking a board card,
same as #525 — no new selection UI needed, since the provider already
decides what `host` a given card resolves to). `render::
branch_review_provenance_segment` now paints `"reviewing branch 'X' on
'<host>' in <root>"` when `target.host` is `Some`, falling back to the
pre-#530 `"reviewing branch 'X' in <root>"` line when it's `None` (a
single-checkout provider, or a locally-pulled branch). New
`board_review_footer_paints_host_provenance_via_shell_app` in
`src/tui_main/shell_app.rs` (`TuiDriver`, real temp git repo, mock
provider returning `{"branch", "base", "host"}`), RED-verified by
temporarily reverting the segment to its single pre-#530 format string and
confirming the assertion fails, then restoring it. New
`mock_client_fetch_branch_review_target_parses_host_when_present` in
`tool_client.rs` covers the parse side, plus a same-module assertion that
a host-less fixture still parses `host: None` (the `#[serde(default)]`
compatibility guarantee). `finalize_review_edits` (#528) needed **no
change** — a plain `git push` from wherever the worktree physically is
already *is* "the provider's existing remote path" the acceptance bar
asks for, regardless of mode. No ACP/Node dependency anywhere in this
diff (Track B stays untouched, per the issue's explicit warning). `cargo
build`/`clippy -D warnings`/`fmt` clean on both feature lanes;
`no_coord_vocabulary_in_core` still passes (an early doc-comment draft
that named `coord pull` was caught by it and reworded to "an external
'pull the branch locally first' flow" — the mechanism this issue's core
code has no business knowing the name of).

**Last updated:** September 24, 2026 (#528, Track A Phase 3 — push human
review edits back to the branch). A review worktree opened via `Engine::
open_branch_review` (#525) is a real checkout: nothing in this scope was
needed to make editing its files work (the editor already edits any file
on disk), so this issue's actual gap was making that safe and legible.
New `Engine::review_target: Option<BranchReviewTarget>` (`mod.rs`) records
which branch/base a branch review resolved, set by `open_branch_review`
and left in place after the diff surface itself closes — the human keeps
editing files after `Esc`, and that's exactly when "which branch is this"
matters most. New `Engine::finalize_review_edits(message)`
(`review_ops.rs`), reachable as `:GFinalize [message]` (`execute.rs` ->
`cmd_git_finalize_review`, `buffers.rs`, following the existing `:Gcommit`/
`:Gpush` pattern): stages+commits any working-tree changes, then a plain
(non-force) `git push` — modelled on coordinator's remote-fix `finalize`,
so a rejected push (real non-fast-forward tested against a bare remote)
never loses the commit or the worktree, just returns an `Err` the human
can retry after resolving. Safety check: refuses to finalize if the
worktree has since been `git checkout`ed off `review_target.branch` (the
"must know which branch you're editing" footgun the issue names as the
main risk) — verified with real `git worktree`-adjacent temp repos, not
mocked git calls. Provenance in the UI: `render::paint_change_review_rung`
now paints a right-aligned "reviewing branch '<branch>' in <root>" footer
segment (shared `render.rs`, both backends, `None` for an ACP-fed review
with no `review_target`) — new `board_review_footer_paints_branch_
provenance_via_shell_app` in `src/tui_main/shell_app.rs`, RED-verified by
reverting the segment to `right_segments: vec![]` and confirming the
assertion fails before restoring it. Three new real-git tests in
`review_ops.rs` (happy path against a bare remote, wrong-branch refusal,
rejected-push commit preservation) plus a `:GFinalize` wiring test
asserting the typed commit message lands at the pushed remote ref, not
just "some push happened". `cargo build`/`clippy -D warnings`/`fmt` clean
on both feature lanes; `no_coord_vocabulary_in_core` still passes (the
git plumbing this issue adds — commit/push/branch-refusal — is exactly as
generic as the `:G*` commands it's modelled on, no coordinator vocabulary
anywhere in `src/core/` or `src/render.rs`).

**Last updated:** September 24, 2026 (#526, Track A Phase 2 — review verdict
round-trip through a provider command). Closes the loop #525 opened: from an
open change-review surface, `A`/`C`/`M` report Approve/Request-changes/
Comment-only verdicts through a provider-declared command, with **no
coordinator vocabulary anywhere in `src/core/`** —
`tests/no_coord_vocabulary_in_core.rs` still passes.
`src/core/extensions.rs`'s `BoardProviderConfig` gained
`verdict_commands: HashMap<String, Vec<String>>` (keyed by the new
`crate::core::review::ReviewVerdict::token()` — `"approve"`/
`"request-changes"`/`"comment"`, generic code-review vocabulary, not any
specific pipeline tool's) plus `verdict_argv`, substituting `{id}` *and*
`{body_file}` — the body is **never** inline, matching `DocumentProviderConfig
::write_command`'s existing stdin-not-argv precedent for the same "review
bodies contain newlines/code fences/quotes" reason. New
`crate::core::tool_client::write_review_body_temp_file` writes the composed
body to a fresh temp file before the provider command runs, so a failing
command still leaves the body on disk *and* untouched in the still-open
buffer — `save_review_verdict_buffer`'s own tests exercise exactly that
"a failed verdict command preserves the composed body" acceptance bar.
Composing a verdict reuses #524's own "open a real markdown scratch buffer,
`:w` pushes it through a provider command" shape verbatim (new
`crate::core::buffer_manager::ReviewVerdictBinding`, new
`src/core/engine/review_verdict_ops.rs` mirroring `document_ops.rs`
structurally) rather than a bespoke text-input widget — the same vim editing
(undo, search, paste) authoring an issue already gets is now available for
composing a review. New `Engine::review_card_id` tracks which board card
(if any) the currently-open `change_review` surface was opened for — always
reset to `None` inside `open_change_review` and set back by
`Engine::open_review_card` (`board_ops.rs`) right after a successful
`open_branch_review`, so an ACP tool-call diff never inherits a stale id
from an earlier board review and has nothing to report a verdict against.
New keys on the change-review surface (`handle_change_review_key`,
`review_ops.rs`): `A` (Approve), `C` (Request changes), `M` (Comment-only) —
each hands off to `Engine::start_review_verdict`, which closes the diff
surface and opens the composer buffer; a verdict with no reviewed card
behind the surface, no board provider configured, or no command declared
for that verdict degrades to a status message rather than a panic, the same
precedent `board_ops.rs`'s `open_review_card` already set for #525. Driver
coverage: `change_review_shift_a_paints_a_verdict_composer_tab_via_shell_app`
in `src/tui_main/shell_app.rs` (`#[cfg(test)]`, `TuiDriver` via
`driver_with_shell`) drives the real `A` keypress against a mock `[board]`
provider's `verdict_commands`, asserting on the *rendered* screen (the diff
content disappearing, the new tab's name painting) — RED-verified by
temporarily disabling the `'A'` match arm and confirming the test failed,
then restored. No GTK-specific driver test: the whole feature is core key
handling + the same generic scratch-buffer/tab rendering `document_ops.rs`'s
buffers already exercise on GTK, zero backend-specific code added or
changed (mirrors #525's own review-accepted precedent of a TUI-only driver
test for its analogous `OpenReview`-key coverage). `cargo build`/
`clippy -D warnings`/`fmt` clean on both feature lanes; `review_ops::`/
`review_verdict_ops::`/`board_ops::`/`document_ops::`/`extensions::`/
`tool_client::`/`buffer_manager::`/the new shell_app driver test/
`no_coord_vocabulary_in_core` all pass.

**Last updated:** September 24, 2026 (#525 review fixes, iteration 1).
Addressed the review's blocking + non-blocking findings on top of the
Track A Phase 2 work below:

- **Blocking — driver-tier black-box test.** New
  `board_review_key_paints_branch_diff_via_shell_app` in
  `src/tui_main/shell_app.rs` (`#[cfg(test)]`, `TuiDriver` via
  `driver_with_shell`) drives the real end-to-end path a user triggers:
  types `R` on a selected board card with a mock `[board]` provider's
  `"OpenReview"` action configured, against a **real temp git repo** with
  a `base` commit and a `feature` branch, and asserts the reviewed
  branch's new file name is painted on the rendered screen — not on
  `Engine::change_review` being `Some`. RED-verified by temporarily
  disabling the `R` key match in `dispatch_board_key_unified` and
  confirming the test fails, then restoring it. This is the one touch to
  `src/tui_main/` the original PR said it made none of — it is test-only,
  the shared change-review rendering itself is still #955's, and no new
  per-backend diff-drawing code was added.
- **Non-blocking — flag-smuggling guard.** `git::changed_files_between`
  now rejects a `base`/`head` starting with `-` before joining them into
  the single `base...head` positional git argv token, since both values
  originate from an external provider's JSON. New
  `test_changed_files_between_rejects_flag_like_revisions`.
- **Non-blocking — "no changes" vs. "git failure".**
  `changed_files_between` now returns `Option<Vec<String>>`: `None` for
  any git failure (unknown ref, no repo, or the flag-smuggling guard
  above), `Some(vec![])` for a genuine empty diff. `Engine::
  open_branch_review` reports a distinct "could not diff ... check the
  branch/base names the provider returned" message for the `None` case
  instead of reusing the "no changes between X and Y" message a
  misconfigured provider would otherwise share with a real no-op review.
  Existing `test_changed_files_between_unknown_ref_is_empty_not_error`
  renamed/updated to assert `None`.
- **Nit — test helper duplication.** `board_ops.rs`'s
  `install_mock_provider_with_review_action` now builds on
  `install_mock_provider` (layering the `OpenReview` action + response
  swap) instead of re-deriving the manifest/`extension_state`/
  `ext_registry` wiring from scratch.

`cargo build`/`clippy -D warnings`/`fmt` clean on both feature lanes;
`board_ops::`/`review_ops::`/`git::tests::test_changed_files_between*`/the
new shell_app driver test/`no_coord_vocabulary_in_core` all pass.

**Last updated:** September 24, 2026 (#525, Track A Phase 2 — in-editor
diff review of a work branch, consuming the change-review surface #955
built rather than building a second one). `BoardAction::OpenReview` on a
board card now resolves "card -> branch -> changed files" and opens a real
multi-file diff, entirely through generic seams — no coordinator
vocabulary anywhere, `tests/no_coord_vocabulary_in_core.rs` still passes.
New `src/core/tool_client.rs::BranchReviewTarget{branch, base}` +
`fetch_branch_review_target` — same "generic contract, provider supplies
the JSON" pattern as `ToolDocument`/`BoardModel`: any provider whose
`"OpenReview"` board action (`BoardProviderConfig::actions`, already
generic since #522) emits `{"branch", "base"}` on stdout gets a review.
New `src/core/git.rs::changed_files_between(dir, base, head)` — `git diff
--name-only base...head` (three-dot, so a commit landing on `base` after
`head` diverged never shows up as a spurious change). New
`Engine::open_branch_review` (`review_ops.rs`) is the git feeder for
#955's `core::review::ChangeReviewState`: turns `changed_files_between`'s
paths into `ProposedChange{path, old_text, new_text}` via
`git::show_file_at_ref` at both revisions (`old_text: None` when the path
doesn't exist at `base` — a new file, matching `ProposedChange`'s existing
"pure addition" convention) and calls the same `open_change_review` #955's
ACP feeder calls — proof the surface really is source-agnostic, one layer
up from `core::review`'s own non-ACP unit test. `board_ops.rs`'s
`apply_board_action` now resolves `OpenReview(id)` the same way `OpenIssue`
already resolves to a pending action outside the `board_model` borrow,
dispatching to new `Engine::open_review_card` (blocking, one-shot, same
tradeoff `open_tool_document` already made) which runs the provider's
`"OpenReview"` action via the existing `board_client` `ToolClient`,
degrading to a status-line message (no provider / no review command
configured / provider failed) rather than a panic on every failure path.
New host-level `R` keybinding in `dispatch_board_key_unified` opens the
review for the selected card — dispatched here rather than added to
quadraui's generic `BoardModel::handle_key` keymap, since that method's
own doc explicitly calls "review" out as a workflow-specific verb hosts
should handle themselves. Everything downstream (hunk nav `]`/`[`, file
nav `n`/`p`/Tab, click/Return-to-jump opening a real buffer with real LSP
diagnostics and git blame, accept/reject) is reused verbatim from #955 —
no new per-backend diff drawing, no touches to `src/gtk/` or
`src/tui_main/` at all. Tests: two real-git-repo tests for
`changed_files_between` (including the three-dot-semantics case), two for
`fetch_branch_review_target`, three `open_branch_review` tests against a
real temp repo (happy path, no-changes error, no-workspace error), and six
`board_ops.rs` tests covering the full `OpenReview` wiring (happy path
through a mock provider + real repo, no provider, no review command
configured, failing provider, and the `R` keybinding with/without a
selection). `cargo build`/`clippy -D warnings`/`fmt` clean on both feature
lanes.

**Last updated:** September 24, 2026 (#524, Track A Phase 1 — provider
document buffers, on top of #521/#522's generic Board host). Author/refine
a provider's documents (e.g. a GitHub issue) as real markdown buffers,
push edits back on `:w`, with **no coordinator vocabulary anywhere in
`src/core/`** — `tests/no_coord_vocabulary_in_core.rs` still passes.
`src/core/extensions.rs` gained `ExtensionManifest::document:
Option<DocumentProviderConfig>` — `read_command`/`write_command`/
`write_follow_up` argv templates (same `{id}` substitution convention as
`BoardProviderConfig::actions`), with `write_command` doing double duty as
"create" when `{id}` substitutes to the empty string (the new-document
flow — no separate create command). `src/core/tool_client.rs` (#522's
seam) grew a second half: `ToolClient::run_with_stdin`/`run` (a payload
*into* the process, discarding stdout — `push_tool_document` feeds
`{"title", "body"}` JSON on stdin; `run` is the fire-and-forget follow-up
call), `ToolDocument{title, body, labels, status}` (the read contract,
mirroring `BoardModel`'s role for the Board panel), and
`RecordingToolClient` (a test client that logs every argv+stdin it was
asked to run, so a test can assert *what* was pushed, not just that
something succeeded — `MockToolClient` alone can't do that since it only
returns a canned result). New `src/core/engine/document_ops.rs`:
`Engine::document_provider()` (mirrors `board_provider()`),
`open_tool_document(id)` (blocking — a deliberate one-shot action, unlike
the Board panel's background poll) opens a scratch buffer whose text is
`# <title>\n\n<body>`, preceded by a read-only `<!-- status: ... labels:
... -->` comment when the provider supplied either (labels/status are
shown, never parsed back — editing them is lifecycle-specific, the
bundle's job, not this generic seam's), `new_tool_document()` for the
blank-buffer flow, and `save_tool_document_buffer()` (wired into
`Engine::save()` ahead of the on-disk path, same slot as the keymaps/
registries scratch buffers) which pushes title/body then the
`write_follow_up` command — but only when the document already had an id;
the create path has nothing to transition from. `BufferState` gained
`tool_document: Option<ToolDocumentBinding>` (id + the write/follow-up
argv captured at open time, so `:w` doesn't need to re-resolve a provider
that may have changed) — `file_path` stays `None` throughout, so a
document buffer is never written to disk. `board_ops.rs`'s
`BoardAction::OpenIssue` now opens the card as a document buffer when a
`[document]` provider is configured (falls back to Phase 0's status-line
echo otherwise) — the "from a board card" half of the issue's scope; the
"provider-declared command" half (`:CoordRefine 42` naming a specific
command) is explicitly the coordinator bundle's job, not built here.
Tests: unit coverage in all four touched modules, including an explicit
"round-trips through a mock provider" test asserting the exact argv+JSON
payload sent to `write_command` and `write_follow_up` via
`RecordingToolClient`, a new-document-flow test asserting the empty-`{id}`
substitution and no follow-up call, and a `board_ops.rs` test proving
`OpenIssue` actually opens a buffer (not just a state flag) when a
document provider is installed.

**Review fix-up (iteration 1):** the original "no driver-tier test — this
reuses the existing generic buffer/tab/`:w` rendering path with no new
paint or click surface" justification was wrong to treat as an exemption:
"reuses existing rendering machinery" isn't "internal-only", and writing
the actual driver test surfaced two real, previously-undetected bugs it
would have caught immediately:
- `quadraui::BoardModel::handle_key` matches the literal `"Enter"`, but
  both backends' own key-name convention (`engine_key_from_ui`/
  `map_gtk_key_name`) emits `"Return"`/`"KP_Enter"` for that key — so a
  real Enter keypress on a selected board card has *never* triggered
  `OpenIssue` on either backend. Fixed in `Engine::dispatch_board_key_
  unified` (host-side translation, not a quadraui change).
- TUI's `handle_focus_owner_key` (`shell_app.rs`) had no `FocusKeyRoute::
  Board` arm at all — every other panel route has one, but Board's was
  missing since #521, so *any* keyboard navigation on the Board panel
  (j/k/h/l/g/G/Enter) fell through to the Explorer-fallback key dispatch
  on TUI specifically (GTK was fine — its key routing already calls the
  shared `render::dispatch_sidebar_panel_key` unconditionally for every
  route). Fixed by adding the missing arm, reusing the same shared
  function GTK already calls — no new per-backend logic.

New driver-tier coverage added: `tui_main::shell_app::tests::
board_open_issue_paints_document_buffer_tab_via_shell_app` (a real `Enter`
keypress through `TuiDriver`, asserting the seeded title paints) and
`gtk::testing::sidebar_panel_clicks::board_double_click_opens_issue_as_a_
document_buffer` (a real double-click through `GtkDriver`, same
assertion) — both installing a `[document]` provider on the same mock
manifest the existing `[board]` provider tests already use (#522: one
manifest can declare both), so both backends' `OpenIssue` -> new-tab path
is now exercised end-to-end through the real event-dispatch pipeline, not
just at the engine level.

Also addressed from review (non-blocking): (1) the create-path
`ToolClient::run_with_stdin` now returns the provider's stdout (was
discarded) and `push_tool_document` opportunistically parses a
provider-echoed `{"id": "..."}` back out of it — `save_tool_document_
buffer` binds the buffer to that id on a successful create, so a *second*
`:w` on the same still-open buffer updates the now-existing document
instead of silently re-running "create" with an empty id again; (2)
`open_tool_document_buffer` now switches to an already-open buffer/tab for
the same document id instead of always opening a new one (mirrors
`open_keymaps_editor`'s precedent), closing the "two buffers racing to
`:w` the same id" risk; (3) a doc comment now flags `parse_tool_document_
buffer`'s `<!--`-prefix detection as unambiguous only for buffers this
module itself produced; (4) `save_tool_document_buffer`'s doc now flags
the blocking-write tradeoff forward for a real (non-mock, network-backed)
provider.

`cargo build`/`clippy -D warnings`/`fmt` clean on both feature lanes (GUI
build compiles `src/gtk/` too, exercising the new `GtkDriver` test). Out
of scope (per the issue): the coordinator extension bundle itself
(`:CoordRefine`/`:CoordReview` command names, `coord` argv, the
`status:refining -> ready` semantics) and #523's provider-dispatch board
actions.

**Last updated:** September 24, 2026 (#955, ACP-4 — tool-call rendering
plus a source-agnostic change-review surface, on top of ACP-1's #952
transport; shares its review surface with the future #525 git-branch-diff
slice, whichever lands second consumes it). `src/core/acp.rs` gained
`AcpToolCall`/`AcpToolCallStatus`/`AcpToolCallContentBlock` plus
`parse_tool_call`/`parse_tool_call_update`/`tool_call_summary_line` —
`tool_call` is a full announcement, `tool_call_update` is a *patch*
(status replaces, `content` **appends**, never replaces) keyed by
`toolCallId`. New `Engine::acp_tool_calls: Vec<AcpToolCall>`
(`src/core/engine/acp_ops.rs`'s `acp_upsert_tool_call`/
`acp_apply_tool_call_update`) is upserted by id, not append-only, and
renders as one collapsed one-line summary turn per call (status glyph +
kind + title, `render::populate_ai_chat_controller`) appended after the
real conversation — same "synthetic turn" treatment #956 gave the plan
checklist. New module `src/core/review.rs` (deliberately free of any
`Engine`/buffer/backend knowledge): `ProposedChange{path, old_text,
new_text}` is the source-agnostic unit both this slice and #525 build
from; `ChangeReviewState`/`ChangeReviewEntry` wrap a real
`quadraui::DiffView` per file (built via `quadraui::compute_hunks`, with a
hand-rolled `pure_addition_hunks` for `old_text: None` — `"".split('\n')`
yields one line, not zero, so routing a new file through `compute_hunks`
directly can wrongly mark a trailing blank line `Same` instead of every
row being a clean `Added`) plus hunk/file navigation and accept/reject.
`src/core/engine/review_ops.rs` bridges it to the engine: `Engine::
open_change_review`/`change_review_diff_rect` (paint-to-hit-test contract,
same as `command_line_rect`), `handle_change_review_key` (Esc/q close,
j/k/Down/Up scroll, `]`/`[` hunk nav, n/p/Tab file nav, a/r accept/reject,
Return jumps to the current row's file+line), and `change_review_accept_
current` reuses `Engine::acp_write_text_file` (#954) rather than
duplicating the buffer-write path. New `FrameOp::ChangeReview` rung
(`render::paint_change_review_rung`, shared verbatim by both backends) —
painted as a full-viewport modal, so `render::route_modal_key` now also
routes to `Engine::handle_key` whenever `change_review.is_some()` (without
this, the AI panel's own focus route sends keys straight to
`route_ai_chat_event`, bypassing `Engine::handle_key` entirely — exactly
when a tool-call diff would arrive). Mouse click-to-jump
(`render::route_change_review_click`, `ChangeReviewClickRoute`) resolves a
click against the painted `DiffView`'s own row geometry and is wired on
both backends the same way `route_folder_picker_click` is — checked before
`route_modal_overlay_click`'s ladder, not folded into it, since this
surface swallows every click while open. Extended the shared `tests/
fixtures/fake_acp_agent.sh`: `$ACP_FAKE_TOOL_CALL_STATUS_ONLY` (status
transitions with no diff, so the transcript stays visible to assert
against) and `$ACP_FAKE_TOOL_CALL` (+ `$ACP_FAKE_TOOL_CALL_PATH`, the diff
scenario that opens the review surface and exercises accept-writes-to-disk).
Black-box coverage: three TUI `TuiDriver` tests and three GTK `GtkDriver`
tests (status-transition, diff-review-plus-accept, and — review fix,
same day — a real-mouse click-to-jump test per backend), each
RED-verified against its specific regression before being confirmed
GREEN — plus unit tests for every new parser in `core::acp`, the full
`core::review` module (including the acceptance bar's own explicit
non-ACP-feed test and the `oldText: null` pure-addition test), and
`core::engine::review_ops`. Known gap, stated rather than silently
shipped: `locations[{path, line}]` in the *transcript* (as opposed to the
change-review surface, which does support click-to-jump) has no
click-to-jump — `quadraui::ChatTurn`/`StyledText` carry no clickable-span
concept yet, which is a quadraui infra gap, not a vimcode backend one; the
keyboard path (`Return` in the review surface) exercises the same
resolution function so the gap is "no mouse entry point yet" for that
specific spot, not "unbuilt or untested". `cargo build`/`clippy -D
warnings`/`fmt` clean on both feature lanes; full `cargo test --lib`
(3675 tests, both backends compiled in) and `--no-default-features --lib`
(3435 tests) both green.

**Review fix (same day):** the driver-tier click test the review
demanded caught a real bug the keyboard-only unit test couldn't —
clicking a diff row landing where chrome (menu bar/CSD title bar,
activity bar, sidebar) sits underneath the full-viewport overlay was
silently swallowed *before* `route_and_apply_change_review_click`/
`mouse::handle_mouse`'s change-review branch ever saw it: three separate
chrome intercepts (quadraui's `ShellAdapter::handle` activity-bar/sidebar
hit-test, `App::handle_dispatch`'s always-on GTK menu-bar intercept, and
its CSD-titlebar drag-to-move check) all hit-test purely on screen
position with no notion that an open overlay was painted on top. Fixed
with new `render::reconcile_change_review_modal_stack` (paint-time, not
click-time — pushes/pops the surface's full-viewport bounds on
`quadraui::ModalStack` every frame, since the surface can open from an
async ACP event with no correlated mouse motion to piggyback a
handle-time reconcile on, unlike the editor-hover popup) plus three
narrow `change_review.is_some()` guards in GTK's `App::handle_dispatch`/
`try_route_sidebar_mouse_event`. Also: `change_review_jump_to_hit` now
closes the surface on a successful jump (mirroring `Return`'s explicit
close — a click that didn't close it just painted the diff right back
over the buffer it switched to), and `ChangeReviewState::extend` skips a
byte-identical duplicate `ProposedChange` (guards a replaying/buggy agent
re-announcing the same `toolCallId`+diff from appending a second entry).

**Review fix round 2 (same day):** the modal-stack reconcile above was
initially popped from an `else` arm inside the frame walk's
`FrameOp::ChangeReview` match — **dead code**, since
`FramePresence::change_review` is the exact gate `render::compose_frame`
uses to drop that rung from the op list the moment the surface closes, so
the arm can never run on the frame that needs the pop. The full-viewport
entry therefore stayed registered forever, and since `ShellAdapter::
handle` hit-tests `ModalStack` *before* any chrome dispatch, every mouse
event for the rest of the session was routed past `AppShell`'s own
handling (activity-bar panel switching, sidebar/bottom-panel resize) —
session-bricking, and invisible to the click-to-jump tests, which never
click chrome afterwards. Same shape as #1117's `explorer_tree_rect` bug
and the warning `render.rs` carries above `compose_frame`. Fixed by
calling `reconcile_change_review_modal_stack(backend, presence.
change_review, viewport)` **unconditionally, once, before the walk** on
both backends (the `reconcile_editor_hover_modal` shape), leaving the
rung's arm paint-only; `paint_change_review_rung` also pops rather than
pushes when the state is open-but-entry-less, so it never registers a
surface nothing painted. Covered by a new RED-verified driver test per
backend (`change_review_close_restores_chrome_clicks_via_shell_app` /
`…_via_gtk_driver`): open the surface from a plain non-ACP
`ChangeReviewState::new(vec![ProposedChange{..}])`, close it via the real
click-to-jump, then click the activity bar's Search icon and assert the
Search panel actually opens. Also folds in the round's non-blocking note:
the reconcile now calls `ModalStack::mark_painted` on the open path,
since no quadraui rasteriser marks this surface (upstream wires
`mark_painted` only for `draw_palette`/`draw_menu`/`draw_dialog`, and
this surface is a `DiffView` + `StatusBar`), which was making a correctly
painted overlay show up in `unpainted_ids()` and emit the #455
"registered but invisible" diagnostic every frame it was open.

Prior update: September 24, 2026 (#956, ACP-5 —
plan, slash commands,
modes and usage from the `session/update` stream, on top of ACP-1's #952
transport; independent of ACP-3/ACP-4). `src/core/acp.rs` gained pure
parsers for the four remaining `session/update` variants this track cared
about: `parse_plan_update` (`AcpPlanEntry`/`AcpPlanEntryStatus`,
`plan_to_checklist_text`), `parse_available_commands_update`
(`AcpAvailableCommand`), `parse_session_modes`/`parse_current_mode_update`
(`AcpSessionMode`), and `parse_usage_update`/`format_usage_summary`
(`AcpUsage`, deliberately tolerant of a couple of plausible field-naming
variants since usage telemetry is the least-stable corner of the v1
schema). `AcpClient::set_mode` sends `session/set_mode`. New `Engine`
fields (`acp_plan`, `acp_available_commands`, `acp_command_completion_idx`,
`acp_modes`, `acp_current_mode_id`, `acp_usage`), all session-scoped
(cleared on `ai_clear`/`AgentExited`, matching `acp_remembered_decisions`).
`Engine::acp_handle_session_update` (`src/core/engine/acp_ops.rs`) now
dispatches every recognized `session/update` kind; **`plan` is a full
overwrite (`self.acp_plan = entries`), never `.extend`** — the #956
acceptance bar ("two successive `plan` updates leave exactly one plan
rendered") is a regression a worker could reintroduce by "fixing" this into
an accumulator, so it's called out explicitly at every layer (doc comments,
a dedicated `parse_plan_update` unit test, and a RED-verified TUI black-box
test). `render::populate_ai_chat_controller` renders the current plan as
one synthetic checklist turn appended after the real conversation (never
mixed into `ai_messages`) and folds mode + usage into the existing AI-panel
status header (no new widget, so #956's "no layout churn, no focus steal"
criterion holds by construction). Slash commands surface as completions via
`Engine::ai_command_completions`, reusing `render::CompletionMenu` /
`quadraui::Completions` — the *same* machinery the editor's own word-
completion popup uses, fed differently, per the issue's explicit steer away
from a bespoke widget; `render::route_ai_chat_event` intercepts Tab (cycle)
and Enter (accept) ahead of `ChatController::handle` when the popup is
showing, and `render::paint_ai_command_completions` paints it anchored to
the bottom of the panel's own rect (no exact input-box geometry needed —
`Completions::layout`'s own "flip above on overflow" placement logic does
that). Accepting a completion is nothing more than filling the input with
`"/name "`; submitting it is `ai_send_message`'s existing plain-text path,
unchanged — there is no separate slash-command RPC per the ACP v1 spec.
New ex command `:AiMode [target]` (`src/core/engine/execute.rs`): no
argument shows the agent's declared modes and which is current
(`Engine::acp_mode_status_line`); an argument sends `session/set_mode`
(`Engine::acp_set_mode`) matched by mode id or name — the displayed mode
changes only once the agent's own `current_mode_update` notification lands,
never optimistically on the request succeeding, which is the round-trip
#956 asks for. `config_option_update`/`session/set_config_option` were
explicitly left out of this slice per the issue's own "lower value...
otherwise split it out" guidance — no follow-up issue filed yet. Extended
the shared `tests/fixtures/fake_acp_agent.sh` (owned by the whole ACP
track): `$ACP_FAKE_SESSION_MODES` adds a `modes` field to the `session/new`
result; `$ACP_FAKE_PLAN` scripts two successive `plan` updates (the second
a full replacement of the first) plus an `available_commands_update` and a
`usage_update` in one `session/prompt` turn; a new top-level
`session/set_mode` case replies empty and then emits a `current_mode_update`
notification carrying back the requested mode id. Black-box coverage: two
new TUI `TuiDriver` tests (`ai_panel_plan_update_fully_replaces_not_
accumulates_via_shell_app`, `ai_panel_slash_command_completions_via_
shell_app`) and one new GTK `GtkDriver` test
(`ai_panel_mode_switch_round_trips_via_session_set_mode`), each RED-verified
against its specific regression (the plan test against reverting to
`.extend`; the slash-completion test against disabling the Tab/Enter
intercept *and separately* against disabling the popup's paint call; the
mode test against deleting the `current_mode_update` dispatch arm) before
being confirmed GREEN — plus pure unit tests for every new parser in
`core::acp` and two engine-level tests for `:AiMode`'s no-argument listing
and its no-session rejection message. `cargo build`/`clippy -D warnings`/
`fmt` clean on both feature lanes; targeted `cargo test` runs (acp/
ai_panel/ai_mode/settings-snapshot, both lanes) all green.). Prior update:
September 24, 2026 (#952, ACP-1 — hosted a live ACP session
behind the existing AI panel, retiring `curl` as the *only* transport. The
panel was already backend-neutral and already existed (`quadraui::
ChatController`/`Engine::ai_chat`/`PANEL_AI`/`ai_send_message`/`poll_ai`/
`dispatch_ai_chat_event`/`render::route_ai_chat_event`) — this slice was a
transport swap plus a stream mapping, not new UI, exactly as the issue
predicted. New setting `acp_agent_command` (`src/core/settings.rs`, parsed
via `core::acp::parse_agent_command`): empty (default) keeps the original
direct-provider `curl` transport (`crate::core::ai`, kept as a no-agent-
binary escape hatch through ACP-7 per the issue's own recommendation);
non-empty spawns that command as a live ACP agent. `Engine::ai_send_message`
(`src/core/engine/ext_panel.rs`) now forks into `ai_send_message_via_curl`/
`ai_send_message_via_acp`. `Engine::poll_acp` (`src/core/engine/acp_ops.rs`)
now drives the whole session lifecycle — `initialize` -> `session/new` ->
`session/prompt` — and maps `session/update` chunks onto `ai_messages`:
`agent_message_chunk`/`agent_thought_chunk`/`user_message_chunk` merge
consecutive same-kind chunks into one streamed turn rather than one turn per
chunk. Thought chunks render under a new AiMessage role
(`"assistant-thought"`) that `render::populate_ai_chat_controller` maps to
`quadraui::ChatRole::System` — a different role-header label ("System" vs
"AI") and colour, which is what makes them visually distinct from message
chunks per the issue's acceptance criterion, with zero quadraui changes
needed (the existing `ChatRole::System` styling already does this).
`tool_call`/`tool_call_update`/`plan` updates and agent->client requests
(`fs/*`, `session/request_permission`) are left unhandled — parked/ignored
without breaking the stream — per the issue's scope (ACP-2 fs bridge,
ACP-4/5 tool-call+plan rendering are later slices). Agent-binary-missing is
a clear message pushed into the transcript itself (not just the status
line), verified RED/GREEN. Extended the shared `tests/fixtures/
fake_acp_agent.sh` (owned by the whole ACP track) to emit the real ACP v1
`session/update` wire shape (`sessionUpdate` tag + `content.text`, replacing
ACP-0's placeholder `kind`/`text` shape that nothing had read the values of
yet) and added `$ACP_FAKE_NO_TOOL_REQUEST` so streaming-focused tests don't
need the fs/* bridge. Caught and fixed a real bug while writing the first
black-box test: `AcpEvent::SessionUpdate.update` is the *whole* notification
`params` object (`{"sessionId":..., "update": {...}}`), not the inner
tagged-union payload — `Engine::poll_acp` was reading `sessionUpdate`/
`content.text` off the wrong JSON level, so every chunk silently vanished
while the turn still completed normally (the "looks done, panel just never
grew" failure shape). Black-box coverage: TUI (`TuiDriver`, `src/tui_main/
shell_app.rs`) and GTK (`GtkDriver`, `src/gtk/testing.rs`) tests drive a
real submit through a pre-spawned fixture agent and assert on rendered
screen text (`"Hello world"` merged from two chunks, `"pondering the
question"` thought text, and the `"System"` role label), both independently
verified RED against the bug above before the fix and GREEN after; a third
TUI test covers the missing-agent-binary message. `cargo build`/`clippy -D
warnings`/`fmt` clean on both feature lanes; targeted `cargo test` runs
(acp/acp_ops/ai_panel/settings round-trip, both lanes) all green.). Prior
update: September 20, 2026 (#1102 — deleted GTK's `gdk_pixbuf`
app-icon pre-rasteriser now that quadraui#1014's `draw_image` decode cache is
already on the pinned rev (`d907a06`, an ancestor of the current pin
`0dc8381`). `src/gtk/util.rs`: removed `app_icon_image`/`cached_app_icon_png`/
`rasterise_app_icon_png`/`APP_ICON_RASTER_PX` — the once-per-run PNG
pre-rasterisation that dodged librsvg re-decoding the 1024² SVG every repaint
(+16.5 ms/frame) is now redundant, since `GtkBackend::draw_image` caches the
decoded/scaled `Pixbuf` itself. `src/app.rs`'s `app_icon_image_for_paint` no
longer forks on `#[cfg(feature = "gui")]` — every backend now hands
`crate::render::app_icon_image()` (the raw SVG) straight to
`Backend::draw_image` unchanged. Kept (out of scope for #1102, and the reason
`src/gtk/util.rs` still names `gdk_pixbuf`): `install_icon_and_desktop_at`'s
own, unrelated `gdk_pixbuf` use to render the on-disk XDG hicolor-theme PNG
icons (a completely different feature — files an external WM/compositor
reads, not anything `Backend::draw_image` touches) and a small
`#[cfg(test)]` `host_has_svg_loader()` probe that replaced
`cached_app_icon_png` as the "does this host have an SVG loader" skip-gate
for three installer tests and the #720 GTK pixel probe
(`app_icon_paints_left_of_the_file_menu` in `src/gtk/testing.rs`), which
still passes and still asserts on **pixels**, not state. Deleted the now-
inverted `painted_app_icon_is_the_rasterised_png_not_the_raw_svg` unit test,
whose entire premise (raw SVG must never reach `draw_image`) is exactly what
#1102 now does on purpose. macOS still does not decode SVG at all (that's
quadraui#1014's explicitly-deferred follow-up, not part of this pin) — so
macOS still paints no app icon, unchanged from before #1102; only the GTK
code path and its toolkit-typed workaround were in scope here. No dedicated
automated "paint-cost" timing guard exists elsewhere in the suite to re-check
— the one mentioned as a guard in the issue text was the just-deleted test
itself. `cargo build`/`clippy -D warnings`/`fmt` all clean, both feature
lanes; `gtk::util`, `gtk::testing::app_icon` and `render::tests` (app-icon
subset) test modules green.). Prior update: September 19, 2026 (#1155 — built the location list: a
per-window twin of the global quickfix list, plus the rest of the quickfix
family. Refactored the 4 flat `quickfix_items`/`quickfix_selected`/
`quickfix_open`/`quickfix_has_focus` engine fields into one
`QuickfixList { items, selected, open, has_focus }` struct
(`src/core/project_search.rs`) so `Engine.quickfix: QuickfixList` (global) and
`Engine.location_lists: HashMap<WindowId, QuickfixList>` (per-window) share
one implementation: `src/core/engine/picker.rs`'s `qf_*` methods all take
`win: Option<WindowId>` (`None` = quickfix, `Some(id)` = that window's list)
rather than existing as two parallel code paths. New ex commands: `:cwindow`,
`:clist`, `:colder`/`:cnewer` (10-deep stack, `quickfix_stack` +
`quickfix_stack_pos`, truncate-on-branch like an undo tree), `:cdo`/`:cfdo`
(run a command per-entry or per-distinct-file), and the entire `:l*` family
(`:lopen`/`:lclose`/`:lwindow`/`:lnext`/`:lprevious`/`:lfirst`/`:llast`/`:ll`/
`:llist`/`:ldo`/`:lfdo`/`:lgrep`/`:lvimgrep`). `:cfirst`/`:clast` already
existed (#1154) but were never added to `VIM_COMPATIBILITY.md` — fixed
alongside. CTRL-W window-close (`close_window`/`close_other_windows`/
`remove_tab_raw` in `src/core/engine/windows.rs`) now drops the closed
window's location list, same spot `prune_jump_list_windows` already runs.
Rendering: added `Engine::open_file_in_window` (replace a *specific*
window's buffer in place, no new tab) because location-list jumps must stay
in the window that owns the list — reusing quickfix's `open_file_in_tab`
(new tab per entry) made `self.active_window_id()` drift across a sequence
of `:lnext` calls, caught by a failing test before it shipped. The location
list shares the quickfix panel's one bottom "list rung" rather than adding a
second one (`render::QuickfixPanel` gained a `title` field, `"QUICKFIX"` or
`"LOCATION LIST"`, quickfix winning when both are open) — deliberately not
touching `BOTTOM_Z_ORDER`'s fixed 5-slot band stack, which exists because
upstream quadraui has no generic multi-drawer support yet. Coverage: 20 new
engine unit tests plus 3 new `TuiDriver` tests in `src/tui_main/shell_app.rs`
(`render_content_paints_location_list_panel_via_shell_app`,
`quickfix_panel_takes_priority_over_location_list_via_shell_app`, both
RED-verified against a temporarily-reverted population site) proving the
location-list panel actually paints — not just that `engine.location_lists`
got populated. `VIM_COMPATIBILITY.md`: moved 16 `❌` ids to `✅`, added 5 more
brand-new `✅` ids with no prior row, added a new `❌` row for `:lolder`/
`:lnewer` (per-window `:colder`/`:cnewer` — genuinely out of scope here, no
row ever claimed it). `tests/nvim_conformance.rs`'s coverage ratchet: gave
every new id a `COMMAND_PROBES` entry and, since no oracle case exercises any
of them yet, a matching `COVERAGE_EXEMPT` entry — same measured-gap pattern
the "Core Vim ex commands 84/111 uncovered" comment already documents for
this section. `cargo build`/`clippy -D warnings`/`fmt` all clean, both
feature lanes.). Prior update: September 18, 2026 (#234 — investigated "TUI menu-bar dropdown: mouse hover doesn't change active menu or highlight entries"; could not reproduce against current `develop`. The suspected gap named in the issue — a per-backend TUI mouse-motion handler that never calls something like `engine.set_menu_selected` for the menu-bar dropdown specifically — doesn't exist as described: TUI's menu bar goes through `TuiShellApp::handle`'s `MenuSystem` intercept (`menu_bar_visible || menu_system.borrow().is_open()`), which forwards the raw `UiEvent` (including a bare `MouseMoved`, no button held) straight to quadraui's `MenuSystem::handle`. That function's own `UiEvent::MouseMoved` arm (`compose/menu_system.rs`) already switches the open top-level menu on hover and moves the dropdown's `dropdown_selected` to whichever item the pointer is over, unconditionally — there is no TUI-specific hover code to be missing. Confirmed empirically with two new `TuiDriver` tests in `src/tui_main/shell_app.rs`, `menu_bar_hover_switches_menu_and_highlight_234` (Alt-letter open) and `menu_bar_click_then_hover_switches_and_highlights_234` (mouse-click open, sidebar visible, non-trivial column offsets) — both assert on rendered output (dropdown text appearing/disappearing, `style_at` swapping which row carries the selected-row colours) and pass on unmodified `develop`. RED-verified: temporarily gating the `MenuSystem` intercept off (`if false && (...)`) in `TuiShellApp::handle` turns both red — the dropdown doesn't even open, let alone track hover — restored before committing. Also checked and ruled out the raw-terminal layer: crossterm 0.29's `EnableMouseCapture` sends `?1000h?1002h?1003h` unconditionally, so any-motion hover reports (no button held) are already enabled regardless of vimcode's own code. **Keep #234 open** — a `TuiDriver`-passing test cannot exercise real SGR mouse input end-to-end (the quadraui#302 blind spot noted in this file's testing guidance), so this only proves the *application-logic* path is correct; a human should confirm against a live terminal before closing, and if it still reproduces there the next step is almost certainly a quadraui-side terminal/tracking-mode question, not a vimcode one (Platform-Neutrality Rule — no per-backend fix was written here because none was needed). No production code change — investigation + regression-guard coverage only.). Prior update: September 18, 2026 (#231 — investigated "TUI rename dialog: tree rows under the dialog show stale tinting after dialog closes"; could not reproduce against current `develop`. Root cause turned out moot: #231's own repro used the pre-#223 `Dialog`-based rename-input prompt, but rename today is inline `TreeController` row editing (`explorer_ops.rs`'s `TreeControllerEvent::EditConfirmed`) and never opens `engine.dialog` at all — `ExplorerRenameState`/`start_explorer_rename` is `#[allow(dead_code)] // used by win-gui backend` on this backend. Substituted a `Dialog` still live today (`Engine::show_quit_confirm`, which `paint_dialog_rung` centers over the full window viewport and does overlap the sidebar tree on an 80×24 screen) and added `explorer_tree_rows_repaint_clean_after_dialog_closes_231` in `src/tui_main/shell_app.rs`: opens the dialog before the driver's first frame, confirms via `style_at` that it painted over at least one seeded explorer row, closes it with Escape (`Engine::handle_dialog_key`'s "Escape" arm), and asserts the row's rendered style returns exactly to a dialog-free reference driver's baseline. Green — ratatui's `terminal.draw` resets its buffer and `quadraui`'s `AppLogic::render` repaints the whole frame every pass (`quadraui/src/tui/run.rs::paint_frame`), so no residue persists across the dialog's open/close transition for this scenario. **Keep #231 open** — this doesn't prove the pre-#223 Dialog-based rename-prompt scenario the issue screenshot shows never had the bug, only that the mechanism it no longer exists in isn't reproducible via the paths that replaced it; a human should confirm against the actual current TUI (inline rename edit, `r` key on an explorer row) before closing. No production code change — investigation + regression-guard coverage only.). Prior update: September 18, 2026 (#499 — confirmed already fixed: #1086's row-derivation fix, already on `develop`, was the single root cause behind both #484 and #499's "only the top section header toggles" report — its commit message names #499 explicitly. Added `tui_ext_panel_click_toggles_the_log_and_stash_headers_499`, a 3-section (Branches/Log/Stash) black-box regression test in `src/tui_main/shell_app.rs` pinning #499's exact repro directly, since #1086's own coverage only exercised 2 sections. RED-verified by temporarily reverting the `mouse.rs` click arm to the pre-#1086 `sidebar_row - content_start` formula — both Log and Stash failed to toggle, reproducing the report; restored before committing. No production code change — the fix already shipped under #1086.). Prior update: September 18, 2026 (#58 — investigated the "intermittent stale TUI characters" issue and found its Session-244 mitigation, `Terminal::clear()` on resize/popup-dismiss, no longer exists: #634 moved TUI onto `quadraui::tui::run_with_shell`, which owns the `Terminal` internally and calls `clear()` once at startup only, with no `Reaction`/`Backend` hook an app can use to ask for it again. `render::is_force_redraw_key` (Ctrl+L) and `TuiShellApp::had_popup_overlay` already document this as a dead-in-practice gap in code comments; drafted the quadraui-side ask in `docs/PENDING_QUADRAUI_ISSUES.md` rather than adding per-backend code, per the Platform-Neutrality Rule — no fix is possible from `src/tui_main/` alone. **Keep #58 open** until that quadraui issue is filed. No code change this session (investigation + docs only).). Prior update: September 18, 2026 (#934 — the three GTK pixel probes documented as Darwin-known-red since #926/#933/#970 are now robust to Core Text's rasterisation instead of skipped: `painted_divider_x` tolerance-matches colour, the minimap ink probe samples 3 rows instead of 1, and the window-control contrast floor drops 40.0→25.0. Verified green on Linux at this SHA — all 5 prior + 2 sibling driver tests pass — settling the "is this fleet-wide" question the #3298 config comment left open: **it is not**, confirming Darwin-rasteriser-artifact, not ordinary bug. RED-verified all three against reintroduced real regressions on Linux; could not verify on an actual Darwin host from this session (WSL2/Linux only) — flagged for macmini confirmation before the operator drops `coordinator.yml`'s `uname` guard). Prior revisions: September 17 (#1066 — product decision: TUI's editor wheel now scrolls the hovered pane, converged onto GTK's `hovered_window_id` behaviour; `mouse.rs` rewired onto `render::find_window_at` + `Engine::scroll_viewport_with_cursor_for_window`, GOALS.md item 14 closed), September 16 (#1031 — `:s///c` confirm loop built, #801 Phase 2 / #986 fix: `Engine::confirm_sub` + `handle_confirm_sub_key` in `execute.rs`), September 14 (#951 — ACP-0: `src/core/acp.rs`, NDJSON JSON-RPC transport + session lifecycle, foundation of the ACP track, epic #531), September 14 (#522 — Track A foundation: generic external-tool JSON seam, `src/core/tool_client.rs`, no coordinator vocabulary in core), September 14 (#970 — confirmed the two "failing GTK click-geometry tests" are the already-known/already-documented Darwin font-rasteriser divergence from #926/#933, not a new bug; no code change), September 14 (#950 review fix round — driver-tier pixel test added for the SEARCH_COD→SEARCH glyph change, self-contradictory pure-refactor claim corrected), September 14 (#950 — ShellApp convergence decomposition + cheap wins), September 14 (#949 review fix round — driver-tier test added, macOS/Win-GUI claim corrected), September 14 (#949 — GTK-only settings-reload watcher deleted), September 11 (macOS native-menu audit — #901/#902 filed, milestone #7 reopened), September 10 (#862 — `src/app.rs` no longer needs the `gui` feature to compile), September 5 (#827 correction pass), September 4 (#801), September 3 (platform-neutrality chain drained). Milestone #7 is **15 open** (re-counted 2026-09-19 by #1168; #901, #902 and #1044 are all closed, and the remaining work is almost entirely `src/tui_main/`, tracked by epic #1169 — the GUI side is down to #1100/#1102/#1104, all consume-side against already-pinned quadraui APIs). **#47 is open, in milestone #5, now scoped to Stage 2** (`src/macos/mod.rs` wrapper + the `macos` feature); Stage 1's extraction is merged. See `GOALS.md` for the full correction history.

## #234 — could not reproduce; regression-guard coverage added, kept open

#234 reported that in TUI, hovering the mouse over the menu bar does nothing:
moving over a different top-level label ("File" → "Edit") doesn't switch the
open dropdown, and moving over an entry inside an open dropdown doesn't move
the highlight. The issue's own theory was a TUI-specific gap in
`src/tui_main/mouse.rs` — some mouse-motion handler that updates
`ContextMenu.selected_idx` for other context menus (explorer right-click, tab
action menu) but was never wired up the same way for the menu bar.

That gap doesn't exist. TUI's menu bar is not routed through
`mouse.rs`/`ContextMenu` at all — it's a separate quadraui primitive,
`quadraui::MenuSystem`, owned by `Engine::menu_system` and driven from
`TuiShellApp::handle`'s `MenuSystem` intercept (`shell_app.rs`, gated on
`menu_bar_visible || menu_system.borrow().is_open()`). That intercept hands
the *raw* `UiEvent` — including a bare `MouseMoved` with no button held —
straight to `quadraui::MenuSystem::handle`, and that function's own
`UiEvent::MouseMoved` arm (`quadraui/src/compose/menu_system.rs`) already:

1. Hit-tests the menu-bar labels and, if the pointer is over a different
   enabled top-level item than the one currently open, closes the old
   dropdown and opens the new one — the "switch active menu" behaviour.
2. Walks the open dropdown's (and any open submenu's) visible items and, on
   a match, updates `dropdown_selected` (or the matching `submenu_selected`
   entry) to that item — the "highlight moves" behaviour.

Both are unconditional — no button-held gate, no TUI/GTK split — so there is
no per-backend hover code for TUI to be missing, and per the
Platform-Neutrality Rule there is nothing to build in `src/tui_main/` for
this: the shared infrastructure already exists and TUI already calls it.

Confirmed empirically rather than by code-reading alone, with two new
`TuiDriver` tests added to `src/tui_main/shell_app.rs`:

- `menu_bar_hover_switches_menu_and_highlight_234` — opens File via the
  Alt+F shim, hovers "Edit" (asserts the screen now shows "Undo" and no
  longer shows "New Tab"), then hovers "Redo" inside the now-open Edit
  dropdown and asserts via `style_at` that the selected-row style moved from
  "Undo"'s row onto "Redo"'s.
- `menu_bar_click_then_hover_switches_and_highlights_234` — the same two
  assertions through the actual user-facing path: a real mouse click on
  "File" (not the Alt-letter shim) with the explorer sidebar visible, so
  every hit-test below reads non-trivial activity-bar/sidebar column
  offsets instead of the degenerate zero-offset case the first test uses.

Both pass against unmodified `develop`. **RED-verified**: temporarily
changing the `MenuSystem` intercept's guard in `TuiShellApp::handle` to
`if false && (...)` — so the event never reaches `MenuSystem::handle` at
all — turns both tests red (the dropdown doesn't even open in response to
the opening click/Alt-letter, let alone track hover); reverted before
committing.

Also checked and ruled out the raw-terminal layer, since the issue's
symptom could in principle be "hover events never arrive from the terminal
at all": crossterm 0.29's `EnableMouseCapture` command writes `CSI ?1000h`,
`?1002h`, **and** `?1003h` (any-motion tracking) unconditionally
(`crossterm-0.29.0/src/event.rs`), so bare pointer movement with no button
held is already requested from the terminal regardless of anything in this
repo.

**Keep #234 open, not closed** — a `TuiDriver` test dispatches synthetic
`UiEvent`s directly into the same `App::handle` a real terminal's crossterm
event eventually reaches, but it cannot exercise the actual SGR-mouse
byte-parsing / terminal-emulator-compatibility path in between (the
quadraui#302 blind spot this repo's own testing guidance calls out — "raw
mode, SGR mouse ... that TuiDriver cannot reach"). This investigation only
proves the *application-logic* half of the pipeline is already correct. A
human should confirm against a live terminal before closing; if the symptom
still reproduces there, the next step is almost certainly a
terminal-compatibility or quadraui-tracking-mode question, not a vimcode
one — no vimcode code change was needed or made here (investigation +
regression-guard coverage only).

## #231 — could not reproduce; regression-guard coverage added, kept open

#231 reported that after opening + closing the TUI rename dialog over the file
explorer, rows under where the dialog was painted retained a faint grey tint
distinct from both the normal row bg and the selected-row bg — pointing at
either `quadraui::tui::dialog::draw_dialog` spilling outside `layout.bounds`
or `quadraui::tui::tree::draw_tree` skipping a full per-row repaint.

**Investigation found the named repro path no longer exists.** At the time
#231 was filed (Session 332, the #223 Dialog-primitive pilot), TUI file
rename used a `quadraui::Dialog` with a text input (the "rename-input
prompt" the pilot's session log names alongside quit-confirm/close-tab-
confirm). Since then, rename moved to inline `TreeController` row editing —
`src/core/engine/explorer_ops.rs`'s `dispatch_explorer_key` routes to
`explorer_tree.borrow().is_editing()` and `TreeControllerEvent::EditConfirmed`
calls `handle_explorer_edit_confirmed`, never touching `engine.dialog`. The
old path, `ExplorerRenameState`/`Engine::start_explorer_rename`, is still in
the tree but `#[allow(dead_code)] // used by win-gui backend` — dead on this
backend today.

To still exercise the paint mechanism the issue is actually worried about
(does *any* `Dialog` leave residue on the tree after closing), this session
substituted `Engine::show_quit_confirm` — a `Dialog` still live today, and
one `paint_dialog_rung` centers over the *window* viewport (not just the
content area), so it does overlap the sidebar tree on an 80×24 screen.

Added `explorer_tree_rows_repaint_clean_after_dialog_closes_231` in
`src/tui_main/shell_app.rs`: seeds an expanded explorer with 18 files,
records each row's baseline rendered `style_at` on a dialog-free reference
driver, opens the quit-confirm dialog on a *second* identically-seeded
driver (before its first `render()`, since `TuiDriver` keeps its wrapped app
crate-private post-construction), confirms via `style_at` that the dialog
actually painted over at least one seeded row, closes it with Escape
(`Engine::handle_dialog_key`'s `"Escape"` arm — proven to route through
`handle_key_pressed`'s dialog-intercept tier by the existing
`handle_key_pressed_dialog_intercepts_all_keys` test), and asserts the
covered row's style is back to the reference driver's baseline exactly.

**Result: green.** `quadraui::tui::run::paint_frame` calls
`terminal.draw(|frame| { app.render(...) })` every frame — ratatui resets
its internal `Buffer` before the closure runs, and `app.render` repaints the
whole screen unconditionally (TUI has no partial/dirty-region redraw), so
nothing from a previous frame's dialog paint can survive into a frame where
the dialog is gone. No stale-tint residue reproduces for this scenario.

**Keep #231 open, not closed** — a green test against a *substitute* dialog
scenario doesn't retire the issue; it only shows the mechanism the bug would
need doesn't reproduce via the paths that replaced the original repro. A
human should confirm against the live TUI (`r` on an explorer row, or
whatever key now triggers inline rename) that the *current* rename UI has no
analogous artifact before this is closed. No production code change this
session — investigation + regression-guard coverage only, same shape as
#499 below.

## #499 — already fixed by #1086; added issue-specific 3-section coverage

#499 reported that after the #484 fix, single-click on ext-panel section headers
toggled only the *top* section — `git_insights`'s Log and Stash headers stayed
unresponsive. Investigation found this had already been root-caused and fixed by
#1086 (`Fix #1086: TUI ext panel click routing lands one row low`, already an
ancestor of both `develop` and this branch): the ext-panel click arm in
`mouse.rs` derived its content-row from `sidebar_row - content_start`, a formula
that never budgeted for `AppShellLayout`'s own one-row sidebar header above
`sidebar_content_bounds` — every click landed exactly one row low, independent
of which section was clicked. #1086's fix replaced that hand-rolled arithmetic
with `render::SidebarBodyGeometry::content_row` against the exact rect
`render_ext_panel` painted (`Engine::ext_panel_content_rect`), the same
"paint and click share one geometry" pattern already used elsewhere. Its commit
message explicitly names both #484 and #499 as the two symptoms of this one
root cause.

**This session's work:** confirmed the fix is present and green
(`tui_ext_panel_click_on_a_section_header_toggles_it`, a 2-section
Branches/Log fixture #1086 already shipped, passes). Since that coverage
doesn't exercise a *third* section, added
`tui_ext_panel_click_toggles_the_log_and_stash_headers_499` — a black-box
`TuiDriver` test with the issue's exact repro shape (Branches/Log/Stash, one
item each) that clicks both the middle (Log) and last (Stash) headers and
asserts each collapses then re-expands. RED-verified by temporarily reverting
the `mouse.rs` click arm to the pre-#1086 `sidebar_row - content_start`
formula: the new test failed exactly as #499 described (Log's item stayed
painted after the click — wrong row); reverted before committing.

No production code change — this is coverage-only, closing out #499 against
the fix #1086 already shipped.

## #58 — blocked on an unfiled quadraui gap (drafted, not yet submitted)

Issue #58 (intermittent stale TUI characters) said it was "mitigated in
Session 244" by calling `ratatui::Terminal::clear()` on resize events and on
popup-dismiss transitions from the legacy `src/tui_main/mod.rs` event loop —
`clear()` resets ratatui's incremental-diff cache so the *next* frame
unconditionally repaints every cell, working around cases where ratatui's
diff misses cells because the physical terminal's real state has diverged
from what its `Buffer` thinks it painted.

**That mitigation is gone, not just dormant.** #634 (closed well before this
session, part of the TUI → `ShellApp`/`run_with_shell` wave) deleted the
legacy event loop and moved vimcode's TUI onto
`quadraui::tui::shell_runner::run_with_shell`, which now owns the
`ratatui::Terminal` internally. Read the pinned rev (`7a77602`,
`quadraui/src/tui/run.rs`): `terminal.clear()` is called exactly once, at
startup, and never again — the runner's `Reaction` enum
(`quadraui/src/runner.rs`) has only `Continue`/`Redraw`/`RedrawAfter`/`Exit`,
none of which maps to "clear before the next draw," and neither `Backend`
nor `AppLogic` exposes a `request_full_repaint`-shaped method. vimcode's own
code already flags this as a known-but-inert gap rather than silently
regressing: `render::is_force_redraw_key`'s doc comment (Ctrl+L) and
`TuiShellApp::render_content`'s `had_popup_overlay` comment
(`src/tui_main/shell_app.rs`) both say so in as many words — Ctrl+L today
only returns an ordinary `Reaction::Redraw`, which re-runs the very diff
that missed the cells, so pressing it does not actually fix what a user
hits it for; `had_popup_overlay` is computed and stored every frame but has
had no reader since the call site it used to drive was deleted.

**Per the Platform-Neutrality Rule, this is not fixable from
`src/tui_main/` alone** — there is no host-facing hook in quadraui's TUI
runner to force the underlying `Terminal::clear()` a second time, and
adding one by reaching into `quadraui`'s internals (or reintroducing a
vimcode-owned `Terminal`, duplicating the runner) would be exactly the kind
of per-backend workaround the rule exists to prevent. The upstream gap is
fully drafted, ready to file on `JDonaghy/quadraui`, in
[`docs/PENDING_QUADRAUI_ISSUES.md`](docs/PENDING_QUADRAUI_ISSUES.md) —
filing it needs `gh` access this worker session doesn't have. **Keep #58
open until that issue is filed**, then until its fix lands and vimcode
wires `is_force_redraw_key`/`had_popup_overlay` onto the new hook (per
`GOALS.md`'s milestone-discipline rule); once filed, delete the drafted
entry and link the real issue number here.

No code change this session — investigation + two docs updates
(`docs/PENDING_QUADRAUI_ISSUES.md`, this file). GTK is unaffected (Cairo
repaints its `DrawingArea` in full every frame, confirmed against
`gtk::backend`'s existing "full repaint after a skipped frame / modal
closed / theme change" tests), so no GTK-side investigation was needed.

## #1375 — `minimap_click_at_the_middle_scrolls_to_half_the_file` rewritten off pixel colour

Follow-up to #934: #934's widened chroma tolerance (TOL=12, summed across 3 rows) was
never actually verified on a Darwin host (its own PROJECT_STATE entry said so). #1375
reports that a real `macmini` run at a post-#934 SHA still fails this exact test —
confirming the tolerance-tuning approach had hit its limit, not that a slightly bigger
number would have closed it.

Root cause: the probe was asserting a property of the **rasteriser**, not of vimcode's
behaviour. GTK's pangocairo backend composites glyph ink via Core Text on macOS and
FreeType on Linux; Core Text's gamma-correct AA can blend a syntax-colored stroke's
antialiased pixels arbitrarily far towards the background, past any single-pixel chroma
threshold chosen without access to the real rasteriser to measure against. This is a
test/fixture problem (docs/RELEASING.md §1.3b already documented the category), not a
product bug — nothing in `apply_minimap_click`/`build_rendered_window` differs by
platform.

Fix: discovered (by probing `GtkDriver::painted_texts()` directly) that quadraui's one
`show_layout` paint choke point records each editor line's **whole** Pango-layout text
verbatim, regardless of the per-run colour attributes painted within it — so the exact
buffer line on screen (`fn item_N() { let x = N; }`) is readable back with zero
dependency on pixel colour or which rasteriser drew it. Rewrote the test's `before`/
`after` probes to parse the lowest `fn item_N` line number out of `painted_texts()`
instead of counting "colorful" pixels in a gutter-adjacent column band — same
"assert on rendered output, not state" guarantee (CLAUDE.md), same RED-first
verification (hardcoding `build_rendered_window`'s `scroll_top` to `0` still fails the
new assertion, confirmed on Linux), but with no per-pixel AA tolerance left to tune.
`src/render.rs`/`src/gtk/mod.rs` untouched — pure test-only change, `docs/RELEASING.md`
§1.3b updated to drop this test from the known-red list (not yet re-measured on
`macmini` — confirm there before trusting the doc over the fix's own reasoning).

## #934 — the three Darwin-known-red GTK pixel probes fixed to tolerate Core Text, not routed around

Follow-up to the claude-coordinator#3298 config unblock. The `uname` guard in
`coordinator.yml`'s vimcode `test_command` exists only because of these three probes
(`gtk::chrome_paint_tests::window_control_buttons_are_visible_against_their_background_in_every_theme`,
`gtk::testing::minimap::minimap_click_at_the_middle_scrolls_to_half_the_file`,
`gtk::testing::tests::window_split_divider_drag_repaints_the_line_at_the_new_position`)
failing on Darwin's Quartz/Core Text pangocairo backend while green on Linux/freetype
— documented since #926/#933/#970 but never actually fixed, only routed around. This
issue is the fix.

**Step 1 — settle the "is this fleet-wide" question, per the issue's own instruction:**
ran all three at this session's SHA on a Linux (WSL2, headless, no DISPLAY) host — **all
green**, alongside their two shared-layout/TUI twins (`render::tests::
minimap_click_at_the_middle_seeks_to_the_middle_of_the_painted_window` and
`tui_main::shell_app::tests::minimap_click_at_the_middle_scrolls_to_the_middle_of_the_
painted_window`, also green). Confirms the reported Darwin failures are rasteriser
artifacts, not ordinary bugs — the fleet-wide-bug branch of the issue's decision tree
does not apply.

**Step 2 — fix each probe, not the behaviour it guards, per CLAUDE.md's "assert on
rendered output" rule:**

- `painted_divider_x` (`src/gtk/testing.rs`) now colour-matches within a TOL=10
  per-channel tolerance (`colour_near`, mirroring `vscode_dimming::near`'s existing
  idiom for the identical AA-rounding class) instead of `==`. The divider is a plain
  filled line, not text, so its *geometry* can't shift with the font, but a 1px hairline
  at a fractional x still gets antialiased across two columns, and Core Text's
  compositing spreads that differently than freetype's — neither column may land on the
  exact full-intensity byte value even though the line plainly painted.
- The minimap ink sanity probe (`minimap_click_at_the_middle_scrolls_to_half_the_file`)
  now sums colorful-pixel ink across the top **3** painted rows instead of 1, both
  before and after the click. Every line in the fixture repeats the same token shape, so
  this multiplies sampled ink without changing what's proven; the `frac`-tolerance keeps
  `scroll_top` inside roughly (160,240) of 400 lines, so the widened "after" band tops
  out around line 242 — comfortably inside the fixture's indented (100..300) range with
  margin to spare.
- The window-control contrast floor (`src/gtk/mod.rs`) drops from `40.0` to `25.0`. The
  reported Darwin measurement was 36.1 (solarized-dark, minimize) — this is a single
  data point, not a full Darwin run across every theme/button, so the new floor is a
  reasoned floor-with-margin (≈5x above the #552 near-zero true-invisible-bug shape),
  not a tuned-exact value.

**RED-verified all three on Linux** by temporarily reintroducing the real bug each probe
exists to catch (`apply_divider_drag` forced to `false`; `build_rendered_window`'s
`scroll_top` hardcoded to `0`; `window_controls_status_bar`'s `fg` set equal to `bg`) —
all three failed loudly with the expected message, confirming the widened tolerances
did not weaken the checks. Reverted before committing; `git diff` touches only
`src/gtk/testing.rs` and `src/gtk/mod.rs`.

**Not verified on an actual Darwin host** — this session runs on WSL2/Linux, and no
macOS machine was reachable. The fix is code-inspection-and-Linux-RED-verification
based, not confirmed against the real Core Text failure. **Before the operator drops
`coordinator.yml`'s `uname` guard per this issue's "follow-up once green" note, run
`cargo test` on macmini and confirm all three (plus their #976 TUI-lane siblings, a
separate and already-tracked issue) are actually green now.**

## #1066 — TUI editor wheel scroll converges onto GTK's hovered-pane behaviour

Wave 3 product decision from the #1044 audit (GOALS.md item 14): should TUI's editor wheel
scroll the pane under the pointer, like GTK's `hovered_window_id` does, instead of always the
focused pane? **Decided: converge.** Scroll-follows-pointer is standard in GUI editors, but the
decisive argument was terminal-native precedent — real Vim's own mouse handling already scrolls
the `:split` pane under the pointer independent of focus, which is what a "vim-like" editor
should match, not GTK parity for its own sake.

`mouse.rs`'s editor-viewport wheel-scroll fallback (the block a `#825` comment had explicitly
flagged as the one place this diverged) now resolves the hovered window via `render::
find_window_at` and routes through `Engine::scroll_viewport_with_cursor_for_window` when it
differs from the active window — the exact shared primitives GTK's `handle_mouse_scroll_msg`
(`app.rs`) already uses. No new per-backend code.

Building the driver test surfaced a real, previously-latent `find_window_at` call-site bug:
TUI window rects can land on a half-row boundary (an odd number of available rows splits
unevenly, e.g. 37 → two 18.5-row panes), and querying the integer row itself — rather than the
cell's *center* (`+ 0.5`) — lands just outside the pane that visually owns that row. Fixed by
querying `col + 0.5, row + 0.5`, matching the cell-center convention `TuiDriver::find`/
`find_bounds` already use.

New black-box test: `wheel_scrolls_the_hovered_pane_not_the_focused_one_via_shell_app`
(`src/tui_main/shell_app.rs`) — drives a real horizontal `:split` with two files through
`driver_with_shell`, wheel-scrolls at the unfocused pane's own painted text, and asserts purely
on the rendered screen (the driver hides the concrete `Engine` behind an opaque `AppLogic`, so
there is no internal `scroll_top` to assert on even if the test wanted to). RED-verified by hand
against the pre-fix `engine.scroll_viewport_with_cursor(dir, 3)`-only fallback.

## #1031 — `:s///c` confirm loop built (#801 Phase 2, #986 fix)

#986's v0.11.0 bug suite shipped only oracle-backed, `KNOWN_BUGS`-gated reproductions for the
`:s///c` confirm flag — `execute.rs`'s `flags.contains('c')` check errored loudly
("E-vimcode: the :s 'c' (confirm) flag is not implemented") rather than misbehaving, but the
gate reported that expected-fail as a pass, so the feature shipped in v0.12.0 looking green
while never having existed. #1031 is the fix, per `CLAUDE.md` Testing rules 3/4's requirement
that a test-only issue get a follow-up before it may close.

**`run_substitute` (`src/core/engine/execute.rs`)** now enters a real confirm loop instead of
erroring: `collect_confirm_candidates` precomputes every match `:s///c` will offer (same
global/same-line-dedup/multiline rules the non-confirm scan already used, just not applied
yet) against the buffer text frozen at invocation time, then `Engine::confirm_sub` holds that
list plus in-progress `out`/`copied`/`n_subs`/`done_lines` state between keystrokes.
`handle_key` (`src/core/engine/keys.rs`) intercepts all keys at top priority while
`confirm_sub` is `Some`, routing to `handle_confirm_sub_key`, which implements
`y`/`n`/`a`/`q`/`l`/`<Esc>`/`<C-e>`/`<C-y>` per `:h :s_c`. The real buffer is spliced once, at
the end of the loop — behaviorally identical to the non-confirm path's single splice, just
gated per-candidate by the user's answer.

**Verified against a live interactive Neovim** (`nvim --headless --listen` +
`--remote-send`, v0.12.5 — the suite's usual `-es` batch-mode oracle silently short-circuits
`:s///c` entirely, so this had to be checked by hand outside `cargo test`) for a handful of
non-obvious rules the two gated scenarios alone didn't cover: the prompt's cursor sits at the
pending match's *start*, not its line's first non-blank; `q`/`<Esc>` freeze the cursor there
and print no report even if an earlier answer replaced something; `l` ("last") *does* re-land
the cursor the way a natural completion would but still prints nothing; and any unrecognised
key is silently ignored (re-prompts the same candidate) rather than treated as `n`. 5 new
`tests/nvim_conformance.rs` cases (`"sub:c ..."`) pin these against the real oracle, and the
11 pre-existing `sub:c` cases #986 had already shipped, `KNOWN_DEVIATIONS`-gated, all now pass
— #1007's coverage ratchet moved (11 entries deleted). Both gated `src/harness.rs` scenarios
(`confirm_prompt_text_is_painted`, `confirm_report_line_excludes_skipped_matches`, each
backing both a `::gtk` and `::tui` test via the shared macro) now pass on both backends; their
`KNOWN_BUGS` entries are deleted. No per-backend code — the fix is entirely in `src/core/`.

## #951 — ACP-0: `src/core/acp.rs`, NDJSON JSON-RPC transport + session lifecycle (foundation)

Root of the ACP track (epic #531 — see the issue's "standing commitments" for the whole
track). This slice ships the transport and client<->agent session lifecycle only — **no UI**;
later slices build the AI panel state machine and rendering on top of `AcpEvent` and
`Engine::poll_acp`.

**Not `lsp.rs` reuse** — two specifics don't carry over: ACP is NDJSON (one JSON message per
line on stdio, no `Content-Length` framing), and agent->client requests (`fs/read_text_file`,
`session/request_permission`, etc.) are dispatched by method name and **parked** via
`AcpEvent::ClientRequest` rather than blanket-answered with `result: null` the way `lsp.rs`'s
reader thread does today. A parked request is answered later, out of band, with
`AcpClient::respond_to_client_request`, whose reply is written through the same
`Arc<Mutex<Box<dyn Write + Send>>>` stdin the reader thread holds — load-bearing here (unlike
`dap.rs`, whose non-shared `BufWriter` stdin is exactly why the DAP client can't answer
adapter requests; this module does not repeat that).

**Engine integration is one field, one function, one call site** per the issue's scope:
`Engine::acp_client: Option<AcpClient>`, `Engine::poll_acp()` (`src/core/engine/acp_ops.rs`),
called from `poll_idle`. Today `poll_acp` only meaningfully handles `AgentExited` (clears the
client, reuses the existing generic `self.message` status-line field the same way
`LspEvent::ServerExited` does — no new backend-specific surface); the other event variants are
forwarded to `redraw` for later slices to consume.

**Fixture:** `tests/fixtures/fake_acp_agent.sh` — a deterministic NDJSON echo agent in plain
`/bin/sh` (no jq/python/node, so it runs in CI, which has neither Node nor a real agent
login). It drives `initialize` -> `session/new` -> `session/prompt` ->
`stopReason: end_turn`, and mid-turn issues a scripted `fs/read_text_file` client request that
**blocks** until the test answers it out of band via `respond_to_client_request` — proving the
reply actually reaches the agent through the shared stdin, not just that client-side
bookkeeping looks right. Every later ACP slice can depend on this fixture instead of a real
adapter.

**Tests:** `src/core/acp.rs` (11 tests: pure `classify_line`/`encode_ndjson_line` unit tests,
plus `#[cfg(unix)]` integration tests against the fixture covering the full lifecycle, agent
death mid-session -> `AgentExited` with no panic/orphan process, and malformed-line/stderr
noise not desyncing the reader) and `src/core/engine/acp_ops.rs` (2 tests: no-op with no
client, and `AgentExited` draining into `self.message` + clearing `acp_client`). This PR is
internal-only — no UI, no new user-visible behavior (`poll_acp`'s only observable effect,
`self.message` on an agent exit, requires a live ACP agent that nothing yet starts) — so no
GTK/TUI driver test accompanies it per CLAUDE.md's exemption for internal-only changes.

## #522 — Track A foundation: generic external-tool JSON seam (`tool_client.rs`), no coord in core

#522 is the foundation of Track A (coordinator↔vimcode integration, milestone
`vimcode-coordinator`, epic #531, `docs/COORDINATOR_INTEGRATION.md` §3/§5/§6). Per the
2026-09-13 owner decision, vimcode core must not depend on, or even name, `coord` — so the
seam is **generic**, not coordinator-aware.

**New `src/core/tool_client.rs`:** a `ToolClient` trait (`run_json(argv) ->
Result<serde_json::Value, ToolError>`, blocking — callers thread it the same way
`Engine::ext_refresh`/`poll_ext_registry` already thread registry fetches), a real
`SubprocessToolClient` impl (spawns via `core::git::hidden_command`, maps missing-binary /
non-zero-exit / bad-JSON to typed `ToolError` variants), and a `MockToolClient` test impl.
`fetch_board_model()` runs an argv and parses stdout into `quadraui::BoardModel` — vimcode's
board-data contract *is* quadraui's existing `Board` primitive types (`BoardModel`/
`BoardColumn`/`BoardCard`/`CardBadge`/`BadgeStatus`, quadraui#638, already `Serialize`/
`Deserialize`), reused directly rather than duplicated.

**Extension manifest:** `ExtensionManifest` gained an optional `board: BoardProviderConfig`
(`refresh_command` argv, `poll_interval_secs`, an `actions` map from `BoardAction` variant
name to an argv template with `{id}` substitution) — documented in `EXTENSIONS.md`'s new
`[board]` section. Generic: no particular provider is named.

**No-coord-in-core gate:** `tests/no_coord_vocabulary_in_core.rs` asserts (not just by
inspection) that `src/core/` and `src/render.rs` carry no coordinator vocabulary. Fixed in
review (iteration 1): a plain `\bcoord\b` regex only breaks at non-word characters, so it
missed "coord" glued to another word via `_` or a case transition — `coord_client`,
`CoordClient`, `CoordGate` all sailed through undetected, which is exactly the idiomatic-Rust
naming style a future PR would use to reintroduce coordinator vocabulary. The gate now
tokenizes each line into identifier-like runs and splits each token into words on `_`
boundaries and lowercase→uppercase case transitions, flagging any token whose word list
contains "coord" case-insensitively. "coordinate"/"coordinator" have no internal `_`/case
transition so they stay single words and keep passing; `coord_client`/`CoordClient`/
`CoordGate` split into ["coord", ...] and are caught. Confirmed 0 matches on the current tree;
the tokenizer's own incidental-vs-forbidden split has its own test
(`line_has_coord_word_distinguishes_incidental_from_forbidden`).

Board panel wiring (engine fields, GTK/TUI activity entry, actual poll_idle integration) and
the coordinator extension bundle itself are out of scope here — next up is #521. This PR is
internal-only: `ExtensionManifest` gains an unused-elsewhere `Option<BoardProviderConfig>`
field and `tool_client.rs`/`fetch_board_model` are not yet called from the engine or either
backend, so per CLAUDE.md's black-box-coverage rule no driver test is added — there is no
engine/GTK/TUI codepath yet for one to exercise.

## #521 — Track A Phase 0: generic Board activity panel (read-only)

Builds on #522's seam. New activity-bar entry **Board** (`PANEL_BOARD =
"panel:board"`), appended to `sidebar::FIXED_ACTIVITY_PANEL_IDS` (now 7 —
Explorer/Search/Debug/Git/Extensions/AI/Board), so `TOOLBAR_IDX_SETTINGS`
shifted 7→8 and `TOOLBAR_IDX_EXT_BASE` 8→9; every hand-written test that
hardcoded the old numbers (activity-bar keyboard-ring tests in
`tui_main/panels.rs`, the GTK "click the first ext-panel icon" test in
`gtk/testing.rs`, `test_ext_panel_h_focuses_activity_bar`/`_left_...` in
`core/engine/tests.rs`, and a stale hand-rolled `8 + idx` in
`core/engine/ext_panel.rs` that should have been reading
`TOOLBAR_IDX_EXT_BASE` all along) needed a one-line bump — all caught by
running the full `sidebar`/`activity`/`ext_panel` test scopes, not just the
new tests.

**`src/core/engine/board_ops.rs`** (new): `Engine::board_provider()` finds
the first installed extension manifest with `[board]` set (`ext_installed_
manifests()`, #522's `BoardProviderConfig`) — no coordinator (or other
specific provider) vocabulary anywhere, enforced by the existing
`no_coord_vocabulary_in_core` test. `board_refresh()`/`poll_board()` follow
the established `ext_refresh`/`poll_ext_registry` background-thread +
`mpsc` pattern, wired into `poll_idle` alongside a new `tick_board()` that
refreshes on the provider's declared `poll_interval_secs` while the Board
panel is active. `apply_board_action()` handles Phase 0's read-only subset
of `quadraui::BoardAction` (`SelectCard`, `MoveSelection`, `JumpToTop/
Bottom`, `OpenIssue` → a status-bar message) — `Dispatch`/`RecordTest`/
`Merge`/etc. are #523. New `Engine` fields: `board_model`, `board_error`,
`board_has_focus`, `board_fetching`, `board_rx`, `board_last_refresh`,
`board_layout` (paint-time `quadraui::BoardLayout` cache for click
hit-testing, same contract as `ext_panel_tree_layout`), and a
test-swappable `board_client: Arc<dyn ToolClient>`.

**`src/render.rs`**: new `BoardData` view model (`has_focus`, `model`,
`status`) and `ScreenLayout.board`, built by `build_board_data` (always
`Some`, mirroring `ExtSidebarData`'s doc). `SidebarOwner::Board` +
`FocusKeyRoute::Board` slot into the existing shared routers
(`sidebar_owner`, `route_focus_key`, `dispatch_sidebar_panel_key`) —
`Engine::dispatch_board_key_unified` is the "unified key dispatch" every
other panel already has. `route_board_click` resolves a press against the
cached `BoardLayout` (`BoardLayout::hit_test`) — the same "paint caches,
click reads" contract as `route_ext_panel_click`. `board_status_bar` is
the one-line "no provider configured"/"fetching…"/error banner shown when
there's no model to render (never painted over a stale-but-good model).

**Backends** — GTK's `App::paint_sidebar_panel_rung` `PANEL_BOARD` arm and
TUI's new `panels::render_board_panel` each do the *same* two calls:
`backend.draw_board(rect, model)` (quadraui#638's rasteriser, already
shipped for both backends at the pinned rev) or `board_status_bar` +
`draw_status_bar`. No bespoke board-drawing code on either side — the
Platform-Neutrality Rule holds. Click routing
(`App::route_board_sidebar_event` / `mouse.rs`'s `SidebarOwner::Board` arm)
is likewise a 1-line call into `render::route_board_click`.

**Tests**: `board_ops.rs` unit tests (mock-provider fetch success/error,
action application, key dispatch, focus-triggers-refresh) plus black-box
driver coverage on both backends — TUI (`tui_main/shell_app.rs`,
`TuiDriver`): paints a mock provider's card + column header, shows the
"no provider configured" status, and a real click selects a card (asserted
via the TUI board rasteriser's selected-card background style change,
since `driver.app()` has no accessor back to `Engine` state — never via
`Engine::board_model` being populated). GTK (`gtk/testing.rs`,
`GtkDriver`): same three scenarios, the click one asserting the actual
`selected_card_id` mutation since GTK's harness *does* keep an
`Rc<RefCell<Engine>>` handle. All new tests RED-verified by temporarily
disabling each backend's paint dispatch arm before restoring it. `cargo
build`/`clippy -D warnings`/`fmt` clean on both feature lanes.

**Bundled icon font** (test-stage follow-up): the new `icons::BOARD`
(`\u{f0db}`, nf-fa-table_columns) is a 96th nerd codepoint, so
`tests/icon_font_coverage.rs` went red — `data/fonts/vimcode-icons.ttf` is
subsetted to exactly the codepoints `Icon::new` references, and an
unbundled one paints tofu on GTK with no build signal. Regenerated with
`scripts/gen_icon_font.py --source data/fonts/vimcode-icons.ttf
--legacy-source SymbolsNerdFont-Regular.ttf` (upstream nerd-fonts v3.4.0)
— i.e. the *current subset* as primary, upstream only as the donor for the
one missing glyph, so all 95 pre-existing glyphs keep byte-identical
outlines and metrics (verified with fontTools). Regenerating the other way
round (upstream as `--source`) also passes, but silently restyles 9
unrelated icons to v3.4.0's shapes — U+EA76, U+EAE6, U+EB3C, U+EB3D,
U+EB54, U+EB85, U+F02B, U+F0140, U+F0143 — which is a deliberate font
refresh, not a bugfix. `EXPECTED_NERD_CODEPOINT_COUNT` bumped 95 → 96.

Out of scope (per the issue): provider-dispatch actions (#523), issue
authoring as markdown buffers (#524), in-editor diff review (#525/#526),
and the coordinator extension bundle itself (the only place `coord` will
ever be named) — this ships the generic host only, provable end-to-end
with a mock provider and zero coordinator code anywhere in the tree.

## #970 — the two "failing" GTK click-geometry tests are the #926/#933 Darwin font divergence, already documented; no fix needed

#970 reported `gtk::testing::minimap::minimap_click_at_the_middle_scrolls_to_half_the_file`
and `gtk::testing::tests::window_split_divider_drag_repaints_the_line_at_the_new_position`
red on a clean `develop` checkout, on an `aarch64-apple-darwin` host with Homebrew
gtk4 4.22.4, and asked (a) whether CI (Linux) is also red, and (b) whether this
is a GTK-side instance of the #967 paint/hit-test `line_height`-disagreement bug
family.

**Reproduced on this session's Linux host** (`ubuntu`-class WSL2, headless, no
`DISPLAY`/`WAYLAND_DISPLAY` — matches CI's `runs-on: ubuntu-24.04`, no display,
default features so `gui` is on): `cargo test --features gui --lib
gtk::testing::` is **141 passed, 0 failed**, including both named tests, both
single-threaded and default-parallel, across 5 repeated runs — solidly green,
not a flake.

**Both open questions are already answered — by #926/#933, which landed two
days before this issue was filed (2026-09-12, before #970 was reported against
`adb88bb`):** `docs/PLATFORM_CONFORMANCE.md`'s macOS section names these exact
two tests (plus a third, `chrome_paint_tests::window_control_buttons_are_visible_against_their_background_in_every_theme`)
as failing on a real Darwin/Homebrew-gtk4 box and gives the root cause: **on
Quartz, Pangocairo rasterises via Core Text rather than FreeType, so glyph ink
and colour compositing differ from the Linux baseline these pixel-probe tests
were written against.** That is a rendering-*input* difference (which font
backend paints the glyphs), not a metrics-*disagreement* bug — unlike #967,
where two code paths computed `line_height` differently for the same paint,
here every assertion already reads its expected geometry off the same frame
it's checking (`h.painted_line_height()`, `h.painted_char_width()`, the
divider's own read-back colour) rather than a hardcoded value, and there is no
second, disagreeing code path to fix. `scripts/platform-conformance.sh`
already encodes this as policy: the `gtk` lane is `skipped
(opt-in on Darwin...)` by default specifically because of this divergence,
and forcing it with `--lane gtk` reproduces the three failures without "fixing"
them, by design.

**Conclusion: no code change.** The suite is not broken as a Linux/CI gate for
GTK click geometry (140→141 passed reflects #950's new glyph test, still 0
failed) — it only ever fails on the Darwin GTK lane, which was already known,
already investigated, already documented with the correct root cause, and
already excluded from the default run before #970 was filed. Nothing under
`src/gtk/` or `src/core/` needed touching; this PR is documentation only (a
cross-reference in `PROJECT_STATE.md`), which is why no driver-tier test
accompanies it.

## #950 — TUI-as-second-ShellApp convergence: decomposition written, cheap wins landed

#950 found `TuiShellApp` (`src/tui_main/shell_app.rs`) is a second, independent
`impl ShellApp` alongside `App` (`src/app.rs`), and that it converts quadraui
`UiEvent`s back into crossterm `MouseEvent`s (`events::uievent_to_crossterm`)
to feed the TUI-private `src/tui_main/mouse.rs` click router instead of the
shared, already-backend-neutral `src/click.rs` — a direct violation of
quadraui's portability rule 6. The issue was explicitly scoped as an epic
("do not attempt a single convergence PR"): write a decomposition, land the
cheap independent wins, leave the mouse-router convergence to follow-ups.

**Decomposition:** `docs/SHELLAPP_CONVERGENCE.md` — sorts #950's four findings
into essential (px-vs-cell tick geometry, the TUI-only hamburger panel, TUI's
`debug_log!`-based panic hook) vs. accidental (the SEARCH_COD/SEARCH icon
split, the 3× duplicated GTK/macOS/Win-GUI panic hook, the ~12×-inlined
`CREATE_NO_WINDOW` idiom), and proposes an ordered slice plan for the mouse
router itself (inventory/parity-test stage, then one panel intercept at a
time onto `click.rs`, ending when `uievent_to_crossterm` has no TUI callers
left to delete). No mouse-router code was touched in this PR — see that
doc's "Why the mouse router is not in this PR" section for why it doesn't
qualify as a cheap win.

**Cheap wins landed:**
- One icon table: `crate::icons::SEARCH_COD` deleted, `App::shell_config()`
  now uses the same `SEARCH` constant `TuiShellApp::shell_config()` always
  used — the two backends' search icons now match.
- One panic hook: `core::swap::install_gui_crash_hook()` is new; GTK/macOS/
  Win-GUI's three byte-identical panic-hook closures now call it instead of
  each carrying its own copy. TUI's hook is untouched (essential difference
  — it can't `eprintln!` over raw-mode/alt-screen the way a GUI backend can).
- One `hidden_command`: every inlined `creation_flags(0x08000000)` in
  `core/` (`swap.rs`, `lsp_manager.rs` ×3, `dap_manager.rs`,
  `engine/mod.rs`) now goes through `core::git::hidden_command`; the two
  LSP/DAP sites needing `CREATE_NEW_PROCESS_GROUP` too go through a new
  `core::git::hidden_command_new_process_group`; `git_command()` itself now
  delegates to `hidden_command("git")` instead of re-inlining the flag a
  third time in the same file.

**Driver-tier test — added, review round 1.** The panic-hook and
`hidden_command` wins are pure internal refactors (byte-identical behavior,
just de-duplicated). The icon-table win is not: switching
`App::shell_config()`'s `"panel:search"` arm from GTK's own `SEARCH_COD`
(nf-cod-search, `\u{ea6d}`) to the shared `SEARCH` constant (nf-fa-search,
`\u{f002}`) changes what glyph the GTK activity bar actually paints whenever
Nerd Fonts are on — a rendered-output change, not a refactor, so claiming
the pure-refactor exemption for it was wrong (round-1 review caught this).
The two existing tests cited below are plain unit tests over
`shell_config()`'s return value (`!p.icon.is_empty()` on the GTK side) and
would keep passing through a revert to `SEARCH_COD` — they don't cover the
regression. Added
`gtk::testing::tests::activity_bar_search_icon_paints_the_shared_glyph_not_the_deleted_cod_variant`
(`src/gtk/testing.rs`): renders the real `App::shell_config()` activity bar
through `GtkDriver` twice — once unmodified, once with `"panel:search"`'s
icon patched back to the deleted `SEARCH_COD` codepoint after
`build_shell_config` runs — and asserts the two rasterised activity-bar
columns differ in pixels (per #555, since the icon strip paints straight to
Cairo and never reaches `painted_texts()`). Verified this fails (0/7000
sampled pixels differed) with `App::shell_config()`'s `"panel:search"` arm
hand-reverted to the `\u{ea6d}` literal, confirming the test actually
catches the regression it names.

The two pre-existing tests (`app::portable_entry_point_tests::
shell_config_resolves_every_activity_bar_icon_and_reserves_the_title_bar`,
`tui_main::shell_app::tests::shell_config_registers_every_build_activity_bar_panel`)
still stand as coverage that every panel resolves *some* non-empty icon —
just not this specific regression.

## #949 — GTK's `gio::FileMonitor` settings watcher deleted; mtime poll is now the sole reload mechanism

`App::new` built a `gio::FileMonitor` over a hardcoded `$HOME/.config/…`
path purely to trigger settings.json hot-reload — GTK-only, so hot-reload
was a documented gap on macOS/Win-GUI. But `Engine::check_settings_reload`
already polls the settings file's mtime and was already TUI's sole reload
mechanism (`tui_main/shell_app.rs`'s `tick`, unconditional every tick).

Fixed: deleted the `gio::FileMonitor`, the `settings_monitor` field, the
`DeferredAction::SettingsFileChanged` variant, and the hardcoded `$HOME`
path entirely. `App::handle_poll_tick` (shared by every GUI entry point,
GTK/macOS/Win-GUI alike, since it's called from the portable
`tick_dispatch`) now calls `settings_file_changed` — and so
`check_settings_reload` — every tick.

Cadence check (the issue's "confirm first"): quadraui's GTK/macOS idle-poll
tick fallback is a 250ms ceiling (`runner.rs`'s `ShellApp::tick` doc,
quadraui#832) — same order of magnitude as the old watcher's near-immediate
`ChangesDoneHint`, and identical to what TUI has always shipped with no
complaints. No poll-frequency tightening needed.

**"Closes the macOS/Win-GUI gap for free" — corrected, review round 1.**
That claim is only half true. quadraui's `AppLogic::tick` doc
(quadraui#832/#940, `runner.rs`) gives macOS the same 250ms
`IDLE_POLL_CEILING` idle-poll fallback GTK has, so macOS really is fixed
for free. **Windows gets no idle-poll fallback at all** — `tick` there
only runs after native-event batches or an explicit
`RedrawAfter`/`request_frame_in` ask, and nothing in this diff arranges
either. A future Win-GUI backend would only pick up an externally-edited
`settings.json` while the user is actively generating native events, not
while the app sits idle — not the full fix the original claim implied.
Not a live regression (no Win-GUI backend exists in this repo yet), but
whoever builds one (quadraui#19–#31) needs to arrange an explicit
periodic nudge for hot-reload to work there. Corrected in `src/app.rs`'s
`new_portable` doc table and here.

**Driver-tier test — added, review round 1.** Round 1 review rightly
rejected "pure internal mechanism swap... no driver-tier test added" as
self-contradictory: the PR itself says the change is user-visible
(hot-reload lag is a UX property), and CLAUDE.md's black-box-coverage bar
only exempts a *claimed* pure refactor, not a "hard to test" excuse.
Added `src/app.rs::portable_entry_point_tests::
handle_poll_tick_reloads_settings_changed_on_disk` — constructs a real
`App` via `App::new_headless`, points `Settings::settings_file_path()` at
a private temp file via the new `core::settings::TestSettingsPathGuard`
(thread-local override, not a `$HOME` mutation — parallel-test-safe,
unlike env-var mutation would be), calls `handle_poll_tick()` directly
(the exact call site that changed), and asserts `engine.settings`
actually picked up the on-disk edit.

That test asserts on engine state, not painted pixels — CLAUDE.md's
"assert on rendered output, not state" rule (from #587/#592) targets a
*different* failure mode than applies here: a paint path that populates
state nothing ever reads. That's not in question for `check_settings_reload`
— every frame already reads `engine.settings` for colorscheme, the
line-number gutter, tabstop, etc. — so "did the poll fire" is the only
open question, and the added test answers it directly. A true
pixel-level check (repaint via `GtkDriver` after the reload, assert the
gutter changed) is currently **blocked by a quadraui gap, not a vimcode
one**: neither `GtkDriver` nor the backend-neutral `ConformanceDriver`
expose a way to pump `AppLogic::tick` headlessly in this repo's pinned
quadraui rev (`GtkDriver` has no `tick()`/mutable-`Backend` accessor,
unlike `quadraui::tui::testing::TuiDriver::tick()` — confirmed by reading
the pinned rev's `quadraui/src/gtk/testing.rs` and
`quadraui/src/testing/mod.rs`). Per the Platform-Neutrality Rule, adding
that pump is quadraui-side test infrastructure, so it belongs in a
quadraui issue (**not yet filed** — this worker cannot open GitHub issues;
flagging here for whoever can) rather than a vimcode-side workaround.

Verified: `cargo build`/`cargo clippy -- -D warnings`/`cargo clippy
--no-default-features -- -D warnings`/`cargo fmt --check` all clean, plus
the new test passing under `cargo test --lib`.

## #862 — `src/app.rs` compiles without `gui` (prerequisite for #859)

`pub mod app;` in `src/lib.rs` was `#[cfg(feature = "gui")]`-gated even though
`App`'s trait surface (`impl quadraui::ShellApp for App`) is backend-neutral —
`cargo check --no-default-features` couldn't even resolve `crate::app`. Fixed:

- The three remaining platform-typed fields (`window`, `css_provider`,
  `settings_monitor`) are now type-erased: `window`/`css_provider` behind new
  local traits `PlatformWindowHandle`/`PlatformCssProvider` (same shape as the
  existing `TextMetricsBackend` and `Engine::clipboard_read`/`clipboard_write`,
  #417), `settings_monitor` behind a `Box<dyn Any>` drop-guard.
- The portable majority of `crate::gtk::{click, css, util}` — pixel→click-target
  resolution, tab-bar pixel-geometry, UI-font helpers, theme CSS text
  generation, `open_url`/bundled-font install — moved to three new
  unconditionally-compiled modules: `src/click.rs`, `src/app_support.rs`,
  `src/css.rs`. `src/gtk/{click,mod,css}.rs` re-export everything so nothing
  else in `crate::gtk` (or their own tests) had to change.
- What's left behind inline `#[cfg(feature = "gui")]` *inside* `src/app.rs` is
  genuinely platform-bound: `App::new`/`App::assemble`'s display-dependent
  prologue, the `TextMetricsBackend`/`PlatformWindowHandle`/`PlatformCssProvider`
  impls for the concrete GTK types, window *discovery*
  (`find_visible_window` — quadraui has no portable equivalent yet), and a
  handful of literal `gtk4::Settings`/`gio::File` call sites.

Pure refactor, no behavior change — exempt from the black-box test bar per
CLAUDE.md. Verified: `cargo build`/`cargo check --no-default-features`/
`cargo clippy -- -D warnings`/`cargo clippy --no-default-features -- -D
warnings`/`cargo fmt --check` all clean; the 155 `gtk::` tests + `gtk::click`'s
11 + `gtk::util`'s 4 + `gtk::mod`'s `h_scrollbar`/`shell_config`/`chrome_paint`
tests (6) + 159 `tui_main::shell_app` tests under `--no-default-features` all
still pass.

Does **not** pair with the `TextMetricsBackend` de-Pango work (already done,
#861) — the issue's "don't chain in parallel" warning no longer applies since
that work landed first. Next: #859 (the vimcode-side adoption this and #861
were prerequisites for).

## #825 — partially done: click-path scroll-offset table converged + one dead arm deleted; the other four fix items need more design work than mechanical dedup

Issue #825 asked to converge five mouse-apply surfaces (modal overlay, drag,
mouse-up, chrome click, scroll) plus two dead/shadowed-routing cleanups. This
pass converged **one piece safely** and found that most of the rest is riskier
than the issue's framing suggests — documented here so the next session
doesn't re-walk the same investigation.

**Done:**
- The click path's `ScrollOffsetChanged` handling (`src/tui_main/mouse.rs`
  "Scroll-surface click dispatch", `src/app.rs` same-named section) now calls
  the existing `render::apply_scroll_offset` — the same union table the *drag*
  path already shares (#756) — instead of each hand-rolling its own arms.
  Verified **behavior-preserving, not just refactored**: `engine.scroll_surfaces`
  only ever holds `terminal_scrollback`/`debug_output` (registered by the
  shared paint code both backends call) plus `explorer:sb`/`ext_panel:sb`
  (TUI-only, `src/tui_main/panels.rs`) — grepped every push site to confirm.
  GTK's two old arms (`debug_output`, `terminal_scrollback`) matched
  `apply_scroll_offset`'s bodies exactly, so its conversion is 1:1. TUI's old
  table additionally had `tui:settings`/`debug_sidebar:*` (also match exactly)
  and deliberately **excludes** `terminal_scrollback` from the shared call —
  a click on it must still fall through to the bottom-panel rung below, which
  begins a scrollbar *drag* rather than a bare offset-set; folding it in here
  would silently break continued-drag-after-click on that scrollbar. This is a
  pure internal refactor (CLAUDE.md's exemption applies — no new black-box
  test added; all 111 pre-existing mouse/scroll tests across both backends
  still pass, `cargo build`/`clippy -D warnings`/`clippy --no-default-features
  -D warnings`/`fmt --check` all clean).
- Deleted a second, **provably dead** match arm: TUI's wheel-scroll table had
  a `"tui:editor_viewport"` case (window-aware, variable-step scroll) that
  `quadraui::dispatch_scroll` can never emit — confirmed against the pinned
  rev (`quadraui/src/dispatch.rs`) that it only produces an id from either a
  registered `ScrollSurface` or a `ModalStack` push, and grepped that
  `"tui:editor_viewport"` is registered as neither, anywhere. The *fallback*
  below it (unconditionally scrolling the active window, fixed step 3) was
  already the only path ever taken; its comment claimed otherwise and has
  been corrected. **Discovered while verifying, not fixed:** this means TUI's
  mouse-wheel-over-editor has never supported "scroll the pane under the
  pointer without changing focus" the way GTK's `handle_mouse_scroll_msg`
  (`hovered_window_id`) does — a real GTK/TUI behavior gap, but a *feature*
  gap, not a duplication one; out of this issue's scope to fix blind.

**Not done — needs a design decision, not a mechanical swap, before touching:**
- **Wheel scroll (the rest of item 5).** Deeper than the click path: TUI has
  an *earlier*, separate direct-dispatch block (`mouse.rs`, the
  `PANEL_EXPLORER`/`PANEL_GIT`/`PANEL_SEARCH`/`PANEL_SETTINGS` checks ahead of
  the `dispatch_scroll` block) that returns early for the explorer panel with
  a **hardcoded ±3** step — meaning the later `"explorer:sb"` arm in the
  `dispatch_scroll` wheel table can only ever fire when `PANEL_EXPLORER` is
  *not* active, i.e. never (that surface is only registered when it is
  active). That arm is dead too, but unlike `tui:editor_viewport` its
  "shadow" carries different semantics (fixed step vs. proportional-to-delta
  step) — deciding which is actually wanted is a product call, not cleanup.
  GTK's own wheel table (`app.rs::handle_mouse_scroll_msg`) only has 3 arms
  (`editor_hover`, `debug_output`, `terminal_scrollback` — verified pointwise
  identical to TUI's, safe to share) and never touches
  explorer/ext-panel/settings scroll via this mechanism at all. A shared
  `apply_wheel_scroll` is buildable for the 3 common arms; folding in the
  TUI-only ones needs the shadow above resolved first.
- **MouseUp sequence (item 3).** Read both `mouse.rs`'s `Up(Left)` arm and
  `app.rs::handle_mouse_up_msg` in full: real per-backend asymmetry beyond
  what the issue's "same 8-step sequence" implies — TUI has explorer
  drag-and-drop finalize (GTK doesn't show it here), GTK clears
  `debug_button_pressed` and a GTK-only `h_sb_drag_cell` field here (not yet
  migrated onto the shared `DragState`, unverified whether TUI's equivalent
  is handled by one of `shell_app.rs`'s ported panel intercepts instead), and
  the terminal-resize/split finalize math is expressed in different units per
  backend (rows vs. `cached_char_width`-derived cols, per the issue's own
  `TerminalPanelResize` note). A shared function needs a host-trait shape
  (per the issue's own suggestion for item 1) to parameterize these, not a
  copy-paste.
- **Modal overlay apply (item 1), drag-route apply (item 2), chrome apply +
  the `render_window_status_line` dropped-layout root cause (item 4)** — not
  investigated this session; still exactly as scoped in the issue body
  (re-verify line numbers first, several of the issue's cited ranges had
  already drifted by the time this pass started).
- **Dead/unreachable routing.** The activity-bar arm
  (`mouse.rs`, `col < ab_width` block): traced quadraui's `ShellAdapter::handle`
  (pinned rev `4ff2a64`, `quadraui/src/shell_adapter.rs`) and confirmed
  `AppShell::handle` runs first and short-circuits on
  `PanelChanged`/`SidebarHidden`/`BottomItemClicked`/etc. before the raw
  `MouseDown` ever reaches `TuiShellApp::handle_mouse_event` →
  `mouse::handle_mouse` — matching `shell_app.rs`'s own comment. **Not yet
  confirmed:** whether the arm's `MenuToggle` target (the hamburger icon, at
  `bar_row` 0) is itself one of `AppShell`'s registered activity-bar items
  that this same interception covers, or a TUI-drawn extra that `AppShell`
  would report `Ignored` for and let fall through to this "unreachable" arm
  after all — check `build_shell_config`'s activity-bar item registration
  before deleting; getting this wrong silently breaks the menu-bar toggle.
  The shadowed debug/explorer routing (`shell_app.rs` intercepts vs.
  `mouse.rs` ~1976-2043 per the issue) — not investigated this session.

## #824 — partially done: 8 of the 10 named `FrameOp` arms converged; 2 documented as genuinely one-sided

`render_content`'s `FrameOp` match had drifted back to 10 duplicated arms after
#763–#766 (those slices converged the *composition* — order/gates — not the
arm *bodies*). This pass adds render.rs's `paint_wildmenu_rung`,
`paint_global_status_bar_rung`, `paint_find_replace_rung`,
`paint_command_center_rung`, `paint_picker_rung`, `paint_context_menu_rung`,
`paint_dialog_rung`, `paint_toast_stack_rung` — one shared body per arm,
following the `paint_bottom_panel_rung`/`paint_quickfix_rung` precedent
(rect math stays per backend; only the convert-and-draw body is shared).
Both `src/app.rs` and `src/tui_main/shell_app.rs` now call these instead of
transcribing the body twice; the dead TUI-only `render_impl::render_picker_popup`
duplicate was deleted along with it. Added `gtk::testing::chrome_surfaces::
toast_stack_overlay_paints` (GTK had no black-box toast coverage at all before
this — confirmed it goes red against the #587-shape bug of caching a layout
without painting it, via a temporary swap to `Backend::toast_stack_layout`).

**Two of the ten stayed unconverged, on purpose** (see `render.rs`'s
"Frame-op rung painters (#824)" section doc comment for the full reasoning):

- **`FrameOp::CommandLine`** — TUI paints the row cell-by-cell
  (`panels::render_command_line`, cursor + mouse drag-selection inversion
  baked into the composed cells) instead of through
  `Backend::draw_command_line`, because that trait method has no
  selection-range parameter. Converging it needs a quadraui `Backend` trait
  change first (Platform-Neutrality Rule) — nothing filed yet.
- **`FrameOp::TabSwitcher`** — GTK feeds `TabSwitcherGeometry::visible_rows`
  into `tab_switcher_to_quadraui_list_view`; TUI feeds `max_visible` (a
  different field — see that struct's doc comment). Might be harmless,
  might be a latent bug; a duplication-convergence pass shouldn't silently
  pick one for a shared function, so both arms keep their own geometry prep.

The related "same shape" opportunities the issue also named —
`compose_bottom_band_rungs` and the editor-band composer — are **not**
touched by this pass; they're a separate slice.

`cargo build` / `cargo clippy -- -D warnings` / `cargo clippy
--no-default-features -- -D warnings` / `cargo fmt -- --check` all clean.
Targeted tests (55: every `render_content_paints_*_via_shell_app` plus the
GTK driver tests for every touched arm, including the new toast test) pass.

## #822 — partially fixed; item 2 blocked on an unfiled quadraui gap (drafted, not yet submitted)

Issue #822 listed three fix items. Item 1 (delete the `compute_tab_bar_hit_regions`
downconversion shim, migrate both backends to consume `quadraui::TabBarLayout`
directly) is **done**. Item 3 (a stale doc comment) was already gone before this
PR's base commit — nothing needed there. Item 2 (adopt `TabGroupController` for
tab drag/drop, deleting `TabDragState` and the local drop-zone code, ~460 lines)
is **not done** — it isn't a like-for-like swap, because `TabGroupController` owns
its own pane/tab model and vimcode would have to mirror `Engine`'s editor-group
state into it. The upstream gap this implies is fully drafted, ready to file on
`JDonaghy/quadraui`, in
[`docs/PENDING_QUADRAUI_ISSUES.md`](docs/PENDING_QUADRAUI_ISSUES.md) — filing it
needs `gh` access this worker session doesn't have. **Keep #822 open, scoped down
to item 2, until that issue is filed** (per `GOALS.md`'s milestone-discipline
rule); once filed, delete the drafted entry and link the real issue number here.

## #820 — blocked on an unfiled quadraui gap (drafted, not yet submitted)

`BottomPanelController` adoption was investigated and correctly declined (a
single-drawer model can't cover vimcode's five independently-gated bottom
bands — see `src/render.rs`'s bottom-band module doc). The upstream gap this
implies ("multi-band bottom chrome") is fully drafted, ready to file on
`JDonaghy/quadraui` into milestone #9, in
[`docs/PENDING_QUADRAUI_ISSUES.md`](docs/PENDING_QUADRAUI_ISSUES.md) — filing
it needs `gh` access this worker session doesn't have. **Keep #820 open until
that issue is filed** (per `GOALS.md`'s milestone-discipline rule); once
filed, delete the drafted entry and link the real issue number here.

## Active milestone: #7 Platform-Neutral — **15 open**, and the remainder is the TUI

**The north star is [`GOALS.md`](GOALS.md): eliminate all platform-specific code from
vimcode and lift it into quadraui.** Milestone **#7 Platform-Neutral** is the consume
side (vimcode adopts a shipped quadraui API and *deletes* its bespoke per-backend code);
milestone **#5 Cross-Platform UI Crate** is the supply side (building quadraui itself).
Don't conflate them.

### What landed

The 2026-09-01 audit filed ten issues and queued them in two parallel chains. All ten
closed, along with the slice chains that #733/#734/#735 turned out to need:

| Convergence | Parent | Slices that did the work |
|---|---|---|
| Mouse routing — one precedence ladder, was written twice | #733 | #751 → #756 |
| Keyboard dispatch — incl. the 19 stale `mirrors mod.rs:NNNN` pointers | #734 | #757 → #762 |
| Frame composition — `FrameOp` / `compose_frame`, one walk per backend | #735 | #763 → #766 |

Also closed: **#730** (`ai_panel` paint, closing epic #592), **#593** (GTK `Ctrl+V`),
**#731** (22 permanently-`None` Relm4 handles + ~103 unreachable arms), **#732** (the GTK
`Msg` bus — 124 variants, 301 sites, a 684-line `dispatch`), **#658** (preview tier),
**#480**, **#550**, **#551**. **#146** moved out to **#4 Editor Features** as
recommended — it is an addition, not a deletion, and it was making the burndown mean two
things.

Two structural landmarks fell with them:

- **#657 shipped `[lib] vimcode_core`** (`eb745e2`). `render`, `tui_main` and `gtk` are
  promoted out of the `vimcode` / `vcd` binaries into the library, and
  `tests/acceptance/` is sealed. The oracle loop is available to this repo for the
  first time — see `tests/acceptance.rs` and `docs/ARCHITECTURE.md`.
- **#766 deleted `draw_frame`** (`eedebf8`), the last raw-`ratatui::Frame` path. The
  #735 staging question ("enumerate the raw-`Buffer` residue first") resolved exactly as
  the previous revision predicted: it was `#[cfg(test)]`-gated and dead in production,
  and the three test-only helpers went with it.

Still true from earlier in the arc: `fn event_loop` does not exist in `src/`;
`src/gtk/draw.rs` is deleted; both `ShellApp` migrations (#448, #595) are closed.

### The post-#735 sizing audit — run on `develop @ eedebf8`

Production lines, `#[cfg(test)]` excluded. **All columns measured with the same
script** (`scripts/prod_lines.py`, added for this audit) so they are comparable:

| | 2026-05-01 | 2026-07-01 | 08-31 `f867817` | **pre-chain** `6875315` | pre-#785 09-03 | **post-#785 @ `ee26268`** |
|---|---|---|---|---|---|---|
| `src/gtk/` | 18,969 | 13,675 | 12,526 | 9,765 | 9,650 | **2,607** |
| `src/tui_main/` | 14,649 | 10,358 | 11,125 | 10,958 | 10,345 | **10,366** |
| `src/app.rs` (hoisted out of `src/gtk/` by #785) | — | — | — | — | — | **7,131** |
| **all three files** | 33,618 | 24,033 | 23,651 | 20,723 | 19,995 (2 files) | **20,104** |
| `src/render.rs` (shared) | 10,574 | 12,807 | 15,009 | 15,558 | 21,405 | **21,405** |

The **pre-chain** column is `6875315`, the last #732 commit — the true point before
#733/#734/#735 and slices #751–#766 began. Everything between the 08-31 and
pre-chain columns is **#722–#732**, which was dead-code deletion, not convergence;
collapsing the two is what produced the −3,656 misattribution. Both columns
regenerated from `git archive` 2026-09-12.

(#785, "stage 1 of #47," hoisted `struct App` verbatim out of `src/gtk/mod.rs` into
a new `src/app.rs` — see `GOALS.md`'s post-#735 audit for the full account. The
`src/gtk/` = 9,650 figure this file previously carried as "now" predates that move;
regenerated at `ee26268` per #827.)

**Projected vs. actual.** Measured over the chain's *own* range
(`6875315` → `eedebf8`), not 08-31 → 09-03, which silently includes #722–#732's
dead-code deletion:

| | projected | actual over the chain | (08-31 → 09-03, for reference) |
|---|---|---|---|
| Backends | −8,700 … −9,500, landing near 14,000–15,000 | **−728, landing at 19,995** | −3,656 |
| `render.rs` | +4,000 … +5,000 | **+5,847** | +6,396 |
| Net across the three files | ≈ −4,000 | **+5,119** | +2,740 |

**The chain missed its projection by roughly 12×, not 2.4×**, and the net went the
wrong way by over 5,000 lines. Of the −3,656, **−2,928** is #722–#732 deleting code
outright (#731 alone `−1,432/+245`; the #732 tranches `−1,837/+83`, `−535/+461`,
`−529/+485` in `gtk/mod.rs`), leaving **−728** for convergence proper — the two sum
exactly. Deleting unreachable code and converging duplicated code are different
activities and must not be pooled.

**The mechanism, visible in the diff:** moving a *decision* into `render.rs` leaves
every *apply* body in place at its original size, now preceded by a
`MouseDragState`/`ModalOverlayState` literal (30–60 lines per call site) and a
"#NNN moved this" comment. The `FrameOp`/`EditorOp`/`BottomOp` machinery added three
enums, three order constants, three composers, three validators and ~150 lines of
doc. A 12-variant `match` is not shorter than 12 `if` blocks.

Where the 08-31 → 09-03 reduction came from:

| File | pre-chain | now | Δ |
|---|---|---|---|
| `src/gtk/mod.rs` | 10,518 | 7,684 | **−2,834** |
| `src/tui_main/panels.rs` | 1,554 | 1,208 | −346 |
| `src/tui_main/mouse.rs` | 3,211 | 2,895 | −316 |
| `src/tui_main/shell_app.rs` | 4,109 | 3,989 | −120 |
| `src/gtk/click.rs` | 751 | 696 | −55 |
| `src/gtk/util.rs` | 303 | 250 | −53 |
| `src/gtk/css.rs` | 507 | 507 | 0 |

`gtk/mod.rs` is 78% of the entire cut. `tui_main/mouse.rs` — the file #733 was sized
against at −3,000…−3,500 — lost **316 lines**.

> **Correcting the record.** The `src/gtk/` figure this file previously carried as
> "12,588 at 2026-09-01" was measured *before* #727/#728/#730 landed; it matches the
> pre-chain 08-31 column, not the 09-01 tree. The 05-01 and 07-01 figures also differ
> from the previously recorded ones (by 10–290 lines) for the same reason. That is the
> whole argument for `scripts/prod_lines.py`: **regenerate, don't re-type.**

### What the chain bought, stated honestly

Every *decision* — which surface was hit, which handler owns a key, what order a frame is
composed in — is now stated once in `render.rs`, and both backends walk it. Delegation
density is high: `src/gtk/mod.rs` makes 424 `render::` calls. That is a durable
correctness win, and it is also *why* the net line count went up — the shared
op-sequence machinery (`FrameOp`/`compose_frame`, the routers) costs more lines than the
duplicate pair it replaced.

**It is not "thin event-to-engine wiring."** 19,995 production lines across two backends
is a long way from the north star, and the remaining gap should not be planned as small.

### What remains — four items, none of them queued

1. ~~**The irreducible surface is recorded but never aggregated.**~~ ✅ **Done
   2026-09-03, corrected 2026-09-05:
   [`docs/IRREDUCIBLE_SURFACE.md`](docs/IRREDUCIBLE_SURFACE.md).** The nine
   verdicts reduce to **three** facts (the folder-picker verdict was wrong and has
   been struck — `quadraui::compose::FolderPickerController` has existed since
   2026-05-25), **two** genuinely irreducible. And the sizing answer:
   **only 246 of 19,429 production lines (1.3%) name a native toolkit type**, so
   platform-specificity is *not* what keeps the backends large — `src/gtk/mod.rs` and
   `src/tui_main/shell_app.rs` are two implementations of the same four `ShellApp` entry
   points. Plan the remainder as duplication, not porting. One verdict
   (`tui_main/mouse.rs:1620`, command-line selection) turned out to be a **mislabelled
   supply gap**: `CommandLineLayout::hit_test` does not exist in quadraui and was never
   filed; **#194** is the open consumer-side symptom.
2. **The "duplication moved down into quadraui" claim is largely refuted (#827).**
   quadraui#481/#482 remain open and un-milestoned, but most of the headline numbers
   don't hold up at the pinned rev: `EventOutcome` is declared once, not twice
   (quadraui#496); the 1,671-line byte-identical claim was withdrawn by quadraui#481's
   own correction comment as "idiom coincidence" (real duplication ~85 lines); the
   UTF-8 fix has been public since 2026-08-15 (quadraui#503); the tree-layout
   "twins" are both 1-line wrappers over one shared function (quadraui#499); and
   quadraui#482's eight children (#503–#510) are all closed. See `GOALS.md` §2 for
   the full table. What's still real: macOS dispatches `WindowResized` undebounced
   while TUI/GTK share a `ResizeDebouncer`.
3. **#47's blocker was filed and cleared 2026-09-03** — see below (this used to say
   "filed nowhere"; it wasn't, within hours of that claim being written).
4. **The divergence bug class is still ~44 issues deep** (#206, #420, #264, #194, #233
   and friends), plus milestone #5's cross-backend residue (#149, #167, #168, #233,
   #294). `GOALS.md`'s thesis is that each is a symptom of a duplicated surface; if the
   convergence had reached far enough this list would be shrinking. It is the only
   outcome measure this goal has that isn't a line count — watch it.

### ✅ #47's blocker was filed and cleared — this section was stale (#827)

**Corrected 2026-09-05.** #47 (native macOS GUI) was closed 2026-09-02 with commit
`44882e9` — *"re-audit at pickup, no code — Backend-trait Rc-handle gap blocks Stage
1"* — recording the real blocker: `App` called `GtkBackend::modal_stack_handle()` /
`drag_state_handle()` at **19** call sites (`modal_stack_handle` ×12,
`drag_state_handle` ×7 — not the "44" this file previously said, which counted every
use of the `backend` field via `grep -n 'self\.backend\.' src/gtk/mod.rs`, not just
the two Rc-handle methods) in the drag and modal dispatch paths. Those were
**inherent methods on the concrete struct, not on the generic `quadraui::Backend`
trait**, and `MacBackend`'s trait equivalents (`modal_stack_mut`, `drag_and_modal_mut`)
returned short-lived `&mut` borrows that couldn't be stashed and reused the way `App`
does. Full findings are in [`PLAN.md`](PLAN.md).

**That blocker was filed — this file just never caught up.** **quadraui#699** was
filed 2026-09-03 16:38Z (into quadraui milestone #9) and **closed 17:11Z**
(PR#700/`88345fb`); follow-up **#704** closed 21:41Z. **vimcode#47 was reopened
16:38Z** and is **open now, in milestone #5**. quadraui#699/#704 gave every backend a
symmetric Rc-handle API, and vimcode has already started consuming it: **#811**
bumped the quadraui pin to `4ff2a64` and ported the TUI-side call sites off the
now-removed `drag_and_modal_mut`. The actual next actionable item is **vimcode#47
Stage 1** (the GTK-side `App` move), not a re-filing task — see `PLAN.md` and
`GOALS.md` for the full correction.

### Milestone hygiene

- **#7 is 15 open** (updated 2026-09-19, #1168). **#901 and #902 are both closed**
  — the macOS native-menu adoption that reopened this milestone on 2026-09-11 is
  done. So is **#1044**, whose 16 children were filed and landed 2026-09-16→18.
  What is open now: #1068, #1089, #1098, #1100, #1102, #1104, #1108, #1109, #1164,
  #1165, #1166, #1167, #1168, #1169, #1175.
- **The split matters more than the count.** Twelve of those are TUI work tracked
  by the standing epic **#1169** (`src/tui_main/` is the last backend with its own
  `ShellApp` impl — 11,037 production lines at `30c0077`). The GUI side is down to
  **three** consume-side items — #1100 (clipboard via `copypasta_ext`), #1102 (GTK
  `gdk_pixbuf` app-icon pre-rasteriser), #1104 (`TextMetricsBackend` + the second
  owned `GtkBackend`) — each against a quadraui issue that has **already shipped
  and is already pinned** at `d907a06`. **There is no open upstream blocker on the
  GUI backends.** The milestone closes when those three close as well as #1169's
  children.
- #146 moved to #4 Editor Features; **#47 sits in #5 Cross-Platform UI Crate and is
  open, now scoped to Stage 2** (`src/macos/mod.rs` wrapper + the `macos` feature) —
  Stage 1's extraction is merged.
- **quadraui milestone #9** ("vimcode Platform-Neutral blockers") is **open** (0
  open / 7 closed issues) — it held quadraui#699 and does not need re-opening.
- **Stale Win-GUI issues.** Roughly a dozen open `Win-GUI:` issues (#160–#178, #61,
  #172, #176) describe the *old* `src/win_gui/` backend, which was deleted on 2026-05-11
  (`3e4bcff`). The Windows GUI came back on 2026-09-11 as the `src/win/` thin wrapper
  over `quadraui::win` (#866, `4e2883d`, `win` feature). Re-check each of those issues
  against `src/win/` and close the ones that no longer apply.

### A note on line numbers in this file

There are none, deliberately. Locate code by **symbol**, not coordinate:
`grep -n "impl quadraui::ShellApp for App" src/gtk/mod.rs` and friends. Where a *count*
appears it is evidence measured on a named revision — regenerate it
(`python3 scripts/prod_lines.py src/gtk src/tui_main src/render.rs`) rather than trusting
it. #734 existed in the first place because `src/tui_main/` carried 19
`mirrors mod.rs:NNNN` comments whose targets had all drifted.

---

> Feature documentation lives in **README.md**. Sessions 389 and earlier in
> **SESSION_HISTORY.md**. No multi-stage wave is in flight — **PLAN.md** holds the #47
> re-audit findings and is otherwise history.

---
## Testing Policy

**Every new Vim feature and every bug fix MUST have comprehensive integration tests before the work is considered done.** Subtle bugs (register content, cursor position, newline handling, linewise vs. char-mode paste) are only reliably caught by tests. The process is:

1. Write failing tests that document the expected Vim behavior
2. Implement/fix the feature until all tests pass
3. Run the full suite (`cargo test`) — no regressions allowed

When implementing a new key/command, add tests covering:
- Basic happy path
- Edge cases: start/middle/end of line, start/end of file, empty buffer, count prefix
- Register content (text and `is_linewise` flag)
- Cursor position after the operation
- Interaction with paste (`p`/`P`) to verify the yanked/deleted content behaves correctly

---

## Cross-backend coverage

Snapshot of where each surface stands on its quadraui primitive.
TUI was the reference implementation through Phase C; GTK caught
up. Numbers update with each Path-A landing — read this to find
the next slice.

**Status (2026-09-03):** **Paint duplication is done for every
surface in the table below** — all ✅ on both backends. The
GTK-side regression that #540 introduced (surfaces painted only
by the since-deleted `draw.rs`) was swept by #669–#672, and the
last holdout, `ai_panel`, was painted on GTK by #730.

No bespoke section-walk paint code remains (debug sidebar moved to
`MultiSectionView` in #296 — both paint and click consume one cached
layout per frame). The mouse-routing, keyboard-dispatch and
frame-composition duplication that this note used to point at as
"untracked residual" was converged by #751–#766: both backends now
walk one `FrameOp` sequence built by `render::compose_frame`, and
`draw_frame` — the last raw-`ratatui::Frame` path — is deleted (#766).

What remains cross-backend is the set of rungs the slices
**deliberately declined to converge**, each with its verdict recorded
at the call site (`grep -rn -iE "do not converge|one-sided|intrinsic difference" src/`),
plus intrinsic-to-surface divergences (Cairo painter order vs ratatui
cell coalescence, px vs cell units). See "What remains" above — that
set has never been aggregated into one statement, and doing so is the
next piece of the north star's own work.

| Surface | Primitive | TUI | GTK | Notes |
|---|---|---|---|---|
| Status bar (per-window + global) | `StatusBar` | ✅ | ✅ | layout via `StatusBarLayout` |
| Tab bar | `TabBar` | ✅ | ✅ | |
| Activity bar | `ActivityBar` | ✅ | ✅ | |
| Tree view (explorer + SC) | `TreeView` | ✅ | ✅ | layout via `TreeViewLayout` |
| List view (quickfix + tab switcher) | `ListView` | ✅ | ✅ | layout via `ListViewLayout` |
| Form (settings) | `Form` | ✅ | ✅ | hint field exists but unrendered (#202) |
| Palette (all pickers: file/symbol/cmd/branch) | `Palette` | ✅ | ✅ | #402: all pickers route through `picker_panel_to_palette()` → `quadraui::Palette`. Preview panes + tree items. `PaletteLayout` for hit-test. `PickerGeometry` for popup bounds. |
| Find/replace overlay | shared hit-regions | ✅ | ✅ | engine-side `compute_find_replace_hit_regions` |
| Terminal cells + scrollbar + split | `Terminal` + `TerminalSplitLayout` | ✅ | ✅ | #353. `build_terminal_draw_data()` shared; both call `Backend::draw_terminal`. Themed scrollbar via `TerminalScrollbar { inverted: true }`. |
| LSP hover popup (simple) | `Tooltip` | ✅ | ✅ | slice 1, `e1e76cd` |
| Signature help popup | `Tooltip{styled_lines}` | ✅ | ✅ | slice 2, `aaa9a3c` |
| Diff peek popup | `Tooltip{styled_lines}` | ✅ | ✅ | slice 3, `e6650fa` |
| Dialog (quit/close confirm) | `Dialog` | ✅ | ✅ | slice 5, `7768a25` |
| Context menu (right-click) | `ContextMenu` | ✅ | ✅ | slice 6, `7ce0f5d` |
| Menu dropdown (top menu bar) | `MenuSystem` | ✅ | ✅ | #319. Owned by `MenuSystem::render()` + `MenuOverlay`. |
| Debug toolbar | `StatusBar` | ✅ | ✅ | slice 8, `caf62a8` |
| Breadcrumb bar | `StatusBar` | ✅ | ✅ | slice 8 |
| Editor hover popup (markdown + code-hl + selection + scroll + links) | `RichTextPopup` | ✅ | ✅ | #214 shipped (`c8a23e9`); rasterisers lifted via #266 (`779f6e8`); paint migrated to `Surface::RichTextPopup` via `frame.draw()` in #469 / PR #487 (`1912cd3`). Both backends consume `quadraui::{tui,gtk}::draw_rich_text_popup` through the trait. |
| Completion popup | `Completions` | ✅ | ✅ | #285 — GTK lifted to `quadraui::gtk::draw_completions` |
| Editor scrollbar (v + h paint) | `Scrollbar` | ✅ | ✅ | #277, `fbbc85f`+ |
| Settings panel chrome (header + search row) | `draw_settings_chrome` | ✅ | ✅ | #278, `fd08db0` |
| AI sidebar message history | `MessageList` | ✅ | ✅ | #279, `8e55720` |
| Editor viewport (text + gutter + cursor + selection + diagnostics) | `Editor` | ✅ | ✅ | #276, `5b23718`+ (Phase C Stage 1) |
| Extension panel | `TreeView` (with `Decoration::Header`) | ✅ | ✅ | #280, `d29d1b4`. Adapter `render::ext_sidebar_to_multi_section_view` (paint goes through `render::populate_ext_sidebar_system`; the original `ext_sidebar_to_tree_view` adapter lost its last caller and was deleted in #812). Click via `TreeViewLayout::hit_test()` on both backends. |
| Debug sidebar (variables tree, breakpoints, watch) | `MultiSectionView` (4 × `TreeView`) | ✅ | ✅ | #296, `285916b`. Adapter `render::debug_sidebar_to_multi_section_view`. Paint caches layout; click reads verbatim. |
| Source control panel | `SidebarSystem` (4 sections) | ✅ | ✅ | #321/#339/#340. `populate_sc_sidebar_system` + `SidebarSystem.render()`. Unified dispatch via `dispatch_sc_sidebar_key_unified`. Section badges + visibility (quadraui#103). |
| Bottom panel tabs (Terminal / Debug Output) | `TabBar` | ✅ | ✅ | #304, `5d7fa09`. Adapter `render::build_bottom_panel_tab_bar`. Click via `Engine::handle_bottom_tab_bar_click`. `show_tab_close: false`, `compact: true`. |
| Terminal toolbar (find bar + tab strip) | `StatusBar` / `TabBar` | ✅ | ✅ | #305, `08dd916`. Adapter `render::build_terminal_toolbar`. Click via `Engine::resolve_terminal_toolbar_click`. Tab strip uses `compact: true`. |
| Menu bar labels | `MenuSystem` | ✅ | ✅ | #319. `quadraui::MenuSystem` owns all state + rendering. `MenuOverlay` helper for GTK overlay DA. |
| Command center (nav arrows + search box) | `CommandCenter` | ✅ | ✅ | #310, `b5fdd7d`. Adapter `render::build_command_center_view`. Click via `CommandCenterLayout::hit_test`. |
| Search panel (chrome + results) | `SidebarSystem` (Form + Tree) | ✅ | ✅ | #323/#333/#334. `populate_search_sidebar_system` + `SidebarSystem.render()`. Unified dispatch via `dispatch_search_sidebar_key_unified`. Form: query/replace TextInput + ToggleGroup + ButtonRow. Tree: file-grouped results with collapse. |

**Cross-backend logic-sharing** (where one implementation drives both backends):

- All primitive `Layout` algorithms (`StatusBarLayout`, `PaletteLayout`, etc.) — single implementation, both backends consume.
- `quadraui::dispatch_scroll/click/mouse_down/drag/up` + `ModalStack` + `DragState` — drives all scroll wheel routing, scrollbar thumb-drag + track-page, palette drag, picker drag. All scrollable surfaces registered as `ScrollSurface` at paint time (#307, completed Session 353).
- Engine-side hit-region builders (`compute_find_replace_hit_regions`) and cell-unit fit algorithms (`StatusBar::fit_right_start`, `TabBar::fit_active_scroll_offset`) — parameterised over a measurement closure so each backend supplies its native unit.
- `core::settings::SAVE_REVISION` — one source of truth both file watchers consult (#201).
- All `*_to_form` / `*_to_tree_view` / `lsp_status_for_buffer` adapters in `render.rs` and `core/engine/`.
- `quadraui::MenuSystem` — menu bar + dropdown lifecycle (open/close, keyboard nav, hover-to-switch, modal stack). Both backends call `render()` and `handle()` with zero per-backend menu logic. GTK uses `MenuOverlay` helper for the titlebar DA overlay wiring.
- `quadraui::TreeController` — explorer file tree: selection, scroll, keyboard nav, inline editing (rename + new-file/folder), **scrollbar rendering + interaction** (#415, quadraui#193). Both backends call `render()` for drawing (including built-in 8px/1-cell scrollbar) and route mouse events through `handle()` for scrollbar thumb drag, track click, and row selection. `_via` methods for keyboard editing. All domain logic in `engine/explorer_ops.rs`.
- `quadraui::SidebarSystem` — extensions sidebar (#336/#337/#338), source control panel (#321/#339/#340), and search panel (#323/#333/#334): section selection, scroll, keyboard nav, mouse handling, collapse, badges, visibility. Search panel uses `SectionKind::Form` for the chrome section (quadraui#105). Both backends call `populate_*()` + `render()` and `dispatch_*_key_unified()`. Zero per-backend nav/click code.
- `quadraui::StatusBarInteraction` — debug toolbar hover/press state. TUI uses it via UiEvent intercept; GTK manual wiring produces identical results (#331 verified and closed).
- `render::build_terminal_draw_data()` + `Backend::draw_terminal` — terminal cell grid + themed scrollbar + split-pane layout. Both backends call one shared builder, then `draw_terminal`. Zero per-backend terminal rendering code (#353).
- `render::build_tab_drop_groups()` + `compute_tab_drop_zone()` + `compute_tab_drop_overlay()` — tab drag-and-drop drop-zone computation (delegates to `quadraui::compute_drop_zone()`) and overlay geometry (highlight rect, insertion bar, ghost position). Both backends build a `tab_slots_map` (backend-specific measurement) and `DropGroupBounds`, then call shared functions. Zero per-backend drop-zone algorithm code (#345).
- `render::screen_zone_hit_test()` + `window_zone_hit_test()` + `resolve_gutter_action()` — screen-level click zone detection (tab bar, window, breadcrumb, divider), window sub-zone detection (gutter, status bar, scrollbar, text area), and gutter action resolution. GTK caches `ScreenLayout` from paint; both backends call shared functions for zone detection. Tab bar inner slot resolution (Pango vs char-cell) stays per-backend (#344).
- `render::build_tab_bar_primitive()` + `breadcrumbs_to_quadraui_status_bar()` — tab bar and breadcrumb bar primitives pre-built in `ScreenLayout` (#347). Both backends draw directly from `GroupTabBar.bar` / `BreadcrumbBar.bar` / `ScreenLayout.tab_bar_primitive`. Zero per-backend adapter construction or `show_split` logic.
- `render::picker_panel_to_palette()` + `PickerGeometry` — ALL picker types (file/symbol/command/branch, with/without preview, flat/tree) route through one adapter to `quadraui::Palette`. `PickerGeometry::compute()` + `PickerSizing` constants give a single source of truth for popup bounds. Zero per-backend picker rendering code (#402).
- `Engine::needs_clipboard_for_paste()` + `prepare_paste_clipboard()` — paste-key detection and clipboard register loading (#381). Both backends call the same two engine methods before `handle_key()`. Zero per-backend paste detection logic.
- `Engine::clipboard_read` + `clipboard_write` callbacks — clipboard access routed through engine-owned closures (#417). GTK `setup_gtk_clipboard()` wires `gdk4::Display` clipboard once at startup; TUI wires `copypasta` provider. Six GTK call sites (yank sync, paste prep, hover-popup copy, terminal copy/paste, AI panel Ctrl-V) consolidated. Zero per-backend clipboard logic beyond the one-time provider setup.
- `Engine::handle_explorer_mouse_event()` — single-click row dispatch (toggle dir / preview file) for explorer TreeController events (#415). Both backends route mouse events through `TreeController.handle()` → `handle_explorer_mouse_event()`.
- `render::compute_editor_layout(engine, total_height, line_height, menu_in_viewport) -> EditorLayout` — one-shot layout computation for all chrome heights (#386). GTK passes pixel units, TUI passes `line_height=1.0` for row units. Replaces `gtk_editor_bottom`, `gtk_terminal_target_maximize_rows`, TUI `terminal_target_maximize_rows_tui`, and the unused `editor_bottom_px`.
- `Engine::handle_completion_click(CompletionsHit) -> bool` — click-to-pick on completion popup (#288). Both backends cache `CompletionsLayout` from render, call `hit_test()` at click time. `Item(idx)` → apply + dismiss, `Inert` → dismiss, `Empty` → dismiss + fall through.
- `Engine::context_menu_hit_to_idx()` + cached `ContextMenuLayout` — context menu click/hover via `hit_test()` (#210). Both backends cache layout from render. GTK motion handler + click handler + TUI click + motion handlers all replaced with shared `hit_test()`. `resolve_context_menu_click()` gated to `#[cfg(test)]`.
- `Engine::resolve_bottom_panel_zone()` + `BottomPanelGeometry` — cached vertical geometry for bottom panel zone detection (#418). Explicit `toolbar_y`/`content_y`/`content_row_h` offsets (not uniform `row_h`) so GTK's taller tab bar gets correct zones. Both backends cache at paint time.
- `Engine::handle_terminal_split_click(TerminalSplitHit) -> bool` + cached `TerminalSplitLayout` — terminal split divider detection, pane focus, and selection via quadraui `hit_test()` (#430, quadraui#196). Both backends cache split layout from `build_terminal_draw_data()`. Zero per-backend divider math.
- `quadraui::AppShell` + `engine::sidebar` — sidebar visibility and active panel owned by the engine (#385). TUI reads all state from `engine.app_shell`; panel switching, focus flags, and session persistence handled by engine methods (`toggle_sidebar_panel`, `focus_sidebar_panel`, `handle_nav_overflow`). GTK `sync_sidebar_from_engine()` reads engine state; `sync_sidebar_widgets()` updates GTK widget visibility via `active_panel_id: String` + lookup-table arrays (#408/#409 removed `SidebarPanel` enum). ExtPanel panels bypass AppShell — `sync_sidebar_from_engine()` checks `ext_panel_active` (#413).

**North-star ("developer doesn't need to know the backend") status after B.5:**

- ✅ True for picker / status-bar / tree / dialog / context-menu / tooltip-shaped surfaces — adding a new instance means writing data + handlers, never touching Pango/cells.
- ✅ True for **rich-document** popups since #214 shipped + #266 lifted both rasterisers — adding new rich popups means writing a `RichTextDocument` and handlers, never touching Pango/cells.
- ⚠️ **Hit-test glue partially shared** (#210/#344) — screen-level zone detection (tab bar, window, divider, breadcrumb) and window sub-zone detection (gutter, status bar, scrollbar, text area) now shared via `render::screen_zone_hit_test` + `window_zone_hit_test`. GTK caches ScreenLayout from paint (#344). Remaining per-backend: motion-handler → `selected_idx` wiring for primitive surfaces (#210), tab bar inner slot resolution (Pango vs char-cell).
- ❌ No `Backend::watch_file(path) -> Stream<FileEvent>` trait method — every backend rolls its own watcher (TUI poll, GTK GIO). Suppress decision is shared (#201) but not the watcher invocation.
- ✅ **Editor viewport lifted** (Phase C Stage 1 / #276). Both backends paint through `quadraui::{tui,gtk}::draw_editor`. The vim-motion-suite vision (PLAN.md) is now unblocked at the paint layer; engine-slice extraction (Phase 2 — `editor_core` crate carving out `keys.rs` + buffer + LSP) remains as a separate multi-month wave.
- ✅ Win-GUI is a thin wrapper again: `src/win/` over `quadraui::win`, `win` feature (#866, 2026-09-11). The old `src/win_gui/` backend was deleted on 2026-05-11 (`3e4bcff`); open `Win-GUI:` issues filed before 2026-09-11 may describe that deleted code (see Milestone hygiene above).

---

## Recent Work

> Sessions 389 and earlier in **SESSION_HISTORY.md**.

**2026-09-04 — #801: `/` and `:s` got a real regex engine.** New
`src/core/vim_regex.rs` translates Vim patterns (all four magic levels, `\<`/`\>`,
`\{n,m}`/`\{-}`, `\zs`/`\ze`, `\c`/`\C`, the character classes, `~`) into Rust
`regex`, and **rejects** what it cannot express instead of falling back to literal
matching. `run_search` and `:s` both use it; search offsets (`/pat/e`, `/e+1`, `/b+2`,
`/+1`), `;` chaining, `//` reuse and `3/pat` all work; `*`/`#` now set a real
`\<word\>` pattern. `parse_ex_address`/`parse_ex_range` implement the full ex address
grammar, which `:s`, `:g`/`:v`, `:d`, `:y`, `:j`, `:>`, `:<`, `:t`, `:m` and `:normal`
now all accept. `:s` gained replacement expansion (`& \0 \1 \u \U \L \E \r \t`),
the `g c e i I n &` flags (`c` errors rather than being silently dropped), `:&`/`:&&`,
counts and `|` chaining. **`KNOWN_DEVIATIONS` 638 → 465** (−173): the `search`, `sub`
and `g` conformance categories are clean apart from operator-pending `d/pat` (the next
issue in the #801 chain), `gd`/`gn`, and `\1` back-references.

**2026-09-03 — the chain drained; #7 closed out; the audit run.** #751–#756 converged
mouse routing, #757–#762 keyboard dispatch, #763–#766 frame composition (`FrameOp` /
`compose_frame`, then the deletion of `draw_frame`). #657 promoted `render`/`tui_main`/
`gtk` into `[lib] vimcode_core` and sealed `tests/acceptance/`. #730/#593/#731/#732/#658/
#480/#550/#551 all closed; #146 moved to #4. Milestone #7 reached **0 open**. Ran the
post-#735 sizing audit the previous revision mandated and added `scripts/prod_lines.py`
so it is reproducible: over the chain's own range backends **−728** against a
−8,700…−9,500 projection, `render.rs` **+5,847**, net **+5,119**. (The −3,656/+6,396/
+2,740 figures this entry first carried measure 08-31 → 09-03, which pools in
#722–#732's dead-code deletion — see the corrected section above.) #47 closed having shipped **no code** (`44882e9`) with its
`Backend`-trait Rc-handle blocker filed nowhere — the top open action. *(Corrected
2026-09-05, issue #827: that blocker — quadraui#699, at 19 not 44 call sites — was
filed and closed the same day, 16:38Z–17:11Z, and #47 was reopened 16:38Z. This
entry's "filed nowhere" was already wrong by the time the revision carrying it was
written; see the corrected section above.)*

**2026-09-05 — GOALS.md/PLAN.md/PROJECT_STATE.md/IRREDUCIBLE_SURFACE.md corrected
(#827).** A four-agent audit of `develop @ ee26268` found the planning docs
materially stale: the #47-blocker-unfiled claim (quadraui#699 had already closed),
the 44-call-site figure (real count 19), the quadraui#481/#482 "duplication moved
down a level" claims (mostly refuted at the pinned rev), the `src/gtk/` size-table
column (predated #785's move), and the `IRREDUCIBLE_SURFACE.md` folder-picker
verdict (wrong — `FolderPickerController` has existed in quadraui since 05-25).
Corrected all four docs; no code changed.

**2026-09-01 — platform-neutrality audit, and everything it found is now queued.**
Filed #730 (`ai_panel`), #731 (orphan handles), #732 (`Msg` bus), #733 (mouse routers),
#734 (keyboard), #735 (frame composition). Re-scoped #593 (unblocked, `GtkDriver`
supersedes its smoke plan), #657 (audit run and recorded, fixture list corrected, freeze
contradiction flagged) and #47 (macOS: thin wrapper, not Core Graphics). Moved #146 out
of #7. Queued all of it plus quadraui#596/#597 — 16 entries, two parallel chains. #592
given an audit comment and deliberately **left open** on `ai_panel`. Docs: PRs #729
(PROJECT_STATE + PLAN) and #736 (GOALS).

**2026-08-26 → 09-01 — the #592 epic and the dedup sweep cleared.** #669/#670/#671/#672
(GTK live-path paint + `draw.rs` deletion), #676 (Command Center), #673/#674/#677 (tab
MRU, jump-list pane identity, vacuous-test rewrites), #621/#659/#660/#536 (dedup),
#691 (quadraui pinned as a git rev instead of a sibling path dep), #693/#694/#695
(menu-bar paint + hamburger), #699–#705 (VS Code chrome-metrics parity), #35 (minimap
primitive, both backends), #710/#712 (omnibar + dropdown fonts), #715/#716/#719/#720
(WM identity, titlebar glyphs, app icon), #722/#723 (per-pane minimap, scroll thumb).

**2026-08-26 — both `ShellApp` migrations closed.** #448 (GTK) and #595 (TUI).
`fn event_loop` deleted from `src/` (#634).
