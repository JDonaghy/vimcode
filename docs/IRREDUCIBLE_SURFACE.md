# The irreducible per-backend surface

> **What this settles.** [`GOALS.md`](../GOALS.md)'s post-#735 audit left one item open:
> the convergence slices (#751–#766) recorded verdicts in code wherever they *declined*
> to converge a rung, but nobody had aggregated those into a statement of the per-backend
> surface that **stays** — which is what would let anyone judge how far 19,995 production
> lines is from the north star. This is that statement.
>
> _Measured 2026-09-03 on `develop` @ `8e333a8`. Corrected 2026-09-05 (issue #827)
> — the folder-picker row below was wrong; struck, not just re-verdicted. Regenerate,
> don't trust:
> `python3 scripts/prod_lines.py src/gtk src/tui_main src/render.rs` and
> `python3 scripts/native_lines.py gtk src/gtk/*.rs`._

## 1. The nine recorded verdicts are three facts

```
grep -rn -iE "do not converge|not converged|one-sided|intrinsic difference" src/
```

returns nine anchors, but several are cross-references to the same decision. They reduce
to three:

| Fact | Anchors | Verdict |
|---|---|---|
| **Frame metrics are px on GTK, cells on TUI.** `FrameMetrics` carries only `line_height`/`char_width` and answers one question: "is this reserved band at least one line tall". Rect math stays per backend because Cairo painter-order and ratatui cell coalescence differ intrinsically. | 1 — `render.rs:7162` | ✅ **Irreducible.** Already reduced to the minimum: a unit, not a geometry. See §2b for a caveat on how thin that "unit" actually stays. |
| **GTK's menu bar *is* its client-side titlebar.** `App::setup` pins `engine.menu_bar_visible = true` unconditionally (#552); TUI shows its menu row only in vscode-mode or via Alt. | 2 — `gtk/testing.rs:5944`, `tui_main/shell_app.rs:6032` | ✅ **Irreducible.** A property of CSD, not a transcription. Handled by fixture, not by branching production code. |
| **Command-line text selection is TUI-only.** GTK has no `cmd_sel`/`cmd_dragging` state, no inverted-cell read-back, and paints its command line through `Surface::CommandLine`, which exposes no character-offset hit test. | 1 — `tui_main/mouse.rs:1620` | ❌ **Not irreducible — mislabelled.** See §2a. |

### 1b. Struck: "the folder / workspace picker is TUI-only" (#827)

A fourth row previously sat in the table above, verdicted ✅ **Irreducible**: *"GTK
opens a native `GtkFileChooser`, deferred through `PendingFileDialog` and run from
`tick()`, so there is no GTK canvas surface to paint, hit-test or arbitrate."* That
verdict was wrong, not just stale — it never checked upstream. `quadraui::compose::FolderPickerController`
has existed since **2026-05-25**, and its own module doc explicitly instructs
consumers (vimcode named) to delete the local picker and rewire both backends through
the shared controller — the aggregation that produced this row grepped `src/` for
"do not converge" anchors and never looked at what quadraui already shipped. This is
an ordinary **#7 Platform-Neutral adoption gap**, not an irreducible surface: file the
adoption issue against `FolderPickerController`, don't carry this row forward as a
settled fact.

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

## 3. How much of the backends is actually platform-bound

Production lines that name a toolkit module or type (`gtk4::`/`gio::`/`glib::`/`gdk::`/
`pango`/`cairo`; `ratatui::`/`crossterm::`/`Buffer`/`Frame`/`Rect`):

| File | Production | Native-touching | |
|---|---:|---:|---:|
| `src/gtk/mod.rs` | 7,684 | 62 | 0.8% |
| `src/gtk/click.rs` | 696 | 8 | 1.1% |
| `src/gtk/css.rs` | 507 | 5 | 1.0% |
| `src/gtk/util.rs` | 250 | 8 | 3.2% |
| `src/tui_main/shell_app.rs` | 3,989 | 43 | 1.1% |
| `src/tui_main/mouse.rs` | 2,895 | 17 | 0.6% |
| `src/tui_main/render_impl.rs` | 1,267 | 39 | 3.1% |
| `src/tui_main/panels.rs` | 1,208 | 55 | 4.6% |
| `src/tui_main/mod.rs` | 933 | 9 | 1.0% |
| **both backends** | **19,429** | **246** | **1.3%** |

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
struck it as an unactioned adoption gap, not an irreducible fact.)

**What this does NOT claim:** that the remaining ~19,200 lines are mechanically
convergeable. Some of it is real per-backend *structure* — GTK and TUI compose a frame
differently even where neither names Cairo or ratatui. The measure establishes what is
*not* the obstacle; it does not size what is.

**The honest characterisation of the remainder:** `src/gtk/mod.rs` (7,684) and
`src/tui_main/shell_app.rs` (3,989) are two implementations of the same four `ShellApp`
entry points — `setup`, `render_content`, `handle`, `tick`. #751–#766 converged the
*decisions* those implementations make (which surface was hit, which handler owns a key,
what order the frame composes in — `src/gtk/mod.rs` makes 424 `render::` calls). What was
not converged is the implementations themselves. That is ordinary duplication, and it is
the actual remaining work.

## 5. Actions falling out of this

1. **File `CommandLineLayout::hit_test` on quadraui** and link **#194** behind it. Until
   that lands, `tui_main/mouse.rs:1620` should not read as a settled verdict.
2. **Re-grep for the pattern, not just the phrase.** The phrasing that hid this one
   ("that is a quadraui gap") does not match the do-not-converge regex. Any comment
   naming a missing upstream API is an unfiled issue.
3. **Stop sizing this goal in backend line count.** The remaining ~19,200 lines are not a
   platform-porting problem, and a plan that treats them as one will keep missing its
   projection the way the #751–#766 chain did (−3,656 against −8,700…−9,500). Size it as
   what it is: two implementations of four entry points.
4. **File the folder-picker #7 adoption issue.** `quadraui::compose::FolderPickerController`
   has shipped since 2026-05-25 and nothing has adopted it — file the vimcode-side
   deletion/rewire against it (§1b), rather than continuing to treat the TUI-only picker
   as a settled irreducible fact.

## 6. Regenerating this

```bash
grep -rn -iE "do not converge|not converged|one-sided|intrinsic difference" src/
python3 scripts/prod_lines.py src/gtk src/tui_main src/render.rs
python3 scripts/native_lines.py gtk src/gtk/mod.rs src/gtk/click.rs src/gtk/css.rs src/gtk/util.rs
python3 scripts/native_lines.py tui src/tui_main/shell_app.rs src/tui_main/mouse.rs \
    src/tui_main/render_impl.rs src/tui_main/panels.rs src/tui_main/mod.rs
```

Counts here are evidence measured on a named revision, not coordinates. Re-run them
rather than citing this file.
