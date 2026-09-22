# `shell_app.rs` re-audit (round 3) — #1192

> **This document introduces no production code.** Same rule as
> `docs/SHELLAPP_CONVERGENCE.md` (#950) and `docs/BACKEND_SETUP_AUDIT.md`
> (#260): it is the audit, not the convergence. `src/tui_main/` production-line
> delta for this PR is **0** (`scripts/prod_lines.py src/gtk src/tui_main
> src/render.rs` unchanged) — this is exactly the "some issues legitimately
> will not shrink it" case `GOALS.md`'s #1044 section asks every #1169 PR to
> declare.

## 0. What this is, and what it corrects

#1044 (2026-09-16, commit `32d7b27`) decomposed `impl ShellApp for
TuiShellApp` + `mouse::handle_mouse` into 98 rungs and filed a 16-item wave
work order, all recorded in `GOALS.md`. #1192 asks for a round-3 re-audit of
`shell_app.rs` specifically (the file `#1108`'s five-file audit explicitly
excludes, on the theory #1044 already covered it), because the file grew
**+870, +22%** since #1044, no open issue has looked at that growth, and
#1167's convergence shrank `src/tui_main/` less than projected in the same
window.

**Correction before anything else: the "3968" baseline in #1192's own issue
body does not match the audit's actual measured baseline.** The #1044 audit
commit (`32d7b27`, 2026-09-16 19:52) — the same commit that built the rung
table being re-diffed here — measured `shell_app.rs` at **4,459** production
lines, not 3,968 (`git show 32d7b27:GOALS.md`, line 76: *"of which
`src/tui_main/shell_app.rs` — 4,459 (18,442 incl. tests..."*). "3968" appears
nowhere in this repo's git history of `GOALS.md`/`PROJECT_STATE.md`; it is
most plausibly the file's size at the moment the #1044 *issue* was filed,
before the audit itself was written days later (the same window in which
#1043 and other in-flight work were already landing). Grading this round
against the issue-filing-time number overstates the growth `#1169`'s
convergence work is actually responsible for by more than 2×. This audit
uses the **measured, committed baseline (4,459 @ `32d7b27`)** throughout,
since that is the number the rung table itself was built against, and states
both figures wherever it matters so the correction is traceable.

## 1. Sizing — three points, not two

| Commit | Date | `shell_app.rs` prod lines | Δ | Note |
|---|---|---:|---:|---|
| `32d7b27` (#1044 audit) | 2026-09-16 | **4,459** | — | Rung table's actual baseline (see §0) |
| `30c0077` | 2026-09-19 | **4,773** | +314 | `GOALS.md`'s own tracked snapshot — independently confirmed here (`git show 30c0077:src/tui_main/shell_app.rs \| prod_lines.py`) |
| `HEAD` (`62b757d`) | 2026-09-20 (today) | **4,838** | +65 | Current worktree |
| **Total since #1044** | | | **+379** | **9%, not +870/22%** |

`src/tui_main/` as a whole is 11,057 today (confirmed via
`scripts/prod_lines.py`), consistent with the +20-since-`721f670` figure
already tracked in `GOALS.md`'s epic-level table. This audit only re-derives
`shell_app.rs`'s own share of that.

## 2. The +379 lines, attributed to every commit that moved them

`shell_app.rs` had **48 commits** touch it between `32d7b27` and `a1c11c9`
(plus two more, `1c1d6a8`/`c2a39b7`, that touch it but net to 0 prod lines —
confirmed, not just asserted). Every commit's prod-line delta was measured
directly (`git show <sha>:src/tui_main/shell_app.rs \| scripts/prod_lines.py`
at each point in the chain, not inferred from `--stat`, which measures raw
lines and is dominated by test additions — see the gap between raw net
(+5,217 lines, `git diff --stat 32d7b27 a1c11c9`) and prod net (+379) below).
**37 of the 48 commits are net `+0` production lines** — their entire raw
diff landed inside `#[cfg(test)]`, which is the CLAUDE.md-mandated black-box
driver-test tax on every behaviour PR, correctly excluded by
`scripts/prod_lines.py`'s method. The 11 non-zero commits, plus the P0 harness
prerequisite, are the whole story:

| Δ prod | Commit | Bucket | One-line reason |
|---:|---|---|---|
| +25 | `d7555ff` Fix #1043 review | **shared test infra** | `Engine::from_engine` made `pub(crate)` for the `::tui_prod` conformance-harness arm #1044 called P0 — infra, not duplication. |
| +4 | `11043c7` Fix #1057 | **convergence residue** | Wave-1 item 5 (`BottomItemClicked` toggle-vs-show) — done. |
| +36 | `d052bdc` Fix #1062 | **convergence residue** | Wave-2 item 10 (`PanelChanged`/`SidebarHidden`/`SidebarResized` shadow sync) — done; adds `render::sync_shell_event_shadow` call site + an 8-line `TuiShellShadowHost` impl + doc comments explaining what stays TUI-local and why (see diff, `git show d052bdc -- src/tui_main/shell_app.rs`). |
| +45 | `229ddb1` Fix #1063 | **convergence residue** | Wave-2 item 11 (menu-action `EngineAction` applier → `render::apply_engine_action`) — done; adds the `TuiEngineActionHost` seam GTK's mirror-image `GtkEngineActionHost` already has. |
| +7 | `9618a4c` Fix #1067 | **convergence residue** | Wave-3 item 15 (panel-hover-popup link click-to-copy) — done. |
| +61 | `1375810` Fix #1117 | **bug fix (organic)** | Stale `explorer_tree_rect` `Cell` outliving a hidden sidebar could mis-claim an editor click; re-derives `app_shell` sidebar-visibility at `from_engine` construction time. Diff-reviewed (§4): GTK's own `App::explorer_ui_event` path was already correct — no new cross-backend gap. |
| +39 | `c1b3af6` Fix #1117 review | **convergence residue** | Same fix, converged onto the identical predicate `App::explorer_ui_event` already uses (`rect.width > 0.0` alone, no extra staleness guard needed once the root cause was fixed). |
| +4 | `1668d98` Fix #1094 | **bug fix (organic)** | Editor scrollbar paints outside the minimap strip instead of before it — z-order fix, cosmetic, TUI-side paint-order detail. |
| +44 | `fa3666c` Fix #1134 | **convergence residue** | `gx`/`open_url`/`reveal_in_file_manager` routed through quadraui `PlatformServices` — new shared rung, TUI side adds the call-site glue. |
| +20 | `5b8885a` Fix #1107 | **convergence residue** | `shell_config`'s icon table converged across GTK/TUI onto one source. |
| +9 | `9ac269a` Fix #1124 | **convergence residue** | Window title-sync/minimize routed through `Backend::window()` — substitutive-service adoption, same shape as the #901/#902 pattern. |
| +20 | `09d9de9` Fix #1125 | **convergence residue** | `save_workspace_as_dialog`/`open_file_dialog` now call the shared `render::run_save_workspace_as_dialog`/`run_open_file_dialog` rung (quadraui#965) instead of TUI's old unconditional-write / in-canvas-fuzzy-finder shortcuts. Adds one `backend: &'a mut dyn quadraui::Backend` field to `TuiEngineActionHost` — genuinely per-backend (TUI has `backend` in scope synchronously; GTK defers to `tick()`), documented as such. |
| +35 | `e93d2fa` Fix #1109 | **convergence residue** | Adopts quadraui#1015 `set_caret_shape` + reads `kitty_keyboard` from `BackendCaps` instead of a redundant crossterm probe — a milestone-#7 consume-side item (listed in `GOALS.md`'s own open-issues line), not one of #1044's original 16. |
| +14 | `1b44610` Fix #1165 | **convergence / gap-fix** | The two `tick()`-body audit explicitly named out-of-scope for reopening by #1192's own notes — landed 2 real gap fixes. Counted here for completeness, not re-litigated. |
| +3 | `d250420` Fix #1155 | **new behaviour (core-owned)** | Quickfix/location-list ex-commands — the feature lives in `src/core/`; `shell_app.rs`'s only touch is a viewport-size line, no TUI-only surface added. |
| +13 | `eb1b20b` Fix #1164 | **convergence / gap-fix** | `tick()`'s bottom-chrome estimate converged onto one model — the other #1192-exempted item. Counted for completeness. |
| **+379** | | | **Sums exactly to the measured total (§1) — no unattributed residual.** |

### Bucket totals

| Bucket | Lines | Share |
|---|---:|---:|
| Convergence residue (wave items done + milestone-#7 adoptions) | 286 | 75% |
| New behaviour / organic bug fixes | 68 | 18% |
| Shared test/harness infra (the P0 prerequisite) | 25 | 7% |
| **Fresh divergence** (new TUI-only path, no `App` counterpart) | **0** | **0%** |

**The third bucket — the failure mode this epic exists to stop — is empty.**
Every non-test line added to `shell_app.rs` since #1044 either (a) is the
harness plumbing #1044 itself asked for as a P0 prerequisite, (b) is the
TUI-side half of a convergence whose GTK-side half shrank or gained the
matching call by the same PR (verified by reading the diffs, not just the
commit message — see the `git show` excerpts spot-checked in §3), or (c) is
a small, backend-symmetric bug fix / feature addition that does not create a
TUI-only capability GTK lacks. This is a genuine, checked result, not an
assumption: 8 of the 11 non-zero commits' actual diffs were read in full
during this audit (not just their subject lines), specifically to rule out
bucket (c) hiding a bucket-(d) divergence behind convergence language.

## 3. Why "convergence residue" isn't a red flag here

A convergeable rung moving its *decision* into `render::`/`core::` does not
make the TUI call site shrink to zero — it still needs: the call to the
shared function, a `Host`-trait impl for the 1-3 fields that genuinely differ
per backend (mirroring the existing `dispatch_panel_accelerator` shape), and
— this repo's actual convention, confirmed by reading every diff above — a
paragraph of doc comment explaining *why* the remaining TUI-local code is
still there and isn't itself duplicated logic (e.g. `d052bdc`'s comment
tracing `focus_sidebar_panel`'s TUI-only bookkeeping back to "GTK gets real
widget focus from the toolkit instead"). That documentation cost is real and
appears throughout every wave commit; it is not itemized separately above
because it is inseparable from the wiring it explains, but it plainly
accounts for a meaningful fraction of the "+wiring" lines in the table.
None of it is duplicated *behavior* — the whole point of each of these PRs
was deleting the duplicated behavior and leaving exactly this residue behind.

## 4. #1167's arithmetic — the sharpest question, answered

#1167 ("converge `paint_editor_popups` into one `render.rs` rung") does
**not touch `src/tui_main/shell_app.rs` at all** — its diff is entirely in
`src/tui_main/render_impl.rs`, `src/app.rs`, and `src/render.rs`. It is
`#1108`'s file to audit fully, but #1192 explicitly asks this arithmetic be
resolved here since it's the sharpest evidence for "the epic's number is
going the wrong way." Measured directly (`git show 0c34cf4^`/`0c34cf4` +
`scripts/prod_lines.py`):

| File | Before | After | Δ |
|---|---:|---:|---:|
| `src/tui_main/render_impl.rs` (TUI) | 1,344 | 1,322 | **−22** |
| `src/app.rs` (GTK) | 8,860 | 8,849 | **−11** |
| `src/render.rs` (shared) | 22,549 | 22,692 | **+143** |
| **Net across all three** | | | **+110** |

**Answer: the convergence did not delete what it was expected to.** The
commit message calls the two backend functions "~160/~164-line
near-duplicates," which implied a combined removal in that range once one
shared version replaced both. What actually happened is each backend's
*own* function shrank by only 11–22 lines (to "exactly what stays genuinely
per-backend: finding the active window and resolving its five anchor
points," per the commit message) while the new shared
`render::paint_editor_popups` function landed at **+143** lines in
`render.rs` — bigger than "one copy of the ~160-line duplicate," because it
absorbed logic that used to be interleaved with each backend's own anchor
resolution (the five build-adapter→layout()→draw_*→cache blocks) rather
than being a pure move of an already-self-contained function. This is a
**real shortfall against the projection**, not a wash — `src/tui_main/`
alone dropped only 22 lines from a change whose framing implied ~160, and
the three-file total *rose*. It is exactly the "something else in the same
batch added more than it removed" branch of #1192's either/or, and it is a
concrete, sourced answer #1108's own audit of `render_impl.rs` should cite
rather than re-derive.

## 5. The rung table — diffed against #1044's, not rebuilt

#1044's `ShellApp` trait-method rung table (`GOALS.md` §"ShellApp
trait-method rungs") is the one table that lives entirely inside
`shell_app.rs` — the `mouse.rs` rung table is `mouse.rs`'s own territory
(#1044/#1068), out of this file's scope, and untouched by this audit.

| Rung | #1044 verdict | R3 verdict | What changed |
|---|---|---|---|
| `render_content`: 12/14 `FrameOp` arms | already-shared | **unchanged** | No new arm added since #1044; still #824. |
| `FrameOp::CommandLine` base paint | irreducible | **unchanged** | — |
| `FrameOp::CommandLine` click→offset hit-test | already-shared | **unchanged** | — |
| `FrameOp::CommandLine` selection-highlight paint | convergeable (quadraui-gap resolved) | **✅ done** | `#1185` adopted `Backend::draw_command_line_selection` (quadraui#1001) on both backends; `render::command_line_selection_rect`'s stale doc comment corrected in the same PR. |
| `FrameOp::TabSwitcher` | convergeable (bug) | **✅ done** | `#1056` fixed the `max_visible`/`visible_rows` mixup. |
| Menu/Command-Center/sidebar-hover/panel-key/`ClipboardPaste` | already-shared | **unchanged** | — |
| Window-control/CSD/native-menu/`CharTyped`/`WindowClose` | irreducible | **unchanged** | — |
| Alt-menu-letter reveal, hamburger one-shot guard | irreducible | **unchanged** | — |
| `KeyPressed` decode | convergeable | **✅ done (GTK-side)** | `#1060` routed GTK's 4 kept-GTK-spelled keys through `render::engine_key_from_ui`; TUI side (this file) needed no behavior change, only test coverage (+0 prod). |
| Menu-action → `EngineAction` applier | convergeable | **✅ done** | `#1063` — `render::apply_engine_action` + `TuiEngineActionHost`/`GtkEngineActionHost` seam. |
| `PanelChanged`/`SidebarHidden`/`SidebarResized` shadow sync | convergeable | **✅ done** | `#1062` — `render::sync_shell_event_shadow` + `TuiShellShadowHost`. |
| `BottomItemClicked` (Settings) | convergeable | **✅ done** | `#1057`. |
| `take_requested_panel` | convergeable | **not evidenced here** | No `shell_app.rs` diff touches this rung in the audited window — the fix (adopting the override on GTK) lives in `app.rs`, outside this file. Status unconfirmed by this audit; #1068/#1108's territory. |
| `on_bottom_panel_event` | convergeable, low priority | **unchanged** | Still dead on both sides, still blocked the same way. |
| `tick` chore lists | irreducible | **downgraded → 2 real gaps found, both fixed** | `#1165` (audit) + `#1164` (fix) found the bottom-chrome-estimate rung was *not* purely irreducible as #950/#1044 assumed — it had drifted between the two `tick()` bodies. Both explicitly out of scope to reopen per #1192's own notes; recorded here only so the table stays a true diff. |

**New rungs, not in #1044's original 14-row table, discovered by this
growth window** (all `shell_app.rs`-local, all resolved as convergence
adoptions, none left open):

| New rung | Verdict | Note |
|---|---|---|
| `gx`/open-url/reveal-in-file-manager | ✅ convergeable, done | `#1134` — routed through quadraui `PlatformServices`. |
| `shell_config` icon table (setup-time config assembly) | ✅ convergeable, done | `#1107`. |
| Window title-sync / minimize | ✅ convergeable, done | `#1124` — `Backend::window()`, same substitutive-service shape as #901/#902. |
| Hardware caret shape / keyboard-enhancement probe | ✅ convergeable, done | `#1109` — quadraui#1015 adoption. |
| `save_workspace_as_dialog` / `open_file_dialog` | ✅ convergeable, done | `#1125` — quadraui#965 native dialogs, replacing TUI's unconditional-write / in-canvas-fuzzy-finder shortcuts. |
| Explorer stale-rect sidebar-visibility resync | bug, not a rung | `#1117` — GTK was already correct; no gap, see §2/§6. |

Every new rung this window produced is **already closed**, several without
having been named by #1044's original inventory at all — they were found
and fixed inline as bugs/adoptions, not left as debt.

## 6. Cross-check against #1165's method — any new GTK/macOS/Win gap?

#1165 asked the two `tick()` bodies "does GTK have this too?" and found two
real gaps. Applying the same question to every rung that grew in this
window (§2, §5):

- `#1057`/`#1062`/`#1063`/`#1067`/`#1107`/`#1109`/`#1124`/`#1125`/`#1134`:
  each commit's own diff/message states the GTK-side counterpart explicitly
  and either already had the behavior or gained it in the same PR (`Host`
  trait seams exist on both sides — `GtkEngineActionHost`/`TuiEngineActionHost`,
  `GtkShellShadowHost`/`TuiShellShadowHost`). No one-sided landing found.
- `#1117` (explorer stale-rect fix): GTK's `App::explorer_ui_event` is gated
  one level up by `try_route_sidebar_mouse_event`'s freshly-computed
  `ctx.layout.sidebar_content_bounds` — it never had TUI's stale-`Cell`
  problem in the first place (documented in the fix's own comment, quoted
  in `git show 13758100b`). **Not a gap** — GTK's design already avoided it.
- `#1094` (scrollbar/minimap paint order): purely a z-order fix within TUI's
  own cell-paint substrate; no GTK equivalent exists to diverge from (GTK's
  minimap and scrollbar are painted through a different, already-shared
  `render::` rung that was never in the wrong order).
- `#1155` (quickfix/location-list): core-engine feature, both backends read
  the same `Engine.quickfix`/`location_lists` state; `shell_app.rs`'s
  3-line touch is a viewport-size constant, not a TUI-only code path.

**No new cross-backend gap was introduced by this growth window.**

## 7. Sequenced work order

Given §2–§6, the finding is that `shell_app.rs`'s growth is **benign** — no
new item is needed to converge *this file* further; every commit that added
lines to it either already finished the job or has a documented reason the
residue is irreducible glue. The actionable follow-ups this audit actually
surfaces are bookkeeping/verification, not new production code in
`shell_app.rs`:

1. **(cheapest, do first)** File a follow-up against `#1108`/the
   `render_impl.rs`/`render.rs` pairing to resolve §4's shortfall: why did
   `paint_editor_popups`'s shared extraction land at `render.rs: +143`
   instead of nearer the "~160-line duplicate, once" the commit message
   implied — was the pre-existing per-backend code more entangled with
   other logic than the two functions' line counts suggested, or does
   `render::paint_editor_popups` now carry logic that didn't need
   centralizing? Purely investigative — no code change implied unless the
   answer turns up a real missed deletion.
2. **Verification pass, no code:** confirm wave-1/2 items 2, 3, 7, 9, 12 and
   13 from #1044's original 16 (dead `h_sb_drag_cell`, dead-shadowed
   wheel-scroll arms, `click::dispatch_tab_bar_target` routing, scrollbar
   geometry extraction, `take_requested_panel` GTK adoption, Git/Search/
   Settings wheel-scroll) actually landed — none of them touch
   `shell_app.rs`, so this audit cannot confirm them, and `GOALS.md`'s
   "#1043 and #1053–#1067 landed" claim should not be taken as covering
   files this audit didn't read. Belongs to `#1068`/`#1108`'s scope, not a
   new issue — flagging so it isn't silently assumed done.
3. **Documentation fix, trivial — done (#1259):** future rung-audit issues
   should cite the audit *commit's* measured baseline (as this document does
   in §0), not a number frozen at issue-filing time — the 3968-vs-4459 gap in
   #1192's own body cost real analysis time here for a discrepancy that a
   one-line convention fix prevents recurring. Recorded in `GOALS.md`'s "How
   to use this doc" section (the "Milestone discipline" bullets audit-issue
   authors already read) so it's visible before the next rung-audit issue is
   filed, not just here.
4. **No `shell_app.rs`-specific convergence issue is filed by this audit** —
   the highest-risk item found (§4) belongs to a file this audit does not
   own, and every other finding closes clean.

## 8. Explicit statement (acceptance criterion)

**The growth is benign.** +379 production lines since #1044's measured
baseline (not +870/+3968 — see §0's correction), of which 75% is
already-completed convergence-wiring residue, 18% is small organic bug
fixes/features with no cross-backend gap, 7% is the P0 test-harness
infrastructure #1044 itself asked for, and **0% is fresh divergence**. The
single highest-divergence-risk item this audit found is not in
`shell_app.rs` at all — it is **#1167's shortfall** (§4): a "convergence"
PR whose net effect across `render_impl.rs`+`app.rs`+`render.rs` was **+110
lines**, not the reduction its own commit message implied, which is the
concrete mechanism behind the epic-level number moving the wrong way that
`#1108` should resolve on `render_impl.rs`'s own audit.
