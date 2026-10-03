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

---

## No UIA/NSAccessibility/AT-SPI provider wiring exists for any backend's drawn primitives — `coord`'s new `win-native`/`mac-native`/`gtk-native` acceptance drivers' `expect_a11y`/`expect_a11y_within` steps can only ever see native window chrome, never vimcode's own content

**Title:** `quadraui::a11y::A11yInfo` (#835) is data-field groundwork only —
no backend registers an `IRawElementProviderSimple` (Win UIA), an
`NSAccessibility` protocol implementation (macOS AX), or an `AtkObject`/
AT-SPI provider for any drawn primitive (menu bar, activity bar, dropdown,
toast, extensions list, …) on any platform

**Body:**

Found while writing `JDonaghy/vimcode#1646`'s Tier-2 `tests/smoke-spec/*.yaml`
specs for coord's new real-platform drivers (`coord/win_native_driver.py`,
`coord/mac_native_driver.py`, `coord/gtk_native_driver.py`). Those drivers'
`expect_a11y`/`expect_a11y_within` steps walk the *real* OS accessibility
tree (`IUIAutomation`/`AXUIElementCopyAttributeValue`/`gi.repository.Atspi`)
— by design, they ask the real OS, not quadraui, "is there an accessible
element here" (see each driver module's own docstring). That tree can only
ever contain what the platform itself already knows about (the top-level
HWND/NSWindow/GtkWindow, and — only where a backend still uses a *real*
native widget, e.g. macOS's `NSMenu` menu bar — that widget's own native
accessibility) plus whatever a backend explicitly registers a custom
provider for. Grepping `quadraui/src` for `IRawElementProviderSimple`,
`IAccessible`, `UiaRaiseAutomationEvent`, `NSAccessibility` (Win/mac) and
reading `quadraui/src/a11y.rs` end to end (macOS and Linux have no
equivalent module at all) confirms: zero backends register any such
provider for any of quadraui's own drawn primitives. `a11y.rs`'s own module
doc says this outright — `A11yInfo` is "data fields on every primitive"
groundwork per `docs/UI_CRATE_DESIGN.md` decision #6, with "platform AT
wiring (UI Automation / NSAccessibility / AT-SPI) deferred to v1.1", and
"**No AT backend integration is attempted here**". `A11yInfo` is also not
yet wired into any primitive's struct or the `Backend` trait (that module's
own doc, "not wired into any primitive... in this change").

Net effect on `#1646`'s specs: every `expect_a11y`/`expect_a11y_within` step
that targets something vimcode itself draws — the File menu's dropdown
items, an activity-bar icon's panel, an extensions-list row's install
state, a toast's completion text — has nothing to find and fails, on every
one of the three native-driver platforms, regardless of whether the
*application* behaviour being probed is actually correct. This is a
different, lower-level gap than any single numbered vimcode bug: it is why
`#1646`'s win-gui/mac-gui/gtk-gui specs can only exercise *native* window
chrome today (`expect_hit`, `expect_menu`, `expect_closed`, and — on
macOS only, via the one remaining real `NSMenu` — the menu bar's own native
accessibility) and must leave every a11y-tree assertion against vimcode's
own drawn content marked expected-red in a comment pointing at this entry,
rather than quietly weakening those steps to something that cannot fail.

