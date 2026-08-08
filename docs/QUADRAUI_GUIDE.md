# Quadraui Integration Guide

Load this file when the work touches quadraui — migrations, new primitives, cross-backend rendering, or the quadraui dep itself.

## Working with the quadraui dep

vimcode's `quadraui` dep is a **path dep** to `../quadraui/quadraui` — a sibling checkout of [JDonaghy/quadraui](https://github.com/JDonaghy/quadraui). There is no publish step.

**The dep is pinned out-of-band (#638).** Cargo does not pin path deps — `Cargo.lock` has no entry for them — so a build compiles against whatever the sibling directory happens to contain. `quadraui-pin.txt` records the intended commit and `build.rs` enforces it; `.github/workflows/ci.yml` checks out the same rev, so CI and dev machines cannot silently disagree. Read `quadraui-pin.txt` first; it is the authoritative doc for this workflow.

- **Build against the pin (default):** `git -C ~/src/quadraui fetch origin && git -C ~/src/quadraui checkout <sha from quadraui-pin.txt>`.
- **Bump the pin:** put the new sha in `quadraui-pin.txt`, run `cargo test`, commit *that file alone* with a message naming the quadraui change. Bumping is meant to be a small, reviewable, attributable diff — that is the entire point of the mechanism.
- **Co-develop / test a quadraui PR:** uncommitted edits at the pinned rev need nothing (they only warn). To build against a *different* quadraui commit, use `VIMCODE_QUADRAUI_UNPINNED=1 cargo build`. Never export it in a shell you also run `cargo test` in — an off-pin snapshot diff is indistinguishable from a real regression, which is precisely how #625 cost weeks.
- **Which quadraui is this binary?** `vcd --version` / `vimcode --version` print the resolved rev; `cargo test` prints it too and fails `quadraui_pin::tests::test_run_is_against_the_pinned_quadraui` if the run is off-pin.
- **Branching model:** same as vimcode (`develop` = integration, `main` = release-only).
- **If a quadraui change breaks vimcode's build,** fix vimcode on a normal vimcode branch *in the same commit range as the pin bump*, so the breakage and its cause land together.

### Why: quadraui#472 / #625

quadraui#472 hardcoded `char_cell_width == 2` for U+F0000..=U+F9999. vimcode inherited it through the unpinned path dep with **no vimcode commit**, staling six `tui_main::render_impl::tests::snapshot_*` tests on every machine at once. It was misdiagnosed as CI flakiness for weeks (#625, #615, #602), earned a seven-entry `--skip` list in CI, and that skip list then hid a real input-routing defect (#637) for its whole life. The pin turns that class of event into a one-line diff.

### `coord-tui` has the identical exposure

`coord-tui` depends on the same sibling path and its CI also clones `develop`, so one breaking quadraui merge reddens open PRs in both repos at once (quadraui#476; upstream framing in quadraui#529). The mechanism here is deliberately repo-local and dependency-free — a text file, a `build.rs` check, and a CI checkout step — so it can be mirrored into `coord-tui` verbatim. Keep the two consistent, and keep both consistent with whatever quadraui#529 settles on.

## Migration prerequisites (vimcode → quadraui adoption)

Before migrating a vimcode panel/widget to a quadraui primitive, **ALL** of these must be true:

1. **TUI rasteriser exists in quadraui** — `quadraui::tui::draw_<primitive>` plus the layout helper `tui_<primitive>_layout`.
2. **GTK rasteriser exists in quadraui** — `quadraui::gtk::draw_<primitive>` plus the layout helper `gtk_<primitive>_layout`.
3. **Both backends have paint↔click round-trip harnesses passing** — the gate from quadraui's CLAUDE.md "Lessons captured" section. Without these, the migration ships the bug class the harnesses exist to catch.
4. **Consumer pattern validated** — the way vimcode WILL use the primitive has been exercised in a quadraui example or kubeui demo. The consumer-state round-trip test catches integration bugs that primitive-level harness alone can't see.

The principle: **a vimcode primitive migration must collapse paint code on every vimcode-supported backend, not just one.** Partial adoption leaves bespoke paint code alive in the un-migrated backends — two sources of truth. Don't ship a migration as "TUI-only for now."

**If a primitive is missing from a backend,** file a quadraui issue to add it before starting the vimcode-side migration.

**Once Win-GUI gets rebuilt on quadraui,** the rule grows to include `quadraui::win_gui::*` rasterisers and harnesses. Same for macOS later.

## Terminal selection stays on `TerminalSelection`, not `TextRegion` (#564)

Not every quadraui-owned type is the *generic* one, and picking the wrong
quadraui abstraction is still a mistake even though it's "quadraui" either
way. The integrated terminal panel's click-drag text selection is already
fully delegated to quadraui — but through `quadraui::terminal_engine`
(`TerminalSelection`, `TerminalSession::forward_mouse` /
`mouse_reporting_enabled`, `selected_text()`, and `Backend::draw_terminal`
for highlight painting), **not** through the generic selectable-region
pipeline (`dispatch::TextRegion` / `DragTarget::TextSelection` /
`Backend::register_text_region`, as demoed in
`examples/common/selection_app.rs`).

Don't re-litigate this without new information: the generic pipeline has no
concept of scrollback (its `TextRegion` is a fixed-bounds rect of
currently-painted `lines`), while `TerminalSelection` already operates in
display-row space that's scrollback-aware by construction. Re-routing would
add a `Point`↔row/col translation layer around a type that already fits,
for zero behavioral gain. Quadraui's own canonical terminal example
(`examples/common/terminal_app.rs`) doesn't use `TextRegion` for terminal
selection either — see the design note atop
`src/core/engine/terminal_ops.rs` for the full reasoning. `TextRegion` is
still the right call for plain non-PTY selectable text panels; it's just
not this one.

## Cross-backend rendering algorithms

Any "fit X within Y" / "where does Z scroll to" / "which slice fits in N units" logic shared across backends MUST be parameterised over a measurement closure — never hardcode a unit (chars vs pixels). Put the algorithm in `quadraui` as `fn ...<F: Fn(...) -> usize>(..., measure: F)`. Each backend supplies its native measurer (TUI: `chars().count()`, GTK: Pango pixel widths, Win-GUI: DirectWrite, macOS: Core Text). Established examples: `quadraui::StatusBar::fit_right_start` and `quadraui::TabBar::fit_active_scroll_offset`. **When debugging a layout bug present in one backend but not another, suspect units before timing** — see [`quadraui/docs/NATIVE_GUI_LESSONS.md`](https://github.com/JDonaghy/quadraui/blob/main/quadraui/docs/NATIVE_GUI_LESSONS.md) §12, §13, §14.

## Paint↔click integration pattern (CRITICAL)

Quadraui's demos use `AppLogic` + `Backend` trait so paint and click share one function context. **Vimcode can't do that**: paint runs inside ratatui's `terminal.draw()` / GTK's `set_draw_func` closure, click runs in the event loop without one.

**The rule: click NEVER re-derives layout. Paint caches it; click reads the cache.**

```rust
// Engine field:
pub foo_layout: RefCell<Option<quadraui::FooLayout>>,

// Paint (inside draw closure — has the real area rect):
let view = render::foo_to_primitive(&data);
backend.draw_foo(area, &view);  // internally computes layout
let layout = backend.foo_layout(area, &view);
engine.foo_layout.replace(Some(layout));

// Click (in event handler — does NOT recompute):
if let Some(ref layout) = *engine.foo_layout.borrow() {
    match layout.hit_test(rel_x, rel_y) { ... }
}
```

**Why not call `tui_foo_layout` / `gtk_foo_layout` in click?** Because reconstructing the area rect requires reproducing chrome math that ratatui's `Layout::split` or GTK's `DrawingArea` sizing computed. That formula drifts ±1 cell/pixel when wildmenu / per-window status / terminal-panel states change. This caused 4 rounds of smoke failures on #296 (Session 347).

**Corollary**: the same applies to inner body layouts (tree, list, form). If click needs to drill into a section's body, cache that inner layout at paint time too.

**This rule supersedes any suggestion to "compute layout fresh in click for one source of truth."** In vimcode's architecture, the one source of truth IS the layout paint produced.
