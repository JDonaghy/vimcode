# ShellApp convergence — decomposition (#950)

> **What this is.** #950 found that vimcode has two `impl ShellApp` —
> `App` (`src/app.rs`, GTK/macOS/Win-GUI) and `TuiShellApp`
> (`src/tui_main/shell_app.rs`, TUI) — and that the TUI one converts
> quadraui `UiEvent`s *back* into crossterm `MouseEvent`s to feed a
> 2,800+-production-line TUI-private click router (`src/tui_main/mouse.rs`)
> — production lines only, excluding its `#[cfg(test)] mod tests`; the
> parent issue's "3,750-line" figure is the whole-file count *including*
> tests, so the two numbers measure different things, not a discrepancy —
> in violation of quadraui's portability rule 6 ("events are unified at the
> `UiEvent` boundary"). The issue is explicitly scoped as an epic: propose a
> decomposition, land the cheap independent wins, leave the actual
> convergence (starting with the mouse router) to follow-up issues. This
> document is that decomposition. **No mouse-router convergence work has
> been attempted here** — see "Why the mouse router is not in this PR"
> below.

## The sorting rule: essential vs. accidental

`docs/IRREDUCIBLE_SURFACE.md` already established the working definition for
this codebase, applied here to #950's four findings:

- **Essential** — a genuine difference in what the platform *is* (input
  modality, rendering unit, presence/absence of a native chrome element).
  Forcing these to converge would make both backends worse, not better; the
  correct move is to record the difference once (a fixture, a doc comment,
  a cfg), not to keep re-deriving it.
- **Accidental** — two independent implementations of the same policy that
  drifted apart because nobody wrote the shared version, or because a
  shared version exists but one backend still carries a pre-shared-version
  copy. These converge onto one implementation; leaving them split is
  ordinary duplication risk (the SEARCH_COD/SEARCH drift below is exactly
  the failure mode: nobody *decided* GTK should show a different search
  glyph, it just quietly forked).

## #950's four findings, sorted

