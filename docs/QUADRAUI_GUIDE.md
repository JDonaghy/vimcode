# Quadraui Integration Guide

Load this file when the work touches quadraui — migrations, new primitives, cross-backend rendering, or the quadraui dep itself.

## Working with the quadraui dep

vimcode's `quadraui` dep is a **git dependency pinned to a `rev`** in `Cargo.toml` — `quadraui = { git = "https://github.com/JDonaghy/quadraui.git", rev = "<sha>", ... }` — and `[patch.crates-io] vt100` is pinned the same way, to the same rev. There is no publish step; `Cargo.lock` records the fully-resolved 40-char SHA for both, which is the authoritative answer to "which quadraui?".

- **Build against the pin (default):** just `cargo build` — Cargo clones the pinned rev into `~/.cargo/git/` itself. `~/src/quadraui` is irrelevant to a normal build.
- **Bump the pin:** edit the `rev = "..."` in `Cargo.toml` (both the `quadraui` dependency and the `[patch.crates-io] vt100` entry — they must always match), then run `cargo test` so `Cargo.lock` updates and any rendering/behaviour change lands as part of the same reviewable commit. `cargo update -p quadraui` alone will **not** move a rev-pinned git dep — the manifest edit is the bump.
- **Co-develop / test a quadraui branch:** copy `cargo-config-local-quadraui.toml.example` to `.cargo/config.toml` (git-ignored). Cargo's `paths` override redirects the build to your local `~/src/quadraui` checkout by package name, regardless of what rev is pinned or what commit the checkout is on. Delete `.cargo/config.toml` to go back to the pinned rev. Never leave it in place for a `cargo test` run whose result you intend to trust — an off-pin diff still looks exactly like a real regression.
- **Which quadraui is this binary?** `vcd --version` / `vimcode --version` print the resolved rev (`src/quadraui_pin.rs`, sourced from `Cargo.lock` by `build.rs`).
- **Branching model:** same as vimcode (`develop` = integration, `main` = release-only).
- **If a quadraui change breaks vimcode's build,** fix vimcode on a normal vimcode branch *in the same commit range as the pin bump*, so the breakage and its cause land together.

### Why a pin exists at all: quadraui#472 / #625 / #638 / #659

Before #691, the dependency was a **relative sibling path dep** (`path = "../quadraui/quadraui"`), which Cargo cannot pin — `Cargo.lock` had no entry for it, so every build silently compiled against whatever the neighbouring `~/src/quadraui` checkout happened to contain.

That was not hypothetical. quadraui#472 hardcoded `char_cell_width == 2` for U+F0000..=U+F9999; the moment it landed on quadraui `develop` it staled six `tui_main::render_impl::tests::snapshot_*` tests on every machine at once. The failure was misdiagnosed as CI flakiness for weeks (#625, #615, #602), earned a seven-entry `--skip` list in CI, and that skip list then hid a real input-routing defect (#637) for its whole life.

#638 responded with an out-of-band pin: a `quadraui-pin.txt` file naming the intended commit, a `build.rs` check comparing it to the sibling checkout's `HEAD`, and a matching CI checkout step — a hard build failure on drift instead of a silent behaviour change. It worked, but it was still built on top of a shared directory: `quadraui-pin.txt` went on to record *two separate* `QUADRAUI PIN MISMATCH` failures during #659's smoke, both caused purely by `~/src/quadraui` — a checkout shared by every concurrently-running agent on the machine — moving underneath a build that changed nothing in vimcode. The documented escape hatch (`VIMCODE_QUADRAUI_UNPINNED=1`) made it worse for backwards drift: it downgraded the mismatch to a warning and then **failed to compile**, because the code already assumed APIs the stale checkout didn't have yet — there was no way to build against a checkout *older* than what vimcode's own source required.

#691 replaced both path deps with git deps pinned to a `rev`. Cargo locks the resolved SHA in `Cargo.lock` itself, so there is nothing left for a shared directory to disturb, and no out-of-band file or build-time comparison is needed to make drift attributable — `Cargo.lock`'s diff already is that record. `~/src/quadraui` remains useful only for the opt-in local co-development override (`cargo-config-local-quadraui.toml.example`), never for an unmodified build.

### `coord-tui` solved this first

`coord-tui` had the identical exposure on the same kind of path dep and fixed it this exact way in coord#1973 — pinned `quadraui` to a git rev instead of a sibling path — for the same reason: "a quadraui merge could break coord-tui's build/merge with zero coord-tui commits." It later deleted the shared-sibling symlink its test runner used to create (coord#2804), for exactly the reason above: "ONE location shared by every concurrent run on the machine." #691 is vimcode adopting the same pattern; see `tui/Cargo.toml` and `tui/cargo-config-local-quadraui.toml.example` in the `code-coordinator` repo for the sibling implementation.

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