**Ask:** land real AT backend wiring for at least one drawn primitive per
platform (the menu bar/dropdown is the highest-value target — it is the
one every `#1646` spec's `expect_a11y_within` step already needs) —
`IRawElementProviderSimple`/`UiaRaiseAutomationEvent` on Win-GUI,
`NSAccessibility` role/attribute methods on an `NSView` subclass on macOS,
and an `AtkObject`/`gtk_widget_get_accessible` provider (GTK4's own
`GtkAccessible` interface) on GTK — each exposing at minimum a role, a
name, and (for container-like primitives) a children list, keyed off the
same `A11yInfo` fields `#835` already landed. `A11yInfo` would need to
actually reach the `Backend` trait or a per-primitive paint call for any of
this to have data to expose, which `a11y.rs`'s own doc already flags as
the deliberately-deferred next step.

**Test:** a `WinDriver`/`MacDriver`/`GtkDriver` scenario that opens a menu
dropdown and asserts a real UIA/AX/AT-SPI client observes a `MenuItem`-role
element with the expected name — needs each backend's real driver harness
(`win_driver_tests`/`macos::mac_driver_tests`/GTK's own), not just the
in-process `TuiDriver`-style text-screen assertions these backends already
have, since the whole point is whether a real AT client sees anything.

**Blocks:** `JDonaghy/vimcode#1646`'s win-gui/mac-gui/gtk-gui
`expect_a11y`/`expect_a11y_within` seed checks for anything other than
native window chrome. Leave `#1646` open behind this one per `GOALS.md`'s
milestone-discipline rule — the smoke-spec files themselves are not
blocked (they parse and run; their a11y-tree assertions are just
expected-red until this lands), only the *green* state of those specific
steps is.

---

## `WinBackend::register_status_bar_segment_zones` (quadraui#1232) registers each segment's *bar-local* bounds as if they were absolute window coordinates — the inline minimize/maximize/close buttons stay `HTCAPTION` whenever their `StatusBar` isn't painted at `rect.x == 0` (blocks vimcode#1656, vimcode#1675)

**Title:** `crate::primitives::status_bar::StatusBar::layout`/`layout_padded`
returns `hit_regions`/`visible_segments[].bounds` in bar-local (0-origin)
coordinates by design — `WinBackend::draw_status_bar_interactive`'s #1232
zone-registration fix (`register_status_bar_segment_zones`) feeds those
bounds straight into `Backend::register_zone` uncorrected, unlike the
sibling `register_menu_bar_item_zones`/`register_command_center_zones`
helpers the same commit added, whose source primitives (`MenuBar::layout`,
`CommandCenter::layout`) both bake the real `Rect`'s `x`/`y` into their
returned bounds already

**Body:**

vimcode#1656 re-reports, at the pin *including* quadraui#1232's own fix
commit (`fc94f2136bf66139be4da04ce5f16d78736b55cf`, confirmed to be the
exact `rev` in this repo's `Cargo.toml` at investigation time), that a real
`WM_NCHITTEST` sweep across the Win-GUI title band still classifies the
drawn minimize/maximize/close buttons as `HTCAPTION` — swallowing a real
click before it ever reaches the app as a `MouseDown` — while the File/
Edit/.../Help menu-row items *and* the command-centre search box, painted
into the exact same band by the exact same `App::render_content`/
`paint_title_bar_band` call in the exact same frame, both now correctly
read `HTCLIENT` and are genuinely clickable. This is the precise
differential #1232's own commit message claimed to close for all three
("status-bar segments (vimcode's inline minimize/maximize/close buttons)"
named explicitly) — so the fix landed for two of the three widget kinds it
names, not the third.

Root-caused by reading the pinned rev (`fc94f21`) source, not just
re-observing the symptom:

1. `WinBackend::draw_status_bar_interactive` (`quadraui/src/win/backend.rs`)
   paints via `crate::primitives::status_bar::native_surface_paint::paint`
   and then calls `self.register_status_bar_segment_zones(&layout)` on the
   `StatusBarLayout` that call returns — mirroring `draw_menu_bar`'s
   `register_menu_bar_item_zones(&layout)` and `draw_command_center`'s
   `register_command_center_zones(&cc.id, &layout)` exactly, same shape,
   same place in the call sequence.
2. `register_status_bar_segment_zones` (`quadraui/src/win/backend.rs`):
   ```rust
   fn register_status_bar_segment_zones(&mut self, layout: &StatusBarLayout) {
       for (bounds, hit) in &layout.hit_regions {
           if let StatusBarHit::Segment(id) = hit {
               self.register_zone(id.clone(), *bounds);
           }
       }
   }
   ```
   registers `*bounds` verbatim, with no translation by the `rect` that was
   passed into `draw_status_bar_interactive`.
3. But `StatusBar::layout`/`layout_padded`
   (`quadraui/src/primitives/status_bar.rs`) — the function that actually
   computes `hit_regions`/`visible_segments[].bounds` — takes only
   `bar_width: f32, bar_height: f32` (no `x`/`y` at all), and every segment
   rect it builds starts from a `cursor` that begins at `edge_inset` (left
   group) or is derived from `bar_width` alone (right group):
   `Rect::new(cursor, 0.0, w, bar_height)`. These bounds are **bar-local**,
   origin `(0, 0)` at the bar's own top-left — by design, not oversight:
   `native_surface_paint::paint` (same file) computes `bar_layout` this
   way and then manually offsets *only the paint calls*,
   `Rect::new(x + vs.bounds.x, y + vs.bounds.y, ...)`, before returning the
   untranslated `bar_layout` as the function's result. `compose/
   status_bar_interaction.rs`'s `StatusBarInteraction::hit_test` — the
   generic, already-correct, cross-backend click-dispatch path every
   backend (including GTK) uses for ordinary `MouseDown`/`MouseUp`
   handling of a `StatusBar` — confirms this is the intended contract: it
   explicitly subtracts `bar_rect.x`/`bar_rect.y` from the incoming
   `position` (`let local_x = position.x - bar_rect.x`) *before* matching
   against the stored `StatusBarLayout`, precisely because that layout's
   bounds are bar-local.
4. By contrast, `MenuBar::layout` and `CommandCenter::layout`
   (`quadraui/src/primitives/command_center.rs`'s `layout`, e.g.
   `let center_x = bounds.x + ...`) both take the *full* `Rect` and bake
   its `x`/`y` into every returned bound directly — their layouts are
   already absolute/window-space, which is exactly what
   `register_menu_bar_item_zones`/`register_command_center_zones` (and
   `WinBackend::nc_hit_test`'s `z.bounds.contains(point)` check, which
   compares against real `WM_NCHITTEST` screen-turned-DIP coordinates)
   correctly assume.

Net: `register_status_bar_segment_zones` is the one helper of the three
#1232 added that feeds a *bar-local* layout into an API that needs
absolute bounds. For vimcode's window-controls bar — anchored near the
window's right edge, e.g. `rect.x` on the order of 850px in a 1024px-wide
window per vimcode#1656's own measurements — the registered zone for each
button ends up roughly 850px to the left of the button's real screen
position (in fact overlapping whatever vimcode happens to paint at the
*left* edge of the band instead), so a real click at the button's real,
remeasured centre finds no matching zone, `nc_hit_test` falls through to
"covered only by `TITLE_BAR_DRAG_ZONE`" and answers `HTCAPTION`, and the
OS consumes the click as a caption drag before vimcode ever sees it. The
File/Edit/... menu items sit at a small `rect.x` (close to the window's
left edge, after the app-icon slot) if `MenuBar::layout` had the same bug
the resulting offset would be small enough to often still land inside a
item's own hit box by luck — but it doesn't have the bug at all, it's
already absolute. The command-centre search box is mid-window
(`cc_rect.x` on the order of 160–400px per the existing `win-gui.yaml`
fixture geometry) and, likewise, already absolute via `CommandCenter::
layout`, so it also just works. Nothing about vimcode's own call site
(`App::render_content`'s `backend.draw_status_bar_interactive(controls_rect,
&controls_bar, ...)`, identical in shape to every other backend's call to
the same trait method) is at fault — the bug is entirely inside
`quadraui::win::backend`'s zone-registration helper misreading a
documented-local-coordinate primitive as absolute.

**Ask:** `register_status_bar_segment_zones` must translate each
`bounds` by the same `rect.x`/`rect.y` that was passed into
`draw_status_bar_interactive` before calling `register_zone` — i.e.
`self.register_zone(id.clone(), Rect::new(rect.x + bounds.x, rect.y +
bounds.y, bounds.width, bounds.height))`, threading `rect` through from
the call site (it's already in scope at both of `draw_status_bar_
interactive`'s two call sites to this helper). Add a unit test
alongside `register_zone_records_the_zone`/
`draw_status_bar_interactive_registers_its_own_segment_zones_inside_the_band`
(`quadraui/src/win/backend.rs`'s existing test module) that calls
`draw_status_bar_interactive` with a `rect.x` strictly greater than 0 (the
existing test, read at the pinned rev, does not appear to cover a non-zero
`rect.x`/`rect.y` — the exact gap that let this regression ship alongside
the two working cases) and asserts the registered zone's `bounds.x`
reflects that offset, not bar-local `0`.

**Why both of #1232's own existing tests already use a non-zero `rect.x`
and still didn't catch this:** read closely, neither actually probes the
button's *real* absolute screen position — both derive their test point
from the same (buggy, bar-local) layout the production code also
mis-registers, so the bug is invisible to a self-referential check:
- `draw_status_bar_interactive_registers_its_own_segment_zones_inside_the_band`
  (`win/backend.rs`, `rect = Rect::new(600.0, 0.0, 200.0, 32.0)`) computes
  its probe point as `close_rect.x + close_rect.width / 2.0` straight from
  `layout.hit_regions` — never adding the `600.0` back in. If
  `register_status_bar_segment_zones` is fixed to translate by `rect.x`/
  `rect.y` as this entry's **Ask** requires, this *existing* test would
  then need its own probe point fixed to `rect.x + close_rect.x +
  close_rect.width / 2.0` or it would start failing for the opposite
  reason (querying the old, now-stale bar-local point instead of the
  corrected absolute one).
- `win::run`'s `live_window_nchittest_tests` (`status_rect =
  Rect::new(viewport.width - 40.0, 0.0, 40.0, band_height)`) does the same
  thing: `close_button` is set from `status_layout.hit_regions`' raw
  (bar-local) `r.x`/`r.y`, i.e. a point near the *left* edge of that
  40px-wide sub-rect rather than near `viewport.width`. In this fixture
  that mis-derived point likely lands inside the unrelated
  `probe:bar:file` menu-bar zone (painted at `Rect::new(0, 0, 100,
  band_height)`, and correctly absolute per `MenuBar::layout`) and so
  reports "not `HTCAPTION`" anyway — passing for the wrong reason, with no
  signal about the close button's real location at all.

Both need the same fix as the production code: derive the expected
absolute centre independently (`rect.x + bounds.x + bounds.width / 2.0`,
`rect.y + bounds.y + bounds.height / 2.0`), not by reusing whatever
`layout.hit_regions` already reports — otherwise a future regression of
this exact shape (a primitive silently switching between local- and
absolute-coordinate output) could ship past both tests again.

**Test:** `tests/smoke-spec/win-gui.yaml`'s existing
`hit-test-minimize-button-1232`/`hit-test-maximize-button-1232`/
`hit-test-close-button-1232`/`close-button-actually-closes-window-1232`
steps (added for vimcode#1646, already covering this exact scenario)
serve as the Tier-2 acceptance check — re-run them once the quadraui pin
moves past this fix; see that file's own updated header comment
(vimcode#1656) for the real-hardware RED confirmation this entry is based
on.

**UPDATE (vimcode#1675):** an independent bugbash lane re-reported the
identical symptom (real click on the close button at (978,16) leaves the
window open after 2000ms; `WM_NCHITTEST` at all three caption-button
positions still answers `HTCAPTION`) against the pin current at
investigation time (`ca7fcc83afad01ec3422f79366566f3a263b22bf`, confirmed
to be the exact `rev` in this repo's `Cargo.toml`). Re-confirmed by reading
that exact rev's source directly (not just re-citing this entry):
`quadraui/src/win/status_bar.rs::win_status_bar_layout` still calls
`bar.layout_padded(rect.width, rect.height, ...)` with no `rect.x`/`rect.y`
at all, and `quadraui/src/win/backend.rs::register_status_bar_segment_zones`
still registers that bar-local `layout.hit_regions` `bounds` verbatim — the
bug this entry describes is unchanged at the current pin. No new
vimcode-side fix is available for the same reason stated in **Blocks**
below; this update adds `JDonaghy/vimcode#1675`'s own Tier-1 acceptance
coverage closing the gap `src/win/mod.rs::win_driver_tests::
win_gui_smoke_spec_title_band_coordinates_are_stale_1657`'s own doc named
("the caption-button half of that test has no equivalent here, since
`ConformanceHarness` ... does not clone `App::title_bar_rect`") —
`caption_button_real_click_position_is_misclassified_htcaption_1675`
locates the close button's real painted position via `WinDriver::find`
(no `title_bar_rect` needed) and asserts the correct expected
`WinBackend::nc_hit_test` result (`Some(false)`, i.e. `HTCLIENT`) directly.
Source-level RED confirmation only — see that test's own doc for why it
cannot be *executed* even on real Windows hardware available to this
fix (`WinDriver::new`'s offscreen Direct2D surface creation panics
unconditionally off real Windows).

**Blocks:** `JDonaghy/vimcode#1656`, `JDonaghy/vimcode#1675`. Leave both
issues open behind this one per `GOALS.md`'s milestone-discipline rule —
there is no per-backend vimcode-side fix available (`src/app.rs`'s call to
`backend.draw_status_bar_interactive` is already identical in shape to
every other backend's call to the same trait method; the bug is entirely
inside `quadraui::win::backend`'s own zone-registration helper).

---

## Win-GUI's title-band blank strip (between the menu row and the window-control buttons) hit-tests as `HTCLIENT` instead of `HTCAPTION` — the window has no mouse-draggable title-bar region at all (blocks vimcode#1661)

**Title:** A real `WM_NCHITTEST` sweep across the Win-GUI title band, from
just past the last (`Help`) menu label to just before the caption-button
strip, reports `HTCLIENT` almost everywhere instead of `HTCAPTION` — the
unoccupied part of vimcode's drawn title-bar row (no menu label, no
Command Center widget, no caption button painted there) does not drag the
window, unlike an ordinary custom-titlebar app.

**Body:**

vimcode#1661 (a bugbash finding, same real-hardware session as #1656/#1657)
swept `WM_NCHITTEST` from x=350 to x=820 at y=16 — deliberately past the
last menu label and before the caption-button region — and found
`HTCLIENT` at every sampled point except one isolated x=540 result that
correctly read `HTCAPTION` (likely an inter-widget gap, not the wide
empty strip itself). Combined with the already-filed, still-open entry
above (the inline minimize/maximize/close buttons wrongly stay
`HTCAPTION`), the net effect is that the drawn title band has *no*
draggable region at all: the parts that should be `HTCLIENT` (the caption
buttons) are `HTCAPTION`, and the parts that should be `HTCAPTION` (this
blank strip) are `HTCLIENT` — i.e. the assignment looks inverted/misplaced
across the row, not simply incomplete.

Source-level investigation (pinned rev `ca7fcc8`, `quadraui/src/win/
backend.rs` + `quadraui/src/primitives/command_center.rs`) ruled out the
two most obvious hypotheses rather than confirming a third:

1. **Not a "`Bar`-zone never excluded" gap.** `WinBackend::
   register_command_center_zones` (the #1232 helper covering this exact
   strip) explicitly skips the Command Center's own container hit region:
   `CommandCenterHit::Bar | CommandCenterHit::Outside => continue`. So the
   Command Center's full reserved rect (`TitleBarBands::command_center` in
   vimcode's own `render.rs`, spanning the entire menu-end→controls-start
   strip) is never itself registered as an exclusion zone — a correctly
   *sized* Command Center would already leave its own left/right padding
   reporting `HTCAPTION` with no further quadraui change needed. This rules
   out a registration-logic bug in the helper #1232 added.
2. **Not the same "bar-local vs absolute" bug class as the entry above.**
   `CommandCenter::layout` (`quadraui/src/primitives/command_center.rs`)
   bakes `bounds.x`/`bounds.y` into every returned rect already
   (`center_x = bounds.x + (bounds.width - content_width).max(0.0) / 2.0`)
   — unlike the sibling `StatusBar::layout` bug the entry above describes.
   A translation fix of that shape would not help here.

**Leading (unverified) hypothesis:** the three registered Command Center
sub-zones (`Back`, `Forward`, `SearchBox`) collectively span almost the
entire reserved strip on real Win-GUI/DirectWrite hardware, leaving only
the fixed, small `CommandCenterMeasure::GAP_PX` gaps between them
unregistered — which is exactly the shape of the evidence (`HTCLIENT`
nearly everywhere, one narrow `HTCAPTION` island at x=540 the issue's own
reproduction guesses is "an inter-label gap"). If true, the root cause is
a *measurement* bug — `CommandCenterMeasure::from_char_width`'s
`char_width`/`height` inputs (`WinBackend::current_char_width`/
`current_line_height`) resolving to a value scaled wrong for Win-GUI
specifically (e.g. physical pixels vs. DIPs, or a DPI factor applied
twice) — not a hit-test registration bug. This could not be confirmed or
ruled out from this fix's Linux worktree: `CommandCenterMeasure`'s real
width depends on live `DWrite` text measurement
(`super::command_center::draw_command_center`), which has no non-Windows
implementation to run and compare against the Pango-based estimate the
#1657 Tier-1 test already cross-checks for the menu row.

**Ask:** On real Windows hardware, instrument (or single-step) a live
`vimcode.exe` to log the real `CommandCenterLayout` (`bounds`,
`back_bounds`, `forward_bounds`, `search_bounds`) alongside the actual
painted title band, and compare `search_bounds.width` against what
`CommandCenterMeasure::from_char_width`'s Pango/GTK equivalent would
produce for the same label at the same `current_char_width` — if the
real Win-GUI value is far larger, the fix belongs in whatever converts
font-metrics into `current_char_width`/`current_line_height` for
`WinBackend` (DPI-scale handling, most likely), not in `nc_hit_test` or
either zone-registration helper, both confirmed correct by source review
above. Once the true cause is found, add a `win::backend` unit test
(mirroring `draw_command_center_registers_its_own_zones_inside_the_band`)
that constructs a Command Center whose container is deliberately wider
than its content and asserts a point in the resulting *left* padding
reads `Some(true)` — the existing test only probes a point to the left of
the Command Center's container entirely, so it cannot catch a content
block that over-fills its own container.

**Test:** `tests/smoke-spec/win-gui.yaml`'s new `hit-test-blank-strip-*-
1661` steps (added alongside this entry) serve as the Tier-2 acceptance
check; `src/win/mod.rs::win_driver_tests::
win_gui_blank_title_band_strip_is_caption_1661` is the Tier-1 companion
(type-checked only on this worktree, same as every other test in that
module — see its own doc for why it cannot yet be *executed* here or on
dell64). Re-run both once a quadraui fix lands.

**Blocks:** `JDonaghy/vimcode#1661`. Leave that issue open behind this one
per `GOALS.md`'s milestone-discipline rule — there is no per-backend
vimcode-side fix available; both zone-registration helpers vimcode's call
sites reach are confirmed correct by source review above, so the gap (if
the leading hypothesis holds) is in Win-GUI's own text-measurement
pipeline, not in anything `src/win/`, `src/app.rs`, or `render.rs` control.

---

## Seven Win-GUI rasterisers still paint a hardcoded `Theme::default()` instead of the live `self.current_theme` — a runtime `:colorscheme` change only repaints the minimap (blocks vimcode#1667)

**Title:** `:colorscheme vscode-light` on Win-GUI repaints the minimap alone;
the menu bar/tab bar/editor/status bar/Explorer sidebar stay on the
previous theme — reported in vimcode#1667 with a before/after screenshot
pair (minimap flips, everything else doesn't) and confirmed reversible
(switching back to `vscode-dark` restores the minimap, ruling out a one-off
capture glitch).

Root-caused by reading every Win-GUI rasteriser's real source at the
pinned rev (`ca7fcc83afad01ec3422f79366566f3a263b22bf`) and diffing against
`GtkBackend`'s/`MacBackend`'s equivalents at the same rev — not reproduced
on real hardware (no Windows host in this session; see `src/win/mod.rs`'s
top-of-file `#1558` doc section for why this repo's Win-GUI tests can only
be read, not executed, from here or from dell64 today).

`Backend::set_theme` (quadraui's trait method) is wired correctly:
`App::sync_per_frame_backend_state` (backend-neutral, `src/app.rs`,
vimcode) calls it every frame with vimcode's own resolved theme, and
`WinBackend::set_theme`/`theme()` (`win/backend.rs`) store/return it
faithfully via `self.current_theme`. Nine Win-GUI rasterisers already read
that live field correctly, matching `GtkBackend`/`MacBackend`:
`draw_minimap`, `draw_menu_bar`, `draw_activity_bar`,
`draw_status_bar_interactive`, `draw_completions`, `draw_find_replace`,
`draw_scrollbar`, `draw_drop_overlay`, `draw_context_menu` (several of
these were quadraui#789's own fix, per that issue's commit message naming
exactly this set). That's why the minimap alone visibly reacted to
`:colorscheme` in the bug report's screenshot.

Seven more never got that fix and still construct a fresh
`Theme::default()` — several with an explicit comment admitting the gap
outright:

- `win::editor::draw_editor` (`editor.rs:68`) — editor background/
  foreground/cursorline/selection/diagnostics/cursor colours. No theme
  parameter reaches this function at all (`WinBackend::draw_editor` calls
  it with only `cell_width`/`line_height`). Module doc: "colours come from
  `Theme::default()` rather than a live `WinBackend` theme field."
- `win::tab_bar::paint_tab_bar_icons_from_layout` (`tab_bar.rs:285`) — the
  whole tab-row background fill (`theme.tab_bar_bg`) plus every tab's
  active/inactive fill and label colour. Reached from
  `WinBackend::draw_tab_bar_icons` with no theme argument at all (same gap
  shape as `draw_editor`).
- `win::tree::draw_tree` (`tree.rs:103`) — the Explorer sidebar's own
  content rasteriser; its documented "Visual contract" names
  `Theme::tab_bar_bg` for the background fill and `header_bg`/
  `selected_bg`/`inactive_selected_bg`/`muted_fg`/`error_fg`/`warning_fg`
  for rows. `WinBackend::draw_tree` calls it with no theme argument.
- `WinBackend::draw_panel` (`backend.rs:4042`) — explicit comment:
  "`Theme::default()`, not `self.current_theme` — preserves the pre-#859
  `win::panel::draw_panel` behaviour exactly ... `WinBackend` has no live
  theme wired through to panel chrome yet." Paints the bottom/Terminal
  panel's title-bar background (`theme.separator`) and body chrome.
- `WinBackend::draw_sidebar_panel_interactive` (`backend.rs:4381`) —
  explicit comment: "preserves the pre-#862 ... behaviour exactly (it
  delegated to `win::toolbar::draw_toolbar`, which has never taken a live
  theme ...)." Paints a sidebar panel's own toolbar header
  (`bar.bg.unwrap_or(theme.header_bg)` when the panel sets no explicit
  `bar.bg`).
- `WinBackend::draw_split` (`backend.rs:3829`) — explicit comment,
  identical shape: "`WinBackend` has no live theme wired through to split
  chrome yet." Paints window-split divider chrome.
- `WinBackend::draw_split_tree` (`backend.rs:3870`) — same explicit comment
  shape, for the split-tree container chrome.

**Ask:** wire `self.current_theme` through all seven, mirroring whatever
quadraui#789 (or its sibling fixes) did for the nine already-correct
rasterisers above — `draw_editor`/`draw_tree`/`paint_tab_bar_icons_from_
layout` need a `theme: &Theme` parameter threaded from their `WinBackend`
call site (`self.current_theme`), the same shape `draw_menu_bar`/
`draw_activity_bar` already take; `draw_panel`/`draw_sidebar_panel_
interactive`/`draw_split`/`draw_split_tree` just need their local
`let theme = crate::theme::Theme::default();` line (and its "has no live
theme wired through yet" comment) replaced with `let theme =
self.current_theme;`, the same one-line change `draw_find_replace`/
`draw_scrollbar`/`draw_drop_overlay` already show as the fixed shape right
next to them in the same file.

**Test:** `src/win/mod.rs::win_driver_tests::
colorscheme_change_repaints_editor_and_explorer_sidebar_1667` (added
alongside this entry) is the Tier-1 acceptance check — pixel-probes
`draw_editor`'s background and `draw_tree`'s Explorer sidebar background
against `vscode-light`'s resolved theme (`#ffffff`/`#ececec`, both far
from quadraui's own dark `Theme::default()`, `rgb(20, 22, 30)`, so either
backend reading the wrong theme is pixel-exact, no tolerance needed). Only
type-checked here (`#[cfg_attr(target_os = "windows", test)]`, same as
every other test in that module) — see the module-top `#1558` doc section
for why it cannot yet be *executed*, on this host or on dell64; this is a
source-level RED confirmation (every line cited above was read directly at
the pinned rev), not an executed one. `tests/smoke-spec/win-gui.yaml` gets
no new step for this — its own "DELIBERATELY OMITTED" section already
rules out a pixel-content check with today's `win_native_driver.py` step
vocabulary (no `expect_capture_nonblank`-shaped primitive exists yet); that
gap is coordinator-repo work, not something this vimcode PR can add. Covers
only two of the seven (the pair the bug report's own screenshot calls out
most directly, "editor background/text" and "Explorer sidebar") — the
other five (tab bar, Terminal panel, sidebar-panel toolbar, split/
split-tree dividers) have no equivalent probe yet; a follow-up can extend
this test once the fix lands and real-hardware geometry for those can be
confirmed, mirroring how `#1657`/`#1661` grew this same file incrementally.

**Blocks:** `JDonaghy/vimcode#1667`. Leave that issue open behind this one
per `GOALS.md`'s milestone-discipline rule — there is no per-backend
vimcode-side fix available; `WinBackend` is a 1-line quadraui re-export
(`src/win/backend.rs`) and every file named above lives in quadraui, not
in this repo.

---

## `WinDriver` has no `.tick()` (unlike `TuiDriver`/`MacDriver`), and its own `attach_headless` never sets `WinBackend::hwnd` — `tick`/`request_frame_in` scheduling bugs on Win-GUI can't be driver-tested even on real Windows (found fixing vimcode#1668)

**Title:** vimcode#1668 (Win-GUI's embedded terminal panel paints completely
blank — no shell prompt, no echoed input, no output, ever, even after
settling and typing) root-caused to a genuine, fixable-in-vimcode
scheduling gap, not a quadraui rasteriser bug: `quadraui::runner::
ShellApp::tick`'s own doc table says Windows has **no** unconditional
idle-poll fallback (unlike TUI/GTK/macOS's 250ms `IDLE_POLL_CEILING`) —
`tick` only runs again once something calls `Backend::request_frame_in`.
Confirmed directly against the pinned rev: `quadraui::win::run::wndproc`
calls `AppLogic::tick` from exactly one place, its own `WM_TIMER` handler
(`grep -n "tick(ws" quadraui/src/win/run.rs`), and that timer is armed
only by `WinBackend::request_frame_in`'s `SetTimer` call — dispatching a
keypress (`App::handle`/`dispatch_event`) never reaches `tick` at all.
vimcode's own `App::tick_dispatch` (`src/app.rs`) only ever re-armed
`request_frame_in` for one case (an in-flight ACP/AI turn); opening a
terminal panel armed nothing, so `Engine::poll_terminal` (which drains the
PTY session's output) never ran again past the very first frame on
Win-GUI — exactly the reported "blank immediately, blank after 2.5s,
still blank after typing `echo hello-vimcode` + Enter and another 2.5s"
symptom. **Fixed in this repo**, in the one shared, backend-neutral
`App::tick_dispatch` (not a per-backend file): a new
`terminal_poll_rearm_delay` pure function now makes `tick_dispatch`
re-arm `request_frame_in` at 100ms cadence whenever `Engine::
terminal_panes` is non-empty, mirroring the existing `ai_streaming`
re-arm immediately above it. Harmless on GTK/TUI/macOS (they already
tick regardless, via their own `IDLE_POLL_CEILING` fallback) — this is
pure, additive, Win-GUI-targeted scheduling, not a feature decision in a
backend directory.

**The testing gap this entry is actually about:** proving the fix via a
driver-level, rendered-output test (this repo's own black-box coverage
bar) is not achievable against `WinDriver` as it exists today, for two
independent reasons:

1. `quadraui::tui::testing::TuiDriver` and `quadraui::macos::testing::
   MacDriver` both expose a `pub fn tick(&mut self) -> Reaction` a test
   can call directly to deterministically advance the app's idle-poll
   logic. `quadraui::win::testing::WinDriver` has no such method at all
   (`grep -n "pub fn tick" quadraui/src/{tui,macos,win}/testing.rs` finds
   it in the first two, not the third) — there is no way to drive
   `AppLogic::tick` through `WinDriver` short of its own `.render()`/
   `.dispatch()`, neither of which calls it.
2. Even if `WinDriver` grew a `.tick()`, `WinBackend::request_frame_in`
   is a documented no-op whenever `self.hwnd` is `None` ("a
   `request_frame_in` call with no window is simply lost") — and
   `WinDriver::new`'s `WinBackend::attach_headless` call never sets
   `self.hwnd` (only `GtkDriver`'s/`MacDriver`'s backend-attach
   equivalents wire a real/fake window handle their scheduling can act
   on). So a `WinDriver`-based scenario could call `.tick()` and still
   never observe whether a real Win32 `WM_TIMER`/`SetTimer` round-trip
   would have re-fired `tick` on a live message loop — the one thing
   this bug class is actually about.

Same root shape as this file's two already-filed, now-struck "TUI test
drivers can't observe X" entries (quadraui#1060, quadraui#1063) — a
downstream crate (`vimcode`) found a real scheduling/timing bug whose fix
it could ship, but whose *regression coverage* needs a test-harness
primitive quadraui doesn't expose yet.

**Ask:**
1. Give `WinDriver` a `pub fn tick(&mut self) -> Reaction` mirroring
   `TuiDriver`'s/`MacDriver`'s (calls `AppLogic::tick`, applies the
   returned `Reaction` the same way `.dispatch()` already does via
   `apply_outcome`).
2. Either have `WinDriver::new`/`attach_headless` set a real or
   synthetic non-`None` `self.hwnd` so `request_frame_in` isn't a silent
   no-op under test, or give `WinBackend` its own `frame_requests()`/
   `pending_frame_delay()` pair mirroring `TuiBackend`'s (quadraui#832,
   `quadraui/src/tui/backend.rs`) — a test-only call-count/deadline
   accessor that records what a `request_frame_in` call asked for without
   needing a live HWND at all. `TuiBackend` is the one existing precedent
   for this exact shape; `MacBackend` doesn't have it either (confirmed:
   no `frame_requests`/`pending_frame_delay` anywhere under
   `quadraui/src/macos/`), so this would be the second backend to grow it,
   not a novel pattern.

**Test:** this review iteration added real driver-tier coverage for the
*shared* half of this fix — `src/tui_main/app_on_tui_tests.rs`'s
`terminal_poll_rearm_1668` module drives the real, production
`App::tick_dispatch` through `quadraui::tui::testing::TuiDriver::tick()`
and asserts on `TuiBackend::frame_requests`/`pending_frame_delay`
(quadraui#832's existing `Backend`-call-count instrumentation — it turns
out `quadraui::Backend` being sealed does *not* rule this out in general,
only for `quadraui::testing::RecordingBackend` specifically, whose
`request_frame_in` is a no-op; the real `TuiBackend` a `TuiDriver` wraps
already records every call). RED-verified against
`terminal_poll_rearm_delay` always returning `None`. `src/app.rs::
terminal_poll_rearm_tests::rearm_is_requested_only_while_a_terminal_pane_is_open`
remains alongside it as the isolated pure-function unit test. Together
these two fully cover the platform-neutral decision `tick_dispatch`
makes. What neither can reach — and what this entry is actually about —
is `WinBackend` itself: whether a real Win32 `SetTimer`/`WM_TIMER`
round-trip actually re-fires `tick` on a live Windows message loop. That
is the one link this fix's testing cannot close without the `WinDriver`
work asked for above.

**Blocks:** nothing in `JDonaghy/vimcode` directly — `JDonaghy/vimcode#1668`
is fixed and covered by the pure-function unit test plus the `TuiDriver`-
based driver test above regardless of whether this lands. File this as a
quadraui testing-infrastructure improvement, not a `JDonaghy/vimcode`
blocker.

---

## Win-GUI never dispatches Ctrl-modified keyboard shortcuts (Ctrl+`, Ctrl+B) to the engine — root cause isolated to the native WM_KEYDOWN/WM_CHAR translation layer, open real-hardware question (blocks vimcode#1674)

**Title:** A real-hardware bugbash (window focus independently confirmed —
`GetForegroundWindow() == target hwnd` — immediately before each
injection, reproduced via both `System.Windows.Forms.SendKeys` and raw
`user32.dll` `keybd_event(VK_CONTROL + VK_OEM_3)`) found that neither
Ctrl+` (open/toggle terminal) nor Ctrl+B (toggle sidebar) has any visible
effect on Win-GUI, while unmodified keys (typing, Enter, Escape) and mouse
clicks work fine in the same session — isolating the break to the Ctrl
modifier specifically. One side effect: an "open terminal via menu, then
type a command" sequence fell through to the editor's own Normal-mode vim
parser and corrupted the open buffer, because no terminal ever opened to
receive the keystrokes.

**Body:**

vimcode#1674 is a bugbash finding; investigated from this fix's Linux
worktree (no attached Windows host) by reading every candidate dispatch
site at the pinned rev
(`ca7fcc83afad01ec3422f79366566f3a263b22bf`) and writing executing tests
against quadraui's own public API where possible.

Every shared, cross-backend site that "Ctrl+<key> reaches the engine"
could plausibly break at reads correct, and is confirmed correct by
source + test, not assumption:

- `App::setup` (`src/app.rs`, vimcode) registers the same 15-entry
  panel-accelerator table (`render::register_panel_accelerators`) on
  every GUI backend identically — not a Win-GUI-only call site.
- Accelerator matching itself
  (`quadraui::backend_core::BackendCore::match_keypress`) is shared code:
  `WinBackend::match_keypress`/`GtkBackend::match_keypress`
  (`quadraui/src/{win,gtk}/backend.rs`) both delegate to it verbatim. A
  bug here would also break GTK, which the issue confirms works.
- `quadraui::win::events::wm_char_to_uievent` (the pure `WM_CHAR` →
  `UiEvent` translator `win::run`'s live `wndproc` calls) correctly
  recovers **both** reported chords when fed the payload Windows is
  documented to deliver for each — confirmed by two tests added
  alongside this entry
  (`src/win/mod.rs::win_ctrl_key_translation_tests_1674`,
  `ctrl_b_wm_char_recovers_the_base_letter_1674`/
  `ctrl_backtick_wm_char_passes_through_with_ctrl_held_1674`) that
  genuinely **execute** on an ordinary Linux host via `cargo test
  --features win` (no Windows target, no cross toolchain — these are pure
  functions):
  - Ctrl+B: Windows' keyboard driver converts Ctrl+letter to its C0
    control code (`0x02` for B) via `TranslateMessage`;
    `wm_char_to_uievent('\x02', {ctrl:true}, _)` correctly recovers
    `Key::Char('b')` with `ctrl == true`.
  - Ctrl+`: backtick is not a control character, so the function never
    even reaches its Ctrl-recovery branch — it passes the literal
    backtick straight through with `ctrl` still set on `modifiers`,
    which is already exactly the event
    `Engine::handle_vscode_key`'s `"grave" | "\`"` arm
    (`src/core/engine/vscode.rs`) needs. This corrects an earlier,
    narrower theory from the same investigation session (that
    `events::vk_to_named_key` having no `VK_OEM_3`/backtick entry meant
    Ctrl+` had no delivery path at all) — `vk_to_named_key` genuinely has
    no backtick entry, but that's immaterial once `wm_char_to_uievent` is
    confirmed to handle the `WM_CHAR` path correctly on its own.

**What remains genuinely unverified — and why no test anywhere in
`vimcode` or `quadraui`'s own test suite can close it:** whether
Windows' real `TranslateMessage`, running inside `win::run`'s live
message loop, actually *generates* a `WM_CHAR` message at all for
Ctrl+backtick (and, separately, whether `GetKeyState(VK_CONTROL)` reads
`true` at the moment `win_key_modifiers()` samples it for Ctrl+B,
specifically for the *injected* input methods this bugbash used — both
`SendKeys` and `keybd_event` post synthetic input through the same system
input queue real hardware uses, but neither test driver here constructs
one). Win32 keyboard-input references consistently describe `WM_CHAR`
generation for Ctrl held with a non-letter key as layout/driver-dependent,
unlike the uniformly documented Ctrl+letter C0 conversion — but
confirming or ruling that out needs a live Win32 message loop.
`quadraui::win::testing::WinDriver` (the only Win-GUI test harness this
repo or quadraui has) cannot reach it: `WinDriver::ctrl_char` constructs
an already-decoded `UiEvent::KeyPressed` directly, bypassing
`events.rs`/`run.rs`'s `WM_KEYDOWN`/`WM_CHAR` translation entirely by
design — proven by
`ctrl_accelerator_dispatch_reaches_engine_via_win_driver_1674` (added
alongside this entry, same file), which re-runs an accelerator-bound
Ctrl-chord through that exact synthetic path and — as expected — finds
the shared pipeline downstream of translation sound. This is the same
"last link only real hardware can close" shape the already-filed #1668
entry above describes for `WinDriver`'s missing `.tick()`: a downstream
crate found everything source review can check correct, but the
test-harness primitive needed to observe the one remaining, genuinely
OS-level fact doesn't exist in either repo yet.

**Ask:** this needs real-Windows-hardware diagnostic logging, not a code
change sight-unseen — instrument (or single-step) a live `vimcode.exe`
built with `cargo xwin build --features win`, temporarily logging every
`WM_KEYDOWN`/`WM_CHAR`/`WM_SYSKEYDOWN` the real `wndproc`
(`quadraui/src/win/run.rs`) receives (message id, `wparam`, and
`GetKeyState(VK_CONTROL)`'s live read) to a file, then reproducing
exactly the bugbash's Ctrl+`/Ctrl+B injection. Two outcomes determine the
actual fix:

1. If `WM_CHAR` never arrives for Ctrl+backtick (or `WM_KEYDOWN`'s
   `GetKeyState(VK_CONTROL)` read is `false` when it should be `true` for
   Ctrl+B): the fix belongs in `win::run`'s `WM_KEYDOWN` handler — add a
   keyboard-layout-aware fallback (e.g. `ToUnicode`/`MapVirtualKeyW` with
   the Ctrl bit cleared from the keyboard-state buffer passed to
   `ToUnicode`) that synthesizes a `Key::Char` event directly from
   `WM_KEYDOWN` whenever Ctrl is held and `WM_CHAR` does not reliably
   follow, rather than relying solely on `TranslateMessage`'s narrow,
   letter-only C0-control-code behaviour.
2. If the log shows the expected messages arriving with the expected
   modifier state: the bug is not in translation at all, and the next
   place to look is whether `win::run`'s live message loop is somehow
   losing or misrouting the resulting `UiEvent::KeyPressed` before it
   reaches `dispatch_event` — a possibility source review alone cannot
   rule out without that same real log.

**Test:** `src/win/mod.rs`'s new `win_ctrl_key_translation_tests_1674`
module (`ctrl_b_wm_char_recovers_the_base_letter_1674`,
`ctrl_backtick_wm_char_passes_through_with_ctrl_held_1674`) are genuinely
**executing** Tier-1 coverage, confirmed GREEN today — they prove the
translation *function* is not the fault, which is why this entry does not
claim a reproducible code-level defect. `ctrl_accelerator_dispatch_
reaches_engine_via_win_driver_1674` (`win_driver_tests`, same
type-check-only posture every other test in that module has per #1558 —
`WinDriver`'s `HeadlessSurface` always fails to construct off Windows) is
the isolating half for the shared accelerator-dispatch pipeline, using
Ctrl+P (a synchronous panel accelerator) rather than the issue's own
Ctrl+B/Ctrl+` because those two specifically queue a `DeferredAction`
only `tick()` drains, and `WinDriver` has no `.tick()` at all (same gap
the already-filed #1668 entry above describes) — so neither reported
chord's *end effect* is observable through any harness either repo ships
today, only its upstream dispatch. `tests/smoke-spec/win-gui.yaml` gets no
new step for the same reason #1668's own entry gives: no AT-provider
wiring exists for vimcode's drawn sidebar/terminal-panel content, so no
`expect_a11y`/`expect_hit`/`expect_menu` step could ever observe the
panel actually toggling — a step that cannot fail is not coverage.

**Blocks:** `JDonaghy/vimcode#1674`. Leave that issue open behind this one
per `GOALS.md`'s milestone-discipline rule — there is no per-backend
vimcode-side fix available (every shared site this investigation could
check is confirmed correct), and the remaining question is real-Windows
diagnostic work this repo's Linux worktree cannot perform, not a known
defect with a known fix.

---

## `WinBackend::draw_minimap`'s "surface not attached yet" fallback has no theme-background guard — the minimap strip can show raw OS white instead of the active theme (blocks vimcode#1676), and `WinDriver`/`WinBackend` expose no way to drive that state from a downstream crate's test

**Title:** vimcode#1676 reports the editor minimap's ~96px-wide strip
painting as a flat, almost-blank white rectangle regardless of the active
theme (reproduced on both `vscode-dark` and `vscode-light`) — present
immediately on launch in most runs, and reproducible after a window resize
in every run. Not reproduced on real hardware this session (no live,
unlocked Windows host reachable from here — same posture every other entry
in this file states; dell64's own blockers are documented at this file's
`WinBackend::install_menu_bar_now` and title-band entries above).

**Root-caused by reading the real source at the pinned rev
(`ca7fcc83afad01ec3422f79366566f3a263b22bf`), ruling out the obvious
candidates first:**

- **Not a theme-wiring bug like the entry above (`blocks vimcode#1667`).**
  `win::minimap::draw_minimap_scaled` already fills `rect` with
  `theme.background` as its very first paint call, and `WinBackend::
  draw_minimap` already passes `&self.current_theme` — confirmed one of
  the nine Win-GUI rasterisers that entry names as *already* correctly
  wired, not one of the seven still hardcoding `Theme::default()`. So
  whenever this rasteriser actually runs, it paints the right colour;
  `Theme::default()`'s own background (`rgb(20, 22, 30)`, dark) isn't white
  either way, ruling out a stray default-theme fallback as the source of
  the reported white.
- **Not a vimcode-side geometry/unit bug.** The minimap's sizing/rect
  policy (`UnitProfile::px`, `render::gtk_minimap_sizing`,
  `render::minimap_strip_rect`, `render::draw_minimap_strip`) is one
  shared, backend-neutral code path GTK/macOS/Win-GUI all paint through —
  its own doc comments only ever distinguish "pixels" (GTK/macOS/Win) vs
  "cells" (TUI), never a Win-GUI-specific unit. Nothing in `src/render.rs`
  treats Win-GUI differently from GTK/macOS here.
- **A real, if narrow, gap found instead:** `Backend::draw_minimap`
  returns [`MinimapPaintResult`], whose `painted: bool` field is `false`
  exactly when `WinBackend` has no live Direct2D surface attached yet —
  confirmed as a real, reachable, already-tested state in quadraui's own
  `win::backend` test suite
  (`draw_minimap_with_no_surface_reports_unpainted_and_agrees_with_
  minimap_layout`), not a hypothetical. `WinBackend` can genuinely be in
  that state at paint time for two confirmed-in-source reasons: (1) Windows
  fires an initial `WM_SIZE` synchronously from inside `CreateWindowExW`,
  before `win::run`'s own code has a `HWND` to call `attach_surface` with
  (`resize_surface`'s own doc comment states this explicitly), and (2) an
  `EndDraw` failure (device loss / RDP session change) drops `self.surface`
  back to `None` in `end_frame`, recovered lazily by the next `WM_PAINT`'s
  `ensure_surface()` call, not immediately. `WinBackend::begin_frame`'s own
  `Clear()` call — the thing that paints the *entire* render target to the
  live theme's background every frame — is gated on that exact same
  `self.surface.is_some()` check, so a frame landing inside either window
  paints **nothing** anywhere, including the minimap strip, and the
  on-screen result for whatever the OS/DWM shows for an as-yet-uncomposited
  client-area pixel is its own default backing colour — white, not any
  theme's background. `src/render.rs::draw_minimap_strip` (vimcode's own
  shared, backend-neutral call site) never inspects
  `MinimapPaintResult::painted` at all — `let layout =
  backend.draw_minimap(rect, &minimap).layout;` discards it unconditionally
  — so nothing anywhere in this codebase or quadraui's own `win::run`
  retries or papers over an unpainted frame once it's shown. GTK/TUI/macOS
  backends never produce `painted: false` in normal operation (their
  rasterisers always have a live surface by the time any `draw_*` call
  runs), which is why this gap has stayed latent until Win-GUI exposed it.

**Why this matches the report's own timing clues (not proven on real
hardware, but consistent):** "present immediately on launch in most runs"
matches the synchronous first-`WM_SIZE`-before-`HWND` window above; "every
run after a resize" matches an `EndDraw` failure during the resize's
render-target `Resize()` call; the session-restore correlation ("one early
launch in a brand-new workspace... did not show it") is consistent with
more synchronous startup work (restoring persisted panes/splits/cursor
state) before the first paint giving this race more time to land inside
one of those two windows, without proving it.

**Ask:** make `WinBackend` never show a client-area pixel the current
theme hasn't painted, by either (a) not presenting/showing the window until
the first `EndDraw` succeeds (the standard mitigation for this exact
Win32/Direct2D "first-frame white flash" class of bug — `win::run` already
tracks the window's `HWND` and visibility separately from `WinBackend`'s
surface state, so this is `win::run`/`win::backend` work, not a 1-3 line
`src/win/mod.rs` wiring fix — that file is a 1-line quadraui re-export, per
its own module doc), or (b) give `Backend::draw_minimap` (and ideally every
other `draw_*` method with the same "surface not attached" fallback shape)
a theme-background-coloured fallback paint even when `painted` comes back
`false`, so a transiently-unattached surface never shows raw OS white for
*any* widget, minimap included.

**The testing gap this entry is actually about — same shape as this file's
`WinDriver` has no `.tick()` entry above:** proving either fix via a real,
driver-tier, rendered-output test is not achievable from a downstream
crate today. `WinBackend::surface`/`ensure_surface`/`resize_surface` are
all `pub(crate)` to quadraui itself; nothing in `quadraui::win::testing`
(`WinDriver::new`, `.attach_headless`, `HeadlessSurface`) exposes a way to
force a `WinBackend` under test back into the "no surface attached" state
`draw_minimap_with_no_surface_reports_unpainted_and_agrees_with_
minimap_layout` already proves exists — that test lives inside quadraui
and can reach the state directly (same crate, same module); a `vimcode`
test cannot construct it at all, on any OS, because the relevant
constructor/fields are private across the crate boundary.

**Test:** no RED-verifiable Tier-1 scenario could be added for the actual
reported defect, for the reason above — not "could not be executed off
Windows" (this file's usual disclaimer), but "cannot be *constructed* from
this crate regardless of host OS." `src/win/mod.rs::win_driver_tests::
minimap_strip_background_matches_theme_once_a_surface_is_attached_1676`
(added alongside this entry) instead locks down the one piece of this
contract a downstream crate *can* drive today — the happy path, through
the normal `conformance_harness` (which always attaches a surface eagerly
via `attach_headless`) — as a regression guard for whichever fix lands
upstream, and documents in its own comment exactly why it cannot be the
regression test for the reported bug. `tests/smoke-spec/win-gui.yaml` gets
a cross-reference into its existing "DELIBERATELY OMITTED" minimap-content
comment (additive-only, no steps changed) rather than a new step — the
same `expect_capture_nonblank`-shaped primitive gap that comment already
names for "the minimap shows content" applies equally to "the minimap
shows the right *background*", and is coordinator-repo work, not something
this vimcode PR can add.

**Ask (testing infrastructure, additional to the production ask above):**
give `quadraui::win::testing` a way to simulate the unattached/dropped-
surface state from a downstream crate — e.g. a `WinDriver`/`WinBackend`
test-only `drop_surface()` or `simulate_end_draw_failure()` — mirroring
this file's `WinDriver` has no `.tick()` entry's own ask shape (a
test-harness primitive gap, not a production-code gap).

**Blocks:** `JDonaghy/vimcode#1676`. Leave that issue open behind this one
per `GOALS.md`'s milestone-discipline rule — there is no per-backend
vimcode-side fix available; `WinBackend` is a 1-line quadraui re-export
(`src/win/backend.rs`), and every file named above lives in quadraui, not
in this repo.

---

## `AppShell::build_activity_bar` hardcodes `active_accent: None`/`selection_bg: None` with no `Theme` in scope — the activity bar paints no active-view accent line on any backend, already tracked as quadraui#381 by the hardcoding comment itself (blocks vimcode#1689)

**Title:** vimcode#1689 reports the Win-GUI activity bar painting no 2px
left-edge accent strip beside the active (Explorer) icon, despite
vimcode#1547 ("Activity bar: paint an active-view accent line") having
closed with `render::build_activity_bar` setting
`active_accent: Some(theme.activity_active_accent...)` and
`win::activity_bar`'s own rasteriser documenting that it paints the strip
"only when that field is `Some`". A column scan of the real on-screen
activity bar's left edge over the Explorer row band, from the issue's own
side-by-side capture, returns a single uniform colour — the bar's
background, `#262633` — with zero accent pixels.

**Root-caused by reading the real source at the pinned rev
(`ca7fcc83afad01ec3422f79366566f3a263b22bf`), ruling out both halves the
issue itself already names as "looking wired":**

- **Not `render::build_activity_bar` (the function #1547 fixed).** It does
  set `active_accent: Some(..)` from `theme.activity_active_accent`
  exactly as intended (`src/render.rs:19440`, pinned down by that fix's own
  regression test, `build_activity_bar_active_accent_uses_activity_active_
  accent_not_cursor`) — **but that function has zero production callers.**
  `grep -rn 'build_activity_bar(' src/` (the free function, not the
  `AppShell` method of the same name) returns only its own definition, its
  own test, and doc-comment cross-references — no call site in
  `src/app.rs` or anywhere else in this crate ever invokes it. #1547's own
  test file already says this outright: "as of #1434, `render::
  build_activity_bar` itself has no production caller — `App` renders
  through `quadraui::compose::app_shell::AppShell::build_activity_bar`
  instead ... this test pins the adapter's own field-mapping correctness
  so it's ready the moment such a hook lands ... it does not claim the
  accent line paints in the shipped app today." #1547 closed on a function
  nothing calls; this issue is that promise coming due.
- **Not `win::activity_bar::draw_activity_bar`/the shared
  `native_surface_paint::paint`.** Read directly
  (`primitives/activity_bar.rs:656-659`): `if item.is_active { if let
  Some(accent) = bar.active_accent { surface.surface_fill_rect(Rect::new(
  0.0, y, 2.0, row_h), accent); } }` — correct, and already covered by that
  module's own passing `paint_and_hit_test_round_trip` test (a hand-built
  `ActivityBar { active_accent: Some(..), .. }` fixture paints the strip
  fine). Every one of the three backends' rasterisers (`win`/`gtk`/`macos`)
  shares this one `native_surface_paint::paint` function, so none of the
  three has a backend-specific accent bug.
- **The actual break: `AppShell::build_activity_bar`
  (`compose/app_shell.rs:874-934`), which every real `App` on every
  backend (GTK/macOS/TUI/Win-GUI) renders its activity bar through, not
  `render::build_activity_bar`.** Its own doc comment admits the gap
  outright, naming the issue number this entry now files: `active_accent:
  None, // #658: AppShell has no Theme in scope here to source a colour
  from (that wiring is #381's job), so the accent line is left unset
  rather than hardcoded.` `selection_bg: None` sits right next to it, same
  reason. So **every** real `App` render — not just Win-GUI — paints zero
  accent pixels regardless of which panel is active; Win-GUI is just the
  backend vimcode#1689 happened to observe and screenshot. Confirmed this
  is not already superseded: `grep -rn 'active_accent' quadraui/src/
  compose/` at the pinned rev shows `tab_group.rs:484` and
  `bottom_panel.rs:413` carry the identical `None` hardcode for their own
  analogous fields, so this is a systemic "`AppShell`/compose layer has no
  `Theme` to source chrome colours from" gap, not unique to the activity
  bar — but this entry scopes its ask to the activity bar, the one
  vimcode#1689 reports.

**Why vimcode can't fix this itself (Platform-Neutrality Rule):**
`AppShell` and its `build_activity_bar` method are entirely inside
quadraui (`quadraui::compose::app_shell`); `src/win/backend.rs` and
`src/win/mod.rs` are 1-line re-exports (per this file's own established
precedent for Win-GUI entries), and the two GTK/macOS/TUI runners call the
exact same `AppShell::build_activity_bar` through quadraui's own
`shell_adapter.rs` — there is no per-backend call site in this repo for
any fix to attach to. `render::build_activity_bar` (#1547's own, orphaned
function) cannot be wired back in without quadraui first giving `AppShell`
either a `Theme` parameter or a way for the caller to override/post-process
the `ActivityBar` it returns before it reaches `Backend::draw_activity_bar`.

**Ask:** give `AppShell::build_activity_bar` a path to a theme-sourced
`active_accent` (and ideally `selection_bg`) — e.g. an `AppShell::
set_active_accent(Color)` / `set_selection_bg(Color)` setter the host
calls once per frame (mirroring how `Backend::set_theme` already works),
or a `&Theme` parameter threaded through `build_activity_bar` itself. Once
either lands, this repo's own fix is small and already drafted: call
`App::sync_per_frame_backend_state`'s existing per-frame sync point to
push `theme.activity_active_accent`/`theme.cursor` through the new hook,
and delete `render::build_activity_bar`'s now-redundant hand-rolled
adapter (or repoint it to the real call site, whichever the landed API
shape makes more natural) — no new per-backend code either way, matching
the Platform-Neutrality Rule.

**Also missing, same widget (secondary ask, lower priority):** VS Code
additionally paints a rounded-rect background behind the active icon.
`ActivityBarStyle::active_bg` (quadraui#658, the sidecar struct
`win::activity_bar`'s own doc references) already exists for exactly this
and `native_surface_paint::paint` already honours it when set — but
`AppShell::render`'s own call site never passes a non-default
`ActivityBarStyle` (its doc: "`AppShell` still calls the plain
`draw_activity_bar`, which never paints a fill"). Worth wiring alongside
the accent fix above, through whichever hook lands, once the primary ask
is resolved.

**Test:** `src/win/mod.rs::win_driver_tests::
activity_bar_paints_active_accent_strip_on_the_open_panel_1689` (added
alongside this entry) — a Tier-1 black-box scenario against the real
`App`/`WinDriver` pipeline (not a hand-built `ActivityBar` fixture like
the passing rasteriser-level test above): with `Engine::new_for_test()`'s
default state (Explorer active, sidebar visible — no setup needed, this
*is* the issue's own reported scenario), it pixel-probes the activity
bar's left-edge column at the Explorer row's vertical centre and asserts
it equals `theme.activity_active_accent`, and that the same column at the
(inactive) Search row does not. Mechanically certain to fail against the
pinned rev (every function in the root-cause above was read directly) and
to pass once quadraui wires a theme-sourced `active_accent` through
`AppShell::build_activity_bar` — but, per `src/win/mod.rs`'s own top-of-file
#1558 disclaimer, could not be *executed* from this Linux worktree to
observe that RED/GREEN flip directly; a source-level RED confirmation is
stated explicitly in the test's own doc comment instead.

**Blocks:** `JDonaghy/vimcode#1689`. Leave that issue open behind this one
per `GOALS.md`'s milestone-discipline rule — there is no per-backend
vimcode-side fix available; every file in the root-cause above lives in
quadraui, not in this repo.

---

## `Editor`/`EditorPaintOptions` has no way to reserve trailing content width independently of where the v/h scrollbar anchors — the minimap strip can overlap unwrapped buffer text (blocks vimcode#1696, half 1)

**Title:** `Editor::layout_with_options`'s `text_w`/`visible_cols` and its
`v_scrollbar_bounds`/`h_scrollbar_bounds` are both derived from the one
`viewport.width` parameter, with no second parameter letting a caller
reserve *extra* trailing width for content layout alone while leaving the
scrollbar anchored at the viewport's own right edge

**Body:**

vimcode#1696 reports (Win-GUI, but see below for why this is backend-
neutral) that with the minimap on and `'nowrap'` (vim's default), a buffer
line long enough to reach the pane's right edge paints glyphs that extend
under the minimap strip, which then overpaints the last character or two —
visually "losing" `e.` off the end of a Python docstring in the reporting
screenshot.

Root-caused by reading vimcode's own shared frame-composition code
(`src/render.rs`, the `#764`-converged "editor band" path the issue itself
names as the first place to check) side by side with the pinned quadraui
rev's `primitives::editor::Editor::layout_with_options`. The **vimcode-side
arithmetic that decides how many text columns a window gets is already
exactly correct** — this is not a frame-composition bug:

```rust
// src/render.rs, build_screen_layout_with_breadcrumb_row (abbreviated)
let minimap_w = raw_minimap_w + (scroll_gutter_width(scrollbar_reserve, char_width) - scrollbar_reserve);
let rw = build_rendered_window(..., minimap_w); // -> render_viewport_cols below

// build_rendered_window:
let render_viewport_cols = ((rect.width - scrollbar_reserve - minimap_w) / char_width)
    .floor() as usize - gutter_char_width;
//   = (rect.width - raw_minimap_w - scroll_gutter_width) / char_width - gutter_char_width

// the minimap strip itself (same function, `minimap: Vec<RenderedMinimap>`):
WindowRect::new(r.x + r.width - gutter /* == scroll_gutter_width */ - raw_minimap_w, ...)
```

Algebraically, `render_viewport_cols`' right-hand boundary and the
minimap's own `rect.x` are the *same* pixel position — confirmed by a new,
backend-neutral unit test, `to_q_editor_does_not_narrow_the_viewport_for_
the_minimap_strip_1696` (`src/render.rs`), which exercises the real
production functions end to end (no hand-derived arithmetic duplicated in
the test). So `render_viewport_cols`/`RenderedWindow.minimap_reserved_w`
already name the exact column budget VS Code's own minimap-aware layout
would use.

**The gap is that nothing enforces that budget as an actual paint
boundary when `'wrap'` is off** (vim's default, and the setting active in
the reporting screenshot). `render_viewport_cols` is consumed in exactly
one place when building a `RenderedLine` — the `wrap_on && line_char_len >
render_viewport_cols` word-wrap branch in `build_rendered_window`
(`src/render.rs`). The `else` branch (`!wrap_on`, i.e. every nowrap buffer)
hands the *entire, untruncated* line to `RenderedLine::raw_text`, relying
entirely on whatever the backend's own rasteriser does with
`quadraui::Editor`'s `rect`/`EditorLayout::text_bounds` to visually bound
it. And `render::to_q_editor` — the single, shared constructor both
backends' paint (`crate::app::App::paint_editor_windows_rung`) and GTK's
own click resolution (`gtk/click.rs`) call — builds that `Editor` with
`rect = rw.rect` **verbatim**, never consulting
`RenderedWindow.minimap_reserved_w` at all:

```rust
// src/render.rs, to_q_editor
pub fn to_q_editor(rw: &RenderedWindow) -> quadraui::Editor {
    let rect = quadraui::Rect::new(rw.rect.x as f32, rw.rect.y as f32,
                                    rw.rect.width as f32, rw.rect.height as f32);
    // ... rw.minimap_reserved_w is never read here.
```

This is deliberate, not an oversight introduced by this issue — it is
`#1094`'s own shipped design: `RenderedWindow.rect` reaches the pane's
*true* right edge on purpose, specifically so the strip can sit in the gap
between the (logically) narrower text and that true edge, and so
`quadraui`'s own drawn v/h scrollbar (`EditorLayout::v_scrollbar_bounds`/
`h_scrollbar_bounds`, anchored at `viewport.x + viewport.width -
v_scrollbar_w`) lands *past* the strip rather than immediately before it
— exactly the VS-Code-shaped order (`text, strip, scroll column`) #1094's
own doc comment states as the goal, and exactly what the existing,
passing `window_zone_hit_test_h_scrollbar_click_accounts_for_the_minimap_
strip` regression test pins.

**Why this can't be fixed by narrowing `to_q_editor`'s `rect` in vimcode
alone:** confirmed by reading `layout_with_options` at the pinned rev —
`text_w`/`visible_cols` (what bounds glyph painting: Win-GUI's `win::
editor::paint_line_text` slices to `[scroll_left, scroll_left +
visible_cols)` with no further pixel clip to save an over-wide value;
GTK's `paint_text_lines` relies on an earlier Cairo clip sized to
`text_bounds.width`) and `v_scrollbar_bounds`/`h_scrollbar_bounds` are
*both* derived from the one `viewport.width` argument — there is no second
parameter to decouple "how much width content gets" from "where the
scrollbar anchors". Narrowing the `rect`/`viewport` vimcode hands to
`Editor::layout` by `minimap_reserved_w` would fix the text overlap but
pull the v/h scrollbar in to sit flush against the (now-narrower) text,
immediately *before* the strip rather than past it at the pane's true edge
— reopening #1094 for all three pixel/cell backends at once (GTK, TUI, and
Win-GUI all reach this same shared code since `#1433`/`#1434` folded
`tui_main::run` onto the shared `crate::app::App`), not just the backend
this issue happens to screenshot.

**Ask:** give `Editor`/`EditorPaintOptions` a way to reserve additional
trailing content width distinct from the scrollbar anchor — e.g. an
`EditorPaintOptions::reserved_trailing_width: f32` (default `0.0`) that
`layout_with_options` subtracts from `text_w`/`visible_cols` *before*
computing them, while leaving `v_scrollbar_bounds`/`h_scrollbar_bounds`
anchored at the untouched `viewport.width` exactly as today. Once that
lands, vimcode's own fix is a one-line addition at `to_q_editor`'s call
sites (`crate::app::App::paint_editor_windows_rung`, `render::
editor_text_layout`/`tui_editor_text_layout`): pass `rw.minimap_reserved_w`
through as `reserved_trailing_width`, no per-backend code required either
side.

**Why not fixed in vimcode instead:** the only vimcode-side lever
available — narrowing `to_q_editor`'s `rect` — provably regresses #1094's
scrollbar placement on every GUI/TUI backend at once (shown above from the
pinned rev's own source, not a guess); shipping that trade silently would
replace one reported bug with a different, previously-fixed one. The
`docs/IRREDUCIBLE_SURFACE.md`/`CLAUDE.md` bar for a vimcode-side fix
("compare against the quadraui example; if it needs new backend-specific
code, stop") applies the other way here too: there is no backend-specific
code to add on either side of this gap, which is exactly why it has to be
quadraui's own `Editor` primitive that grows the missing parameter, not a
`src/gtk/`/`src/win/`/`src/tui_main/` workaround.

**Test:** `src/render.rs::render::tests::to_q_editor_does_not_narrow_the_
viewport_for_the_minimap_strip_1696` (added alongside this entry) —
backend-neutral, runs on any host today (no Windows/GTK display needed):
builds a `RenderedWindow` fixture with a non-zero `minimap_reserved_w`,
runs it through the real `to_q_editor` + `Editor::layout`, and asserts
`EditorLayout::text_bounds`'s right edge lands past the minimap strip's
own left edge — i.e. it is a **passing** test today that pins the
*unfixed* behaviour as a concrete regression target, documented in the
test's own doc comment as exactly that (mirroring how `docs/
PENDING_QUADRAUI_ISSUES.md` entries elsewhere pin an upstream gap with a
vimcode-side test that cannot itself observe the eventual fix).

**Blocks:** `JDonaghy/vimcode#1696`, half 1 (the minimap-inset half; see
the neighbouring entry below for the tab-bar-toolbar half). Leave that
issue open behind both entries per `GOALS.md`'s milestone-discipline rule
— there is no per-backend vimcode-side fix available for either half.

---

## `TabBar`'s right-aligned segments have no outer-edge inset, unlike `StatusBar`'s `PIXEL_EDGE_INSET` (#1155) — the tab-strip overflow-action glyph can sit flush against (or past) a borderless window's real right edge (blocks vimcode#1696, half 2)

**Title:** `primitives::tab_bar::TabBar::layout`'s right-aligned-segment
placement (`seg_x = bar_width - right_area_width`, continuing flush to
`bar_width`) has no analogue of `primitives::status_bar::PIXEL_EDGE_INSET`
— every right segment, including the trailing `tab:action_menu` `"⋯"`
overflow control vimcode builds in `build_tab_bar_primitive`, is
positioned with **zero** margin from whatever width the caller hands
`layout`, unlike `StatusBar`, which (per issue #1155) already reserves
`PIXEL_EDGE_INSET` (`10.0` DIP) inside its own layout on every "pixel"
backend

**Body:**

vimcode#1696's second symptom: the tab-strip's trailing overflow control
(the `"⋯"` U+22EF ellipsis `build_tab_bar_primitive` appends as the last
`right_segments` entry) renders as two dots rather than a full ellipsis on
Win-GUI, which reads as the glyph being clipped by the window's own right
edge — VS Code's equivalent control sits inset with margin to spare.

Root-caused by reading `src/render.rs`'s tab-bar composition
(`build_tab_bar_primitive`/`build_screen_layout_with_breadcrumb_row`'s
`GroupTabBar` builder, `src/app.rs`'s `paint_tab_bars_rung`) against the
pinned rev's `primitives::tab_bar::TabBar::layout`. Exactly as with the
neighbouring minimap entry above, **vimcode's own width input is not the
bug**: `bounds`/`target.rect` for a group's tab bar is the same
`main_content_bounds`-derived window-group rect every other editor-band
surface uses, and `src/win/mod.rs`'s own `#1561` investigation (reading
`WinBackend::attach_surface`/`resize_surface`) already confirmed the
render target's reported size comes from one un-split `GetClientRect`/
`WM_SIZE` pair with no double-subtraction or DIP/physical mismatch — so
the width vimcode hands to the tab bar is the real window's real content
width, not an inflated one.

The gap is inside `TabBar::layout` itself (`quadraui/src/primitives/
tab_bar.rs`, pinned rev `ca7fcc8`):

```rust
// ── Right-aligned segments ─────────────────────────────────────
if segs_fit {
    let mut seg_x = bar_width - right_area_width;
    for (i, seg) in self.right_segments.iter().enumerate() {
        let w = seg_widths[i];
        let bounds = Rect::new(seg_x, 0.0, w, bar_height);
        ...
```

`seg_x` starts at `bar_width - right_area_width` and the last segment (the
`"⋯"` control, 3 cells wide per `build_tab_bar_primitive`) ends exactly at
`bar_width` — flush against whatever width the caller passed, with no
margin at all. Compare `primitives::status_bar.rs`, which already solved
this identical problem for the status bar (issue #1155):

```rust
// quadraui/src/primitives/status_bar.rs
pub const PIXEL_EDGE_INSET: f32 = 10.0;
// ... StatusBar's own layout subtracts PIXEL_EDGE_INSET from both ends
// before placing left/right segments, so "the ruler segment" (vimcode's
// own render.rs comment: "quadraui#1155 gives pixel backends their own
// outer edge inset — a manual trailing space here would double it")
// never needs a caller-side workaround.
```

`TabBar` never grew the equivalent. On a GTK window (which typically has
some native client-side-decoration/compositor margin outside the Cairo
canvas even before any inset) a flush-to-`bar_width` placement may still
read as "inset enough" by accident; on Win-GUI's borderless client area —
confirmed by #1561's investigation to paint genuinely edge-to-edge, no
extra margin — a flush placement puts the glyph's own bounding box right
at the real window edge, with whatever margin the glyph happens to need
beyond its nominal `width_cells * cell_width` box (e.g. a proportional
DirectWrite rendering of `"⋯"` drawn wider than three monospace cells)
landing partly off-window.

**Why not fixed in vimcode instead:** the only caller-side lever —
narrowing the `bounds`/`target.rect` vimcode hands to `TabBar::layout` —
narrows the *whole* bar (tabs included, not just the trailing segment,
since `layout` takes one `bar_width` for both) and, worse, is a different
rect than the one the same call paints the tab-bar *background* with
(`Backend::draw_tab_bar_icons_layout(target.rect, ...)` does both from one
argument), so narrowing it would leave an unpainted sliver of raw
background colour at the bar's true right edge — trading one visible
defect for another, and only on the backend doing the narrowing (the
opposite of the shared, one-geometry-computation discipline `#703`/`#764`
already established for this exact file). A correct fix needs the inset
*inside* `TabBar::layout` itself, the same place `StatusBar::layout`
already carries it, so the background fill and the segment placement stay
derived from the one call and never desync.

**Ask:** add a `TabBar`-side outer-edge inset mirroring `StatusBar`'s
`PIXEL_EDGE_INSET` — either reuse the same constant (promoted out of
`primitives::status_bar` into a shared location both primitives import)
or give `TabBar` its own, and have `layout` reserve it on the trailing
(and, for symmetry with `StatusBar`, leading) edge before placing
`right_segments`/tabs, the same way `StatusBar::layout` already does for
its own segments.

**Test:** no accompanying vimcode-side test for this half — unlike the
neighbouring minimap entry, there is no vimcode-authored arithmetic to pin
(the formula quoted above is read verbatim from `quadraui::primitives::
tab_bar::TabBar::layout`, which this repo must not edit per the
Platform-Neutrality Rule), and the acceptance bar's own pixel-level check
("the tab-strip toolbar's right edge is strictly inside the window") is
not expressible with the currently-installed `win_native_driver.py`
step vocabulary either — see `tests/smoke-spec/win-gui.yaml`'s matching
cross-reference, added alongside this entry, for why (same
`expect_capture_nonblank`-shaped-primitive gap `#1676`'s entry already
names).

**Blocks:** `JDonaghy/vimcode#1696`, half 2 (the tab-bar-toolbar-inset
half; see the neighbouring entry above for the minimap half). Leave that
issue open behind both entries per `GOALS.md`'s milestone-discipline rule
— there is no per-backend vimcode-side fix available for either half.

---

## `TreeController::render` double-paints its vertical scrollbar — one real, one phantom, different widths, immediately adjacent — and neither is VS Code's thin hidden-at-rest overlay (blocks vimcode#1695)

**Title:** vimcode#1695 reports the Win-GUI Explorer sidebar scrollbar as a
wide (~14-28px), always-visible, light-grey bar with what looks like "a
thumb and a second overlapping rect rather than one thumb on one track" —
compared to VS Code's thin (~10px) overlay that is fully transparent at
rest and fades in on hover/scroll. Root-caused by reading `TreeController`
(`compose/tree_controller.rs`) and `primitives::tree`/`primitives::
scrollbar` directly at the pinned rev
(`ca7fcc83afad01ec3422f79366566f3a263b22bf`) — not reproduced on Win-GUI
hardware (no Windows host in this session), but reproduced **executably on
GTK**, which shares the double-paint and no-hidden-at-rest-state bugs
(Root causes 1 and 2 below) with Win-GUI and macOS
(`GtkBackend::tree_vscrollbar`/`draw_tree`, `MacBackend::
tree_vscrollbar`/`draw_tree`, and `WinBackend::tree_vscrollbar`/`draw_tree`
all delegate to the same `TreeView::vscrollbar`/`primitives::
tree::native_surface_paint::paint` this entry names — confirmed by reading
all three `Backend` impls side by side). The GTK reproduction is
vimcode's own new `src/gtk/testing.rs::scrollbar_paint::
explorer_sidebar_scrollbar_paints_unconditionally_at_rest_1695` and
`::explorer_sidebar_scrollbar_double_paints_an_adjacent_phantom_band_1695`
— both pass **today**, against the pinned rev, proving the two findings
below are real and not speculation from reading source alone.

**This is GTK-parity, not pixel-for-pixel parity — one axis genuinely
diverges.** `quadraui/src/win/tree.rs`'s `draw_tree` hardcodes
`let theme = Theme::default();` and never reads `WinBackend::
current_theme`, unlike `GtkBackend::draw_tree`
(`gtk/backend.rs:2266`, passes `&self.current_theme` into
`crate::gtk::draw_tree`) and `MacBackend::draw_tree`
(`macos/backend.rs:1928`, `let theme = self.current_theme;` then passed
into `super::tree::draw_tree`), both of which thread the live theme
through. Because the phantom inner scrollbar (Root cause 1) is painted
from *inside* that same `draw_tree` call, it renders in quadraui's
hardcoded default theme colours on Win-GUI, not the user's active theme —
while the real, explicit outer scrollbar
(`WinBackend::draw_scrollbar`, `win/backend.rs:3739-3744`) correctly
reads `self.current_theme`. So on real Win-GUI hardware the two stacked
bands this issue reports are not just two different *widths* (as Root
cause 1 below describes) but also two different *theme sources* — one
tracking the user's active colorscheme, one stuck on quadraui's library
default — which plausibly explains "two differently-lit segments" at
least as directly as the track/thumb alpha compositing in Root cause 2
does. This half of the symptom is **Win-GUI-specific**: GTK's and
macOS's `draw_tree` both thread `current_theme` through, so their two-band
reproduction is same-themed throughout and does not exhibit it. A fix
for Root causes 1/2 alone (stopping the double-paint, adding hidden-at-
rest state) would still leave Win-GUI's inner band mis-themed relative to
GTK/macOS unless this is fixed too — see Ask item 5 below. (This
divergence is a corollary of the broader "seven Win-GUI rasterisers paint
`Theme::default()` instead of `self.current_theme`" entry elsewhere in
this file, which already lists `WinBackend::draw_tree` among the seven;
this entry's Ask item 5 below is scoped to this one call site so a fix
for #1695 doesn't have to wait on that broader entry landing first.)

**Root cause 1 (the "two differently-lit segments" bug — not by design):**
`TreeController::render` —

```rust
// quadraui/src/compose/tree_controller.rs
pub fn render(&self, backend: &mut dyn Backend, rect: Rect) {
    let (tree_rect, sb_rect) = self.split_rect(backend, rect);
    let tree = self.build_tree_view(tree_rect);
    backend.draw_tree(tree_rect, &tree);
    if let Some(sb_rect) = sb_rect {
        let sb = self.build_scrollbar(backend, sb_rect);
        backend.draw_scrollbar(sb_rect, &sb);
    }
}
```

`split_rect` narrows `rect` into `tree_rect` (content) and `sb_rect` (the
scrollbar column `TreeController` owns and paints explicitly, second).
But `build_tree_view(tree_rect)` returns a `TreeView` whose `rows` is
`self.rows.clone()` — **every** row, untruncated; `tree_rect`'s *width*
shrank, but its row *count* did not change, and nothing in `build_tree_
view` or the `TreeView` it returns records "a caller already reserved a
scrollbar column, don't paint your own." The shared rasteriser underneath
`backend.draw_tree(tree_rect, &tree)` —
`primitives::tree::native_surface_paint::paint` — ends with:

```rust
// quadraui/src/primitives/tree.rs, `paint`
if let Some(vsb) = tree.vscrollbar(area, item_height) {
    crate::primitives::scrollbar::native_surface_paint::paint(&vsb, surface, theme);
}
```

`area` here is `tree_rect` — already narrowed — but `tree.rows.len()` is
still the *original*, untruncated count, so `TreeView::vscrollbar`'s own
overflow check (`total > visible`, driven by *height*, which `split_rect`
never touched) still finds overflow and paints a **second** scrollbar, at
`tree_rect`'s own right edge:

```rust
// quadraui/src/primitives/tree.rs, `TreeView::vscrollbar`
let track = Rect::new(area.x + area.width - row_height, area.y, row_height, area.height);
```

The two scrollbars use **different width formulas** — the real,
explicit one (`TreeController::scrollbar_track_width`) falls back to
`backend.line_height()`; the phantom inner one derives its width from
`layout_metrics::tree_row_pitch(tree, line_height)`, i.e. `(line_height *
1.4).round()`, ~40% wider. They paint immediately adjacent to each other
(the phantom's right edge is exactly the real one's left edge), each with
its own track/thumb compositing (a brighter thumb band over a dimmer
track band — see Root cause 2) — which is exactly vimcode#1695's "two
differently-lit segments... a thumb and a second overlapping rect" and
exactly accounts for its reported ~14-28px combined width (neither single
scrollbar is that wide on its own; the *sum* of `line_height` +
`(line_height * 1.4).round()` is).

**Root cause 2 (always-visible, no overlay, no hover reveal):**
`primitives::scrollbar::native_surface_paint::paint`'s track alpha is

```rust
let track_alpha = if scrollbar.hovered || scrollbar.dragging { 0.35 } else { 0.20 };
```

— never `0.0`. There is no "fully transparent at rest" state at all, for
either scrollbar instance above. Compounding this, `TreeController::
build_scrollbar` never sets `Scrollbar::hovered`:

```rust
// quadraui/src/compose/tree_controller.rs, `build_scrollbar`
let mut sb = Scrollbar::vertical(..., sb_rect, ..., min_thumb);
sb.dragging = is_dragging;   // `hovered` is left at its `Default` (`false`)
```

`TreeController::handle`'s `MouseMoved` arm exists but never threads a
"cursor is over `sb_rect`" fact into the next `render()` call — there is
no state on `TreeController` to carry it, and `render(&self, ...)` takes
`&self`, not `&mut self`, so it could not update one even if it tried.
VS Code's overlay-on-hover/scroll behaviour needs genuinely new state
(something like `TreeController::set_scrollbar_hover(bool)` /
`note_recent_scroll()`, read by `build_scrollbar`), not a config knob —
this is new quadraui API surface, not a value a downstream caller can
already reach.

**Resolving the issue's own open question:** the "two stacked segments"
*is* a real second-scrollbar-paint bug (Root cause 1), not the intended
thumb-over-track compositing (which *also* exists, per Root cause 2, and
independently produces a lighter-band-over-darker-band look *within* each
single scrollbar — so the visual is actually two compositing effects
stacked on top of each other: two adjacent bands, each itself a
track-dimmed / thumb-brightened composite). On Win-GUI specifically, the
issue's own "Check GTK/macOS before assuming this is Win-GUI-only" ask
has a mixed answer: the double-paint and no-hidden-at-rest-state bugs
(causes 1/2) are confirmed shared with GTK/macOS (reproduced there, see
above), but the two bands' *theme sourcing* is not — see the "GTK-parity,
not pixel-for-pixel" paragraph above and Ask item 5. All three are
described above so a fix doesn't have to re-derive any of this.

**Ask:**
1. Stop the double paint: give `TreeController::render` a way to tell
   `backend.draw_tree` "don't self-paint a scrollbar, I'm handling it" —
   e.g. a `TreeView::style` flag (mirroring `TreeStyle::row_height`'s
   existing shape) that `primitives::tree::native_surface_paint::paint`
   checks before its own `tree.vscrollbar(...)` call, set by
   `TreeController::build_tree_view`. (`ListView`'s `ListController`, if
   one exists with the same split-rect shape, should be audited for the
   identical bug — not confirmed here, out of this issue's scope.)
2. Give `TreeController`/the `Scrollbar` primitive a real hidden-at-rest
   state: `track_alpha`/`thumb_alpha` reaching `0.0` when neither hovered
   nor dragging nor recently scrolled, with the reveal fading in exactly
   as the issue asks. Needs new `TreeController` state (hover tracking
   from `MouseMoved`, a "recently scrolled" timer or frame-counted decay)
   plumbed into `build_scrollbar`.
3. Make the overlay actually overlay: `split_rect` currently *reserves* a
   column (shrinks `tree_rect`) rather than painting over the full-width
   tree content VS Code does. Matching VS Code exactly means `tree_rect`
   should stay full-width and the scrollbar should paint on top of it
   (respecting whatever hidden-at-rest state #2 adds) — a bigger, options-B
   change than #1/#2; worth scoping as a follow-up once those land, rather
   than blocking them on it.
4. Independently, consider whether the default track width (`backend.
   line_height()`, easily 20px+) should instead default toward something
   closer to VS Code's ~10px — `TreeController::set_scrollbar_width`
   already exists as an opt-in override today, so this item alone could
   be satisfied without a quadraui change (a downstream caller can already
   call it) — included here only because the *default* shaping every
   caller's first impression is still `line_height()`-wide.
5. **Win-GUI-specific:** thread `self.current_theme` through
   `WinBackend::draw_tree` → `win::tree::draw_tree` (replace its hardcoded
   `let theme = Theme::default();`), the same one-line shape
   `WinBackend::draw_scrollbar` already uses. Without this, fixing #1/#2
   above leaves Win-GUI's tree-embedded scrollbar painting in the wrong
   theme even after the double-paint and hidden-at-rest gaps are closed —
   a Win-GUI-only divergence from GTK/macOS that Root causes 1/2 alone
   don't cover. (Tracked more broadly, alongside six other Win-GUI call
   sites with the identical `Theme::default()` gap, by this file's
   separate "Seven Win-GUI rasterisers still paint a hardcoded
   `Theme::default()`" entry — either entry's fix closes this item.)

**Test:** `src/gtk/testing.rs::scrollbar_paint::
explorer_sidebar_scrollbar_paints_unconditionally_at_rest_1695` and
`::explorer_sidebar_scrollbar_double_paints_an_adjacent_phantom_band_1695`
(added alongside this entry, both passing today) are executable,
GREEN characterizations of the current (buggy) behaviour above, driven
through vimcode's real Explorer paint path
(`App::paint_sidebar_panel_rung` → `populate_explorer_tree_controller` →
`TreeController::render`) on GTK — chosen over a Win-GUI-only test
because GTK can actually execute headlessly on every CI host, and the bug
is proven shared code, not backend-specific. Both are tripwires, not
locks: they are expected to start failing once quadraui ships Root cause
1/2's fix and the pin is bumped, at which point they document their own
deletion in their doc comments rather than needing to be patched to
match a new "correct" geometry. Neither GTK test covers the theme-
divergence item (Ask item 5) — GTK's `draw_tree` already threads
`current_theme` through, so there is nothing to characterize there; that
item is Win-GUI-only and has no headless-host reproduction available in
this session (confirmed only by reading `win/tree.rs` and `win/
backend.rs` directly against the pinned rev).

**Blocks:** `JDonaghy/vimcode#1695`. Leave that issue open behind this one
per `GOALS.md`'s milestone-discipline rule — there is no per-backend
vimcode-side fix available for Root causes 1/2 or Ask item 5's theme
divergence; every file named above lives in quadraui, not in this repo.
Item 4 of the Ask is the one exception (a caller-side
`set_scrollbar_width` call vimcode itself could make) but was left
bundled here rather than split into a separate vimcode-side PR, since
shipping it alone would only mask part of Root cause 1's double-paint
(the phantom band would merely get narrower, not disappear) and could
read as "fixed" when it is not.

**Related:** vimcode#1695's own "Related" section also names vimcode#723
(minimap scrollbar discarded) and vimcode#1094 as siblings — "together
they decide what the editor pane's right edge looks like." Both are a
different component (`MinimapLayout::scrollbar`, not the Explorer's
`TreeController` this entry covers) and are out of scope here; noted only
so a future reader doesn't have to re-derive that these are siblings, not
duplicates, of this entry.

---

## `TreeView`/`TreeRow` has no indent-guide primitive — every pixel backend's tree rasteriser only offsets rows by `indent`, it never paints the vertical rule VS Code draws per nesting level

**Title:** `primitives::tree`'s indent math (`cursor_x = row_x + 2.0 + row.indent as f32 * indent_px`, `native_surface_paint::paint`) only ever moves the chevron/icon/label block to the right — there is no companion "draw a 1px vertical line at each ancestor's indent column" step anywhere in the primitive, `TreeStyle`, or any of the four backend rasterisers (`win::tree::draw_tree`, `gtk::tree`, `macos::tree`, `tui::tree` all delegate to the same shared `native_surface_paint::paint`/TUI equivalent). VS Code's Explorer draws a faint vertical guide for every ancestor level a deeply-nested row has; vimcode's tree has never drawn one, on any backend.

**Body:**

Surfaced by vimcode#1693 ("Explorer is missing VS Code's structure"), whose ask item 4 ("indent guides — a vertical rule per nesting level") is explicitly called out as "the Win-GUI counterpart of vimcode#38, which covers GTK" — i.e. this is not new information specific to Win-GUI, it is a gap in the shared primitive that happens to have two open vimcode-side tracking issues (GTK's #38, Win-GUI's #1693) because nothing in quadraui paints it on *any* backend yet.

Confirmed at the pinned rev (`ca7fcc83afad01ec3422f79366566f3a263b22bf`) by reading:
- `quadraui/src/types.rs`'s `TreeStyle` — fields are `indent` (px/cells per level), `show_chevrons`, `chevron_expanded`/`chevron_collapsed`, `row_height`. No guide-colour, guide-width, or guide-on/off field exists to even opt into.
- `quadraui/src/primitives/tree.rs` — the only place `row.indent` is read for paint is the `cursor_x = row_x + 2.0 + row.indent as f32 * indent_px` line that positions the chevron/icon/text block. Nothing between the row's left edge and that offset is painted at all (no fill, no line) — it is background colour showing through, by omission rather than by an explicit "no guide" choice.
- `quadraui/src/win/tree.rs`, `gtk/tree.rs`, `macos/tree.rs` — none paint anything in that gap either; all three (and TUI's own tree rasteriser) delegate row content painting to the one shared `native_surface_paint::paint` fn this primitive's module doc describes, so the gap is provably identical across every backend, not independently missing four times.

**Ask:**

1. Add an opt-in indent-guide style to `TreeStyle` — e.g. `pub guide: Option<IndentGuideStyle>` where `IndentGuideStyle { color: Color, width: f32 }` (`None` default preserves today's no-guide behaviour for every existing caller, so this is additive, not breaking).
2. When set, `native_surface_paint::paint` draws one vertical line per ancestor indent level (`0..row.indent`) at `row_x + 2.0 + level as f32 * indent_px + (indent_px / 2.0)` (centered in the level's column, roughly matching VS Code's own placement), spanning the row's full height, in `guide.color`/`guide.width`.
3. TUI's tree rasteriser needs the character-grid equivalent — a single-cell vertical-bar glyph (`│`, U+2502) per ancestor column instead of a sub-cell line, since a terminal cell can't be subdivided (same "TUI ignores this, GUI backends use real sub-pixel geometry" split `TreeStyle::row_height`'s own doc already draws for a different field).

**Test:** None added this round — this entry is a drafted gap report, not a landed fix; a conformance-style scenario asserting guide pixels appear at each ancestor's indent column (mirroring this file's other entries' "Test" sections, which describe characterizations added *alongside* a fix) is the natural shape once the `Ask` lands.

**Blocks:** `JDonaghy/vimcode#38` (GTK) and `JDonaghy/vimcode#1693` (Win-GUI). Leave both open behind this one per `GOALS.md`'s milestone-discipline rule — there is no per-backend vimcode-side fix available; the gap is in `primitives::tree`/`TreeStyle`, shared by every backend.

