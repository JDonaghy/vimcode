# The irreducible per-backend surface

> **What this settles.** [`GOALS.md`](../GOALS.md)'s post-#735 audit left one item open:
> the convergence slices (#751–#766) recorded verdicts in code wherever they *declined*
> to converge a rung, but nobody had aggregated those into a statement of the per-backend
> surface that **stays** — which is what would let anyone judge how far 19,995 production
> lines is from the north star. This is that statement.
>
> _Measured 2026-09-03 on `develop` @ `8e333a8`. Corrected 2026-09-05 (issue #827)
> — the folder-picker row below was wrong; struck, not just re-verdicted. Extended
> 2026-09-16 (#1044) — a full rung-by-rung audit of `TuiShellApp`/`mouse.rs` against
> `App`, adding one new fact (§1, "TUI has no OS window") and correcting §2a's
> command-line-selection verdict (§2c). **§3's table regenerated 2026-09-19 at
> `30c0077` (#1168); §2c's residual gap is now closed upstream — see §2d.**
> Regenerate, don't trust:
> `python3 scripts/prod_lines.py src/gtk src/tui_main src/render.rs` and
> `python3 scripts/native_lines.py gtk src/gtk/*.rs`._

## 1. The nine recorded verdicts are three facts — now four, per #1044

```
grep -rn -iE "do not converge|not converged|one-sided|intrinsic difference" src/
```

returns nine anchors, but several are cross-references to the same decision. They reduce
to three:

| Fact | Anchors | Verdict |
|---|---|---|
| **Frame metrics are px on GTK, cells on TUI.** `FrameMetrics` carries only `line_height`/`char_width` and answers one question: "is this reserved band at least one line tall". Rect math stays per backend because Cairo painter-order and ratatui cell coalescence differ intrinsically. | 1 — `render.rs:7162` | ✅ **Irreducible.** Already reduced to the minimum: a unit, not a geometry. See §2b for a caveat on how thin that "unit" actually stays. |
| **GTK's menu bar *is* its client-side titlebar.** `App::setup` pins `engine.menu_bar_visible = true` unconditionally (#552); TUI shows its menu row only in vscode-mode or via Alt. | 2 — `gtk/testing.rs:5944`, `tui_main/shell_app.rs:6032` | ✅ **Irreducible.** A property of CSD, not a transcription. Handled by fixture, not by branching production code. |
| **Command-line text selection is TUI-only.** GTK has no `cmd_sel`/`cmd_dragging` state, no inverted-cell read-back, and paints its command line through `Surface::CommandLine`, which exposes no character-offset hit test. | 1 — `tui_main/mouse.rs:1620` | ❌ **Not irreducible — mislabelled.** See §2a, corrected by §2c: the hit-test half is now `already-shared`; only the selection-highlight *paint* stays a real, narrower quadraui gap. |
| **TUI has no OS window.** An entire cluster of `UiEvent` arms/`setup` rungs vimcode#1044 walked (`WindowClose`, `MenuActivated`, `ContextMenuItemActivated`/`Dismissed`, `CharTyped` IME composition, window-control-button hover/click, CSD drag-to-move/double-click-maximize, outer-edge resize-cursor hinting, native-menu install in `setup`, initial CSS load) simply has no TUI arm at all — not a divergent reimplementation, an absence by construction. | `src/app.rs` `handle`/`setup`, no TUI counterpart — see `GOALS.md` milestone #7's #1044 rung tables | ✅ **Irreducible.** One fact explaining ~10 separately-named rungs: a terminal has no native window to close, resize by dragging a titlebar, or hang an OS menu/IME off. Recorded once here so a future audit doesn't re-litigate each arm individually. |

### 1b. Struck: "the folder / workspace picker is TUI-only" (#827, adopted #815)

A fourth row previously sat in the table above, verdicted ✅ **Irreducible**: *"GTK
opens a native `GtkFileChooser`, deferred through `PendingFileDialog` and run from
`tick()`, so there is no GTK canvas surface to paint, hit-test or arbitrate."* That
verdict was wrong, not just stale — it never checked upstream. `quadraui::compose::FolderPickerController`
has existed since **2026-05-25**, and its own module doc explicitly instructs
consumers (vimcode named) to delete the local picker and rewire both backends through
the shared controller — the aggregation that produced this row grepped `src/` for
"do not converge" anchors and never looked at what quadraui already shipped. This was
an ordinary **#7 Platform-Neutral adoption gap**, not an irreducible surface.

**Closed by #815:** the TUI-local `FolderPickerState` (and its `collect_dir_entries`/
`filter_dir_entries`/`dir_fuzzy_score` helpers) is deleted; both `TuiShellApp` and
`App` now hold an `Option<quadraui::FolderPickerController>` and paint it through
`Backend::draw_palette` via `FrameOp::FolderPicker` (both backends now compose that
rung — see its updated doc comment). GTK's `open_folder_dialog` no longer opens a
native `gtk4::FileDialog`; it populates the shared controller instead. Key handling
routes through `FolderPickerController::handle` on both backends; mouse handling
through the shared `render::route_folder_picker_click` /
`render::set_folder_picker_selected`. Do not re-add this row.

## 2a. One of the three is a supply gap wearing a verdict's clothes

`tui_main/mouse.rs:1620` files itself under the same "recorded rather than converged"
heading as the others, but its own text says the opposite:

> *"That is a quadraui gap, not a vimcode transcription: per `CLAUDE.md`'s
> Platform-Neutrality Rule the fix is a `CommandLineLayout::hit_test` in quadraui, then
> one shared rung here — not ~80 lines of new GTK-specific selection code."*

That is a **blocked convergence**, not a decision to stay divergent — and the block was
never filed. Verified 2026-09-03: `CommandLineLayout` does not exist anywhere in
quadraui, and no open quadraui issue mentions it. The consumer-side symptom is already
open as **#194** ("Status-bar / command-line messages aren't mouse-selectable — GTK
can't; TUI has offset bug"), which has been sitting without its supply-side blocker
exactly the way #47 was.

**This is the second instance of the same failure mode in one week.** The rule
[`GOALS.md`](../GOALS.md) now states for issues — *a #7 item that turns out to be
supply-blocked stays open behind its blocker* — applies to in-code verdicts too: **a
comment that says "this needs a quadraui API" is an unfiled issue, and grep will not
find it for you.**

## 2b. The `unit_w`/`unit_h` seam is thinner in practice than §1 blesses it (#827)

§1 treats `FrameMetrics`'s `unit_w`/`unit_h` convention as *the* unit seam that keeps
frame-metrics irreducibility down to "a unit, not a geometry." That's the intent, but
`render.rs` doesn't hold the line as cleanly as the fact-table implies. As measured
2026-09-05:

- The `unit_w: f32, unit_h: f32` parameter pair appears in **7 painter function
  signatures** in `render.rs` (`hover_popup_to_quadraui_tooltip`,
  `editor_hover_popup_paint`, `signature_help_to_quadraui_tooltip`,
  `panel_hover_popup_paint`, `diff_peek_to_quadraui_tooltip`,
  `draw_ai_sidebar_panel`, `tab_hover_tooltip_paint`), across roughly **33 uses** of
  the two identifiers in expressions.
- `render.rs` additionally carries **two explicit `char_width > 1.0` backend
  sniffs** — `minimap_reserved_width` (`render.rs:10426`) branches min/max width in
  pixels vs. columns on it, and the editor-viewport scrollbar reservation
  (`render.rs:17256`) reserves 8px only "in the GTK backend (`char_width > 1.0`)".
  Both are comments-as-documentation admitting the unit is not opaque to the
  caller — code downstream of the seam still asks "am I GTK?" by proxy.
- The same file carries **five paired per-backend policy tables** alongside the
  unit convention — constants/branches that assume one shape for `char_width == 1.0`
  (TUI) and another for `char_width > 1.0` (GTK), rather than deriving the answer
  purely from the unit value.

**Record the real state, not the aspiration:** `unit_w`/`unit_h` is a real and useful
convention, but it is a *convention observed by callers*, not an enforced boundary —
`render.rs` still contains explicit backend-identity branches hiding behind the unit
parameter's name. Treat the frame-metrics row in §1 as "irreducible, and mostly but
not entirely behind one seam."

## 2c. #2a's verdict was current in 2026-09-03 and is stale now (#1044)

vimcode#1044 (the full `ShellApp`/`mouse.rs` rung audit — see `GOALS.md` milestone
#7) re-checked §2a's claim against the **currently**-pinned quadraui rev
(`8abca3ae6d25c7fefb8b4d9a1f85ad3edb2c9bfb`), not the `42e0f8f` this section was
last verified against, and found the picture has moved:

- `CommandLineLayout::hit_test` and `::selection_bounds` **do exist** now
  (`quadraui/src/primitives/command_line.rs:98,134` — quadraui#705, landed since
  §2a was written) and vimcode has **already adopted both**, unconditionally
  shared by every backend: `render::command_line_click_char_idx` and
  `render::command_line_selection_rect` (`src/render.rs:21024,21070` at
  `30c0077` — locate by symbol, not line) call straight through to them. The hit-test half of §2a's verdict flips from
  "blocked quadraui gap" to plain **already-shared**.
- What's left is narrower and still real: `quadraui::CommandLine` has no
  `selection` field, so neither backend's `draw_command_line` can paint a
  highlight — `command_line_selection_rect`'s own doc comment already says so
  ("Not wired into either backend's paint path yet"). TUI's `cmd_sel`
  selection is visible today only because it paints the command line
  cell-by-cell with the highlight baked into fg/bg inversion, bypassing
  `draw_command_line` entirely; GTK has **no visual feedback for a selection
  at all**. *(Superseded — see §2d.)*

- **The lesson, stated plainly:** "verified against the pinned rev" has a shelf
  life exactly as long as the pin doesn't move. §2a was correct the day it was
  written and wrong five rev-bumps later without anyone re-checking it — the
  same failure mode `GOALS.md`'s "#47 was closed without its blocker" and this
  file's own struck folder-picker row (§1b) already describe, just with the
  clock running the other direction (an issue can go stale by the *fix*
  landing upstream unnoticed, not only by staying open past its blocker
  landing). Re-verify a "quadraui gap" verdict against the live pin before
  citing it, the same way you'd re-verify an "irreducible" one.

## 2d. §2c's residual gap is closed upstream (2026-09-19, #1168)

The selection-highlight *paint* gap §2c left open is **shipped**: quadraui#1001
landed `Backend::draw_command_line_selection` alongside
`CommandLineLayout::selection_bounds`, and it is live at vimcode's current pin
`d907a06` (bumped by #1133). Verified by `git grep` against that rev, not inferred
from the issue being closed.

**There is no quadraui gap left anywhere in this document.** All nine recorded
verdicts are now either irreducible facts (§1) or ordinary host-side convergence
work; the count of open upstream blockers on the platform-neutrality goal is
**zero**.

What remains here is a *consume-side* task and belongs to #1169, not to quadraui:
vimcode adopts `selection_bounds` but does not yet call
`draw_command_line_selection`, so `render::command_line_selection_rect` still
hand-computes the rect and its doc comment still asserts the upstream API does not
exist. That comment is stale; fixing it is a code change and therefore out of
scope for this documentation pass — deliberately left for the consume-side issue
rather than smuggled into a doc-only PR.

## 3. How much of the backends is actually platform-bound

Production lines that name a toolkit module or type (`gtk4::`/`gio::`/`glib::`/`gdk::`/
`pango`/`cairo`; `ratatui::`/`crossterm::`/`Buffer`/`Frame`/`Rect`):

*Regenerated 2026-09-19 at `30c0077` — the previous revision of this table was
measured before #785 moved `src/gtk/mod.rs`'s mass into `src/app.rs`, and read
`src/gtk/mod.rs` at 7,684 lines when it is now 140.*

> **2026-09-26 (audit R4, `02319b83`): the `src/tui_main/` rows below are
> mostly unreachable code.** Since #1433 (`6ece249`) `tui_main::run` builds the
> shared `App`; `shell_app.rs`, `mouse.rs`, `render_impl.rs`, `panels.rs` and
> most of `mod.rs` have no production caller but are still compiled and still
> counted. The live TUI surface is **≈210** production lines, not the
> ≈10,800 these rows sum to — `prod_lines.py` measures what compiles, not what
> runs. §4's "cannot shrink to `src/macos/mod.rs`'s size" no longer holds;
> vimcode#1434 deletes the unreachable modules.

| File | Production | Native-touching | |
|---|---:|---:|---:|
| `src/gtk/mod.rs` | 140 | 8 | 5.7% |
| `src/gtk/click.rs` | 26 | 3 | 11.5% |
| `src/gtk/css.rs` | 20 | 5 | 25.0% |
| `src/gtk/util.rs` | 319 | 8 | 2.5% |
| **GTK subtotal** | **505** | **24** | **4.8%** |
| `src/tui_main/shell_app.rs` | 4,773 | 51 | 1.1% |
| `src/tui_main/mouse.rs` | 2,882 | 17 | 0.6% |
| `src/tui_main/render_impl.rs` | 1,342 | 43 | 3.2% |
| `src/tui_main/panels.rs` | 1,218 | 63 | 5.2% |
| `src/tui_main/mod.rs` | 769 | 9 | 1.2% |
| **TUI subtotal** | **10,984** | **183** | **1.7%** |
| **both backends** | **11,489** | **207** | **1.8%** |

**The ratio inverted, and that is the headline.** GTK is now *denser* in native
types (4.8%) than the TUI (1.7%) — because GTK has shrunk to the parts that
genuinely must touch the toolkit, while the TUI still carries ~11,000 lines that
name almost no terminal type at all. A low percentage is no longer evidence of
being close to done; it is evidence of duplicated neutral code. See #1169.

*(The "both backends" total dropped from 19,429 to 11,489 — that is GTK
converging, not files going missing: `src/gtk/mod.rs` alone went 7,684 → 140.)*

The #47 re-audit reached the same conclusion independently by hand for `src/gtk/mod.rs`
("only ~40 lines in the whole file touch `gtk4::`/`gio::`/`pangocairo::`/`glib::`
directly").

**Undercount, stated:** a stored widget handle used without naming its type
(`self.window.as_ref()`, `da.queue_draw()`) does not match. In `src/gtk/mod.rs` that
residue is ~15 lines against 62 matched — call it a 25% undercount on the GTK side. Even
tripled, the figure stays under 4%.

## 4. What that means — and what it does not

**The answer to "how far is 19,995 from done" is: platform-specificity is not what is
keeping it there.**

The two backends are, to within about 1.3%, ordinary toolkit-free Rust that happens to
live in a backend directory. The genuinely irreducible surface is the two irreducible
facts in §1 (frame metrics, GTK's CSD menu bar) plus roughly 250 lines of native calls
— a few hundred lines, not tens of thousands. (The folder picker is not a third: §1b
struck it as an adoption gap, not an irreducible fact — and #815 has since closed it.)

**What this does NOT claim:** that the remaining ~19,200 lines are mechanically
convergeable. Some of it is real per-backend *structure* — GTK and TUI compose a frame
differently even where neither names Cairo or ratatui. The measure establishes what is
*not* the obstacle; it does not size what is.

**The honest characterisation of the remainder (rewritten 2026-09-19, #1168):**
this paragraph used to pair `src/gtk/mod.rs` (7,684) against
`src/tui_main/shell_app.rs` (3,989) as "two implementations of the same four
`ShellApp` entry points". **That pairing no longer describes the codebase.** GTK's
half was converged: production `src/gtk/mod.rs` is **140 lines** making **zero**
`render::` calls outside `#[cfg(test)]`, and the shell it used to duplicate now
lives once, in `src/app.rs`, which both GUI backends drive.

What is left is one-sided. `src/tui_main/` still carries its **own**
`impl quadraui::ShellApp` — `TuiShellApp`, 11,037 production lines against
`src/app.rs`'s 8,798 — implementing the same `setup`/`render_content`/`handle`/
`tick`. #751–#766 converged the *decisions* (which surface was hit, which handler
owns a key, what order the frame composes in); what was never converged is the
second implementation itself. That is ordinary duplication, it is now
**exclusively a TUI problem**, and it is the actual remaining work — tracked as
#1169.

## 5. Actions falling out of this

1. **File `CommandLineLayout::hit_test` on quadraui** and link **#194** behind it. Until
   that lands, `tui_main/mouse.rs:1620` should not read as a settled verdict.
2. **Re-grep for the pattern, not just the phrase.** The phrasing that hid this one
   ("that is a quadraui gap") does not match the do-not-converge regex. Any comment
   naming a missing upstream API is an unfiled issue.
3. **Stop sizing this goal in backend line count.** The remaining ~19,200 lines are not a
   platform-porting problem, and a plan that treats them as one will keep missing its
   projection the way the #751–#766 chain did (**−728** against −8,700…−9,500 — see
   `GOALS.md`; the −3,656 originally recorded pooled in #722–#732's dead-code
   deletion). Size it as what it is: two implementations of four entry points — and
   note a function-level audit puts the genuinely-duplicated part of that at only
   ~2,000 ± 500 code lines, with 39% of the surrounding mass being comments.
4. ~~File the folder-picker #7 adoption issue.~~ **Done — #815.** The vimcode-side
   deletion/rewire against `quadraui::compose::FolderPickerController` landed (§1b);
   both backends now share one picker.

## 6. #828: minimap sizing stays two argument sets, not one

`src/render.rs`'s two `char_width > 1.0` backend sniffs (`minimap_reserved_width`,
`build_rendered_window`'s scrollbar-reserve line) were removed in #828 by adopting
quadraui#776's `Backend::scrollbar_reserve()` and `MinimapSizing::VsCodeParity`. The
scrollbar-reserve one converged cleanly: the value now comes from
`quadraui::Backend::scrollbar_reserve()`, asked once by each real caller
(`App::render_content`, `TuiShellApp::render_content` via
`build_screen_for_shell_content`), and `render.rs` just subtracts whatever it's given.

The minimap one did **not** converge to a single `MinimapSizing` value shared by both
backends — tried, and reverted (see `gtk_minimap_sizing`'s doc comment in `render.rs`):

- `MinimapSizing::VsCodeParity::resolve_width` normalises `pane_width` into character
  columns via the caller's own `char_width`, then converts the clamped result back —
  correct when `target_cols`/`min`/`max` genuinely mean "columns of the caller's own
  font". They do for TUI (whose `char_width` is always `1.0` anyway). They do **not**
  for GTK: VS Code's minimap renders in its own small font, decoupled from the editor's,
  so vimcode's GTK minimap width was always meant as an VS-Code-parity ~120px target
  *independent* of the editor's font size — not 120 columns of it.
- Feeding a real editor `char_width` (7-9px) through the shared formula moved an
  ordinary wide GTK pane's minimap from ~120px to ~178-240px.
  `gtk::testing::minimap::minimap_strip_settles_at_vs_code_parity_width_on_a_wide_pane`
  — driven through the real paint path, not just the helper in isolation — caught this
  before it shipped.
- The fix: `TUI_MINIMAP_SIZING`/`gtk_minimap_sizing()` (named and shaped after this
  file's existing `TUI_PICKER_SIZING`/`gtk_picker_sizing()` pair) state their bounds
  directly in the caller's own native unit (columns / raw pixels), and
  `minimap_reserved_width` resolves them with a **forced** `char_width` of `1.0` —
  native-unit-in, native-unit-out — rather than a real one. Each backend's own
  `render_content` (or, for `render.rs`'s tests, each fixture) passes the matching
  constant explicitly; `render.rs` itself never asks `char_width` which one to use.

**Verdict:** ❌ **Not converged — genuine design intent, argument-forked.** The two
`MinimapSizing` values differ because GTK's minimap font and the editor's font are
different scales; this is the same category as the `TUI_PICKER_SIZING` /
`gtk_picker_sizing()` pair already in this file, not an accidental duplication. The
`char_width > 1.0` *runtime branch* is still fully gone from `src/render.rs` — the
fork moved to an explicit, caller-supplied argument (chosen by code that already knows
its own backend by construction), which is what #828's acceptance criterion asks for.

The five other paired sizing tables #828 flagged as "also worth folding in"
(`TUI_PICKER_SIZING`/`gtk_picker_sizing`, `TUI_TAB_SWITCHER_SIZING`/
`gtk_tab_switcher_sizing`, `TUI_FIND_REPLACE_ANCHOR`/`GTK_FIND_REPLACE_ANCHOR`,
`TUI_PICKER_ROWS`/`gtk_picker_rows`, `GTK_DIVIDER_METRICS`/`tui_main/mouse.rs`'s inline
build) were **not** audited in this pass — out of scope for the two sniffs #828
requires; still open.

## 7. #1044: the full `ShellApp`/`mouse.rs` rung audit — irreducible residue

vimcode#1044 asked for a rung-by-rung inventory of *every* decision in
`impl ShellApp for TuiShellApp` (`src/tui_main/shell_app.rs`) and
`mouse::handle_mouse` (`src/tui_main/mouse.rs`), each classified
already-shared / convergeable / irreducible / quadraui-gap against its
`crate::app::App` counterpart — the full per-rung tables and the sequenced
work order live in `GOALS.md` (milestone #7), not here, since this file's job
is the *aggregated facts*, not the raw inventory. What belongs here is the
irreducible side of that audit's result:

- **98 distinct rungs enumerated** (39 at the `ShellApp` trait-method level, 60
  in `mouse.rs`, minus 1 counted from both sides — the command-line-selection
  rung, corrected by §2c above). Of those, **31 verdicted irreducible** — but
  they reduce to the same small fact set this file already carries, plus
  exactly **one new fact**, added to §1's table above: **TUI has no OS
  window.** ~10 of the 31 irreducible rungs (`WindowClose`, native menu/context
  menu, IME `CharTyped`, window-control buttons, CSD drag/resize, native-menu
  `setup`, initial CSS load) are all instances of that one fact, not ten
  separate platform differences — the same aggregation move §1's intro
  already made for the original nine anchors.
- The rest of the 31 are instances of the **existing** facts: px-vs-cell frame
  metrics (tab-bar hit-test, chrome-band geometry caching, column-inverse
  math, terminal-resize units — §1 fact 1 and §2b's caveat about how thin that
  seam actually is) and GTK's-menu-bar-is-its-CSD-titlebar (the Alt-reveal
  shim and the hamburger-corner one-shot guard only exist *because* TUI's bar
  can hide and GTK's can't — §1 fact 2's flip side, not a third fact).
- **Two rungs are explicitly *not* irreducible-surface material** even though
  they look one-sided: TUI's explorer drag-and-drop finalize and its
  hover-link click-to-copy are **feature gaps** (something TUI can do that
  GTK doesn't, with no shared decision to converge *away* from) rather than
  platform constraints. This file catalogs "why a difference has to stay
  different," not "what one backend has that the other lacks" — the latter is
  `GOALS.md` item 4's divergence-bug-class tracking, not this document's
  scope. Recorded so a future pass doesn't add them here by mistake.
- **1 quadraui-gap** survived re-verification against the live pin: the
  `CommandLine::selection` paint field (§2c, drafted in
  `PENDING_QUADRAUI_ISSUES.md`). Zero others were found — every other
  candidate "this needs a quadraui API" rung either already has one adopted
  (§2c) or is genuinely irreducible by the facts above, not blocked on supply.
- **40 already-shared, 26 convergeable** (backlog — see `GOALS.md`). The
  already-shared count is the concrete evidence for this file's §4 thesis:
  most of the *decisions* `mouse.rs` makes already call the same
  `render::`/`click::` functions `app.rs` does; what stays split is
  concentrated in ~16 named, mostly-mechanical items, not a monolithic
  "TUI reimplements everything" problem.

## 8. Regenerating this

```bash
grep -rn -iE "do not converge|not converged|one-sided|intrinsic difference" src/
python3 scripts/prod_lines.py src/gtk src/tui_main src/render.rs
python3 scripts/native_lines.py gtk src/gtk/mod.rs src/gtk/click.rs src/gtk/css.rs src/gtk/util.rs
python3 scripts/native_lines.py tui src/tui_main/shell_app.rs src/tui_main/mouse.rs \
    src/tui_main/render_impl.rs src/tui_main/panels.rs src/tui_main/mod.rs
```

Counts here are evidence measured on a named revision, not coordinates. Re-run them
rather than citing this file.