| # | Finding | Verdict | Why |
|---|---|---|---|
| 1 | TUI round-trips `UiEvent` → crossterm `MouseEvent` to feed `mouse.rs` instead of routing through `src/click.rs` | **Accidental** — the worst one | `src/click.rs`'s own module doc says it was extracted from `src/gtk/click.rs` specifically because nothing in it is GTK-specific (#862); it already works purely in `quadraui::Backend`/`Engine`/geometry terms, i.e. the exact "unify at `UiEvent`" design rule 6 asks for. `mouse.rs` predates that extraction and nobody has gone back to point it at the shared router. Not a case of "the terminal genuinely needs different hit-testing" — cell-vs-pixel geometry is the one place a real difference exists (see `IRREDUCIBLE_SURFACE.md` fact #1), and `click.rs` already abstracts over exactly that (GTK gets pixel-accurate `TabBarPixelHits` from the rasteriser, TUI falls back to char-cell `hit_regions` — same function, two data sources). |
| 2 | `shell_config` built twice from different icon tables (`SEARCH_COD` vs `SEARCH`) | **Accidental** — landed in this PR | Nothing about a magnifying-glass icon is platform-specific; `SEARCH_COD` was a second nerd-font codepoint (`nf-cod-search`) that only `App::shell_config()` used, while `TuiShellApp::shell_config()` and every *other* activity-bar icon in both tables already shared the one `crate::icons` table. Converged onto `SEARCH`; `SEARCH_COD` deleted. |
| 3 | Two separate `tick` chore lists | **Mostly essential, drifting toward accidental in bookkeeping only** | `App::tick`/`handle_poll_tick` (`src/app.rs`) and `TuiShellApp::tick` (`src/tui_main/shell_app.rs`) are *not* duplicate logic — GTK's tick reloads CSS and polls `settings.json` mtime; TUI's tick recomputes a cell-grid viewport (`vw`/`vh` in terminal cells, `content_rows`/`content_cols`) from `backend.viewport()` every frame, which has no GTK equivalent because GTK's viewport is derived from live Cairo/Pango metrics instead. That split is `IRREDUCIBLE_SURFACE.md` fact #1 (px vs. cells) showing up again, and forcing one shared `tick` would mean threading a `TextMetricsBackend`-shaped abstraction through both, which is a real quadraui-infrastructure project (see slice 3 below), not a cheap win. What *is* accidental: each tick's comments independently re-derive "why is this list what it is" (e.g. #949's settings-reload note only lives in `App::tick`, TUI's #731-style hover-feature history only lives in its own tick) instead of pointing at one shared "what a tick does on each backend and why" doc section — a documentation consolidation, not a code change. Left as a follow-up (slice 4). |
| 4 | The panic hook copied four times | **3 accidental + 1 essential** — landed in this PR | GTK/macOS/Win-GUI (`src/gtk/mod.rs`, `src/macos/mod.rs`, `src/win/mod.rs`) carried byte-identical closures — pure copy-paste, no reason for three copies. Extracted to `crate::core::swap::install_gui_crash_hook()`; all three now call it. TUI's hook is **essential** and was *not* merged in: it writes via `debug_log!` instead of `eprintln!`, because a terminal backend owns raw mode / the alternate screen at panic time, where an `eprintln!` is either invisible or corrupts the screen the user is looking at — a real modality difference, not a missed refactor. |
| — | `CREATE_NO_WINDOW` re-inlined ~12× in `core/` despite `git::hidden_command` existing | **Accidental** — landed in this PR | Every one of these was the same idiom (`cmd.creation_flags(0x08000000)` behind `#[cfg(windows)]`) with no per-call-site reason to hand-roll it. Two needed `CREATE_NEW_PROCESS_GROUP` too (LSP/DAP child processes, so the whole process group can be signaled without touching the editor's own console) — that's a genuine second policy, not a reason to keep re-inlining flags, so it got its own named helper (`hidden_command_new_process_group`) rather than being left split. All ~12 call sites now go through one of the two `core::git` helpers; `git_command()` itself was hand-rolling the same flag it now delegates to `hidden_command("git")`. |

## Why the mouse router is not in this PR

Finding 1 is explicitly named in the issue as "the obvious first slice," and
it is the biggest single item (routing TUI clicks through `src/click.rs`
instead of `mouse.rs`+crossterm), but it is not a cheap win:

- `mouse.rs` is ~2,800 production lines carrying vimcode's entire click
  policy for TUI — panel intercepts (debug sidebar, extensions sidebar,
  debug toolbar, explorer `TreeController`), modal-stack priority
  (`#459`), double-click folding, drag state. `TuiShellApp::handle_mouse_event`
  (`src/tui_main/shell_app.rs:1246`) already documents exactly which of
  `event_loop`'s behaviors it has to reproduce before it can even call into
  `mouse.rs` — that surface has to move to `click.rs` symbol-by-symbol, or a
  panel intercept silently regresses.
- `src/click.rs` currently answers "what did this pixel/cell hit," not "what
  should happen next" — `mouse.rs`/`App`'s `UiEvent::MouseDown` arms own the
  action dispatch. Converging means either growing `click.rs` into an action
  router (risking exactly the kind of GTK/TUI action drift #950 is trying to
  eliminate) or keeping `click.rs` hit-test-only and writing one new shared
  dispatch layer above it that both backends call — an actual design
  decision, not a mechanical move.
- Nothing about it is reversible-if-wrong the way the cheap wins are: a
  botched panel-intercept port breaks input handling silently (wrong click
  target, not a compile error), and per CLAUDE.md's testing bar it would
  need TuiDriver-tier black-box coverage for every converged intercept
  before it could land — multiple PRs' worth of work, which is exactly what
  the issue asks be split out rather than attempted in one shot here.

## Proposed decomposition, ordered by value/risk

1. **(this PR) Cheap wins** — one icon table, one GUI panic hook, one
   `hidden_command`/`hidden_command_new_process_group` pair. Zero behavior
   change, zero new test surface (see Testing below), unblocks nothing but
   removes real duplication risk immediately.
2. **Mouse router, stage 1 — inventory + hit-test parity.** Before moving
   any dispatch logic, enumerate every `mouse.rs` panel intercept and every
   `click.rs` `ClickTarget` variant side by side and confirm (with a
   TuiDriver test per intercept) that `click.rs`'s hit-testing already
   agrees with `mouse.rs`'s char-cell math for each one. This is read-only
   with respect to production code — pure verification — and de-risks
   stage 2 by turning "does this even hit the same target" from an
   assumption into a tested fact.
3. **Mouse router, stage 2 — route one intercept at a time through
   `click.rs`+a new shared dispatch layer, delete its `mouse.rs`
   equivalent, delete the corresponding slice of `uievent_to_crossterm`
   usage.** Ordered by how self-contained each intercept is; the debug
   toolbar and explorer `TreeController` intercepts look like the
   smallest/most isolated starting points from `handle_mouse_event`'s own
   doc comment, the `#459` modal-stack-priority gate (shared by all of
   them) the last thing to move since everything else depends on it running
   first. Each intercept is its own PR with its own black-box test pair
   (red-against-`develop`, green-after) per CLAUDE.md's testing bar. Ends
   when `uievent_to_crossterm` has no remaining callers in
   `src/tui_main/` and can be deleted from `src/tui_main/events.rs`.
4. **Tick chore-list documentation consolidation.** Not a code convergence
   (see finding 3's verdict) — write the "what does a tick do on each
   backend and why" explanation once, in one place both `App::tick`'s and
   `TuiShellApp::tick`'s doc comments point at, instead of two independent
   histories that will keep drifting in *wording* even though the
   underlying logic is legitimately different.
5. **`shell_config`/other-drift sweep.** A grep audit (same shape as this
   issue's own finding 2) for any other `crate::icons::*` or `ShellConfig`
   field set from two different places, now that the SEARCH_COD case is
   fixed — a cheap, bounded follow-up, not urgent.

Slices 2 and 3 are quadraui-adjacent in spirit but not in mechanics: the
click-resolution infrastructure (`src/click.rs`) already exists and is
already backend-neutral, so this is an *adoption* gap (milestone #7 in
`GOALS.md`), not a *build* gap (milestone #5) — no quadraui issue is needed
before starting slice 2.

## Cheap wins landed in this PR

- `src/icons.rs` / `src/app.rs`: deleted `SEARCH_COD`, `App::shell_config()`
  now uses the same `crate::icons::SEARCH` `TuiShellApp::shell_config()`
  already used.
- `src/core/swap.rs` (new `install_gui_crash_hook()`) / `src/gtk/mod.rs` /
  `src/macos/mod.rs` / `src/win/mod.rs`: the three identical GUI panic-hook
  closures now call one shared function. TUI's panic hook is untouched —
  see finding 4's verdict for why that split is essential.
- `src/core/git.rs` (new `hidden_command_new_process_group()`, and
  `git_command()` now delegates to `hidden_command("git")` instead of
  re-inlining the flag) / `src/core/swap.rs` / `src/core/lsp.rs` /
  `src/core/lsp_manager.rs` / `src/core/dap.rs` / `src/core/dap_manager.rs` /
  `src/core/engine/mod.rs`: every inlined `creation_flags(0x08000000)` (and
  the two `0x00000200 | 0x08000000` LSP/DAP sites) now goes through one of
  the two `core::git` helpers.

## Testing

Every change in this PR is an internal mechanism swap with identical
observable behavior on every platform (same flags end up set on the same
`Command`s on Windows; same glyph now painted on both backends where two
different glyphs painted before, which is the bug this PR fixes, not a new
behavior to spec a test around beyond what already exists):

- `App::shell_config()`'s existing test
  (`app::portable_entry_point_tests::shell_config_resolves_every_activity_bar_icon_and_reserves_the_title_bar`)
  and `TuiShellApp`'s existing `shell_config_registers_every_build_activity_bar_panel`
  already assert every panel resolves *some* non-empty icon string; neither
  hardcoded the old `SEARCH_COD` codepoint, so both still pass and would
  have failed had the fix regressed icon resolution.
- The `hidden_command`/panic-hook changes have no portable-observable
  effect to assert on in a headless `cargo test` run (Windows-only
  `creation_flags`, and a panic hook that can only be observed by actually
  panicking a live process) — this repo's existing `core::swap::tests` and
  `core::git::tests` suites (both re-run clean above) already cover the
  functions being consolidated.

This PR is a pure internal refactor with no new user-visible behavior
beyond fixing the SEARCH_COD/SEARCH icon-glyph inconsistency (a bug fix
that makes both backends match, verified against the existing icon-coverage
tests above) — no new driver-tier test is added, per CLAUDE.md's
pure-refactor exemption.
