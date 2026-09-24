# VimCode Project State

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
  #172, #176) describe a backend **deleted from this repo on 2026-05-11** (`3e4bcff`).
  Their live counterparts are quadraui#19–#31 / quadraui#580. They should be migrated or
  closed rather than left to imply `src/win_gui/` still exists.

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
- ⏭️ Win-GUI removed from this repo on 2026-05-11 (`3e4bcff`). Will be re-added as a thin wrapper when quadraui ships its Win backend (quadraui#19–#31, quadraui#580). The `Win-GUI:` issues still open on *this* tracker describe that deleted backend — migrate or close them (see Milestone hygiene above).

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
