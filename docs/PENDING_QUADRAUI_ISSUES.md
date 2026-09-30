# Pending quadraui issues — drafted, not yet filed

Vimcode worker sessions in this harness are `git`-only (no `gh` access); filing
GitHub issues, including on `JDonaghy/quadraui`, is a coordinator/human action.
This file holds issue text that a worker has fully drafted and verified but
could not file itself, so the finding survives past the session that found it
(per `GOALS.md`'s milestone-discipline rule: "a comment naming a missing
upstream API is an unfiled issue, and grep will not find it for you" — this
file exists so the comment *is* findable, and the filing doesn't get lost a
third time).

**Coordinator/human action:** file each entry below verbatim on
`JDonaghy/quadraui`, into milestone **#9 "vimcode Platform-Neutral
blockers"**, then delete its entry here and update the citing vimcode issue
(link the filed issue number, leave the vimcode issue **open** behind it per
`GOALS.md`'s rule, do not close on investigation alone).

**Before filing, re-check the gap still exists at the pinned rev.** A draft
below is a snapshot from whatever quadraui rev the drafting session had
checked out; the *pinned* rev (`Cargo.toml`'s `quadraui = { ... rev = "..." }`)
can move — and the gap can close upstream — between when a draft is written
and when it is actually filed. Confirm the gap against the current pin
immediately before filing, not against a memory of when the draft was
written. #1259's audit found four entries whose gap had already closed
(struck below); one of the four (`TuiBackend`'s cursor-position clobber) was
filed as quadraui#1039 on 2026-09-20 for a gap whose fix had already landed
upstream three days earlier (quadraui#1002, `e4ef921`, 2026-09-17) — from a
checkout that predated it — and cost three worker dispatches, each exiting
with zero commits, before quadraui#1039 was closed as a duplicate. A
re-check against the pin immediately before filing would have caught that at
zero cost instead.

---

## ~~`BackendCaps::native_menu` is overloaded across backends — Windows regressed its own `window_chrome` custom title bar (#1562) the moment it also declared `native_menu` (#1582/#1200)~~ — **RESOLVED upstream, struck 2026-09-30 (#1629)**

> **This entry is resolved.** Rather than add the capability split the
> **Ask** below requested, quadraui#1228 (`bc92d47`/`d292a4c`) resolved the
> conflict the other way: `WinBackend::backend_caps()` no longer declares
> `native_menu` at all — a native `HMENU` lives in the non-client area,
> which #1199's drawn caption permanently covers, so #1200's native menu
> bar was unreachable regardless of this overload — and `install_menu_bar`
> is back to the trait's no-op default on Win-GUI, matching
> `GtkBackend`/`TuiBackend`. vimcode#1629 bumped the pin to `d292a4c`
> (quadraui `develop` HEAD) to pick this up: `App::setup`'s existing
> three-way `backend_caps()` branch needed no vimcode-side change — a Win
> backend now falls into the same `window_chrome` arm GTK takes, and
> `capture_window_and_apply_csd`'s `!native_menu` gate now lets
> `set_decorated(false)` run on Windows too. The macOS `native_menu` arm
> (`MacBackend`) is untouched by this change. `src/win/mod.rs`'s
> `win_driver_tests` module replaces the two tests that pinned
> `native_menu: true` on `WinBackend` with `window_chrome`-shaped ones.
> Real-hardware re-verification of #1562/#1582 on dell64 is vimcode#1629's
> own acceptance item. The original repro/isolation/ask below is left
> intact for history.

**Title:** `native_menu: true` means "an OS-global menu bar with zero in-window
footprint" on macOS but "a per-window `SetMenu` `HMENU` sitting directly under
the caption" on Windows — two different things that happen to share one
boolean — so a consumer like vimcode that branches `if native_menu {...} else
if window_chrome {...}` (correct for macOS, where the native caption should
stay) silently starves `window_chrome`'s custom-caption path on any *other*
backend that also grows `native_menu: true`, exactly what happened to
`WinBackend` once quadraui#1200 (#1582's fix) landed alongside quadraui#1199
(#1562's fix) in the same pin.

**Body:**

vimcode#1622 tried to re-verify vimcode#1562 (custom-drawn title bar with an
embedded command centre) on real Windows hardware (dell64) after vimcode#1618
bumped the quadraui pin past both quadraui#1199 (`window_chrome`,
`WM_NCCALCSIZE`/`WM_NCHITTEST`) and quadraui#1200 (`native_menu`, a real
`SetMenu`-backed `HMENU`). Built `vimcode.exe` with `cargo xwin build
--release --target x86_64-pc-windows-msvc --no-default-features --features
win` and launched it directly on dell64 (real `HWND`, `MainWindowTitle`
"VimCode", `Responding: True`). A `PrintWindow(PW_RENDERFULLCONTENT)`
screenshot taken immediately after launch (before any further repaint) shows:

- A stock Windows 11 native caption: app icon, "VimCode" title in the real
  system UI font, and native-style minimize/restore/close glyphs in the
  standard caption position.
- A separate menu row directly underneath showing File/Edit/View/Go/Run/
  Terminal/Help — a real, clickable native (or owner-drawn) Win32 menu, not
  vimcode's own drawn row (confirmed working, satisfying vimcode#1582).
- **No command-centre search box anywhere** — a 3x-zoomed crop of the blank
  space between the title text and the minimize button shows nothing painted
  there at all. **Retracted by the "Correction (#1622 fix round 1)" note
  below — this bullet's crop is inside the native, DWM-owned caption, and
  the entire *client area* was independently found blank/black on a later
  re-check, not selectively the Command Center. Read that note before
  citing this bullet as command-centre-specific evidence.**

This is exactly the pre-#1562 appearance, as if quadraui#1199 had never
landed, even though the pin contains it.

Root-caused by reading `WinBackend::backend_caps()` (`quadraui/src/win/
backend.rs`, pinned rev `6e14d8a`) against vimcode's own `src/app.rs`:

```rust
// quadraui/src/win/backend.rs, WinBackend::backend_caps()
crate::backend::BackendCaps {
    ...
    native_menu: true,    // quadraui#1200 (#1582)
    window_chrome: true,  // quadraui#1199 (#1562)
    ..crate::backend::BackendCaps::empty()
}
```

`vimcode::App::setup`'s menu-bar branch (`src/app.rs`):

```rust
if backend.backend_caps().native_menu {
    // installs a real HMENU via SetMenu; sets menu_bar_visible = false
    ...
} else if backend.backend_caps().window_chrome {
    // GTK's (and any future Win-GUI's) drawn menu bar doubles as the
    // client-side titlebar — pinned visible always
    self.engine.borrow_mut().menu_bar_visible = true;
} else {
    ...
}
```

Since `native_menu` is checked first and is now `true` for `WinBackend`, the
`window_chrome` arm — the one that makes the drawn CSD row (icon + menu
items + controls) live and pins `menu_bar_visible = true` — never runs on
Windows. **This branch does *not* additionally gate the Command Center —
see the "Correction (#1622 fix round 1)" note below; the paragraph above
originally claimed it did, which was wrong.**
`App::capture_window_and_apply_csd` compounds this: it early-returns
whenever `backend.backend_caps().native_menu` is true, so `Backend::window()
.set_decorated(false)` — the call that clears `WS_CAPTION` and hands the
title strip to `win::run`'s `WM_NCCALCSIZE`/`WM_NCHITTEST` handling — is
never invoked either. Win-GUI ends up with its stock decorated window,
`WS_CAPTION` intact, exactly as if `window_chrome` were never declared. This
half — the native caption never being replaced by the drawn CSD row — is
independently reproduced again in the fix-round correction below and is not
in question.

This is *not* a new bug in either branch: both were written correctly for
the backend combination that existed when each landed. `MacBackend` has
declared `{native_menu: true, window_chrome: true}` for a while (`quadraui/
src/macos/backend.rs`) — `window_chrome` there is deliberately a narrower
claim, backing only `begin_window_drag`/`toggle_window_maximize` (see that
method's own doc: "`CAP_CONTRACTS`'s `window_chrome` cap only requires *any*
of the three CSD methods"), not "this backend wants its drawn row to replace
the native caption". macOS's *actual* custom-caption mechanism is a
completely separate, macOS-only hook: `ShellConfig::client_side_titlebar` /
`Backend::titlebar_control_inset` (`quadraui/src/shell.rs`, `quadraui/src/
macos/run.rs`), applied at window-creation time, never touching
`set_decorated`. `WinBackend` doesn't participate in that hook at all — it
reuses the *other* CSD mechanism (`set_decorated` + `WM_NCCALCSIZE`/
`WM_NCHITTEST`), the one GTK also uses, and that one has no distinguishing
signal for "I also have a native_menu, but keep going with CSD anyway".

vimcode's `App::setup`/`capture_window_and_apply_csd` gate on `native_menu`
precisely because that was, at the time each was written, a reliable proxy
for "this backend's menu bar lives entirely outside the window, don't touch
its native chrome" (true only for macOS, the sole `native_menu` backend
before quadraui#1200). Once a second backend could declare `native_menu`
for an unrelated, in-window reason, that proxy broke, and nothing in
`BackendCaps` (or `ShellConfig`) records which meaning applies.

**Ask:** add a capability (or `ShellConfig`/`RunConfig` field, matching the
shape of `client_side_titlebar`) that lets a backend declaring both
`native_menu` and `window_chrome` say which one a generic consumer should
actually route the *drawn CSD row* through — e.g. `native_menu_is_global:
bool` (`true` only for macOS's system-wide menu bar; `false` for a
per-window `SetMenu` like `WinBackend`'s), or fold the distinction into
`window_chrome` itself by splitting it into the two things it currently
conflates ("this backend supports CSD drag/resize gestures" vs "this
backend wants its drawn titlebar row live instead of the native one").
Either shape needs to land with a `CAP_CONTRACTS` update and each backend's
own `backend_caps_declares_*` honesty test extended to cover it, the same
pattern `window_chrome`/`native_menu` themselves already follow.

**Why not fixed in vimcode instead:** the two candidate vimcode-side patches
(reorder `App::setup`'s branch to prefer `window_chrome`, or change
`capture_window_and_apply_csd`'s gate to something other than plain
`native_menu`) both risk changing behaviour on the *other* backend that
declares this identical pair (`MacBackend`) in a way this dev loop has no
macOS hardware to verify against — CLAUDE.md's black-box-coverage bar
can't be met for a change whose only driver-tier check would need a live
`MacDriver`/real Cocoa window. A vimcode-side fix that can't be verified on
the backend it's most likely to break is worse than filing the gap.
`WinBackend`/`MacBackend`'s `backend_caps()` and `App::setup`/
`capture_window_and_apply_csd`'s branch logic are the only things this
touches — no `src/win/`/`src/gtk/` wrapper decision involved, so this is
squarely quadraui-side (the capability model) plus a vimcode-side follow-up
once the new signal exists, not a `src/win/` fix per the Platform-Neutrality
Rule.

**Correction (#1622 fix round 1):** a review of the entry above found its
central causal claim — "the drawn CSD row/command centre never goes live on
Windows" — conflates two rungs this codebase's own #939 fix deliberately
decoupled. Reading `render::FramePresence::from_screen` (`src/render.rs`)
shows `menu_row`/`menu_dropdown` *are* coupled to `screen.menu_bar_visible`
(correctly identified above), but `command_center` is computed as
`title_bar_band_live` alone — explicitly *not* gated on `menu_bar_visible`
or on which arm of `App::setup`'s three-way branch ran; that file's own
doc comment at the field says so in as many words ("the command center
still belongs in that band — same as VS Code on macOS. So this rung depends
only on the band existing, not on whether the drawn menu row is
suppressed"). `ShellConfig::has_title_bar` is set unconditionally `true` for
every GUI backend regardless of `native_menu`/`window_chrome` (`src/app.rs`,
the `if !self.units.is_gui_backend` guard only affects the TUI `cell`
profile), and the `presence.command_center` gate at its `App::render_content`
call site has no dependency on the `native_menu`/`window_chrome` branch
either. This exact scenario is the literal docstring of the existing,
passing regression test `command_center_liveness_is_split_from_menu_bar_
visible` (`src/render.rs`) — RED-verified against its own reverted-fix
comment — and `src/win/mod.rs`'s own `command_center_paints_on_a_native_
menu_backend` test (added in the prior #1618 review-fix commit) asserts the
identical contract specifically for `WinBackend`. So per the codebase's own
tested contract, the Command Center should still paint into the client-area
title-bar band on `WinBackend` even with `native_menu: true` suppressing the
drawn menu row — independent of the branch-order bug this entry blames. That
half of the causal claim is retracted.

A second dell64 session (this fix round) re-ran the same real-hardware
check, twice, on two independently launched processes (fresh `APPDATA`
scratch profiles each time), and got new, more specific evidence pointing at
a different, already-known cause instead:

- `PrintWindow(PW_RENDERFULLCONTENT)` against the live window returned the
  native caption and native menu row correctly, but a **solid white** client
  area below them — no tabs, no status bar, no editor text, no Command
  Center, nothing (not merely a missing search box). After moving/resizing
  the window and forcing `InvalidateRect`/`UpdateWindow`, a second
  `PrintWindow` capture returned a stale, duplicated frame pinned at the
  window's *original* bounds — the same "Windows suspends live composition
  for a locked/occluded window" artifact vimcode#1561's own investigation
  already recorded for this exact host.
- A direct `GetWindowDC` + `BitBlt` read (bypassing `PrintWindow`'s
  synthetic `WM_PRINT`-fallback rendering) showed the native menu row's real
  GDI content faithfully, but **solid black** for the entire client area —
  the identical "solid black" signature vimcode#1558's investigation already
  recorded for `Graphics.CopyFromScreen` under a locked dell64 session.
  `GetForegroundWindow()` returned `NULL` throughout both captures,
  confirming the session was (again) locked at the OS level.

Two independent capture methods, on two separate launches, agree: the
*entire* Direct2D-painted client area is blank/black, not selectively the
Command Center's band. That is the same symptom vimcode#1559 already
discloses ("dell64's interactive session was locked for the whole run...
quadraui's own `win::run` `WM_PAINT` handler already names as a device-loss
trigger for the Direct2D editor surface... editor pane blank every frame")
generalised from "the cursor/editor surface" to every piece of client-area
content, Command Center included — not a Command-Center-specific starvation
bug, and not evidence the #939 decoupling fails to apply to `WinBackend`.

**Net effect on this entry:** the `WS_CAPTION`/native-caption half of the
diagnosis is solid and independently reproduced a second time this round
(real DWM caption, real native min/max/close, real native menu row — none of
it replaced by vimcode's own drawn CSD row) — that part of the **Ask** below
still stands. The "starves the Command Center" framing is retracted: per the
tested contract above, Command Center liveness does not depend on this
branch at all, and the only evidence offered for "no Command Center" is
structurally consistent with (and, on the fresh capture-method evidence
above, better explained by) the identical locked-session Direct2D
suspension #1559 already discloses. `JDonaghy/vimcode#1562`'s Command Center
acceptance criterion remains **unverified** — blocked by that same
dell64-local environmental limitation, not resolved and not falsified by
this investigation. A live, unlocked-session re-check (mirroring #1559's own
outstanding ask) is the only way to close either one.

**Blocks:** `JDonaghy/vimcode#1562`'s `window_chrome`/native-caption
acceptance criterion only, per the correction above — not its Command
Center criterion, which is not blocked on this gap. Leave the issue open
behind this one per `GOALS.md`'s milestone-discipline rule.

---

## `MacDriver` has no `set_double_click_folding(false)`, unlike `TuiDriver`/`GtkDriver` (workaround in vimcode#1576)

Since quadraui#486, `MacBackend::fold_double_click` runs every injected
`MouseDown` through a `DoubleClickDetector` (400 ms window,
`MAC_DOUBLE_CLICK_RADIUS` = 4 pt). `quadraui::tui::testing::TuiDriver` and
`quadraui::gtk::testing::GtkDriver` both expose
`set_double_click_folding(enabled: bool)` so a test doing back-to-back
independent clicks can opt out; `quadraui::macos::testing::MacDriver` does
not, and `MacBackend`'s own `set_double_click_folding` doesn't exist either
(the GTK/TUI backends have a `pub(crate)` one each).

Observed impact: vimcode's `macos::mac_driver_tests::
picker_row_click_hit_band_matches_the_painted_row` sweeps five single clicks
~3 pt apart across one picker row; after the pin bump to `286eb6c` sample 1
arrived as a `UiEvent::DoubleClick`, confirming the wrong picker entry. The
vimcode test now sleeps 450 ms between samples to let the window lapse —
correct but slow, and it hardcodes a private quadraui constant.

**Ask:** add `MacDriver::set_double_click_folding(&mut self, enabled: bool)`
(plus the backing `MacBackend` switch), mirroring the TUI/GTK drivers, so
cross-backend harness helpers can disable folding uniformly. Do the same for
`WinDriver` if it has the same gap.

---

## Win-GUI activity bar hardcodes the ASCII fallback glyph, ignoring the `nerd_fonts_enabled` flag `draw_tree` already reads (blocks vimcode#1558)

**Title:** `win::activity_bar::draw_activity_bar` always paints `Icon::fallback`,
never `Icon::glyph` — `WinBackend::nerd_fonts_enabled` exists and is wired into
`draw_tree` (#804) but never reaches this rasteriser

**Body:**

vimcode#1558 reports that on Windows, with **Nerd Font Icons on**, the
activity bar shows placeholder characters (`⊞ / ! Y # > ▦ *`) instead of real
glyphs, while the macOS build of the identical commit shows the correct
icons. Root-caused primarily by inspection — the reported placeholder
characters are an exact, unambiguous match, see below — and corroborated on
dell64 (this fleet's real Windows 11 host, reachable via WSL2 interop; see
#1558's "Verify on real Windows" section) by successfully cross-building
`vimcode.exe` with `cargo xwin build --release --target
x86_64-pc-windows-msvc --no-default-features --features win --bin vimcode`
and launching it **directly** (not wine) against a real `HWND` on dell64's
desktop. Pixel-level confirmation of the glyph fix itself (this entry's own
ask below, not yet landed) couldn't be captured in that same session: dell64's
interactive console session was independently locked at the OS level
(`logonui.exe` running), which blocks GDI-based screen/window-content capture
regardless of what quadraui paints underneath — see `src/win/mod.rs`'s
`#1558` doc section in vimcode for the full repro chain:

`quadraui/src/win/activity_bar.rs` (pinned rev `a58e5bec`), the paint loop for
each activity-bar row:

```rust
// Uses the ASCII `fallback` — this rasteriser doesn't take a
// per-frame `nerd_fonts_enabled` toggle yet (issue #683 scoped
// TUI/GTK/macOS only; Win-GUI has no Nerd Font wiring at all,
// matching `macos::tree`/`macos::form`'s same fallback-only
// posture until #25's icon-font plumbing lands here too).
let icon_str = item.icon.fallback.as_str();
```

That comment is **stale** — it predates #804/#929, which already gave
`WinBackend` real Nerd-Font wiring: `WinBackend::nerd_fonts_enabled: bool`
(`win/backend.rs`, set via `Backend::set_nerd_fonts`) is read by
`draw_tree`'s call site (`win/backend.rs::draw_tree`, `fn draw_tree`),
which passes `self.nerd_fonts_enabled` straight through to
`super::tree::draw_tree(..., self.nerd_fonts_enabled)`. `draw_activity_bar`'s
call site (same file, `fn draw_activity_bar`) never reads
`self.nerd_fonts_enabled` at all, and `super::activity_bar::draw_activity_bar`'s
signature has no parameter for it — `item.icon.fallback` is the only branch
that exists.

vimcode's `src/icons.rs` fallback strings for the exact activity-bar items
line up character-for-character with the bug report:

```
EXPLORER   fallback = "\u{229e}"  ⊞
DEBUG      fallback = "!"
GIT_BRANCH fallback = "Y"
EXTENSIONS fallback = "#"
AI_CHAT    fallback = ">"
BOARD      fallback = "\u{25a6}"  ▦
SETTINGS   fallback = "*"
```

— `⊞ / ! Y # > ▦ *`, in order, is exactly the reported symptom. This is not a
DirectWrite font-fallback/rendering issue at all for the activity bar
specifically (contrast the tab-bar entry below, which is): the rasteriser is
unconditionally painting the ASCII fallback string, the same string a
`nerd_fonts_enabled == false` GTK/macOS/TUI build would paint. The already-
shipped `macos::activity_bar` rasteriser is the reference fix shape: it takes
`nerd_fonts_enabled: bool` as an explicit parameter and branches
`item.icon.glyph.as_str()` vs `item.icon.fallback.as_str()` on it
(`quadraui/src/macos/activity_bar.rs`, "`nerd_fonts_enabled` picks which half
of each item's `crate::Icon` paints" doc comment).

**Ask:** thread `nerd_fonts_enabled: bool` into
`win::activity_bar::draw_activity_bar`'s signature (mirroring
`macos::activity_bar`'s existing parameter) and branch the icon string on it,
the same one-line change `win/backend.rs::draw_tree` already makes for
`super::tree::draw_tree` — `win/backend.rs::draw_activity_bar` just needs to
pass `self.nerd_fonts_enabled` through at its call site. Update the stale
"#25's icon-font plumbing" comment to note #804/#929 already shipped the
wiring this rasteriser alone never adopted.

**Test:** #1558's acceptance criterion #2 ("a Windows test asserts that an
activity-bar icon glyph resolves to a real font face, not a fallback or
tofu") needs new quadraui-side test infrastructure, not just the fix above —
`crate::testing::TextRun` (`quadraui/src/testing/mod.rs`) currently records
only `{ text: String, bounds: Rect }` per painted run, with no font-face
identity captured, so no existing `WinDriver`/`ConformanceHarness` assertion
can currently distinguish "painted the real Nerd Font glyph" from "painted a
`.fallback` string" or "painted tofu" — all three currently produce some
non-empty `text_runs` entry. At minimum, resolving this issue's own
acceptance bar needs `TextRun` (or a Win-GUI-specific extension of it) to
also record which `IDWriteFontFace`/family a run's glyphs actually resolved
against — e.g. via `IDWriteTextLayout::GetGlyphRunAnalysis` or per-run
`IDWriteFontFace::GetGdiCompatibleGlyphIndices` coverage checks. Absent
that, the best a driver test can assert today is that
`nerd_fonts_enabled(true)` + the fixed rasteriser paints `item.icon.glyph`'s
*text* (not `.fallback`'s) into a `TextRun` — real, but weaker than "resolved
to a real font face" per the issue's own wording.

**Blocks:** `JDonaghy/vimcode#1558` (activity-bar half). Leave that issue
open behind this one per `GOALS.md`'s milestone-discipline rule — there is no
per-backend vimcode-side fix available (`src/win/mod.rs`/`src/win/backend.rs`
are thin wrappers with no rasterising decisions per the Platform-Neutrality
Rule; `App::setup` already calls `render::register_nerd_font_fallback`
identically for every backend, so the vimcode side of this is already
correct and unchanged).

---

## Win-GUI tab-bar/editor Nerd-Font glyphs may lose to DirectWrite's own system fallback due to `AddMappings`/`AddMapping` call order (suspected, needs Windows verification; blocks vimcode#1558)

**Title:** `win::text::build_nerd_font_fallback` calls
`builder.AddMappings(&system_fallback)` **before** its own `AddMapping` for
the registered Nerd Font — if `IDWriteFontFallbackBuilder` mapping priority
is first-added-wins (as MS documentation describes for overlapping ranges),
the app's Nerd-Font mapping can never be reached for any codepoint Windows'
own system fallback table already claims, which very plausibly includes the
Private-Use-Area block Nerd Font glyphs live in

**Body:**

vimcode#1558 also reports Win-GUI tab-bar file icons rendering as "only a
plain document glyph" (not a placeholder character, unlike the activity-bar
half above — see that entry, which is a fully-confirmed, different root
cause). Unlike the activity bar, the tab-bar paint path is *not* the
"unconditionally uses fallback" bug: `render::build_tab_bar_icons`
(vimcode `src/render.rs`) already only constructs `TabIcon` entries when
`icons::nerd_fonts_enabled()` is true, and `icons::file_icon_for_name` /
`icons::Icon::s()` already resolve to the real Nerd-Font PUA codepoint
(`.nerd`, not `.fallback`) in that case — confirmed by reading both
functions; this part of the pipeline is platform-neutral and identical to
GTK/macOS/TUI, which all render tab icons correctly per the issue. So
`quadraui::TabIcon::glyph` genuinely carries the right string by the time it
reaches `win::backend::draw_tab_bar_icons`, which paints it via `self.dwrite`
— the same `DWrite` instance `#929` wires up with the registered Nerd Font's
`IDWriteFontFallback` via `apply_fallback_to_format`/`SetFontFallback`.

The suspected gap is inside `build_nerd_font_fallback`
(`quadraui/src/win/text.rs`, pinned rev `a58e5bec`):

```rust
let system_fallback = unsafe { factory.GetSystemFontFallback()? };
let builder = unsafe { factory.CreateFontFallbackBuilder()? };
unsafe { builder.AddMappings(&system_fallback)? };   // added FIRST

let ranges = [DWRITE_UNICODE_RANGE { first: 0x0, last: 0x0010_FFFF }];
// ... AddMapping(&ranges, &[family], ...) added SECOND, for the whole
// Unicode range, resolving against the app's registered Nerd Font.
```

The function's own doc comment states the intent explicitly: "`family` is
only ever *consulted* for a character none of the higher-priority system
mappings already resolved" — i.e. system fallback is meant to win first, our
mapping is the last resort. That is backwards for a Private-Use-Area icon
font: PUA codepoints have no "correct" system glyph to defer to, and if
Windows' own system fallback table has *any* entry that claims to cover that
range (a generic symbol/dingbat font, `Segoe UI Symbol`, a CJK/emoji fallback
font with broad coverage, or the "Last Resort" font DirectWrite consults for
otherwise-unmapped codepoints), that entry — not the app's registered Nerd
Font — is what a first-added-wins fallback builder would resolve to. The
activity-bar placeholder characters in this same bug report are proven (see
sibling entry above) to be the plain ASCII `.fallback` string, not tofu or a
substituted glyph — but the tab-bar symptom ("a plain document glyph",
implying *something* renders, consistently, for every file regardless of
extension) is consistent with DirectWrite finding one single system-fallback
font that happens to have a "generic document" glyph mapped somewhere in the
PUA range and using it for every Nerd Font codepoint in that font, rather
than ever reaching the app's own registered font.

**This is a hypothesis, not a confirmed root cause** — dell64 (this fleet's
real Windows 11 host, see #1558's "Verify on real Windows" section) is
reachable and `cargo xwin build`/direct-exe-launch both work on it, but
empirically exercising DirectWrite's actual `IDWriteFontFallbackBuilder`
priority needs either a live GUI visual check or a new automated test, and
both avenues hit dell64-local blockers during this investigation (the
interactive session was locked, blocking screen capture; the `cargo xwin
test --lib` binary crashes at Windows DLL-load time with
`STATUS_ENTRYPOINT_NOT_FOUND` before any `#[test]` runs — see `src/win/mod.rs`'s
`#1558` doc section in vimcode for the full repro of both). So this hypothesis
is still unconfirmed, but for those two concrete, named reasons — not for
lack of a host. `IDWriteFontFallbackBuilder`'s actual first-vs-last priority
for overlapping `AddMapping`/`AddMappings` ranges should be verified against
Microsoft's documentation (or empirically, once the blockers above are
cleared) before committing to a fix. If confirmed, the fix is likely as
simple as swapping the order — call `builder.AddMapping(...)` for the app's
font first, then
`builder.AddMappings(&system_fallback)` last, so the app's Nerd Font is
consulted before Windows' own broad-coverage system fallback rather than
after it — mirroring the *intended* cascade shape GTK's
`crate::gtk::with_nerd_font_fallback` doc already describes ("later family in
the list, consulted only for uncovered characters" — for Pango's cascade,
the *primary UI/editor font* is first and genuinely lacks PUA coverage, so
falling through to the Nerd Font next in line works; DirectWrite's
`IDWriteFontFallback` object is a *separate* structure from the primary
format's own font, consulted only when the primary font's glyph lookup
already failed — so within *that* structure, the app's font needs to be
tried before Windows' generic system fallback, not after).

**Ask:** on real Windows hardware, verify whether swapping the
`AddMapping`/`AddMappings` call order in `build_nerd_font_fallback` makes
tab-bar (and editor-body, if any Nerd-Font-glyph content appears there) icons
resolve to the registered Symbols Nerd Font subset instead of a system
substitute. If confirmed, land the reordering; if the true cause is
something else in this pipeline (`register_font_from_memory`'s private
collection, `attach_surface`/`attach_headless` call ordering relative to
`App::setup`, or a device-lost surface rebuild dropping the fallback), file
a follow-up with the real cause once it's found.

**Blocks:** `JDonaghy/vimcode#1558` (tab-bar half). Leave that issue open
behind both entries above per `GOALS.md`'s milestone-discipline rule — there
is no per-backend vimcode-side fix available here either; the vimcode-side
data pipeline (`render::build_tab_bar_icons`, `icons::file_icon_for_name`) is
already correct and platform-neutral.

---

## ~~TUI test drivers can't observe `Backend::request_full_repaint`'s effect from a downstream `ShellApp` (blocks vimcode#1243's black-box test)~~ — **FILED as quadraui#1060, do not file (struck 2026-09-24)**

> **This draft is retired: it is now a real issue.** Filed 2026-09-24 as
> quadraui#1060 (milestone #9), after re-checking the gap at vimcode's pin
> `3020d9e` and quadraui `develop`: there is still no `vt_testing::driver_with_shell`,
> and `build_shell_adapter` and `TuiBackend::take_full_repaint_requested` are
> still `pub(crate)`. The full draft text lives in that issue now. #1243 was
> closed before this was filed, so the black-box test it asked for is tracked
> as **vimcode#1393**, queued behind quadraui#1060 and the pin bump
> vimcode#1388.

---

## ~~TUI runner has no host-facing "force full repaint" hook (blocks vimcode#58)~~ — **SHIPPED, do not file (struck 2026-09-23, #1243)**

> **This draft is retired. The API exists, is pinned, and is now adopted.**
> quadraui#1037 shipped exactly the "Ask" shape 2 below —
> `Backend::request_full_repaint()`, default no-op, implemented on
> `TuiBackend` as a flag `tui::run::run_inner` consumes via
> `take_full_repaint_requested` and answers with `Terminal::clear()` before
> the next `render_frame`. Verified present at vimcode's pin `215e9e4` by
> reading `quadraui/src/backend.rs:1368`, `quadraui/src/tui/backend.rs:1543`
> and `quadraui/src/tui/run.rs:338`, not inferred from the issue being closed.
>
> vimcode#1243 consumed it: `render::is_force_redraw_key`'s Ctrl+L rung and
> `TuiShellApp::render_content`'s `had_popup_overlay` transition — the two
> "consumers waiting on this" named in the draft below — both call the hook
> now, so `had_popup_overlay` has a reader again.
>
> **What is left is test infrastructure, not the hook**, and it is filed as its
> own entry directly above ("TUI test drivers can't observe
> `Backend::request_full_repaint`'s effect…"): the shipped hook has no
> downstream-observable effect under any public driver, so vimcode#1243 cannot
> yet ship the black-box test its own acceptance criteria require.
>
> The original draft is kept below, struck, so the history of the verdict is
> readable — **do not file it.**

### ~~Original draft (superseded by quadraui#1037)~~

**Title:** `tui::run`/`run_with_shell` internalised the `Terminal`, silently
dropping the only mitigation vimcode#58 (stale-character rendering artifacts)
ever had — no `Reaction`/`Backend` hook replaces it

**Body:**

vimcode#58 tracks intermittent stale characters left on screen: ratatui's
incremental diff can miss cells when the physical terminal's real state
diverges from its internal `Buffer` tracking (typical triggers: PTY writes
into the embedded terminal pane, certain resize sequences, popup
dismissal). vimcode's Session-244 mitigation was to call
`ratatui::Terminal::clear()` — which resets the diff cache so the *next*
frame repaints every cell unconditionally — on resize events and on
popup-dismiss transitions, from its own hand-rolled event loop
(`src/tui_main/mod.rs`, pre-#634).

That loop no longer exists. #634 moved vimcode's TUI onto
`quadraui::tui::shell_runner::run_with_shell` (this crate's `tui::run`/
`run_with` family, `quadraui/src/tui/run.rs`), which now owns the
`ratatui::Terminal` internally and calls `terminal.clear()` exactly once,
at startup (`run_with`, `quadraui/src/tui/run.rs:203`) — never again for
the life of the process. Confirmed by reading the pinned rev
(`7a77602`): `Reaction` (`quadraui/src/runner.rs`) has only
`Continue`/`Redraw`/`RedrawAfter(Duration)`/`Exit` — no variant that maps
to "clear before the next draw" — and neither `Backend` nor `AppLogic`
exposes a `request_full_repaint`-shaped method the runner's frame loop
would consult. So there is currently no way for a quadraui-hosted TUI app
to ask for what `Terminal::clear()` gives a raw ratatui app.

vimcode's own code already documents this as a known, currently-inert
gap rather than working around it: `render::is_force_redraw_key`'s doc
comment (Ctrl+L, `src/render.rs`) and `TuiShellApp::render_content`'s
`had_popup_overlay` tracking (`src/tui_main/shell_app.rs`) both say so —
Ctrl+L today only returns `Reaction::Redraw`, which re-runs the same
incremental diff that missed the cells in the first place, so it does not
actually fix anything a user hits it for. `had_popup_overlay` is computed
and stored every frame but has no reader left — the call site it used to
drive (`terminal.clear()`) was deleted along with the legacy loop.

**Ask:** give a TUI-hosted `AppLogic` a way to force the next frame to
paint as if the terminal were blank. Two shapes, either resolves this:

1. A new `Reaction::FullRedraw` variant — `tui::run`'s frame loop calls
   `terminal.borrow_mut().clear()?` before the next `render_frame` when an
   event handler returns it, otherwise identical to `Reaction::Redraw`.
2. A `Backend::request_full_repaint()` method (default no-op) that
   `TuiBackend` implements by setting a flag the runner checks each loop
   iteration before drawing — mirroring how `request_frame_in`/
   `Reaction::RedrawAfter` already thread a scheduling request through the
   same seam, so it needs no new event/dispatch plumbing.

GTK does not need this: Cairo repaints its `DrawingArea` in full every
frame (no incremental diff to desync), which the existing
`gtk::backend` tests documenting "full repaint after a skipped frame /
modal closed / theme change" already rely on. So this is a TUI-only gap
today, but the hook itself should stay on the backend-neutral trait
surface (`Backend`, not a TUI-only escape hatch) so a future diff-based
renderer (a terminal-multiplexer-aware Win-GUI console mode, say) isn't
left with the identical hole.

**Consumers waiting on this, already commented in place:**
`render::is_force_redraw_key` (Ctrl+L) and
`TuiShellApp::render_content`'s `had_popup_overlay` field
(`src/tui_main/shell_app.rs`) both name the exact call site that would
call the new hook the moment it exists.

**Blocks:** `JDonaghy/vimcode#58` — leave that issue open behind this one,
per `GOALS.md`'s milestone-discipline rule.

---

## ~~Multi-band bottom chrome (blocks vimcode#820)~~ — **SHIPPED, do not file (struck 2026-09-22, #1259)**

> quadraui#997 (`d1b1931`) shipped N independently-gated stacked bottom
> bands, and vimcode's pinned rev carries it. This draft is retired — do
> not file it.

---

## ~~`TabGroupController` has no external-model adoption path (blocks vimcode#822)~~ — **SHIPPED, do not file (struck 2026-09-22, #1259)**

> quadraui#998 (`ca6f40a`) shipped an external-model adoption path for
> `TabGroupController`, and vimcode's pinned rev carries it. This draft is
> retired — do not file it.

---

## ~~`quadraui::win::testing` is hard `target_os = "windows"`-gated, not WinAPI-stubbed like `win::backend`/`run`/`shell_runner` (blocks vimcode#928 AC2)~~ — **SHIPPED, do not file (struck 2026-09-23, #1244)**

> quadraui#1038 (landed in the rev this crate is pinned to, `Cargo.toml`)
> gated `win::testing` on `feature = "win"` alone with every real
> Direct2D/GDI call individually `cfg(target_os = "windows")`-stubbed,
> exactly the ask below. vimcode#1244 consumed it: `src/win/mod.rs`'s
> `win_driver_tests` module dropped its `#[cfg(target_os = "windows")]`
> double-gate down to `#[cfg(test)]` alone (each `#[test]` attribute is now
> individually `cfg_attr(target_os = "windows", test)`-gated instead, so the
> bodies type-check everywhere but only actually run on real Windows),
> closing vimcode#928's acceptance criterion #2. This draft is retired — do
> not file it.

### ~~Original draft (superseded by quadraui#1038)~~

**Title:** `win::testing` needs the same `cfg(target_os = "windows")`-per-call
stubbing as `win::backend`/`run`/`shell_runner`, not a module-level
`target_os` gate

**Body:**

vimcode#928 adopts `quadraui::testing::ConformanceDriver` as a single
backend-neutral black-box test harness and requires (acceptance criterion
#2) that `cargo check --no-default-features --features win` type-check the
`WinDriver` instantiation of that harness on an ordinary Linux host — the
same posture `quadraui::win::backend`/`run`/`shell_runner` already have:
gated on `feature = "win"` alone, with every real WinAPI call individually
`cfg(target_os = "windows")`-gated internally and falling back to a stub
everywhere else, specifically so a Linux CI runner can type-check
`WinBackend` (see quadraui's own `ci.yml` "Compile check (win feature)"
step, and `docs/RELEASING.md` §1.4 on the vimcode side, which documents
this as the existing, working pattern).

At the pinned rev (`dbb3023`), `quadraui/src/win/mod.rs:190-192` declares
`pub mod testing;` — the module defining `WinDriver`/`driver_with_shell` —
`#[cfg(target_os = "windows")]`-gated at the module level, with no
internal WinAPI stubbing inside it the way `win::backend`/`run` have. That
means the module (and everything in it) simply does not exist to the
compiler off Windows; there is no `cargo check`/`cargo test --no-run`
invocation on Linux that can even *see* `WinDriver`, let alone type-check
code that constructs one.

vimcode's own `src/win/mod.rs::win_driver_tests` module (added by #928) has
to compound that with its own `#[cfg(target_os = "windows")]` (on top of
`#[cfg(test)]`), so its `ConformanceHarness<WinDriver<...>>` instantiation
is verified only on a real Windows host — never on the Linux fleet this
project develops on day to day. That directly blocks #928's acceptance
criterion #2, which is currently **unmet** and will stay unmet until this
lands.

**Ask:** gate `quadraui::win::testing` the same way `win::backend`/`run`/
`shell_runner` are gated — `feature = "win"` alone, with `WinDriver`'s
internals individually `cfg(target_os = "windows")`-stubbing their WinAPI
calls (window creation, message loop, hit-testing surface) — so
`cargo check --no-default-features --features win` (and
`cargo check --tests` / `cargo test --no-run` with the same flags) type-checks
`WinDriver`/`driver_with_shell`/`ConformanceHarness<WinDriver<...>>` on an
ordinary Linux host, exactly as it already does for `WinBackend` itself.

**Blocks:** `JDonaghy/vimcode#928` — acceptance criterion #2
("`cargo check --no-default-features --features win` type-checks the Win
instantiation on an ordinary Linux host") is unmet until this lands.
`src/win/mod.rs::win_driver_tests` stays double-gated
(`#[cfg(target_os = "windows")]` + `#[cfg(test)]`) in the interim — inert,
and known to be inert, on every host but real Windows. Leave #928 open
behind this one per `GOALS.md`'s milestone-discipline rule; do not treat
the double gate as a workaround that closes the gap.

---

## ~~TUI minimap has no horizontal downsampling (blocks vimcode#1030 deliverable 2)~~ — **SHIPPED, do not file (struck 2026-09-22, #1259)**

> Shipped at `55acc70` and consumed on the vimcode side by #1175. This draft
> is retired — do not file it.
---

## ~~`quadraui::CommandLine` has no `selection` field to paint~~ — **SHIPPED, do not file (struck 2026-09-19, #1168)**

> **This draft is retired. The API exists and is already pinned.** quadraui#1001
> landed `Backend::draw_command_line_selection` alongside
> `CommandLineLayout::selection_bounds`, and vimcode's current pin
> `d907a06` (bumped by #1133) carries it — verified by `git grep` against that
> rev, not inferred from the issue being closed. The shape differs from the
> "Ask" below: upstream chose a **dedicated draw call** rather than a
> `selection` field on `CommandLine`, so do not go looking for the field.
>
> **What is actually left is host-side, and belongs to vimcode#1169, not here:**
> vimcode adopts `selection_bounds` but does not yet call
> `draw_command_line_selection`, so `render::command_line_selection_rect` still
> hand-computes the rect and **its doc comment still claims the upstream API does
> not exist** — that comment is stale and should be deleted along with the helper
> when the paint call is adopted. Fixing it is a code change, deliberately out of
> scope for #1168's documentation pass.
>
> vimcode#194's visual-highlight half is therefore **no longer supply-blocked**.
>
> The original draft is kept below, struck, so the history of the verdict is
> readable — **do not file it.**

### ~~Original draft (superseded)~~

**Title:** `CommandLine`/`draw_command_line` cannot paint a selection highlight —
`CommandLineLayout::hit_test`/`selection_bounds` compute the geometry but nothing
carries it to either backend's paint call

**Body:**

vimcode#1044 (the ShellApp/mouse.rs decomposition audit) re-verified
`docs/IRREDUCIBLE_SURFACE.md` §2a's existing verdict on command-line text
selection and found it **stale, not wrong in direction**: that section says
`CommandLineLayout::hit_test` "does not exist anywhere in quadraui" — true when
written (2026-09-03), but quadraui#705 shipped
`CommandLineLayout::hit_test`/`selection_bounds` (`quadraui/src/primitives/command_line.rs:98,134`,
present at the currently-pinned rev `8abca3a`) and vimcode already adopted both,
unconditionally shared by both backends: `render::command_line_click_char_idx`
and `render::command_line_selection_rect` (`src/render.rs:19933,19979`) call
straight through to them. So the **hit-test** half of the old verdict is now
`already-shared`, not a gap — no issue needed there.

What's left, and is real and current: `render::command_line_selection_rect`'s
own doc comment already names it — `quadraui::CommandLine` carries no
`selection` field, and neither the GTK nor TUI `draw_command_line` in quadraui
paints one. `command_line_selection_rect` computes the paintable highlight rect
and has never been wired into either paint path (its doc: "Not wired into
either backend's paint path yet (#816 review)"). TUI works around this by
painting the command line cell-by-cell with the selection baked into the
foreground/background inversion (`tui_main::panels::render_command_line`) —
so TUI *has* a visible selection highlight today, just via a hand-rolled paint
path instead of the shared primitive. GTK has **no visible highlight at all**:
a user who drags a selection over the GTK command line gets `cmd_sel`/Ctrl+C
behavior with zero visual feedback, because there is nowhere in
`quadraui::CommandLine` to put the selection so GTK's `draw_command_line` could
paint it.

**Ask:** add a `selection: Option<(usize, usize)>` (or similar) field to
`quadraui::CommandLine`, and have both backends' `draw_command_line`
(GTK/Cairo, TUI/ratatui) paint the corresponding highlight rect/cell-inversion
when it's set — the geometry math for GTK is already done and waiting
(`render::command_line_selection_rect`); the ratatui side would let TUI stop
hand-painting the highlight itself and instead pass `selection` through like
every other `CommandLine` field.

**Blocks:** `JDonaghy/vimcode#194` ("Status-bar / command-line messages aren't
mouse-selectable — GTK can't; TUI has offset bug") — the hit-test half of #194
is unblocked (already-shared, as above); the visual-highlight half stays
blocked on this. Leave #194 open behind this one per `GOALS.md`'s
milestone-discipline rule. Also update `docs/IRREDUCIBLE_SURFACE.md` §2a once
this is filed — that section's "does not exist anywhere in quadraui" claim
needs correcting to point at this narrower, still-open gap instead (done in
this same PR, see that file's new §2c).

---

## ~~`TuiBackend` lets a `None` cursor_position clobber a `Some` within one frame (blocks vimcode#1039)~~ — **CLOSED AS DUPLICATE, do not file (struck 2026-09-22, #1259)**

> This draft named a gap that had already closed upstream, unnoticed: the
> fix landed as quadraui#1002 (`e4ef921`) on 2026-09-17, three days before
> this draft was filed (from a checkout that predated the fix) as
> quadraui#1039 on 2026-09-20. quadraui#1039 was closed as a duplicate on
> 2026-09-21, after three worker dispatches each exited with zero commits
> because there was nothing left to change. This draft is retired — do not
> file it, and do not link #1039 as a live issue anywhere citing
> vimcode#1039. The other issues filed from this same batch —
> quadraui#1037, #1038, #1040 — are unaffected and remain live; see this
> file's other entries.

---

## ~~`draw_editor`'s decoration overlays index against the caller's `area`, not the real `buf` extent — panics on terminal resize (blocks vimcode#203)~~ — **LANDED UPSTREAM, consumed by the pin (struck 2026-09-23, #1246)**

> Filed as quadraui#1040 (title: "`draw_editor` bounds-checks overlays against
> `buf.area`, not stale area") and fixed there in `a0961f8` (2026-09-20),
> which also closed the TOCTOU gap in `quadraui/src/tui/run.rs` that produced
> the stale `area` (fix 2 of the two-part ask below — `render_frame`'s
> `Viewport` is now derived from `frame.area()` inside the
> `terminal.draw(...)` closure instead of a pre-draw `terminal.size()`
> query). Confirmed via `git merge-base --is-ancestor a0961f8 <pin>` against
> vimcode's `Cargo.toml` pin (`215e9e4...`, unchanged by this check) that the
> fix commit is already an ancestor of the pinned rev — it landed via an
> earlier, unrelated pin bump (the rev was already this far ahead when #1246
> picked this up), so no `Cargo.toml` edit was needed here.
>
> vimcode#203/#1246's premise — "vimcode carries a host-side guard" — did not
> hold: the #203 investigation (`51776a1`) found no such guard was ever
> added, and none exists in `src/tui_main/render_impl.rs::render_window`
> today (confirmed by inspection — it is still the ~25-line delegator the
> investigation described, no bounds logic to remove). There is also no
> `#203` regression test in vimcode to "stay green" — the crash lived
> entirely inside `quadraui::tui::draw_editor`/`run.rs`'s TOCTOU gap between
> a pre-draw size query and `Terminal::draw`'s internal autoresize, which
> `ratatui::backend::TestBackend`'s fixed-size construction (what
> `quadraui::tui::testing::TuiDriver` drives) cannot reproduce without new
> quadraui-side test infrastructure to resize a `TestBackend` mid-`Terminal`
> lifetime — out of scope for a vimcode-only issue. #203 is safe to close as
> fixed upstream and consumed by the pin.

---

## ~~`quadraui::tui::testing::TuiDriver` has no way to resize its `TestBackend` mid-test — a TOCTOU/resize regression (e.g. quadraui#1040's class of bug) can't be driver-tested from a downstream crate~~ — **FILED as quadraui#1063, do not file (struck 2026-09-24)**

> **This draft is retired: it is now a real issue.** Filed 2026-09-24 as
> quadraui#1063 (milestone #9), after re-checking the gap at vimcode's pin
> `3020d9e` and quadraui `develop`: `TuiDriver` still keeps its
> `Terminal<TestBackend>` private, sizes it once in `new`, and has no
> `resize`/`terminal()` accessor. The full draft text lives in that issue
> now. It blocks no open vimcode issue; it exists so a future resize/TOCTOU
> regression has a driver-tier repro path.

---

## ~~`WinBackend::install_menu_bar_now`'s `SetMenu` call re-enters `wndproc` and panics on `ws.state.borrow_mut()` — Win-GUI crashes on every startup once `native_menu` is declared (blocks vimcode#1614, and transitively vimcode#1559/#1562/#1582)~~ — **FIXED upstream, struck 2026-09-29 (#1618)**

> **This entry is resolved.** The reentrancy fix landed upstream as
> `ce1c763` (a `ModalPumpGuard` around the reentrant `SetMenu` call), with a
> regression test added in `6e14d8a` — both are ancestors of the pin
> `6e14d8a081929cb5c6792c91e15e5ffa26fc47da` that vimcode#1618 bumped
> `Cargo.toml` to. Real-hardware verification on dell64 (#1618) confirms
> `vimcode.exe` launches cleanly with a native menu bar, custom title bar,
> command centre, and glyph-preserving block cursor all present at
> startup — unblocking vimcode#1559/#1562/#1582 for closure. The original
> repro/isolation/ask below is left intact for history.

**Title:** `SetMenu` (called from `WinBackend::install_menu_bar_now`, quadraui#1200) synchronously re-enters `win::run`'s `wndproc` with a nested `WM_SIZE` via `SendMessageW`/`CallWindowProcW` while the outer call already holds `ws.state.borrow_mut()`, panicking with `RefCell already borrowed` at `quadraui/src/win/run.rs:1712:42` (line 1711 at the `cc2b80d` pin) — inside a Win32 callback that cannot unwind, so the process aborts. 100% reproducible: `vimcode.exe` crashes before showing a window, on every launch, on real Windows hardware (dell64).

**Body:**

vimcode#1614 asked to bump the pin to pick up quadraui#1197 (block-cursor
glyph fix), quadraui#1200 (native menu bar), and quadraui#1199 (custom
title-bar chrome) — three real fixes, each individually verified by their
own commit messages via `cargo xwin build`/`test`. But none of those
verifications ran a *live* `vimcode.exe`/example through a real Win32
message loop with `native_menu: true` declared, which is what surfaces
this bug: it is a **runtime reentrancy defect**, invisible to
`HeadlessSurface`-based paint tests and to `cargo check`/`cargo build`
type-checking alike.

Root-caused via a real cross-compiled build (`cargo xwin build --release
--target x86_64-pc-windows-msvc --no-default-features --features win`) run
directly on dell64 (this WSL2 environment's host — `hostname` returns
`dell64`, so no remote-transfer step was even needed): `vimcode.exe test.txt`
launched and immediately crashed, writing the following to stderr and
`%TEMP%\vimcode-crash.log` every time:

```
thread 'main' (N) panicked at .../quadraui/src/win/run.rs:1712:42:
RefCell already borrowed
thread 'main' (N) panicked at .../core/src/panicking.rs:225:5:
panic in a function that cannot unwind
stack backtrace:
  ...
  CallWindowProcW
  SendMessageW
  IsWindowEnabled
  IsWindowEnabled
  Ordinal75
  Ordinal75
  SendMessageW
  CallWindowProcW
  GetFocus
  EnumDisplayDevicesW
  KiUserCallbackDispatcher
  NtUserSetMenu
  ...
```

`quadraui/src/win/run.rs:1712` (`win::run`'s `wndproc`, the pinned rev's
line numbers) is the `WM_SIZE` arm:

```rust
WM_SIZE => {
    let (width, height) = size_from_lparam(lparam.0);
    let viewport = {
        let mut s = ws.state.borrow_mut();   // <-- panics here
        ...
```

The backtrace shows this `WM_SIZE` firing *from inside* `NtUserSetMenu` —
Win32's `SetMenu` synchronously recalculates the non-client area and can
dispatch nested messages to the same window on the same thread before
returning, the same way `DrawMenuBar`/`SetWindowPos` are documented to.
`WinBackend::install_menu_bar_now` (quadraui#1200, `win/backend.rs`) calls
`SetMenu(hwnd, ...)` from inside `Self::attach_surface`, which itself runs
while `win::run`'s window-creation path already holds `ws.state.borrow_mut()`
— so the nested `WM_SIZE`'s own `ws.state.borrow_mut()` panics on the
already-live borrow. `wndproc`'s existing re-entrancy guard —

```rust
if ws.pump_depth.is_pumping() {
    return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
}
```

— (added for #702, the message-pump reentrancy hazard) does **not** cover
this path: `pump_depth` tracks the *main event-loop pump*, which hasn't
started yet during window creation, so a nested message arriving via a
synchronous Win32 API call (not the pump) sails straight through to the
`match msg` arms and hits the live borrow.

**Isolation performed (all on real dell64 hardware, `cargo xwin build
--release --target x86_64-pc-windows-msvc --no-default-features --features
win`, each launched via `Start-Process`/`Get-Process` against the actual
Windows session):**

| quadraui rev | Contains | Result |
|---|---|---|
| `db92e461` (pin before #1614) | none of #1197/#1199/#1200 | **Launches cleanly** — real window, `Get-Process` shows `MainWindowTitle: VimCode`, `Responding: True` |
| `cc2b80d` (has #1197 + #1200, not #1199) | block cursor + native menu bar, no custom title bar | **Crashes on every launch**, identical panic site/message |
| `928f2b5` (develop HEAD; has all three) | block cursor + native menu bar + custom title bar | **Crashes on every launch**, identical panic site/message |

This isolates the regression to quadraui#1200 (`90e4310`, the native-menu-bar
commit) specifically — `window_chrome`/#1199 is not required to reproduce
it, and the block-cursor fix (#1197) is inert here (paint-only, no wndproc
change). `native_menu: true` only gets declared once `WinBackend::
backend_caps()` picks it up (also part of #1200), and `App::setup`
(vimcode `src/app.rs`) calls `Backend::install_menu_bar` unconditionally
for any backend declaring that cap — so any consumer that adopts
`native_menu` the documented way hits this immediately.

**Ask:** `WinBackend::install_menu_bar_now`'s `SetMenu` call needs to not
run while `ws.state` is borrowed on the calling stack — either defer the
actual `SetMenu` call (e.g. via `PostMessage`/a custom registered message,
so it happens on a later, non-reentrant pump iteration) the way `WM_SIZE`'s
own resize-settle debounce (`RESIZE_TIMER_ID`, quadraui#780) already defers
work off the hot path, or make the reentrant `WM_SIZE` (and any other
message that can arrive synchronously from a Win32 call made mid-borrow)
robust to a live borrow — e.g. `try_borrow_mut` with a `DefWindowProcW`
fallback on `Err`, mirroring `pump_depth.is_pumping()`'s existing
graceful-defer shape, extended to cover reentrancy from *any* source, not
just the main pump.

**Test:** a `WinDriver`/real-`wndproc` scenario that installs a menu bar
during window creation (the exact `AppLogic::setup`-time call path
`install_menu_bar`'s own doc describes) and asserts the window finishes
creating without panicking — the existing `HeadlessSurface`-based tests in
`win::backend`'s `#[cfg(test)] mod tests` don't drive a real `wndproc`, so
this needs either a live (non-headless) `CreateWindowExW` in the test itself
(gated `target_os = "windows"`, run via `cargo xwin test` on real hardware
like every other live-window test this crate already has) or a `wndproc`
unit test that directly synthesizes the nested-`WM_SIZE`-during-`SetMenu`
sequence against a fake `WindowState` to reproduce the double-borrow without
needing a live window at all. Observed RED against `cc2b80d`/`928f2b5` (100%
reproducible, real hardware); must be observed GREEN before closing.

**Blocks:** `JDonaghy/vimcode#1614` directly, and transitively
`JDonaghy/vimcode#1559`/`#1562`/`#1582` — none of those three could be
verified (the app never showed a window) let alone closed while this
crash existed. vimcode's pin stayed at `db92e461` (pre-#1197/#1199/#1200)
until this landed; there was no vimcode-side workaround (`App::setup`'s
call-site timing for `install_menu_bar` is irrelevant — the crash happened
inside quadraui's own window-creation sequence regardless of when the
platform-neutral caller invokes the trait method), per the
Platform-Neutrality Rule. **Resolved by #1618** — see the struck-entry
note above.

