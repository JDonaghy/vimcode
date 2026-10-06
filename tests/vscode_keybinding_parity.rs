//! #1730: VS Code mode parity — binding coverage vs VS Code's real per-OS
//! defaults, plus a reachability matrix for what a real keyboard can actually
//! deliver on each surface vimcode ships.
//!
//! `tests/vscode_mode.rs`'s 62 tests prove the **binding table**: they call
//! `Engine::handle_key` with an already-decoded `(key_name, unicode, ctrl)`
//! triple, so they say nothing about whether a real keypress on a real
//! keyboard, through a real terminal or a real GTK window, ever produces that
//! triple. This file adds three things on top:
//!
//! 1. **Binding coverage** — [`VSCODE_BINDINGS`], a table of VS Code's
//!    documented default editor/workbench keybindings (cited by command id)
//!    for the surface `src/core/engine/vscode.rs` actually implements, each
//!    row marked `Matches` / `Missing` against what vimcode does (`MacDiverges`
//!    was a third status #1730 introduced and #1745 deleted — see the
//!    correction below). Every `Matches` row that the existing 62 tests
//!    don't already exercise gets a new engine-level test here (same style as
//!    `tests/vscode_mode.rs`'s own `vk`/`handle_key` idiom — this is
//!    deliberately *not* a driver-tier test: the gap this table closes is
//!    "is every implemented binding asserted", not "does a real keypress
//!    reach the engine", which is (2) below).
//! 2. **Reachability matrix** — [`REACHABILITY_TABLE`], recording whether
//!    each ambiguous or OS-sensitive chord actually reaches the engine
//!    through real input on TUI-legacy-xterm, TUI-kitty, ConPTY, GTK and the
//!    native macOS GUI (`src/macos/`, see the correction below) — plus
//!    driver-tier (`render::engine_key_from_ui`, the one shared decoder both
//!    GTK and TUI call per its own module doc) tests for the cases that are
//!    reachable or unreachable for a provable reason.
//! 3. **Known gaps** — a bidirectional gate (mirrors `src/harness.rs`'s
//!    `KNOWN_BUGS` idiom and `tests/nvim_conformance.rs`'s
//!    `KNOWN_DEVIATIONS`, reimplemented locally here because both of those
//!    are `pub(crate)`/private to their own crate and this file is a
//!    separate integration-test crate) for every binding [`VSCODE_BINDINGS`]
//!    marks `Missing` or wrong. Each gate test asserts *today's* (wrong)
//!    behaviour and panics if that behaviour ever changes — so a fix forces
//!    deletion of the table row and the gate entry, the same way `KNOWN_BUGS`
//!    forces deletion on `FixLanded`.
//!
//! ## Correction (#1745): there is a real macOS GUI backend in this repo
//!
//! This file originally claimed "there is no macOS GUI backend in this repo
//! to regress against" and marked every `macos_gui` reachability cell
//! `NotApplicable` on that basis, leaving every `MacDiverges` row ungated.
//! **That was wrong.** `src/macos/` has shipped a real, non-stub backend
//! since #859/#896: it builds with `--no-default-features --features
//! macos`, wraps `quadraui::macos::MacBackend` through the same
//! `App::handle_dispatch`/`App::handle_key_press` both GTK and TUI run
//! through, and has its own `MacDriver` black-box test tier
//! (`src/macos/mod.rs`'s `mac_driver_tests` module, 20+ tests before #1745)
//! plus a Tier-2 `mac-native` smoke lane on real hardware. macOS is this
//! project's top-priority GUI target, and a VS Code user there expects Cmd,
//! not Ctrl, for every one of the chords below.
//!
//! #1745 did the actual reachability work this correction implies:
//! `normalize_mac_cmd_as_ctrl` (`src/app.rs`, called once at the top of
//! the one shared `App::handle_dispatch`) folds `quadraui::Modifiers::cmd`
//! into `ctrl` for plain Ctrl-to-Cmd substitution chords, and translates the
//! handful of VS Code Mac defaults that are *not* a plain substitution
//! (Option for word-nav, Cmd for line/doc-nav) into the exact `Key`/
//! `Modifiers` shape `src/core/engine/vscode.rs` already understands — gated
//! on the live backend's own `PlatformServices::platform_name() == "macos"`
//! (a runtime capability query, not a `cfg!(target_os)` guess), so GTK's own
//! Cmd-reporting convention (Super/Meta -> `cmd`, quadraui's `gtk/events.rs`)
//! is untouched. See that function's own doc for the full reasoning, and
//! `src/macos/mod.rs`'s `vscode_mode_mac_cmd_1745` module for the `MacDriver`
//! black-box proofs. Every row below marked `Matches` for `Os::Mac` is backed
//! by one of those tests or by this file's own `KNOWN_GAPS` gate; three real
//! gaps remain: `Cmd+Option+F` for Find & Replace is blocked upstream in
//! quadraui (see `docs/PENDING_QUADRAUI_ISSUES.md`), Ctrl+-/Ctrl+Shift+- for
//! navigate back/forward needs its own vimcode-side follow-up issue, and
//! `Cmd+B` (and every other chord whose only real implementation is a
//! `panel_keys` accelerator — see the `toggleSidebarVisibility` row and
//! `REACHABILITY_TABLE`'s matching row below) is dead on the macOS GUI
//! because the Cmd-to-Ctrl fold runs *after* quadraui's own accelerator
//! matching, so it can never retroactively make a Cmd keypress match a
//! Ctrl-literal accelerator binding.
//!
//! ## The macOS-terminal question (deliverable 3)
//!
//! Decision: **macOS TUI should use exactly the same bindings as Linux/Windows
//! TUI — i.e. do nothing extra.** `src/core/engine/vscode.rs` already speaks
//! only in terms of `ctrl`/`shift`/`Alt_*` key names; it has no Mac-specific
//! branch to add one to. That is correct, not an oversight: Cmd is a GUI-only
//! modifier that never reaches a terminal application at all (no terminal
//! emulator, on any OS, forwards it — this is a property of the terminal
//! protocol, not of vimcode or crossterm), so the reachable-chord alphabet on
//! a macOS terminal is identical to Linux's (Ctrl/Alt/Shift + key). VS Code
//! itself falls back to the same thing in a terminal-hosted editor for the
//! same reason. Concretely: no remap is needed, and none should ever be
//! added — a `cfg(target_os = "macos")` branch inside `vscode.rs` gating on
//! Cmd-vs-Ctrl would be dead code (Cmd bytes never arrive) and would violate
//! this repo's platform-neutrality rule for no benefit. Cmd *does* matter on
//! the native macOS **GUI** backend (`src/macos/`) — see the correction
//! above and [`REACHABILITY_TABLE`]'s `macos_gui` column for what that
//! backend needs (and, as of #1745, does) differently from the terminal.
//!
//! ## Native menu bar vs editor bindings: no double-fire (issue acceptance
//! ## condition)
//!
//! The issue's own acceptance condition requires that macOS menu-bar
//! shortcuts not double-fire with editor bindings (e.g. Cmd+C handled by
//! both the native menu and the engine). Confirmed safe by reading the
//! code directly: `render.rs`'s menu-item builder constructs each native
//! menu entry's `key_equivalent` as `quadraui::KeyBinding::Literal("Ctrl+\
//! ...")` — **Ctrl**, not Cmd — and quadraui's own `macos/\
//! menu_bar_install.rs::parsed_to_ns` maps `ctrl` to
//! `NSEventModifierFlags::Control`, so the native `NSMenu`'s key
//! equivalents are Ctrl-based and structurally cannot intercept a Cmd
//! chord. The consequence worth flagging explicitly: the native macOS menu
//! bar still *advertises* Ctrl+S/Ctrl+Z/Ctrl+C in its menus, while the
//! chords that actually work in the editor on macOS, as of #1745, are the
//! Cmd equivalents — a cosmetic (tooltip-only) mismatch, not a functional
//! one, and out of this issue's scope to fix (it would need the menu
//! builder itself to know it's rendering for a Cmd-translating backend,
//! which is a `render.rs` change with its own review, not folded in here).
//!
//! ## Why these specific gated gaps and not others
//!
//! Every [`KNOWN_GAPS`] entry is verifiable by reading this repo's own code
//! (not by trusting a possibly-stale memory of VS Code's UI) — each doc
//! comment below points at the exact function/match-arm that proves the gap.
//! Many more "VS Code default vs vimcode" mismatches almost certainly exist
//! (extensions are out of scope per the issue, and the long tail of less
//! common commands was not exhaustively diffed given the issue's own
//! "small, obvious one-line fixes are fine, everything else is a finding"
//! scope) — the closing report lists every mismatch found, gated or not.

mod common;
use common::*;
use std::fs;
use vimcode_core::core::settings::EditorMode;
use vimcode_core::render::engine_key_from_ui;
use vimcode_core::{Cursor, EngineAction, Mode};

/// Switch engine to VSCode mode (same helper as `tests/vscode_mode.rs`,
/// duplicated here because integration-test files are separate crates and
/// cannot share non-`common` helpers).
fn vscode_mode(e: &mut vimcode_core::Engine) {
    e.settings.editor_mode = EditorMode::Vscode;
    e.mode = Mode::Insert;
    e.visual_anchor = None;
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Binding coverage — VS Code's defaults vs what vimcode implements
// ═══════════════════════════════════════════════════════════════════════════

/// Which OSes a VS Code default keybinding is cited for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // `Windows`/`Mac`/`Linux` document the table even when a row's own test only probes the OS-neutral engine-key shape.
enum Os {
    /// VS Code's Windows and Linux defaults are identical for every command
    /// id this table cites (confirmed by VS Code's own `keybindings.json`
    /// defaults, which key almost everything off the generic `win+linux`
    /// bucket and only override `mac` separately).
    WinLinux,
    Mac,
}

/// How a row in [`VSCODE_BINDINGS`] compares to what `src/core/engine/
/// vscode.rs` implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// vimcode's key name + ctrl flag implements this VS Code default
    /// exactly (for the `Os` the row is cited for). #1745: every row that
    /// used to carry a (now-deleted) `MacDiverges` status because the Mac
    /// chord was not a plain Ctrl-to-Cmd substitution on the *terminal*
    /// build is `Matches` now that the macOS *GUI* backend exists to
    /// translate it (`normalize_mac_cmd_as_ctrl`, `src/app.rs`) —
    /// Cmd is a GUI-only modifier, so the terminal-build reasoning in the
    /// module doc's macOS-terminal decision is unaffected.
    Matches,
    /// VS Code binds this command by default; vimcode's VS Code mode does
    /// not bind it to anything, or binds the chord to a different command
    /// entirely. Gated in [`KNOWN_GAPS`] when a concrete, reproducible
    /// wrong-today behaviour exists to pin; listed in the closing report
    /// either way.
    Missing,
}

/// One row: a VS Code default keybinding, cited by command id, and how it
/// compares to vimcode's VS Code mode.
#[allow(dead_code)]
struct VscodeBinding {
    /// VS Code's own command id (`Help: Open Default Keyboard Shortcuts
    /// (JSON)` is the authoritative source these were read from).
    command_id: &'static str,
    os: Os,
    /// The chord as VS Code spells it (`Ctrl+K Ctrl+C` style for chords).
    vscode_chord: &'static str,
    /// vimcode's engine key name for the equivalent chord, if any.
    vimcode_key: &'static str,
    status: Status,
    note: &'static str,
}

/// VS Code's default editor + core workbench keybindings that
/// `src/core/engine/vscode.rs` implements, is missing, or gets wrong,
/// cross-referenced against that file's own match arms. Linux and Windows
/// share one row (`Os::WinLinux`); Mac gets its own row whenever VS Code's
/// Mac default is not simply "same chord with Cmd for Ctrl".
static VSCODE_BINDINGS: &[VscodeBinding] = &[
    // ── Confirmed matches (Linux/Windows) ──────────────────────────────
    // #1744 fixed the five rows that used to live here as `Status::Missing`
    // (`insertCursorAbove`/`Below`, `copyLinesUpAction`/`DownAction`,
    // `navigateBack`/`Forward`, `jumpToBracket`) — see
    // `ctrl_alt_up_has_a_distinct_wire_representation_from_alt_up` (renamed
    // from `gap_ctrl_alt_up_is_move_line_not_insert_cursor_above` once the
    // fix landed) and its four siblings below for the now-`FixLanded`,
    // deleted `KNOWN_GAPS` entries,
    // `render::alt_key_router_tests` for `route_alt_key`'s own
    // spelling-identity unit coverage, and — the actual black-box proof for
    // all five chords, per CLAUDE.md's rendered-output rule —
    // `src/gtk/testing.rs`'s `mod alt_rung_1744` and
    // `src/tui_main/app_on_tui_tests.rs`'s `mod vscode_mode_alt_rung_1744`.
    VscodeBinding {
        command_id: "editor.action.insertCursorAbove",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Alt+Up",
        vimcode_key: "Alt_Up (ctrl)",
        status: Status::Matches,
        note: "`render::route_alt_key` now takes a `ctrl` parameter and \
               forwards it to `Engine::handle_key` instead of hardcoding \
               `false`; `handle_vscode_key`'s `\"Alt_Up\" if ctrl` arm (above \
               the unguarded move-line arm) calls `vscode_add_cursor_above`. \
               Covered by this file's own \
               ctrl_alt_up_has_a_distinct_wire_representation_from_alt_up \
               (asserts the fix directly, now ungated since #1744 landed) \
               and `render::alt_key_router_tests::\
               ctrl_alt_up_down_add_a_cursor_distinct_from_plain_alt_up_down`.",
    },
    VscodeBinding {
        command_id: "editor.action.insertCursorBelow",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Alt+Down",
        vimcode_key: "Alt_Down (ctrl)",
        status: Status::Matches,
        note: "Same fix as insertCursorAbove, mirrored for Down.",
    },
    VscodeBinding {
        command_id: "editor.action.copyLinesUpAction",
        os: Os::WinLinux,
        vscode_chord: "Shift+Alt+Up",
        vimcode_key: "Alt_Shift_Up",
        status: Status::Matches,
        note: "`Alt_Shift_Up` now dispatches to the new \
               `Engine::vscode_copy_line_up` (duplicate the line upward) \
               instead of `vscode_add_cursor_above` — Ctrl+Alt+Up owns that \
               behaviour now, matching VS Code's real chord for it (see \
               insertCursorAbove above).",
    },
    VscodeBinding {
        command_id: "editor.action.copyLinesDownAction",
        os: Os::WinLinux,
        vscode_chord: "Shift+Alt+Down",
        vimcode_key: "Alt_Shift_Down",
        status: Status::Matches,
        note: "Same swap as copyLinesUpAction, mirrored for Down \
               (`Engine::vscode_copy_line_down`).",
    },
    VscodeBinding {
        command_id: "workbench.action.navigateBack",
        os: Os::WinLinux,
        vscode_chord: "Alt+Left",
        vimcode_key: "Alt_Left",
        status: Status::Matches,
        note: "`route_alt_key`'s mode-independent tier no longer claims \
               `AltBase::Left` unconditionally — it falls through to the \
               VSCode-mode tier when `engine.is_vscode_mode()`, which now \
               maps plain Alt+Left to `\"Alt_Left\"` and \
               `handle_vscode_key` calls `Engine::jump_list_back`, the same \
               function Vim mode's Ctrl-O already uses. Ctrl+**Shift**+Alt+\
               Left is VSCode mode's new alternate home for the \
               sidebar-resize this chord used to always perform (Vim mode \
               keeps plain Alt+Left/Right for that, unchanged) — plain \
               Ctrl+Alt+Left could not be that home: it is already the \
               shipped `panel_keys.nav_back` global accelerator \
               (`Settings::PanelKeys`, `\"<C-A-Left>\"`), claimed by \
               quadraui's accelerator tier *above* `route_alt_key`, which \
               replaces the matched key event rather than letting it fall \
               through (see `route_alt_key`'s own doc).",
    },
    VscodeBinding {
        command_id: "workbench.action.navigateForward",
        os: Os::WinLinux,
        vscode_chord: "Alt+Right",
        vimcode_key: "Alt_Right",
        status: Status::Matches,
        note: "Same as navigateBack, mirrored for `jump_list_forward` / \
               Ctrl-I; Ctrl+Shift+Alt+Right is the sidebar-resize alternate \
               home, for the same `panel_keys.nav_forward` \
               (`\"<C-A-Right>\"`) reason plain Ctrl+Alt+Right couldn't be.",
    },
    VscodeBinding {
        command_id: "editor.action.jumpToBracket",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+\\",
        vimcode_key: "Shift_backslash",
        status: Status::Matches,
        note: "`App::handle_dispatch`'s `Key::Char` arm — the real \
               production decode both backends share, NOT \
               `render::engine_key_from_ui` (see that function's own module \
               doc: its `Key::Char` arm has no production caller) — now has \
               a shift-aware special case for Ctrl+\\: the literal shifted \
               glyph `'|'` already passed straight through unchanged, and a \
               new one-line arm resolves the base key plus an explicit \
               Shift bit (kitty/CSI-u) to `\"Shift_backslash\"` too, \
               distinct from plain Ctrl+\\'s `\"backslash\"`/`\"\\\\\"` \
               (still bound to `open_editor_group`, Vim mode's own Ctrl+\\ \
               meaning, unaffected). `handle_vscode_key`'s new \
               `\"Shift_backslash\" | \"|\"` arm calls \
               `Engine::move_to_matching_bracket` — the same search Vim's \
               `%` already uses. A legacy (non-keyboard-enhanced) terminal \
               still cannot report the Shift bit for this chord at all \
               (same ANSI-C0 shift-blindness as the Ctrl+K/Ctrl+P family — \
               see REACHABILITY_TABLE's new row below), so it stays \
               unreachable there; kitty/CSI-u and GTK can both deliver it.",
    },
    VscodeBinding {
        command_id: "editor.action.moveLinesUpAction",
        os: Os::WinLinux,
        vscode_chord: "Alt+Up",
        vimcode_key: "Alt_Up",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_move_line_up.",
    },
    VscodeBinding {
        command_id: "editor.action.moveLinesDownAction",
        os: Os::WinLinux,
        vscode_chord: "Alt+Down",
        vimcode_key: "Alt_Down",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_move_line_down.",
    },
    VscodeBinding {
        command_id: "editor.action.addSelectionToNextFindMatch",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+D",
        vimcode_key: "d",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_ctrl_d_selects_word, \
               test_vscode_ctrl_d_adds_next_occurrence, \
               test_vscode_ctrl_d_no_word_noop, \
               test_vscode_ctrl_d_at_word_start, \
               test_vscode_ctrl_d_then_type_replaces_all, and \
               test_vscode_ctrl_d_then_backspace_deletes_all.",
    },
    VscodeBinding {
        command_id: "editor.action.selectHighlights",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+L",
        vimcode_key: "L",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_ctrl_shift_l_selects_all, \
               test_vscode_ctrl_shift_l_then_type_replaces_all, and \
               test_vscode_ctrl_shift_l_multiline_cursor_mid_word.",
    },
    VscodeBinding {
        command_id: "editor.action.deleteLines",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+K",
        vimcode_key: "K",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_delete_line. \
               Real-input reachability is a *separate* question — see \
               REACHABILITY_TABLE's `ctrl_shift_k_vs_ctrl_k` row.",
    },
    VscodeBinding {
        command_id: "editor.action.insertLineAfter",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Enter",
        vimcode_key: "Return (ctrl)",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_insert_line_below.",
    },
    VscodeBinding {
        command_id: "editor.action.insertLineBefore",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+Enter",
        vimcode_key: "Shift_Return",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_insert_line_above.",
    },
    VscodeBinding {
        command_id: "editor.action.indentLines",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+]",
        vimcode_key: "bracketright",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_indent_single_line.",
    },
    VscodeBinding {
        command_id: "editor.action.outdentLines",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+[",
        vimcode_key: "bracketleft",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_outdent_single_line.",
    },
    VscodeBinding {
        command_id: "editor.fold",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+[",
        vimcode_key: "Shift_bracketleft",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_ctrl_shift_bracket_left_folds_from_header.",
    },
    VscodeBinding {
        command_id: "editor.unfold",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+]",
        vimcode_key: "Shift_bracketright",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_ctrl_shift_bracket_right_unfolds_region.",
    },
    VscodeBinding {
        command_id: "workbench.action.gotoLine",
        os: Os::Mac,
        vscode_chord: "Ctrl+G",
        vimcode_key: "g",
        status: Status::Matches,
        note: "VS Code deliberately keeps this Ctrl+G even on Mac (Cmd+G is \
               \"Find Next\"), so vimcode's single Ctrl-only binding already \
               matches every OS without a Cmd row at all — the rare case \
               where the naive \"Ctrl on Linux/Win, Cmd on Mac\" rule is \
               wrong *in VS Code's own favour* for vimcode's terminal build.",
    },
    VscodeBinding {
        command_id: "workbench.action.terminal.toggleTerminal",
        os: Os::Mac,
        vscode_chord: "Ctrl+`",
        vimcode_key: "grave",
        status: Status::Matches,
        note: "Same Mac exception as gotoLine: Cmd+` is reserved by macOS \
               for \"cycle windows of the frontmost app\", so VS Code's own \
               Mac default for this command stays Ctrl+`, which is exactly \
               what vimcode's single Ctrl-only binding already produces.",
    },
    VscodeBinding {
        command_id: "workbench.action.quickOpen",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+P",
        vimcode_key: "p",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_ctrl_p_fuzzy_finder.",
    },
    VscodeBinding {
        command_id: "workbench.action.showCommands",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+P",
        vimcode_key: "P",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_ctrl_shift_p_command_palette.",
    },
    // ── Fixed by #1745: plain Ctrl-to-Cmd substitutions on the macOS GUI ──
    // `normalize_mac_cmd_as_ctrl` (`src/app.rs`) folds `Modifiers::cmd`
    // into `ctrl` whenever the live backend reports `platform_name() ==
    // "macos"`, so every row below now matches on that backend too. On
    // macOS *TUI* these stay Ctrl-only per this issue's own decision (Cmd
    // never reaches a terminal).
    VscodeBinding {
        command_id: "workbench.action.toggleSidebarVisibility",
        os: Os::Mac,
        vscode_chord: "Cmd+B",
        vimcode_key: "b (Ctrl or Cmd)",
        status: Status::Missing,
        note: "KNOWN_GAPS::PANEL_ACCELERATOR_CMD_CHORDS_DEAD_ON_MACOS_GUI. \
               `normalize_mac_cmd_as_ctrl`'s fold does reach `Engine::\
               handle_vscode_key`'s \"b\" arm (`EngineAction::ToggleSidebar`) \
               on the macOS GUI the same as Ctrl+B elsewhere — but that is \
               not the chord that actually works. `render::\
               apply_engine_action`'s `ToggleSidebar` arm is a bare \
               `app.draw_needed.set(true)`; the real toggle \
               (`Engine::toggle_sidebar`) only ever runs via the *separate* \
               `panel_keys` accelerator (`DeferredAction::ToggleSidebar`), \
               registered as `quadraui::KeyBinding::Literal(pk.\
               toggle_sidebar)` (default `\"<C-b>\"`). quadraui's own \
               `macos_universal_binding_modifiers` (`macos/backend.rs`) \
               deliberately leaves `Literal` bindings untouched for Mac's \
               own Cmd convention, so that accelerator matches a physical \
               Ctrl+B and never a Cmd+B — and quadraui's shared \
               `runtime::preprocess_event` runs that accelerator match \
               *before* an unmatched keypress ever reaches \
               `App::handle_dispatch`, where this issue's Cmd-fold lives. \
               So on Linux/GTK the chord VS Code documents (Ctrl+B) really \
               does toggle the sidebar (via the accelerator, not the \
               engine-key path above); on the macOS GUI the chord VS Code \
               documents (Cmd+B) does not toggle anything, because the fold \
               runs too late to ever reach that Ctrl-literal accelerator. \
               `cmd_b_does_not_toggle_sidebar_dead_panel_accelerator` \
               (`src/macos/mod.rs`) is the RED proof, driven through the \
               real production dispatch path (not `Engine::handle_key` \
               alone, which can't see this — the defect is in \
               `render::apply_engine_action`, `pub(crate)` and only \
               reachable from inside the `vimcode_core` crate, not this \
               separate integration-test crate); its sibling \
               `ctrl_b_toggle_sidebar_proves_sidebar_visible_is_a_real_\
               paint_oracle` confirms the working Ctrl+B path on the same \
               fixture, ruling out a broken test rather than a real gap. \
               This is the general shape for every chord whose only real \
               implementation is a `panel_keys` accelerator \
               (`toggle_sidebar`, `focus_explorer`, `focus_search`, \
               `terminal_toggle_max`, `focus_notifications`) — see \
               `REACHABILITY_TABLE`'s matching row below. Needs its own \
               follow-up vimcode issue (not quadraui-blocked: the fix is a \
               vimcode-side dispatch-ordering change, e.g. having \
               `apply_engine_action`'s `ToggleSidebar` arm call \
               `Engine::toggle_sidebar` directly instead of relying on the \
               accelerator).",
    },
    VscodeBinding {
        command_id: "workbench.action.togglePanel",
        os: Os::Mac,
        vscode_chord: "Cmd+J",
        vimcode_key: "j (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Same fold as toggleSidebarVisibility, but \"j\" calls \
               `Engine::toggle_terminal()` directly (no `EngineAction` \
               indirection), so it does not share that row's pre-existing \
               dispatch gap.",
    },
    VscodeBinding {
        command_id: "workbench.action.openSettings",
        os: Os::Mac,
        vscode_chord: "Cmd+,",
        vimcode_key: "comma (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Same fold as toggleSidebarVisibility; also the universal \
               macOS \"Preferences\" chord in every native Mac app.",
    },
    VscodeBinding {
        command_id: "editor.action.commentLine",
        os: Os::Mac,
        vscode_chord: "Cmd+/",
        vimcode_key: "slash (Ctrl or Cmd)",
        status: Status::Matches,
        note: "#1745: `MacDriver`-proven — `src/macos/mod.rs`'s \
               `cmd_slash_toggles_line_comment` drives a real Cmd+/ \
               keypress through `App::handle_dispatch` and asserts the \
               painted line now reads \"# print(1)\".",
    },
    VscodeBinding {
        command_id: "actions.find",
        os: Os::Mac,
        vscode_chord: "Cmd+F",
        vimcode_key: "f (Ctrl or Cmd)",
        status: Status::Matches,
        note: "#1745: `MacDriver`-proven — `src/macos/mod.rs`'s \
               `cmd_f_opens_find` asserts `engine.find_replace_open` flips \
               after a real Cmd+F keypress.",
    },
    VscodeBinding {
        command_id: "editor.action.clipboardCutAction",
        os: Os::Mac,
        vscode_chord: "Cmd+X",
        vimcode_key: "x (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Same fold as toggleSidebarVisibility (also Cmd+C/Cmd+V/Cmd+A \
               for copy/paste/select-all, and Cmd+Z/Cmd+Shift+Z for undo/ \
               redo).",
    },
    // ── Still a gap: not a plain substitution, blocked upstream ─────────
    VscodeBinding {
        command_id: "editor.action.startFindReplaceAction",
        os: Os::Mac,
        vscode_chord: "Cmd+Option+F",
        vimcode_key: "h (Ctrl only)",
        status: Status::Missing,
        note: "NOT a Ctrl-to-Cmd substitution: Cmd+H is macOS's system-wide \
               \"Hide application\" chord, so VS Code's Mac default for \
               Find & Replace is a different chord entirely \
               (Cmd+Option+F), not Cmd+H. KNOWN_GAPS::CMD_OPTION_F_FIND_\
               REPLACE_UNREACHABLE — blocked upstream in quadraui, not a \
               vimcode-side binding gap: quadraui's own macOS translator \
               (`macos/events.rs::ns_key_to_uievent`) resolves a printable \
               key's character from `NSEvent.characters()`, which is \
               Option's *layout-remapped* glyph (Option+F -> 'ƒ', U+0192 \
               on a US keyboard) rather than the base letter — confirmed by \
               reading that function directly. `Key::Char('\u{192}')` \
               matches no arm in `handle_vscode_key`, ctrl or not, so \
               Cmd+Option+F does nothing on a real Mac today. See \
               docs/PENDING_QUADRAUI_ISSUES.md.",
    },
    VscodeBinding {
        command_id: "cursorWordEndRight / cursorWordLeft",
        os: Os::Mac,
        vscode_chord: "Option+Right / Option+Left",
        vimcode_key: "Right/Left (ctrl synthesized from alt on macOS)",
        status: Status::Matches,
        note: "#1745: NOT a Ctrl-to-Cmd substitution — on Mac, word-wise \
               navigation is Option+arrow, while Cmd+arrow is \
               cursorHome/cursorEnd (line start/end); the two modifiers \
               swap roles relative to Linux/Windows' Ctrl=word, \
               Home/End=line split. `normalize_mac_cmd_as_ctrl` \
               translates `Key::Named(Left|Right)` with `alt && !cmd` into \
               the same `ctrl == true` shape Linux/Windows' Ctrl+Left/Right \
               already produces (and clears `alt` so `route_alt_key` does \
               not also claim the chord as `navigateBack`/`navigateForward` \
               — Mac's real chord for that is Ctrl+-/Ctrl+Shift+-, a \
               separate still-open gap below). `MacDriver`-proven: \
               `src/macos/mod.rs`'s \
               `option_right_moves_word_forward_not_navigate_forward`.",
    },
    VscodeBinding {
        command_id: "cursorTop / cursorBottom",
        os: Os::Mac,
        vscode_chord: "Cmd+Up / Cmd+Down",
        vimcode_key: "Home/End (ctrl synthesized, translated from Up/Down on macOS)",
        status: Status::Matches,
        note: "#1745: Mac's document-start/end chord is Cmd+Up/Down, \
               mirroring the cursorWordEndRight/cursorWordLeft split above. \
               vimcode has no Ctrl+Up/Down binding on Linux/Windows (`Engine::\
               handle_vscode_key`'s ctrl match has no \"Up\"/\"Down\" arm) \
               but does have Ctrl+Home/Ctrl+End (document start/end) — \
               `normalize_mac_cmd_as_ctrl` translates `Key::Named(Up|\
               Down)` with `cmd && !alt` into `Key::Named(Home|End)` with \
               `ctrl == true`, reusing that existing binding rather than \
               inventing a new one. `MacDriver`-proven: `src/macos/mod.rs`'s \
               `cmd_down_moves_to_document_end_not_one_line` (and the \
               sibling `cmd_right_moves_to_line_end_not_one_column`, for \
               Cmd+Left/Right's own Home/End-without-ctrl translation).",
    },
    VscodeBinding {
        command_id: "workbench.action.navigateBack / navigateForward",
        os: Os::Mac,
        vscode_chord: "Ctrl+- / Ctrl+Shift+-",
        vimcode_key: "(no \"-\" binding in handle_vscode_key at all)",
        status: Status::Missing,
        note: "Per this issue's own brief, VS Code's real Mac default for \
               navigateBack/navigateForward is Ctrl+-/Ctrl+Shift+- — NOT \
               plain Alt+Left/Right (that is Win/Linux's default, already \
               `Matches` above, and #1745's `normalize_mac_cmd_as_ctrl` \
               deliberately reroutes Mac's plain Option+Left/Right to \
               word-move instead, clearing `alt` so `route_alt_key` cannot \
               also claim it as navigate-back/forward on this backend — see \
               that row's note). KNOWN_GAPS::\
               CTRL_MINUS_NAVIGATE_BACK_FORWARD_ON_MAC_UNVERIFIED: left \
               unimplemented in vimcode rather than guessed at, because \
               confirming the exact chord against VS Code's own \
               `keybindings.json` needs a live web lookup this session did \
               not have access to; confirmed only that vimcode has no \"-\" \
               binding at all today (`grep -n '\"-\"' src/core/engine/\
               vscode.rs` is empty), so Ctrl+-/Ctrl+Shift+- are both no-ops \
               on every backend, Mac included. Needs its own follow-up \
               vimcode issue once the real chord is verified — not \
               quadraui-blocked, since `-` is an ordinary `Key::Char` no \
               different from the other symbol chords this issue fixed.",
    },

    // ═════════════════════════════════════════════════════════════════════
    // #1746: expand the table to VS Code's full default editor/workbench
    // set. Every row below is cross-referenced directly against
    // `src/core/engine/vscode.rs`'s own match arms (grepped, not recalled
    // from memory) — see this file's own closing report (in the PR/commit
    // body, not duplicated here) for the handful of real mismatches these
    // rows turned up, each gated in KNOWN_GAPS below.
    // ═════════════════════════════════════════════════════════════════════

    // ── Clipboard / undo / select-all (Win/Linux) ──────────────────────────
    VscodeBinding {
        command_id: "editor.action.clipboardCopyAction",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+C",
        vimcode_key: "c (ctrl)",
        status: Status::Matches,
        note: "`handle_vscode_key`'s ctrl `\"c\"` arm calls `vscode_copy`. \
               Covered by this file's own \
               test_vscode_ctrl_c_copy_no_selection_copies_current_line.",
    },
    VscodeBinding {
        command_id: "editor.action.clipboardCutAction",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+X",
        vimcode_key: "x (ctrl)",
        status: Status::Matches,
        note: "`\"x\"` arm calls `vscode_cut`. Covered by this file's own \
               test_vscode_ctrl_x_cut_no_selection_cuts_current_line.",
    },
    VscodeBinding {
        command_id: "editor.action.clipboardPasteAction",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+V",
        vimcode_key: "v (ctrl)",
        status: Status::Matches,
        note: "`\"v\"` arm calls `vscode_paste`. Covered by this file's own \
               test_vscode_ctrl_v_pastes_copied_text.",
    },
    VscodeBinding {
        command_id: "undo",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Z",
        vimcode_key: "z (ctrl)",
        status: Status::Matches,
        note: "`\"z\"` arm calls `Engine::undo`. Covered by \
               tests/vscode_mode.rs::test_vscode_undo_redo_still_works.",
    },
    VscodeBinding {
        command_id: "redo",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Y",
        vimcode_key: "y (ctrl)",
        status: Status::Matches,
        note: "`\"y\"` arm calls `Engine::redo`. Covered by \
               tests/vscode_mode.rs::test_vscode_undo_redo_still_works. \
               (VS Code's own Mac default for redo is Cmd+Shift+Z, NOT \
               Cmd+Y — see KNOWN_GAPS::CMD_SHIFT_Z_REDO_UNREACHABLE_ON_MAC \
               below for why the generic Cmd-to-Ctrl fold does not cover \
               this one.)",
    },
    VscodeBinding {
        command_id: "editor.action.selectAll",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+A",
        vimcode_key: "a (ctrl)",
        status: Status::Matches,
        note: "`\"a\"` arm calls `vscode_select_all`. Covered by this \
               file's own test_vscode_ctrl_a_select_all.",
    },

    // ── Word / document cursor movement + selection (Win/Linux) ────────────
    VscodeBinding {
        command_id: "cursorWordRight",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Right",
        vimcode_key: "Right (ctrl)",
        status: Status::Matches,
        note: "Covered by this file's own \
               test_vscode_ctrl_right_moves_word_forward_and_clears_selection.",
    },
    VscodeBinding {
        command_id: "cursorWordLeft",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Left",
        vimcode_key: "Left (ctrl)",
        status: Status::Matches,
        note: "Covered by this file's own \
               test_vscode_ctrl_left_moves_word_backward.",
    },
    VscodeBinding {
        command_id: "cursorTop",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Home",
        vimcode_key: "Home (ctrl)",
        status: Status::Matches,
        note: "Covered by this file's own \
               test_vscode_ctrl_home_moves_to_document_start.",
    },
    VscodeBinding {
        command_id: "cursorBottom",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+End",
        vimcode_key: "End (ctrl)",
        status: Status::Matches,
        note: "Covered by this file's own \
               test_vscode_ctrl_end_moves_to_document_end.",
    },
    VscodeBinding {
        command_id: "cursorWordRightSelect",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+Right",
        vimcode_key: "Shift_Right (ctrl)",
        status: Status::Matches,
        note: "Not in the Ctrl+K/P shift-blind family above: Ctrl+Shift+ \
               Arrow carries an explicit CSI modifier parameter even on a \
               legacy terminal (arrow keys are never ANSI C0 codes), so \
               this is reachable everywhere Ctrl+Arrow is. Covered by this \
               file's own \
               test_vscode_ctrl_shift_right_extends_selection_by_word.",
    },
    VscodeBinding {
        command_id: "cursorWordLeftSelect",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+Left",
        vimcode_key: "Shift_Left (ctrl)",
        status: Status::Matches,
        note: "Same CSI-param reachability as cursorWordRightSelect. \
               Covered by this file's own \
               test_vscode_ctrl_shift_left_extends_selection_by_word.",
    },
    // ── #1746 review correction: these four rows previously claimed
    //    `Status::Matches` on the strength of tests that feed
    //    `handle_key("Shift_Home"/"BackSpace"/..., None, true)` directly —
    //    triples the one shared decoder (`render::engine_key_from_ui`) can
    //    never actually emit from a real keypress, on any backend. See each
    //    row's own KNOWN_GAPS gate below for the exact match-arm proof. ───
    VscodeBinding {
        command_id: "cursorTopSelect",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+Home",
        vimcode_key: "(decodes identically to plain Shift+Home, ctrl bit lost)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_SHIFT_HOME_END_CTRL_BIT_DROPPED_BY_DECODER. \
               `render::engine_key_from_ui`'s `NamedKey::Home if shift` arm \
               is `Some((\"Shift_Home\".to_string(), None, false))` — it \
               matches on `shift` alone and returns unconditional \
               `ctrl: false`, *before* the ctrl-aware `NamedKey::Home => \
               (\"Home\", None, ctrl)` arm beneath it ever runs. So a real \
               Ctrl+Shift+Home keypress, on GTK or TUI (kitty or legacy — \
               this is not a legacy-terminal-only gap, unlike the Ctrl+K \
               family), decodes exactly like plain Shift+Home: \
               `handle_vscode_key`'s ctrl=false arm routes `\"Shift_Home\"` \
               to `vscode_extend_selection(\"SmartHome\")` (select to \
               smart line start) instead of the ctrl=true arm's \
               `vscode_extend_selection(\"DocStart\")` (select to document \
               start) that `test_vscode_ctrl_shift_home_extends_selection_\
               to_doc_start` exercises directly via `handle_key`. The same \
               arm also breaks VS Code Mac's `cursorTopSelect` (Cmd+Shift+ \
               Up): `normalize_mac_cmd_as_ctrl` (`src/app.rs`) translates \
               that chord to `Home` with `ctrl: true, shift: true` and \
               forwards it into this same decoder arm, which drops the \
               ctrl bit identically. See this gap's driver-tier RED proof \
               below.",
    },
    VscodeBinding {
        command_id: "cursorBottomSelect",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+End",
        vimcode_key: "(decodes identically to plain Shift+End, ctrl bit lost)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_SHIFT_HOME_END_CTRL_BIT_DROPPED_BY_DECODER. \
               Mirrors cursorTopSelect above — `NamedKey::End if shift`'s \
               arm has the identical unconditional-`false` shape, so a real \
               Ctrl+Shift+End decodes like plain Shift+End \
               (`vscode_extend_selection(\"LineEnd\")`) instead of \
               `DocEnd`.",
    },
    VscodeBinding {
        command_id: "deleteWordRight",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Delete",
        vimcode_key: "(decodes identically to plain Delete, ctrl bit lost)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_BACKSPACE_DELETE_CTRL_BIT_DROPPED_BY_DECODER. \
               `render::engine_key_from_ui`'s `NamedKey::Delete` arm is \
               `Some((\"Delete\".to_string(), None, false))` — it never \
               inspects the incoming `ctrl` flag at all, unlike every \
               other named key with a ctrl-sensitive arm. So a real \
               Ctrl+Delete keypress, on GTK or TUI, decodes exactly like \
               plain Delete: `handle_vscode_key`'s ctrl=false `\"Delete\"` \
               arm deletes one character under the cursor instead of \
               calling `vscode_delete_word_forward` (the ctrl=true arm \
               `test_vscode_ctrl_delete_deletes_word_forward` exercises \
               directly via `handle_key`, bypassing the decoder). On a \
               legacy (non-kitty) terminal this is additionally confusable \
               with `^?`/`^H`-style control bytes depending on the \
               terminal's own Delete encoding, but the decoder bug above \
               means even kitty/GTK/macOS GUI — which *can* report ctrl \
               correctly — never get the chance to.",
    },
    VscodeBinding {
        command_id: "deleteWordLeft",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+BackSpace",
        vimcode_key: "(decodes identically to plain BackSpace, ctrl bit lost)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_BACKSPACE_DELETE_CTRL_BIT_DROPPED_BY_DECODER. \
               Mirrors deleteWordRight above — `NamedKey::Backspace`'s arm \
               has the identical unconditional-`false` shape, so a real \
               Ctrl+BackSpace decodes like plain BackSpace (deletes one \
               char, not a word) on GTK/TUI-kitty/macOS GUI. On a legacy \
               (non-kitty) terminal, Ctrl+Backspace conventionally arrives \
               as the C0 byte `^H` (0x08), which vimcode's own legacy-byte \
               handling resolves to ctrl+`\"h\"` — opening find-and-replace \
               — rather than either Backspace behaviour; this file's own \
               driver-tier gate below pins the GTK/kitty-side decoder bug, \
               which is the reproducible, surface-independent half of this \
               gap. VS Code's own Mac default for this command is Option+ \
               Backspace, NOT a Cmd substitution — see \
               KNOWN_GAPS::OPTION_BACKSPACE_DELETE_WORD_LEFT_UNBOUND_ON_MAC \
               below.",
    },

    // ── Plain (unmodified) cursor movement + Shift-extend (Win/Linux/Mac,
    //    identical — no modifier divergence) ───────────────────────────────
    VscodeBinding {
        command_id: "cursorRight",
        os: Os::WinLinux,
        vscode_chord: "Right",
        vimcode_key: "Right",
        status: Status::Matches,
        note: "The non-ctrl, non-shift arm of `handle_vscode_key`'s plain \
               match: `vscode_clear_selection()` then `move_right_insert()`. \
               New test: test_vscode_plain_right_clears_selection_and_moves.",
    },
    VscodeBinding {
        command_id: "cursorLeft",
        os: Os::WinLinux,
        vscode_chord: "Left",
        vimcode_key: "Left",
        status: Status::Matches,
        note: "Same plain arm, `move_left()`. New test: \
               test_vscode_plain_left_moves_cursor_left.",
    },
    VscodeBinding {
        command_id: "cursorUp",
        os: Os::WinLinux,
        vscode_chord: "Up",
        vimcode_key: "Up",
        status: Status::Matches,
        note: "Same plain arm, `vscode_do_move(\"Up\")`. New test: \
               test_vscode_plain_up_moves_cursor_up.",
    },
    VscodeBinding {
        command_id: "cursorDown",
        os: Os::WinLinux,
        vscode_chord: "Down",
        vimcode_key: "Down",
        status: Status::Matches,
        note: "Same plain arm, `vscode_do_move(\"Down\")`. New test: \
               test_vscode_plain_down_moves_cursor_down.",
    },
    VscodeBinding {
        command_id: "cursorHome",
        os: Os::WinLinux,
        vscode_chord: "Home",
        vimcode_key: "Home",
        status: Status::Matches,
        note: "`vscode_smart_home` toggles between first-non-whitespace and \
               column 0, matching VS Code's own `cursorHome` smart-home \
               behaviour exactly. New test: \
               test_vscode_plain_home_smart_toggle.",
    },
    VscodeBinding {
        command_id: "cursorEnd",
        os: Os::WinLinux,
        vscode_chord: "End",
        vimcode_key: "End",
        status: Status::Matches,
        note: "Same plain arm, moves to `get_line_len_for_insert`. New \
               test: test_vscode_plain_end_moves_to_line_end.",
    },
    VscodeBinding {
        command_id: "cursorPageUp",
        os: Os::WinLinux,
        vscode_chord: "Page_Up",
        vimcode_key: "Page_Up",
        status: Status::Matches,
        note: "Covered by this file's own \
               test_vscode_page_up_moves_cursor_up_a_viewport.",
    },
    VscodeBinding {
        command_id: "cursorPageDown",
        os: Os::WinLinux,
        vscode_chord: "Page_Down",
        vimcode_key: "Page_Down",
        status: Status::Matches,
        note: "Covered by this file's own \
               test_vscode_page_down_moves_cursor_down_a_viewport. VS \
               Code's `cursorPageUpSelect`/`cursorPageDownSelect` \
               (Shift+Page_Up/Down) are a separate, real gap — see \
               KNOWN_GAPS::SHIFT_PAGEUP_PAGEDOWN_SHIFT_BIT_DROPPED_BY_\
               DECODER below.",
    },
    VscodeBinding {
        command_id: "cursorRightSelect",
        os: Os::WinLinux,
        vscode_chord: "Shift+Right",
        vimcode_key: "Shift_Right",
        status: Status::Matches,
        note: "`key_name.starts_with(\"Shift_\")` arm, no ctrl: \
               `vscode_extend_selection(\"Right\")`. Covered by this \
               file's own \
               test_vscode_shift_right_extends_selection_without_ctrl.",
    },
    VscodeBinding {
        command_id: "cursorLeftSelect",
        os: Os::WinLinux,
        vscode_chord: "Shift+Left",
        vimcode_key: "Shift_Left",
        status: Status::Matches,
        note: "Same Shift_ arm, `vscode_extend_selection(\"Left\")`. New \
               test: test_vscode_shift_left_extends_selection_without_ctrl.",
    },
    VscodeBinding {
        command_id: "cursorUpSelect",
        os: Os::WinLinux,
        vscode_chord: "Shift+Up",
        vimcode_key: "Shift_Up",
        status: Status::Matches,
        note: "Same Shift_ arm, `vscode_extend_selection(\"Up\")`. New \
               test: test_vscode_shift_up_extends_selection_without_ctrl.",
    },
    VscodeBinding {
        command_id: "cursorDownSelect",
        os: Os::WinLinux,
        vscode_chord: "Shift+Down",
        vimcode_key: "Shift_Down",
        status: Status::Matches,
        note: "Covered by this file's own \
               test_vscode_shift_down_extends_selection_without_ctrl.",
    },
    VscodeBinding {
        command_id: "cursorHomeSelect",
        os: Os::WinLinux,
        vscode_chord: "Shift+Home",
        vimcode_key: "Shift_Home",
        status: Status::Matches,
        note: "Same Shift_ arm, `vscode_extend_selection(\"SmartHome\")`. \
               New test: \
               test_vscode_shift_home_extends_selection_without_ctrl.",
    },
    VscodeBinding {
        command_id: "cursorEndSelect",
        os: Os::WinLinux,
        vscode_chord: "Shift+End",
        vimcode_key: "Shift_End",
        status: Status::Matches,
        note: "Same Shift_ arm, `vscode_extend_selection(\"LineEnd\")`. New \
               test: test_vscode_shift_end_extends_selection_without_ctrl.",
    },

    // ── Editing / palette / panels (Win/Linux) ─────────────────────────────
    VscodeBinding {
        command_id: "workbench.action.quit",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Q",
        vimcode_key: "q (ctrl)",
        status: Status::Matches,
        note: "VS Code's own Linux default (Windows has no default binding \
               for this chord — Alt+F4 is the OS convention there instead, \
               so vimcode binding Ctrl+Q on Windows too is a harmless \
               superset, not a mismatch). Covered by this file's own \
               test_vscode_ctrl_q_quits_when_no_unsaved_changes.",
    },
    VscodeBinding {
        command_id: "editor.action.commentLine",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+/",
        vimcode_key: "slash (ctrl)",
        status: Status::Matches,
        note: "Covered by this file's own \
               test_vscode_ctrl_slash_toggles_line_comment.",
    },
    VscodeBinding {
        command_id: "editor.action.expandLineSelection",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+L",
        vimcode_key: "l (ctrl)",
        status: Status::Matches,
        note: "`\"l\"` arm calls `vscode_select_line`, which expands \
               further on repeated presses exactly like VS Code's \
               `expandLineSelection`. Covered by \
               tests/vscode_mode.rs::test_vscode_select_line and \
               test_vscode_select_line_extends.",
    },
    VscodeBinding {
        command_id: "workbench.action.gotoLine",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+G",
        vimcode_key: "g (ctrl)",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_ctrl_g_goto_line \
               (same binding as the Os::Mac row above — VS Code uses this \
               chord unchanged on every OS).",
    },
    VscodeBinding {
        command_id: "workbench.action.toggleSidebarVisibility",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+B",
        vimcode_key: "b (ctrl, via panel_keys accelerator)",
        status: Status::Matches,
        note: "On Linux/GTK and TUI the real toggle happens via the \
               `panel_keys.toggle_sidebar` accelerator (default `\"<C-b>\"`, \
               `DeferredAction::ToggleSidebar`), matched *before* the \
               keypress reaches `handle_vscode_key`'s own `\"b\"` arm — see \
               the `Cmd+B` row above for the full dispatch-ordering \
               writeup. The end result is still correct on this platform \
               (unlike macOS GUI's Cmd+B): a physical Ctrl+B really does \
               toggle the sidebar. Covered by \
               tests/vscode_mode.rs::test_vscode_ctrl_b_toggle_sidebar \
               (engine-level: asserts `handle_vscode_key`'s own \
               `EngineAction::ToggleSidebar` return, not the accelerator \
               path, which this separate integration-test crate cannot \
               reach — see the `Cmd+B` row's note on why that distinction \
               matters).",
    },
    VscodeBinding {
        command_id: "workbench.action.togglePanel",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+J",
        vimcode_key: "j (ctrl)",
        status: Status::Matches,
        note: "`\"j\"` arm calls `Engine::toggle_terminal()` directly (no \
               accelerator indirection, unlike toggleSidebarVisibility \
               above). Covered by \
               tests/vscode_mode.rs::test_vscode_ctrl_j_toggle_terminal.",
    },
    VscodeBinding {
        command_id: "workbench.action.terminal.toggleTerminal",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+`",
        vimcode_key: "grave (ctrl)",
        status: Status::Matches,
        note: "Covered by \
               tests/vscode_mode.rs::test_vscode_ctrl_backtick_toggle_terminal \
               (same chord as the Os::Mac row above — no OS divergence \
               for this one).",
    },
    VscodeBinding {
        command_id: "workbench.action.openSettings",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+,",
        vimcode_key: "comma (ctrl)",
        status: Status::Matches,
        note: "Covered by \
               tests/vscode_mode.rs::test_vscode_ctrl_comma_settings.",
    },
    VscodeBinding {
        command_id: "editor.toggleWordWrap",
        os: Os::WinLinux,
        vscode_chord: "Alt+Z",
        vimcode_key: "Alt_z",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_alt_z_toggle_wrap.",
    },
    VscodeBinding {
        command_id: "workbench.action.showCommands",
        os: Os::WinLinux,
        vscode_chord: "F1",
        vimcode_key: "F1",
        status: Status::Matches,
        note: "F1 is VS Code's own built-in alternative to Ctrl+Shift+P — \
               see REACHABILITY_TABLE's `Ctrl+K vs Ctrl+Shift+K`-family \
               rows for why that matters on a legacy terminal. Covered by \
               this file's own test_vscode_f1_opens_command_palette.",
    },
    VscodeBinding {
        command_id: "workbench.action.toggleMenuBar",
        os: Os::WinLinux,
        vscode_chord: "F10",
        vimcode_key: "F10",
        status: Status::Matches,
        note: "Review correction (#1746): F10 is NOT actually VS Code's \
               default for `toggleMenuBar` — that command ships with no \
               default keybinding at all, and F10 is VS Code's real \
               default for `workbench.action.debug.stepOver`. vimcode's \
               own F10-toggles-menu-bar behaviour is fine on its own \
               terms, it just is not a VS Code parity claim; kept here \
               (rather than deleted) only as a record of that vimcode \
               choice, cross-referenced against this file's own \
               test_vscode_f10_toggles_menu_bar — do not cite this row as \
               evidence of VS Code parity.",
    },

    // ── #1746 deliverable 1: the previously-missing categories — save,
    //    Win/Linux find/replace (Mac already had rows for these via
    //    Cmd+F/Cmd+Option+F), and the editor-group/tab command family
    //    (Win/Linux and Mac share these chords unmodified — VS Code's own
    //    defaults have no Mac override for any of them) ───────────────────
    VscodeBinding {
        command_id: "workbench.action.files.save",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+S",
        vimcode_key: "s (ctrl)",
        status: Status::Matches,
        note: "Not inside `handle_vscode_key` at all — `Engine::handle_key`'s \
               own top-level `ctrl && key_name == \"s\"` check (`src/core/\
               engine/keys.rs`) runs before the `is_vscode_mode()` dispatch \
               and calls `save_with_format`, so this works identically in \
               every editor mode, VS Code included. New test: \
               test_vscode_ctrl_s_saves_file.",
    },
    VscodeBinding {
        command_id: "actions.find",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+F",
        vimcode_key: "f (ctrl)",
        status: Status::Matches,
        note: "`handle_vscode_key`'s `\"f\"` arm calls `open_find_replace`. \
               Previously only exercised via the `Os::Mac` `actions.find` \
               row below (Cmd+F, `MacDriver`-proven) — this Win/Linux row \
               was the deliverable-1 gap. Covered by this file's own \
               (pre-existing, previously unreferenced-by-any-row) \
               test_vscode_ctrl_f_opens_find.",
    },
    VscodeBinding {
        command_id: "editor.action.startFindReplaceAction",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+H",
        vimcode_key: "h (ctrl)",
        status: Status::Matches,
        note: "`handle_vscode_key`'s `\"h\"` arm calls `open_find_replace` \
               then sets `find_replace_show_replace = true`. Same gap as \
               actions.find above — only the `Os::Mac` row (a different \
               chord, Cmd+Option+F, currently `Missing`) existed before \
               this round. Covered by this file's own (pre-existing, \
               previously unreferenced-by-any-row) \
               test_vscode_ctrl_h_opens_find_replace.",
    },
    VscodeBinding {
        command_id: "workbench.action.quickOpenPreviousRecentlyUsedEditorInGroup",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Tab",
        vimcode_key: "Tab (ctrl)",
        status: Status::Matches,
        note: "Not inside `handle_vscode_key` either — `Engine::handle_key`'s \
               own top-level `ctrl && key_name == \"Tab\"` check (`src/core/\
               engine/keys.rs`, same pre-dispatch tier as Ctrl+S above) \
               opens (or cycles forward through) the MRU tab switcher, the \
               vimcode analogue of VS Code's 'show all editors by most \
               recently used' overlay. New test: \
               test_vscode_ctrl_tab_opens_tab_switcher.",
    },
    VscodeBinding {
        command_id: "workbench.action.closeActiveEditor",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+W",
        vimcode_key: "(unbound in VS Code mode)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_W_CLOSE_ACTIVE_EDITOR_UNBOUND_IN_VSCODE_MODE. \
               `handle_vscode_key` has no `\"w\"` arm outside the Ctrl+K \
               chord's own `\"w\"` (closeAllEditors, see that row above) — \
               a plain Ctrl+W falls into the ctrl match's `_ => {}` no-op. \
               Note this is specific to VS Code mode: the Vim-mode \
               CTRL-W-as-window-command and panel-focus CTRL-W intercepts \
               that share the same key name (`src/core/engine/keys.rs`, \
               lines 313 and 6748) sit either before or inside Vim's own \
               key-handling path, which `is_vscode_mode()`'s early return \
               (line 471) never reaches.",
    },
    VscodeBinding {
        command_id: "workbench.action.splitEditor",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+\\",
        vimcode_key: "(unbound)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_BACKSLASH_SPLIT_EDITOR_UNBOUND. \
               `handle_vscode_key`'s ctrl match has no arm for the literal \
               backslash key at all (only its Shift-modified sibling, \
               `\"Shift_backslash\" | \"|\"`, which jumps to the matching \
               bracket — see that row's REACHABILITY_TABLE entry above); \
               vimcode has no split-editor command wired to this chord in \
               VS Code mode.",
    },
    VscodeBinding {
        command_id: "workbench.action.previousEditor",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+PageUp",
        vimcode_key: "(unbound — decodes as plain Page_Up, see below)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_PAGEUP_PAGEDOWN_PREV_NEXT_EDITOR_UNBOUND. \
               Doubly unimplemented: `handle_vscode_key`'s ctrl match has \
               no `\"Page_Up\"` arm (only the non-ctrl classifier at line \
               ~1133 of `vscode.rs`, which exists purely to exclude \
               Page_Up/Page_Down from undo-group-per-keystroke accounting, \
               not to handle the ctrl chord), so there is nowhere for this \
               command to dispatch to even if the ctrl bit arrived intact; \
               and separately, `render::engine_key_from_ui`'s \
               `NamedKey::PageUp` arm hardcodes `ctrl: false` \
               unconditionally (the same arm \
               `gap_shift_pageup_pagedown_shift_bit_dropped_by_decoder` \
               already pins for the Shift bit), so a real Ctrl+PageUp \
               keypress couldn't deliver the ctrl bit anyway.",
    },
    VscodeBinding {
        command_id: "workbench.action.nextEditor",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+PageDown",
        vimcode_key: "(unbound — decodes as plain Page_Down, see above)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_PAGEUP_PAGEDOWN_PREV_NEXT_EDITOR_UNBOUND. \
               Mirrors previousEditor above, for `NamedKey::PageDown`.",
    },
    VscodeBinding {
        command_id: "workbench.action.focusFirstEditorGroup",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+1",
        vimcode_key: "(unbound)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_1_FOCUS_FIRST_EDITOR_GROUP_UNBOUND. \
               `handle_vscode_key`'s ctrl match has no digit arms at all \
               (`\"1\"` through `\"9\"`); vimcode has no editor-group-focus \
               command wired to any digit chord in VS Code mode. VS Code \
               binds Ctrl+2 through Ctrl+9 the same way for groups 2-9; \
               not enumerated as separate rows since the gap and its gate \
               are identical for all of them.",
    },
    VscodeBinding {
        command_id: "editor.foldAll",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+K Ctrl+0",
        vimcode_key: "(unbound)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_K_FOLD_ALL_UNFOLD_ALL_UNBOUND. \
               `vscode_ctrl_k_dispatch`'s match only has arms for \
               `\"c\"`/`\"u\"`/`\"w\"`/`\"f\"` (see those rows above); a \
               second-key `\"0\"` falls through to that function's own \
               `_ => false` and the chord is dropped with no action.",
    },
    VscodeBinding {
        command_id: "editor.unfoldAll",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+K Ctrl+J",
        vimcode_key: "(unbound)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_K_FOLD_ALL_UNFOLD_ALL_UNBOUND. Mirrors \
               editor.foldAll above, for `\"j\"` as the second key — note \
               this is a *different* `\"j\"` from the plain Ctrl+J \
               (togglePanel) row above: that one fires without a pending \
               Ctrl+K chord, this one would need \
               `vscode_ctrl_k_dispatch` to add its own `\"j\"` arm.",
    },
    VscodeBinding {
        command_id: "deleteLeft",
        os: Os::WinLinux,
        vscode_chord: "BackSpace",
        vimcode_key: "BackSpace",
        status: Status::Matches,
        note: "The plain (non-ctrl) `\"BackSpace\"` arm. New test: \
               test_vscode_plain_backspace_deletes_char_before_cursor.",
    },
    VscodeBinding {
        command_id: "deleteRight",
        os: Os::WinLinux,
        vscode_chord: "Delete",
        vimcode_key: "Delete",
        status: Status::Matches,
        note: "Covered by this file's own \
               test_vscode_plain_delete_removes_char_under_cursor.",
    },
    VscodeBinding {
        command_id: "type",
        os: Os::WinLinux,
        vscode_chord: "Enter",
        vimcode_key: "Return",
        status: Status::Matches,
        note: "VS Code has no dedicated command id for a plain Enter \
               keypress in a text editor — it dispatches through the \
               generic `\"type\"` command (args `{\"text\": \"\\n\"}`), the \
               same command every printable character goes through. \
               Covered by this file's own \
               test_vscode_plain_return_inserts_newline.",
    },
    VscodeBinding {
        command_id: "tab",
        os: Os::WinLinux,
        vscode_chord: "Tab",
        vimcode_key: "Tab",
        status: Status::Matches,
        note: "Covered by this file's own test_vscode_plain_tab_inserts_indent.",
    },
    VscodeBinding {
        command_id: "outdent",
        os: Os::WinLinux,
        vscode_chord: "Shift+Tab",
        vimcode_key: "ISO_Left_Tab",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_shift_tab_outdent.",
    },
    VscodeBinding {
        command_id: "hideSuggestWidget",
        os: Os::WinLinux,
        vscode_chord: "Escape",
        vimcode_key: "Escape",
        status: Status::Matches,
        note: "`\"Escape\"` arm's completion-popup-first priority. Covered \
               by tests/vscode_mode.rs::test_vscode_escape_dismisses_completion.",
    },
    VscodeBinding {
        command_id: "removeSecondaryCursors",
        os: Os::WinLinux,
        vscode_chord: "Escape",
        vimcode_key: "Escape",
        status: Status::Matches,
        note: "Same `\"Escape\"` arm, next priority tier (clears \
               `extra_cursors` when no completion popup is open). Covered \
               by tests/vscode_mode.rs::test_vscode_escape_clears_extra_cursors.",
    },
    VscodeBinding {
        command_id: "editor.action.formatDocument",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+K Ctrl+F",
        vimcode_key: "k (ctrl) then f (ctrl)",
        status: Status::Matches,
        note: "`vscode_ctrl_k_dispatch`'s `\"f\"` arm calls \
               `lsp_format_current`. Covered by \
               tests/vscode_mode.rs::test_vscode_ctrl_k_ctrl_f_format.",
    },
    VscodeBinding {
        command_id: "workbench.action.closeAllEditors",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+K Ctrl+W",
        vimcode_key: "k (ctrl) then w (ctrl)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_K_CTRL_W_LEAVES_ONE_TAB_OPEN. \
               `vscode_ctrl_k_dispatch`'s `\"w\"` arm loops `Engine::\
               close_tab`, but `close_tab` itself refuses to close the \
               last tab in a group (`src/core/engine/windows.rs`: `if \
               self.active_group().tabs.len() <= 1 { ...; return false; }`), \
               so the loop always leaves exactly one editor open plus a \
               \"Cannot close last tab\" message — VS Code's real \
               `closeAllEditors` leaves none. Previously (first #1746 \
               round) this row carried `test_vscode_ctrl_k_ctrl_w_closes_\
               all_editors_in_group`, asserting that current (wrong) \
               behaviour directly (`Some(1)`, \"should close every tab but \
               the last\") while the row itself was marked `Matches` — a \
               false parity claim. That test is now \
               `gap_ctrl_k_ctrl_w_closes_every_tab` below, asserting the \
               *correct* `Some(0)` outcome under `gap_gate` instead.",
    },

    // ── Mac: more plain Ctrl-to-Cmd substitutions, same \
    //    `normalize_mac_cmd_as_ctrl` generic fold arm as toggleSidebar\
    //    Visibility/togglePanel/openSettings/commentLine/clipboardCutAction \
    //    above — not independently re-tested at the engine level, since \
    //    the fold delivers the exact same (key_name, ctrl=true) shape the \
    //    Win/Linux rows above already exercise; see `normalize_mac_cmd_\
    //    as_ctrl`'s own doc for the `MacDriver` reachability proof. ───────
    VscodeBinding {
        command_id: "editor.action.clipboardCopyAction",
        os: Os::Mac,
        vscode_chord: "Cmd+C",
        vimcode_key: "c (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"c\" arm as the \
               Win/Linux row above.",
    },
    VscodeBinding {
        command_id: "editor.action.clipboardPasteAction",
        os: Os::Mac,
        vscode_chord: "Cmd+V",
        vimcode_key: "v (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"v\" arm as the \
               Win/Linux row above.",
    },
    VscodeBinding {
        command_id: "undo",
        os: Os::Mac,
        vscode_chord: "Cmd+Z",
        vimcode_key: "z (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"z\" arm as the \
               Win/Linux row above.",
    },
    VscodeBinding {
        command_id: "editor.action.selectAll",
        os: Os::Mac,
        vscode_chord: "Cmd+A",
        vimcode_key: "a (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"a\" arm as the \
               Win/Linux row above.",
    },
    VscodeBinding {
        command_id: "editor.action.deleteLines",
        os: Os::Mac,
        vscode_chord: "Cmd+Shift+K",
        vimcode_key: "K (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"K\" arm as the \
               Win/Linux deleteLines row above. Unlike toggleSidebar\
               Visibility's Cmd+B, this one's real implementation sits \
               inside `handle_vscode_key` itself, not a separate \
               `panel_keys` accelerator, so the fold's ordering problem \
               does not apply here.",
    },
    VscodeBinding {
        command_id: "editor.action.insertLineAfter",
        os: Os::Mac,
        vscode_chord: "Cmd+Enter",
        vimcode_key: "Return (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same ctrl-Return arm \
               as the Win/Linux insertLineAfter row above.",
    },
    VscodeBinding {
        command_id: "editor.action.insertLineBefore",
        os: Os::Mac,
        vscode_chord: "Cmd+Shift+Enter",
        vimcode_key: "Shift_Return (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same ctrl-Shift_Return \
               arm as the Win/Linux insertLineBefore row above.",
    },
    VscodeBinding {
        command_id: "editor.action.addSelectionToNextFindMatch",
        os: Os::Mac,
        vscode_chord: "Cmd+D",
        vimcode_key: "d (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"d\" arm as the \
               Win/Linux addSelectionToNextFindMatch row above.",
    },
    VscodeBinding {
        command_id: "editor.action.expandLineSelection",
        os: Os::Mac,
        vscode_chord: "Cmd+L",
        vimcode_key: "l (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"l\" arm as the \
               Win/Linux expandLineSelection row above.",
    },
    VscodeBinding {
        command_id: "editor.action.indentLines",
        os: Os::Mac,
        vscode_chord: "Cmd+]",
        vimcode_key: "bracketright (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"]\" arm as the \
               Win/Linux indentLines row above.",
    },
    VscodeBinding {
        command_id: "editor.action.outdentLines",
        os: Os::Mac,
        vscode_chord: "Cmd+[",
        vimcode_key: "bracketleft (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"[\" arm as the \
               Win/Linux outdentLines row above.",
    },
    VscodeBinding {
        command_id: "workbench.action.quickOpen",
        os: Os::Mac,
        vscode_chord: "Cmd+P",
        vimcode_key: "p (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"p\" arm as the \
               Win/Linux quickOpen row above.",
    },
    VscodeBinding {
        command_id: "workbench.action.showCommands",
        os: Os::Mac,
        vscode_chord: "Cmd+Shift+P",
        vimcode_key: "P (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"P\" arm as the \
               Win/Linux showCommands row above.",
    },
    VscodeBinding {
        command_id: "editor.action.selectHighlights",
        os: Os::Mac,
        vscode_chord: "Cmd+Shift+L",
        vimcode_key: "L (Ctrl or Cmd)",
        status: Status::Matches,
        note: "Generic Cmd-to-Ctrl fold, reaches the same \"L\" arm as the \
               Win/Linux selectHighlights row above.",
    },

    // ── Mac: real gaps turned up by this pass (#1746) ──────────────────────
    VscodeBinding {
        command_id: "redo",
        os: Os::Mac,
        vscode_chord: "Cmd+Shift+Z",
        vimcode_key: "(no \"Z\" ctrl arm at all)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CMD_SHIFT_Z_REDO_UNREACHABLE_ON_MAC. VS Code's \
               real Mac default for redo is Cmd+Shift+Z — NOT Cmd+Y, which \
               is what the generic Cmd-to-Ctrl fold would produce if redo's \
               Mac chord were a plain substitution of vimcode's Ctrl+Y. \
               `handle_vscode_key`'s ctrl match has a lowercase `\"y\"` arm \
               for redo but no `\"Z\"` (uppercase, i.e. Ctrl+Shift+Z) arm \
               at all — confirmed by reading the match directly — so \
               neither Linux/Windows' own Ctrl+Shift+Z nor the Mac GUI's \
               folded equivalent does anything today.",
    },
    VscodeBinding {
        command_id: "deleteWordLeft",
        os: Os::Mac,
        vscode_chord: "Option+BackSpace",
        vimcode_key: "(no \"Alt_BackSpace\" arm at all)",
        status: Status::Missing,
        note: "KNOWN_GAPS::OPTION_BACKSPACE_DELETE_WORD_LEFT_UNBOUND_ON_MAC. \
               VS Code's Mac default for deleteWordLeft is Option+Backspace \
               (Linux/Windows' is Ctrl+Backspace, already `Matches` above) \
               — NOT a Cmd substitution, so `normalize_mac_cmd_as_ctrl`'s \
               fold (gated on `modifiers.cmd`) never even sees it. \
               `render::alt_chord_base` (`src/render.rs`) only recognises \
               `Left`/`Right`/`Up`/`Down` and single-character keys as an \
               Alt-chord base — `NamedKey::Backspace`'s multi-character \
               name (`\"BackSpace\"`) falls through that filter entirely, \
               so `route_alt_key` returns `Fallthrough` for Alt+Backspace \
               and the chord never reaches `vscode_alt_key_name` at all; \
               even if it did, `handle_vscode_key`'s `Alt_` match has no \
               `\"Alt_BackSpace\"` arm either. Confirmed directly from both \
               functions, not guessed.",
    },
    VscodeBinding {
        command_id: "deleteWordRight",
        os: Os::Mac,
        vscode_chord: "Option+Delete",
        vimcode_key: "(no \"Alt_Delete\" arm at all)",
        status: Status::Missing,
        note: "KNOWN_GAPS::OPTION_DELETE_WORD_RIGHT_UNBOUND_ON_MAC. Mirrors \
               deleteWordLeft above for Delete — same `alt_chord_base` \
               multi-character-name gap, same missing `handle_vscode_key` \
               arm.",
    },

    // ── Real mismatches (not just unbound) found while expanding this \
    //    table — Ctrl+K's two "add"/"remove" comment sub-chords both call \
    //    the *same* toggle function, so each one does the wrong thing \
    //    whenever the line is already in the opposite state. One-line-fix \
    //    candidates (give `vscode_ctrl_k_dispatch`'s "c"/"u" arms their own \
    //    add-only/remove-only comment helpers instead of sharing \
    //    `toggle_comment`), filed as a follow-up rather than fixed here \
    //    per this issue's own scope note. ───────────────────────────────────
    VscodeBinding {
        command_id: "editor.action.addCommentLine",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+K Ctrl+C",
        vimcode_key: "k (ctrl) then c (ctrl)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_K_CTRL_C_TOGGLES_INSTEAD_OF_ADD_ONLY. \
               `vscode_ctrl_k_dispatch`'s `\"c\"` arm calls \
               `Engine::toggle_comment` — the exact same call its `\"u\"` \
               arm makes for removeCommentLine below. VS Code's \
               `addCommentLine` is idempotent (a no-op on an \
               already-commented line); vimcode's Ctrl+K Ctrl+C instead \
               *uncomments* an already-commented line, because it toggles \
               rather than only adding.",
    },
    VscodeBinding {
        command_id: "editor.action.removeCommentLine",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+K Ctrl+U",
        vimcode_key: "k (ctrl) then u (ctrl)",
        status: Status::Missing,
        note: "KNOWN_GAPS::CTRL_K_CTRL_U_TOGGLES_INSTEAD_OF_REMOVE_ONLY. \
               Mirrors addCommentLine above: `vscode_ctrl_k_dispatch`'s \
               `\"u\"` arm also calls `Engine::toggle_comment`, so VS \
               Code's `removeCommentLine` (a no-op on an uncommented line) \
               instead *adds* a comment to an uncommented line.",
    },

    // ── Decoder bug (not platform-specific — affects every backend
    //    identically, so it does not belong in REACHABILITY_TABLE's
    //    per-surface framing) found while checking Shift+Page_Up/Down ──────
    VscodeBinding {
        command_id: "cursorPageUpSelect",
        os: Os::WinLinux,
        vscode_chord: "Shift+Page_Up",
        vimcode_key: "(decodes identically to plain Page_Up, shift bit lost)",
        status: Status::Missing,
        note: "KNOWN_GAPS::SHIFT_PAGEUP_PAGEDOWN_SHIFT_BIT_DROPPED_BY_\
               DECODER. `render::engine_key_from_ui`'s `NamedKey::PageUp` \
               arm is `Some((\"Page_Up\".to_string(), None, false))` \
               unconditionally — unlike its `Home`/`End` siblings a few \
               lines above, which emit a distinct `\"Shift_Home\"`/ \
               `\"Shift_End\"` name when `shift` is set, `PageUp` never \
               even inspects `shift`. So a real Shift+Page Up keypress on \
               any backend decodes exactly like plain Page Up, and \
               `handle_vscode_key` (which has no `\"Shift_Page_Up\"` arm \
               either) has no way to ever learn Shift was held.",
    },
    VscodeBinding {
        command_id: "cursorPageDownSelect",
        os: Os::WinLinux,
        vscode_chord: "Shift+Page_Down",
        vimcode_key: "(decodes identically to plain Page_Down, shift bit lost)",
        status: Status::Missing,
        note: "KNOWN_GAPS::SHIFT_PAGEUP_PAGEDOWN_SHIFT_BIT_DROPPED_BY_\
               DECODER. Mirrors cursorPageUpSelect above — \
               `NamedKey::PageDown`'s arm has the identical unconditional- \
               `false` shape.",
    },
];

/// The table above must stay internally consistent: every `Missing` row
/// names its `KNOWN_GAPS` label in its `note`, and no row's `vscode_chord`/
/// `command_id` is empty. Cheap structural self-check so a future edit that
/// adds a row without wiring it up fails loudly instead of silently.
#[test]
fn vscode_bindings_table_rows_are_well_formed() {
    for row in VSCODE_BINDINGS {
        assert!(!row.command_id.is_empty());
        assert!(!row.vscode_chord.is_empty());
        if row.status == Status::Missing {
            assert!(
                row.note.contains("KNOWN_GAPS::"),
                "Missing row for {:?} does not name its KNOWN_GAPS label",
                row.command_id
            );
        }
    }
}

// ─── New engine-level coverage: implemented bindings the 62 tests skip ─────
//
// Same idiom as `tests/vscode_mode.rs` (`handle_key` with an already-decoded
// triple) — this section exists purely to close the "every *implemented*
// binding has an engine-level test" half of deliverable 1. Real-input
// reachability is section 2, below.

#[test]
fn test_vscode_ctrl_a_select_all() {
    let mut e = engine_with("hello\nworld\n");
    vscode_mode(&mut e);
    e.handle_key("a", Some('a'), true);
    assert_eq!(e.mode, Mode::Visual);
    assert_eq!(e.visual_anchor, Some(Cursor { line: 0, col: 0 }));
}

#[test]
fn test_vscode_ctrl_c_copy_no_selection_copies_current_line() {
    let mut e = engine_with("hello\nworld\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 2 };
    e.handle_key("c", Some('c'), true);
    let (text, _) = e.get_register_content('+').expect("register populated");
    assert_eq!(text, "hello\n");
}

#[test]
fn test_vscode_ctrl_x_cut_no_selection_cuts_current_line() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    e.handle_key("x", Some('x'), true);
    assert_eq!(buf(&e), "aaa\nccc\n");
    let (text, _) = e.get_register_content('+').expect("register populated");
    assert_eq!(text, "bbb\n");
}

#[test]
fn test_vscode_ctrl_v_pastes_copied_text() {
    let mut e = engine_with("hello world\n");
    vscode_mode(&mut e);
    // Select "hell" (columns 0..4) and copy it. This hand-builds the
    // selection that `Shift+Right` x4 would produce, so it must also set
    // `visual_end_exclusive` the same way `vscode_extend_selection` does
    // (#1788 review) — otherwise the selection defaults to Vim's inclusive
    // convention and this copies "hello" (5 chars) instead of "hell".
    e.visual_anchor = Some(Cursor { line: 0, col: 0 });
    e.mode = Mode::Visual;
    e.visual_end_exclusive = true;
    e.view_mut().cursor = Cursor { line: 0, col: 4 };
    e.handle_key("c", Some('c'), true);
    // Clear selection, move to end of buffer, paste.
    e.visual_anchor = None;
    e.mode = Mode::Insert;
    e.view_mut().cursor = Cursor { line: 0, col: 11 };
    e.handle_key("v", Some('v'), true);
    // Exact match, not just "something longer got pasted" — a weaker
    // assertion here would pass against #1788's off-by-one (it pasted
    // "hello", 5 chars, for this 4-char selection) just as easily as
    // against a correct fix.
    assert_eq!(
        buf(&e),
        "hello worldhell\n",
        "paste must insert exactly the 4-char copied span, not one extra char"
    );
}

/// #1788: a 6-char Shift+Right selection of "hello " (indices 0..6 in
/// "hello world") must copy exactly those 6 characters, not 7. VSCode mode's
/// selection cursor sits *after* the last selected char (exclusive end) —
/// this used to be extracted with Vim's own inclusive-of-cursor range,
/// which is correct for Vim's Visual mode but copies one extra trailing
/// character in VSCode mode.
///
/// **Verified RED against unfixed `develop`:** before the fix, the final
/// assertion fails with `"hello worldhello w"` (7-char selection plus
/// the extra 'w') instead of `"hello worldhello "`.
#[test]
fn test_vscode_ctrl_c_six_char_selection_copies_exactly_six_chars() {
    let mut e = engine_with("hello world\n");
    vscode_mode(&mut e);
    e.handle_key("Home", None, true); // Ctrl+Home
    for _ in 0..6 {
        e.handle_key("Shift_Right", None, false);
    }
    e.handle_key("c", Some('c'), true); // Ctrl+C
    let (text, _) = e
        .get_register_content('+')
        .expect("Ctrl+C must populate the clipboard register");
    assert_eq!(
        text, "hello ",
        "Ctrl+C must copy exactly the 6-char selection"
    );

    e.handle_key("End", None, true); // Ctrl+End
    e.handle_key("v", Some('v'), true); // Ctrl+V
    assert_eq!(
        buf(&e),
        "hello worldhello \n",
        "Ctrl+V after Ctrl+C must re-insert exactly the 6 copied characters"
    );
}

/// Same scenario as above, but via Ctrl+X: the cut-deletion range was
/// already correct (`vscode_delete_selection` is exclusive-end), only the
/// clipboard payload was wrong — so this pins both halves independently.
#[test]
fn test_vscode_ctrl_x_six_char_selection_cuts_exactly_six_chars() {
    let mut e = engine_with("hello world\n");
    vscode_mode(&mut e);
    e.handle_key("Home", None, true); // Ctrl+Home
    for _ in 0..6 {
        e.handle_key("Shift_Right", None, false);
    }
    e.handle_key("x", Some('x'), true); // Ctrl+X
    assert_eq!(
        buf(&e),
        "world\n",
        "Ctrl+X must delete exactly the selection"
    );

    e.handle_key("End", None, true); // Ctrl+End
    e.handle_key("v", Some('v'), true); // Ctrl+V
    assert_eq!(
        buf(&e),
        "worldhello \n",
        "Ctrl+V after Ctrl+X must re-insert exactly the 6 cut characters"
    );
}

#[test]
fn test_vscode_ctrl_right_moves_word_forward_and_clears_selection() {
    let mut e = engine_with("hello world\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.visual_anchor = Some(Cursor { line: 0, col: 0 });
    e.mode = Mode::Visual;
    e.handle_key("Right", None, true);
    assert!(e.visual_anchor.is_none(), "Ctrl+Right must clear selection");
    assert!(
        e.cursor().col > 0,
        "Ctrl+Right must move the cursor forward"
    );
}

#[test]
fn test_vscode_ctrl_left_moves_word_backward() {
    let mut e = engine_with("hello world\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 11 };
    e.handle_key("Left", None, true);
    assert!(
        e.cursor().col < 11,
        "Ctrl+Left must move the cursor backward"
    );
}

#[test]
fn test_vscode_ctrl_home_moves_to_document_start() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 2, col: 2 };
    e.handle_key("Home", None, true);
    assert_eq!(*e.cursor(), Cursor { line: 0, col: 0 });
}

#[test]
fn test_vscode_ctrl_end_moves_to_document_end() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("End", None, true);
    assert_eq!(e.cursor().line, 2);
}

#[test]
fn test_vscode_ctrl_shift_right_extends_selection_by_word() {
    let mut e = engine_with("hello world\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Shift_Right", None, true);
    assert!(
        e.visual_anchor.is_some(),
        "Ctrl+Shift+Right must start a selection"
    );
    assert!(e.cursor().col > 0);
}

#[test]
fn test_vscode_ctrl_shift_left_extends_selection_by_word() {
    let mut e = engine_with("hello world\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 11 };
    e.handle_key("Shift_Left", None, true);
    assert!(
        e.visual_anchor.is_some(),
        "Ctrl+Shift+Left must start a selection"
    );
    assert!(e.cursor().col < 11);
}

#[test]
fn test_vscode_ctrl_shift_home_extends_selection_to_doc_start() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 2, col: 2 };
    e.handle_key("Shift_Home", None, true);
    assert_eq!(*e.cursor(), Cursor { line: 0, col: 0 });
    assert!(e.visual_anchor.is_some());
}

#[test]
fn test_vscode_ctrl_shift_end_extends_selection_to_doc_end() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Shift_End", None, true);
    assert_eq!(e.cursor().line, 2);
    assert!(e.visual_anchor.is_some());
}

#[test]
fn test_vscode_ctrl_delete_deletes_word_forward() {
    let mut e = engine_with("hello world\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Delete", None, true);
    assert_eq!(buf(&e), " world\n");
}

#[test]
fn test_vscode_ctrl_backspace_deletes_word_backward() {
    let mut e = engine_with("hello world\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 11 };
    e.handle_key("BackSpace", None, true);
    assert_eq!(buf(&e), "hello \n");
}

#[test]
fn test_vscode_ctrl_slash_toggles_line_comment() {
    let mut e = engine_with("print(1)\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("slash", None, true);
    assert!(
        buf(&e).contains('#'),
        "Ctrl+/ should add a comment marker: {:?}",
        buf(&e)
    );
}

#[test]
fn test_vscode_ctrl_f_opens_find() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.handle_key("f", Some('f'), true);
    assert!(e.find_replace_open);
    assert!(!e.find_replace_show_replace);
}

#[test]
fn test_vscode_ctrl_h_opens_find_replace() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.handle_key("h", Some('h'), true);
    assert!(e.find_replace_open);
    assert!(e.find_replace_show_replace);
}

#[test]
fn test_vscode_ctrl_q_quits_when_no_unsaved_changes() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    let action = e.handle_key("q", Some('q'), true);
    assert_eq!(action, EngineAction::Quit);
}

#[test]
fn test_vscode_shift_right_extends_selection_without_ctrl() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Shift_Right", None, false);
    assert_eq!(e.visual_anchor, Some(Cursor { line: 0, col: 0 }));
    assert_eq!(e.cursor().col, 1);
}

#[test]
fn test_vscode_shift_down_extends_selection_without_ctrl() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Shift_Down", None, false);
    assert!(e.visual_anchor.is_some());
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_vscode_page_down_moves_cursor_down_a_viewport() {
    let mut e = engine_with(&"line\n".repeat(200));
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Page_Down", None, false);
    assert!(e.cursor().line > 0, "Page_Down must move the cursor down");
}

#[test]
fn test_vscode_page_up_moves_cursor_up_a_viewport() {
    let mut e = engine_with(&"line\n".repeat(200));
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 150, col: 0 };
    e.handle_key("Page_Up", None, false);
    assert!(e.cursor().line < 150, "Page_Up must move the cursor up");
}

#[test]
fn test_vscode_f1_opens_command_palette() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.handle_key("F1", None, false);
    assert!(e.picker_open);
}

#[test]
fn test_vscode_f10_toggles_menu_bar() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    let before = e.menu_bar_visible;
    e.handle_key("F10", None, false);
    assert_eq!(e.menu_bar_visible, !before);
}

#[test]
fn test_vscode_plain_return_inserts_newline() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 5 };
    e.handle_key("Return", None, false);
    assert_eq!(buf(&e), "hello\n\n");
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_vscode_plain_tab_inserts_indent() {
    let mut e = engine_with("\n");
    vscode_mode(&mut e);
    e.settings.expand_tab = true;
    e.settings.tabstop = 4;
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Tab", None, false);
    assert_eq!(buf(&e), "    \n");
}

#[test]
fn test_vscode_plain_delete_removes_char_under_cursor() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Delete", None, false);
    assert_eq!(buf(&e), "ello\n");
}

// ─── #1746: new engine-level coverage for the rows added to expand the
// table to 100+ (plain arrow/Home/End movement, plain Shift-extend, Ctrl+K
// Ctrl+W) ────────────────────────────────────────────────────────────────

#[test]
fn test_vscode_plain_right_clears_selection_and_moves() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.visual_anchor = Some(Cursor { line: 0, col: 0 });
    e.mode = Mode::Visual;
    e.handle_key("Right", None, false);
    assert!(
        e.visual_anchor.is_none(),
        "plain Right must clear any active selection"
    );
    assert_eq!(e.cursor().col, 1);
}

#[test]
fn test_vscode_plain_left_moves_cursor_left() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 3 };
    e.handle_key("Left", None, false);
    assert_eq!(e.cursor().col, 2);
}

#[test]
fn test_vscode_plain_up_moves_cursor_up() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 2, col: 0 };
    e.handle_key("Up", None, false);
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_vscode_plain_down_moves_cursor_down() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Down", None, false);
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_vscode_plain_home_smart_toggle() {
    let mut e = engine_with("    hello\n");
    vscode_mode(&mut e);
    // Cursor past the indent: first Home press goes to the first
    // non-whitespace column (VS Code's "smart home").
    e.view_mut().cursor = Cursor { line: 0, col: 7 };
    e.handle_key("Home", None, false);
    assert_eq!(
        e.cursor().col,
        4,
        "first Home press should go to indent end"
    );
    // Pressing Home again from the indent-end column goes to column 0.
    e.handle_key("Home", None, false);
    assert_eq!(e.cursor().col, 0, "second Home press should go to column 0");
}

#[test]
fn test_vscode_plain_end_moves_to_line_end() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("End", None, false);
    assert_eq!(e.cursor().col, 5);
}

#[test]
fn test_vscode_plain_backspace_deletes_char_before_cursor() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 5 };
    e.handle_key("BackSpace", None, false);
    assert_eq!(buf(&e), "hell\n");
}

#[test]
fn test_vscode_shift_left_extends_selection_without_ctrl() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 3 };
    e.handle_key("Shift_Left", None, false);
    assert!(e.visual_anchor.is_some());
    assert_eq!(e.cursor().col, 2);
}

#[test]
fn test_vscode_shift_up_extends_selection_without_ctrl() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 2, col: 0 };
    e.handle_key("Shift_Up", None, false);
    assert!(e.visual_anchor.is_some());
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_vscode_shift_home_extends_selection_without_ctrl() {
    let mut e = engine_with("    hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 7 };
    e.handle_key("Shift_Home", None, false);
    assert!(e.visual_anchor.is_some());
    assert_eq!(
        e.cursor().col,
        4,
        "Shift+Home uses the same smart-home target"
    );
}

#[test]
fn test_vscode_shift_end_extends_selection_without_ctrl() {
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Shift_End", None, false);
    assert!(e.visual_anchor.is_some());
    assert_eq!(e.cursor().col, 5);
}

// ─── #1746 deliverable 1: new engine-level coverage for the save /
//     tab-switcher rows added above. `test_vscode_ctrl_f_opens_find` and
//     `test_vscode_ctrl_h_opens_find_replace` already existed in this file
//     (used only by the two `Os::Mac` rows' notes, never by a Win/Linux
//     row — exactly the gap this round's new `actions.find`/`editor.
//     action.startFindReplaceAction` `Os::WinLinux` rows above close) ──────

#[test]
fn test_vscode_ctrl_s_saves_file() {
    let path = std::env::temp_dir().join("vimcode_test_vscode_ctrl_s.txt");
    let _ = fs::remove_file(&path);
    let mut e = engine_with("hello\n");
    vscode_mode(&mut e);
    e.active_buffer_state_mut().file_path = Some(path.clone());
    e.handle_key("s", Some('s'), true);
    let written = fs::read_to_string(&path).expect("Ctrl+S should write the file to disk");
    assert!(written.contains("hello"), "wrote: {written:?}");
    let _ = fs::remove_file(&path);
}

#[test]
fn test_vscode_ctrl_tab_opens_tab_switcher() {
    let mut e = engine_with("aaa\n");
    vscode_mode(&mut e);
    e.new_tab(None);
    assert!(
        !e.tab_switcher_open,
        "test setup: tab switcher starts closed"
    );
    e.handle_key("Tab", None, true);
    assert!(e.tab_switcher_open, "Ctrl+Tab should open the tab switcher");
}

// #1746 review correction: `test_vscode_ctrl_k_ctrl_w_closes_all_editors_
// in_group` used to live here, asserting today's (wrong) `Some(1)` outcome,
// while its own row above was marked `Matches` with a note claiming the
// opposite ("closes every tab in the active group") — a false parity claim.
// Moved to section 3 as `gap_ctrl_k_ctrl_w_closes_every_tab`, which asserts
// the *correct* VS Code outcome and is gated to panic until it lands.

// ═══════════════════════════════════════════════════════════════════════════
// 2. Reachability matrix — does real input ever produce the key the table
//    above says `handle_vscode_key` wants?
// ═══════════════════════════════════════════════════════════════════════════

/// One surface vimcode ships — `macos_gui` is `src/macos/`, a real backend
/// since #859/#896 (#1730 wrongly assumed otherwise; see the module doc's
/// correction).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// Reaches the engine as the correct, unambiguous key.
    Yes,
    /// Does not reach the engine as the correct key, for the stated reason —
    /// either it collides with a different chord, or the modifier is not
    /// deliverable on that surface at all.
    No,
    /// No backend exists on this surface to answer the question.
    NotApplicable,
}

/// One row of the reachability matrix: a chord, and whether it survives the
/// trip from real input to `Engine::handle_vscode_key` on each surface.
#[allow(dead_code)]
struct ReachabilityRow {
    chord: &'static str,
    tui_legacy_xterm: Reach,
    tui_kitty_or_csiu: Reach,
    conpty_legacy: Reach,
    gtk: Reach,
    macos_tui: Reach,
    macos_gui: Reach,
    reason: &'static str,
    /// #1746 (deliverable 2): what a TUI user on `tui_legacy_xterm` or
    /// `conpty_legacy` should press instead, when the chord's `Reach` on
    /// either of those is `No`. VS Code's own alternative binding where one
    /// exists (e.g. F1 for the command palette); otherwise a proposed
    /// fallback (the command palette by name is always available since F1
    /// is reachable on every surface — see its own row below). `"n/a"` for
    /// rows that are already `Yes` on both legacy surfaces, or whose `No`
    /// cells are `macos_tui`/`macos_gui` only (no terminal fallback is
    /// meaningful for a GUI-only modifier — see the macOS-terminal decision
    /// in this file's module doc).
    fallback: &'static str,
}

/// The reachability matrix (deliverable 2). Driver-tier tests for the rows
/// backed by `engine_key_from_ui` (the shared decoder both GTK and TUI call
/// — see that function's own module doc in `src/render.rs`) follow below;
/// ConPTY rows are not independently re-verified here (this sandbox has no
/// Windows host — `tests/conpty_*.rs`'s own doc explains why that suite only
/// ever runs on real Windows hardware) and are recorded from the shared
/// decoder's own reasoning: ConPTY forwards crossterm-compatible VT
/// sequences, so the same legacy-vs-enhanced split applies once Windows
/// Terminal's own Win32-input-mode (ConPTY's analogue of the kitty protocol)
/// is or isn't negotiated.
static REACHABILITY_TABLE: &[ReachabilityRow] = &[
    ReachabilityRow {
        chord: "F1 (show command palette)",
        tui_legacy_xterm: Reach::Yes,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::Yes,
        gtk: Reach::Yes,
        macos_tui: Reach::Yes,
        macos_gui: Reach::Yes,
        reason: "The keystone fact every `fallback` field below leans on \
                 (\"F1 opens the command palette on every surface\") — \
                 `render::engine_key_from_ui`'s `NamedKey::F(n) => \
                 Some((format!(\"F{n}\"), None, false))` arm never \
                 inspects `ctrl`/`shift` and has no ambiguity to resolve: \
                 a bare function key is a dedicated escape sequence on \
                 every surface vimcode ships, with no C0-control-code or \
                 shift-bit collision the way Ctrl+<letter> has. Proven \
                 directly below by `f1_decodes_unconditionally_as_f1`.",
        fallback: "n/a — already `Reach::Yes` everywhere, no fallback \
                   needed; this is the fallback every other row's \
                   `fallback` field points at.",
    },
    ReachabilityRow {
        chord: "Ctrl+K (chord prefix) vs Ctrl+Shift+K (delete line)",
        tui_legacy_xterm: Reach::No,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::No,
        gtk: Reach::Yes,
        macos_tui: Reach::No,
        macos_gui: Reach::Yes,
        reason: "Ctrl+<letter> is an ANSI C0 control code (`letter & 0x1F`) \
                 — the byte stream is shift-invariant for letters, so a \
                 legacy terminal cannot send Shift information for this \
                 chord at all; crossterm's legacy decoder can only report \
                 Char('k')+CONTROL, with no SHIFT bit, regardless of \
                 whether Shift was physically held. Only the kitty keyboard \
                 protocol / CSI-u (TUI), GDK's always-resolved keysym \
                 (GTK), or AppKit's always-resolved `NSEvent` flags (macOS \
                 GUI — confirmed directly from `macos/events.rs`: Shift is \
                 its own flag bit, independent of Ctrl, same as GDK) \
                 carries that bit.",
        fallback: "#1746: VS Code ships no non-Shift alternative for \
                   deleteLines. Propose: F1 (always reachable — see that \
                   row below) opens the command palette; run \"Delete \
                   Line\" by name instead of the chord.",
    },
    ReachabilityRow {
        chord: "Ctrl+P (quick open) vs Ctrl+Shift+P (command palette)",
        tui_legacy_xterm: Reach::No,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::No,
        gtk: Reach::Yes,
        macos_tui: Reach::No,
        macos_gui: Reach::Yes,
        reason: "Same C0-control-code argument as Ctrl+K/Ctrl+Shift+K, \
                 substituting 'p' for 'k'.",
        fallback: "#1746: VS Code's own built-in alternative — F1 opens the \
                   command palette directly (`workbench.action.\
                   showCommands`'s own row below), so a legacy-terminal \
                   user reaches the exact same place Ctrl+Shift+P would \
                   have via a chord that *is* reachable there.",
    },
    ReachabilityRow {
        chord: "Ctrl+[ / Ctrl+Shift+[ (outdent vs fold)",
        tui_legacy_xterm: Reach::Yes,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::Yes,
        gtk: Reach::Yes,
        macos_tui: Reach::Yes,
        macos_gui: Reach::Yes,
        reason: "Not actually ambiguous: `engine_key_from_ui` has an \
                 explicit `!keyboard_enhanced` fallback that recognises the \
                 legacy byte for Ctrl+Shift+[ (which a non-enhanced \
                 terminal sends as Ctrl+3's byte) and resolves it to \
                 `Shift_bracketleft` — see that function's `lower == '3'` \
                 arm. Listed here precisely to document that this pair is \
                 *not* in the same unresolved state as Ctrl+K/Ctrl+P above.",
        fallback: "n/a — already `Reach::Yes` everywhere, no fallback needed.",
    },
    ReachabilityRow {
        chord: "Alt+Up / Alt+Down (move line)",
        tui_legacy_xterm: Reach::Yes,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::Yes,
        gtk: Reach::Yes,
        macos_tui: Reach::Yes,
        macos_gui: Reach::Yes,
        reason: "Alt+arrow is a dedicated escape sequence with no C0-style \
                 ambiguity; reachable everywhere Alt itself is deliverable, \
                 which includes every surface vimcode ships today (macOS \
                 Option key behaves as Alt in a terminal, and as `alt` in \
                 `quadraui::Modifiers` on the macOS GUI).",
        fallback: "n/a — already `Reach::Yes` everywhere, no fallback needed.",
    },
    ReachabilityRow {
        chord: "Ctrl+Alt+Up / Ctrl+Alt+Down (VS Code's real insertCursorAbove/Below)",
        tui_legacy_xterm: Reach::Yes,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::Yes,
        gtk: Reach::Yes,
        macos_tui: Reach::Yes,
        macos_gui: Reach::Yes,
        reason: "#1744: fixed, and was never actually a terminal-encoding \
                 limitation like the Ctrl+K/Ctrl+P family above — Ctrl+Alt+ \
                 arrow has a dedicated, unambiguous escape sequence on every \
                 surface, same as plain Alt+arrow. The old `Reach::No` row \
                 here recorded a gap in vimcode's own `render::route_alt_key` \
                 (no `ctrl` parameter at all), not a wire-protocol ambiguity; \
                 that parameter now exists and forwards `ctrl` through to \
                 `Engine::handle_vscode_key`, which tells Ctrl+Alt+Up/Down \
                 apart from plain Alt+Up/Down via its own `if ctrl` guard. \
                 `macos_gui` reaches it the same way: the physical Control \
                 key is `modifiers.ctrl`, distinct from `modifiers.cmd` \
                 (quadraui's `macos/events.rs`), so Ctrl+Alt+Up/Down never \
                 even touches #1745's Cmd-fold.",
        fallback: "n/a — already `Reach::Yes` everywhere, no fallback needed.",
    },
    ReachabilityRow {
        chord: "Ctrl+\\ (split editor) vs Ctrl+Shift+\\ (jump to matching bracket)",
        tui_legacy_xterm: Reach::No,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::No,
        gtk: Reach::Yes,
        macos_tui: Reach::No,
        macos_gui: Reach::Yes,
        reason: "Same C0-control-code argument as Ctrl+K/Ctrl+Shift+K above: \
                 Ctrl+\\ is the ANSI C0 byte 0x1C regardless of Shift (the \
                 physical key's shifted glyph, '|', XORs down to the exact \
                 same control byte), so a legacy terminal has no way to \
                 report the Shift bit for this chord at all. kitty/CSI-u \
                 (explicit Shift modifier alongside the base key), GTK \
                 (GDK hands over the literal shifted glyph '|' directly), \
                 and the macOS GUI (AppKit's `NSEvent.characters()` hands \
                 over the same literal shifted glyph) can all report it. \
                 NOT via `render::engine_key_from_ui` — that function's \
                 `Key::Char` arm has no production caller on any backend \
                 (see its own module doc); the real production path is \
                 `App::handle_dispatch`'s `Key::Char` arm, which forwards \
                 the literal '|' glyph unchanged (already matched by \
                 `handle_vscode_key`'s `\"Shift_backslash\" | \"|\"` arm) \
                 and has its own one-line special case for the \
                 explicit-Shift-bit shape, producing `\"Shift_backslash\"` \
                 directly.",
        fallback: "#1746: VS Code ships no alternative binding for \
                   jumpToBracket. Propose: F1 opens the command palette; \
                   run \"Go to Bracket\" by name instead of the chord.",
    },
    ReachabilityRow {
        chord: "Ctrl+Shift+L (select all occurrences) vs Ctrl+L (expand line \
                 selection, chord prefix for the former)",
        tui_legacy_xterm: Reach::No,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::No,
        gtk: Reach::Yes,
        macos_tui: Reach::No,
        macos_gui: Reach::Yes,
        reason: "#1746: the same C0-control-code argument as Ctrl+K/Ctrl+ \
                 Shift+K above, substituting 'l' for 'k' — a legacy \
                 terminal's Ctrl+L and Ctrl+Shift+L both decode to the \
                 same shift-blind `Char('l')+CONTROL`, so \
                 `editor.action.selectHighlights` is unreachable there \
                 while plain Ctrl+L (`expandLineSelection`) still works. \
                 This row was missing from the table before #1746 even \
                 though the underlying ambiguity is identical to the \
                 already-documented K/P/\\ rows — added while auditing \
                 every Ctrl+Shift+<letter> binding in VSCODE_BINDINGS for \
                 this issue's own deliverable 2.",
        fallback: "#1746: VS Code ships no non-Shift alternative for \
                   selectHighlights. Propose: F1 opens the command \
                   palette; run \"Select All Occurrences of Find Match\" \
                   by name instead of the chord.",
    },
    ReachabilityRow {
        chord: "Cmd+<key> (any VS Code Mac-default chord)",
        tui_legacy_xterm: Reach::NotApplicable,
        tui_kitty_or_csiu: Reach::NotApplicable,
        conpty_legacy: Reach::NotApplicable,
        gtk: Reach::NotApplicable,
        macos_tui: Reach::No,
        macos_gui: Reach::Yes,
        reason: "Cmd is a GUI-only modifier; no terminal emulator on any OS \
                 forwards it to the foreground application, so it is \
                 structurally unreachable on macOS TUI — this is the fact \
                 behind this issue's own macOS-terminal decision (module \
                 doc). `macos_gui` is `Yes` as of #1745 — confirmed \
                 directly from quadraui's own `macos/events.rs`: AppKit's \
                 `NS_FLAG_COMMAND` maps to `Modifiers::cmd`, a real, \
                 distinct bit a live `MacBackend` reports on every Cmd \
                 keypress (`src/macos/` is a thin wrapper around it, no \
                 key-decoding of its own, per its own module doc) — and \
                 `normalize_mac_cmd_as_ctrl` (`src/app.rs`) now reads \
                 it and folds it into `ctrl` before `vscode.rs` ever sees \
                 the event. See this table's own three Option/Cmd-arrow \
                 rows below for the handful of Mac defaults that are *not* \
                 a plain Ctrl-to-Cmd substitution.",
        fallback: "n/a — GUI-only chord; macOS TUI has no Cmd modifier to \
                   receive at all, so there is no terminal fallback \
                   distinct from the Linux/Windows TUI default (same \
                   chord, Ctrl instead of Cmd — this file's own \
                   macOS-terminal decision in the module doc).",
    },
    ReachabilityRow {
        chord: "Option+Left/Right (word-nav) vs Cmd+Left/Right (line start/end) vs Cmd+Up/Down (doc start/end)",
        tui_legacy_xterm: Reach::NotApplicable,
        tui_kitty_or_csiu: Reach::NotApplicable,
        conpty_legacy: Reach::NotApplicable,
        gtk: Reach::NotApplicable,
        macos_tui: Reach::No,
        macos_gui: Reach::Yes,
        reason: "Not reachable at all on macOS TUI, for the same reason as \
                 the plain Cmd+<key> row above (Cmd never reaches a \
                 terminal; Option+arrow reaching a terminal at all is moot \
                 here since vimcode's TUI binding for word-nav is \
                 Ctrl+Left/Right, not Option+Left/Right — VS Code's own \
                 terminal-hosted editor falls back the same way). On the \
                 macOS GUI, reachable via `normalize_mac_cmd_as_ctrl`'s \
                 three arrow-translation arms — `MacDriver`-proven by \
                 `src/macos/mod.rs`'s `option_right_moves_word_forward_not_\
                 navigate_forward`, `cmd_right_moves_to_line_end_not_one_\
                 column`, and `cmd_down_moves_to_document_end_not_one_line`.",
        fallback: "n/a — same GUI-only-modifier reasoning as the generic \
                   Cmd+<key> row above; macOS TUI already has the \
                   Linux/Windows Ctrl+Left/Right word-nav binding as its \
                   own default, which is the fallback.",
    },
    ReachabilityRow {
        chord: "Cmd+Option+F (Find & Replace)",
        tui_legacy_xterm: Reach::NotApplicable,
        tui_kitty_or_csiu: Reach::NotApplicable,
        conpty_legacy: Reach::NotApplicable,
        gtk: Reach::NotApplicable,
        macos_tui: Reach::No,
        macos_gui: Reach::No,
        reason: "KNOWN_GAPS::CMD_OPTION_F_FIND_REPLACE_UNREACHABLE. Blocked \
                 upstream in quadraui, not a vimcode binding gap: \
                 `macos/events.rs::ns_key_to_uievent` resolves a printable \
                 key's character from `NSEvent.characters()`, which is \
                 Option's layout-remapped glyph (Option+F -> 'ƒ', U+0192, \
                 on a US keyboard) rather than the base letter — confirmed \
                 by reading that function directly. See \
                 docs/PENDING_QUADRAUI_ISSUES.md.",
        fallback: "n/a — GUI-only chord, same as the generic Cmd+<key> \
                   row; Ctrl+H (Linux/Windows' default) already works on \
                   the macOS TUI as the fallback.",
    },
    ReachabilityRow {
        chord: "Cmd+<key> for a panel_keys-accelerator-only chord (toggle_sidebar, \
                 focus_explorer, focus_search, terminal_toggle_max, \
                 focus_notifications)",
        tui_legacy_xterm: Reach::NotApplicable,
        tui_kitty_or_csiu: Reach::NotApplicable,
        conpty_legacy: Reach::NotApplicable,
        gtk: Reach::NotApplicable,
        macos_tui: Reach::No,
        macos_gui: Reach::No,
        reason: "KNOWN_GAPS::PANEL_ACCELERATOR_CMD_CHORDS_DEAD_ON_MACOS_GUI. \
                 The general form of the `toggleSidebarVisibility` \
                 (`Cmd+B`) row above: these five actions' only real \
                 implementation is a `panel_keys` accelerator — registered \
                 as `quadraui::KeyBinding::Literal(...)`, matched by \
                 quadraui's shared `runtime::preprocess_event` *before* an \
                 unmatched keypress ever reaches `App::handle_dispatch` \
                 (where #1745's Cmd-to-Ctrl fold lives). quadraui's own \
                 `macos_universal_binding_modifiers` deliberately leaves \
                 `Literal` bindings untouched for Mac's own Cmd convention, \
                 so the accelerator only ever matches the physical Ctrl \
                 chord — by the time the fold could turn a Cmd keypress \
                 into something that looks like Ctrl, the accelerator- \
                 matching step has already run and missed it. Unlike the \
                 `Cmd+<key> (any VS Code Mac-default chord)` row above, \
                 this is `No`, not `Yes`: that row covers chords whose real \
                 implementation is reached by `Engine::handle_vscode_key` \
                 itself (where the fold's `ctrl`-true event does land); \
                 this row covers the handful whose real implementation \
                 sits one layer further out, in the accelerator the fold \
                 never gets a chance to retroactively satisfy. See \
                 `toggleSidebarVisibility`'s `VSCODE_BINDINGS` row and \
                 `src/macos/mod.rs`'s \
                 `cmd_b_does_not_toggle_sidebar_dead_panel_accelerator` for \
                 the concrete, driver-proven instance.",
        fallback: "n/a — GUI-only chord; the Ctrl-literal accelerator \
                   (e.g. `<C-b>`) already works unmodified on macOS TUI, \
                   same as every other platform, since it was never \
                   routed through the Cmd fold in the first place.",
    },
];

#[test]
fn reachability_table_rows_are_well_formed() {
    for row in REACHABILITY_TABLE {
        assert!(!row.chord.is_empty());
        assert!(!row.reason.is_empty());
        assert!(
            !row.fallback.is_empty(),
            "{:?} has an empty fallback",
            row.chord
        );
        // #1746 non-blocking review fix: deliverable 2 asks every row whose
        // chord is unreachable on a legacy terminal surface to record a
        // real fallback, not just an empty or placeholder string. A row is
        // only exempt (`"n/a"`-prefixed) when both legacy surfaces are
        // already `Yes` — anything else must spell out what to press
        // instead, enforced here so a future row can't silently skip it
        // the way the four `Matches`-claimed rows this round downgraded
        // originally did.
        let legacy_blocked = row.tui_legacy_xterm == Reach::No || row.conpty_legacy == Reach::No;
        if legacy_blocked {
            assert!(
                !row.fallback.starts_with("n/a"),
                "{:?} is unreachable on a legacy terminal surface \
                 (tui_legacy_xterm={:?}, conpty_legacy={:?}) but its \
                 fallback field starts with \"n/a\" — deliverable 2 \
                 requires a real fallback here",
                row.chord,
                row.tui_legacy_xterm,
                row.conpty_legacy
            );
        }
    }
}

/// Driver-tier proof for [`REACHABILITY_TABLE`]'s `F1` row: a bare function
/// key decodes the same regardless of ctrl/shift, since it has no
/// Ctrl+<letter>-style ambiguity to resolve — the fact every other row's
/// `fallback` field leans on ("F1 is reachable on every surface").
#[test]
fn f1_decodes_unconditionally_as_f1() {
    use quadraui::{Key, Modifiers, NamedKey};
    let (name, _, _) =
        engine_key_from_ui(&Key::Named(NamedKey::F(1)), Modifiers::default(), false).unwrap();
    assert_eq!(name, "F1");
    let shift_ctrl = Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    };
    let (name_with_mods, _, _) =
        engine_key_from_ui(&Key::Named(NamedKey::F(1)), shift_ctrl, false).unwrap();
    assert_eq!(
        name_with_mods, "F1",
        "F1 must decode the same name regardless of modifiers held \
         alongside it — there is no ambiguity for a bare function key to \
         resolve"
    );
}

/// Driver-tier proof for the `Ctrl+K vs Ctrl+Shift+K` / `Ctrl+P vs
/// Ctrl+Shift+P` rows: without keyboard enhancement, a terminal reporting a
/// bare Ctrl+letter combo (no SHIFT bit — which is all a legacy terminal can
/// ever report for this family, see the row's `reason`) decodes to the same
/// engine key name whether or not Shift was actually held, so VS Code's
/// Ctrl+Shift+K can never reach the engine as anything but Ctrl+K's "k".
#[test]
fn ctrl_k_without_keyboard_enhancement_cannot_produce_the_shift_variant() {
    use quadraui::{Key, Modifiers};
    let mods = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    let (name, _, ctrl) = engine_key_from_ui(&Key::Char('k'), mods, false).unwrap();
    assert_eq!(
        name, "k",
        "a legacy terminal's Ctrl+K decode must stay lowercase \"k\" (the \
         chord-prefix binding) — it has no way to carry the Shift bit that \
         would route to \"K\" (delete line) instead"
    );
    assert!(ctrl);
}

/// The kitty/CSI-u-protocol counterpart: once the terminal (or GTK, which
/// always has this fidelity) *can* report Shift alongside Ctrl, the decoder
/// correctly produces the distinct uppercase name VS Code's Ctrl+Shift+K
/// needs.
#[test]
fn ctrl_shift_k_with_keyboard_enhancement_produces_the_distinct_name() {
    use quadraui::{Key, Modifiers};
    let mods = Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    };
    let (name, _, ctrl) = engine_key_from_ui(&Key::Char('k'), mods, true).unwrap();
    assert_eq!(
        name, "K",
        "with a Shift bit available (kitty/CSI-u, or GTK's always-resolved \
         keysym), Ctrl+Shift+K must decode distinctly from Ctrl+K"
    );
    assert!(ctrl);
}

/// #1744: `Ctrl+Alt+Up` (`Alt_Up` with `ctrl: true`, the exact name+flag
/// `route_alt_key` now hands `handle_vscode_key` for that chord — see
/// `vscode_alt_key_name`'s doc) has a distinct wire representation from plain
/// Alt+Up after all: `handle_vscode_key`'s `"Alt_Up" if ctrl` arm (checked
/// before the unguarded move-line arm) routes to `vscode_add_cursor_above`
/// instead. Before the fix this test's name described reality (`ctrl` was
/// accepted but ignored, identical outcome to plain Alt+Up); it now pins the
/// opposite fact, renamed to match.
#[test]
fn ctrl_alt_up_has_a_distinct_wire_representation_from_alt_up() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    e.handle_key("Alt_Up", None, true);
    assert_eq!(
        buf(&e),
        "aaa\nbbb\nccc\n",
        "Ctrl+Alt+Up must not move any line, unlike plain Alt+Up"
    );
    assert_eq!(
        e.view().extra_cursors.len(),
        1,
        "Ctrl+Alt+Up's real VS Code effect (insertCursorAbove) must happen"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Known gaps — bidirectional gate (local reimplementation of
//    `src/harness.rs`'s `KNOWN_BUGS` idiom; that one is `pub(crate)` and
//    unreachable from this separate integration-test crate)
// ═══════════════════════════════════════════════════════════════════════════

/// What [`gap_gate`] found. Each gate body in this file asserts the
/// *correct*, VS-Code-matching outcome (not today's actual, wrong
/// behaviour) — so while a listed gap is still open, the body panics, and
/// `ExpectedGap` is the normal, passing-test outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GapOutcome {
    /// Body ran to completion and is not listed — the ordinary case; never
    /// actually produced by this file's own gated tests (every call site
    /// passes a label from [`KNOWN_GAPS`]), but kept so the four-state model
    /// mirrors `src/harness.rs::known_bug_gate_outcome` exactly, including
    /// the unlisted half of its table.
    Pass,
    /// Body panicked and is listed — the gap is still open, as expected.
    ExpectedGap,
    /// Body panicked and is *not* listed — a regression or a new,
    /// undocumented gap.
    Regression,
    /// Body ran to completion but is listed — the fix landed; the
    /// `KNOWN_GAPS` entry (and the corresponding `VSCODE_BINDINGS` row) must
    /// be deleted.
    FixLanded,
}

/// Labels of gaps that are *expected* to still reproduce today. THIS LIST
/// MAY ONLY EVER SHRINK — an entry is deleted in the same change that fixes
/// the gap it names (see [`gap_gate`]'s `FixLanded` panic, which enforces
/// that deletion is not optional).
///
/// #1744 fixed every gap this list ever named (`CTRL_ALT_UP_IS_MOVE_LINE`,
/// `CTRL_ALT_DOWN_IS_MOVE_LINE`, `ALT_SHIFT_UP_IS_ADD_CURSOR`,
/// `ALT_LEFT_RIGHT_IS_SIDEBAR_RESIZE`,
/// `CTRL_SHIFT_BACKSLASH_JUMP_TO_BRACKET_UNBOUND`); the `gap_*` tests for
/// those five are now plain (ungated) assertions of the fixed behaviour,
/// same as `src/harness.rs`'s own `KNOWN_BUGS` empty-list state. #1745 adds
/// three new entries, all still open — see the matching `VSCODE_BINDINGS`
/// rows (`editor.action.startFindReplaceAction`,
/// `workbench.action.navigateBack / navigateForward`, and
/// `workbench.action.toggleSidebarVisibility`) for the full writeup.
///
/// `PANEL_ACCELERATOR_CMD_CHORDS_DEAD_ON_MACOS_GUI` is the one entry here
/// with no [`gap_gate`] call of its own: the defect it names
/// (`render::apply_engine_action`'s `ToggleSidebar` arm never calling
/// `Engine::toggle_sidebar`) is only reachable through the real production
/// dispatch path, which is `pub(crate)` to `vimcode_core` and therefore
/// invisible from this separate integration-test crate — see
/// `src/macos/mod.rs`'s `cmd_b_does_not_toggle_sidebar_dead_panel_
/// accelerator` for that entry's actual RED proof and the deletion
/// instructions that live there instead. The structural self-check
/// (`vscode_bindings_table_rows_are_well_formed`) still requires this
/// label's `VSCODE_BINDINGS` row to name it, so a future edit that adds a
/// `Missing` row without wiring up *some* gate still fails loudly.
const KNOWN_GAPS: &[&str] = &[
    "CMD_OPTION_F_FIND_REPLACE_UNREACHABLE",
    "CTRL_MINUS_NAVIGATE_BACK_FORWARD_ON_MAC_UNVERIFIED",
    "PANEL_ACCELERATOR_CMD_CHORDS_DEAD_ON_MACOS_GUI",
    // #1746, added while expanding VSCODE_BINDINGS past 100 rows — see each
    // label's own VSCODE_BINDINGS row for the full writeup and its gate
    // test below for the RED proof. Verified RED against unfixed `develop`
    // by temporarily removing these six labels and re-running this suite:
    // all six gate tests failed with `Regression` (the exact panics this
    // issue's new code is supposed to pin), confirming each one can fail.
    "CMD_SHIFT_Z_REDO_UNREACHABLE_ON_MAC",
    "OPTION_BACKSPACE_DELETE_WORD_LEFT_UNBOUND_ON_MAC",
    "OPTION_DELETE_WORD_RIGHT_UNBOUND_ON_MAC",
    "CTRL_K_CTRL_C_TOGGLES_INSTEAD_OF_ADD_ONLY",
    "CTRL_K_CTRL_U_TOGGLES_INSTEAD_OF_REMOVE_ONLY",
    "SHIFT_PAGEUP_PAGEDOWN_SHIFT_BIT_DROPPED_BY_DECODER",
    // Added in this #1746 review round: the decoder-level ctrl-bit-drop
    // gaps the previous round's `Matches` rows missed, plus the
    // `closeAllEditors` false-parity correction, plus the deliverable-1
    // rows added for the editor-group/tab family and Win/Linux find/
    // replace/save. Verified RED against unfixed `develop` the same way as
    // the six above (all eight labels below temporarily removed, re-ran
    // this suite: all eight gate tests failed with `Regression` — the
    // exact panics each one's own doc comment claims — then restored).
    "CTRL_SHIFT_HOME_END_CTRL_BIT_DROPPED_BY_DECODER",
    "CTRL_BACKSPACE_DELETE_CTRL_BIT_DROPPED_BY_DECODER",
    "CTRL_K_CTRL_W_LEAVES_ONE_TAB_OPEN",
    // Deliverable-1 editor-group/tab family additions, same round.
    "CTRL_W_CLOSE_ACTIVE_EDITOR_UNBOUND_IN_VSCODE_MODE",
    "CTRL_BACKSLASH_SPLIT_EDITOR_UNBOUND",
    "CTRL_PAGEUP_PAGEDOWN_PREV_NEXT_EDITOR_UNBOUND",
    "CTRL_1_FOCUS_FIRST_EDITOR_GROUP_UNBOUND",
    "CTRL_K_FOLD_ALL_UNFOLD_ALL_UNBOUND",
];

/// Run `body`, gated on whether `label` is listed in [`KNOWN_GAPS`]. Mirrors
/// `src/harness.rs::known_bug_gate_outcome` exactly (see that function's doc
/// for the full table and the panic-hook/guard-poisoning rationale, which
/// applies unchanged here); reimplemented locally because that one is
/// `pub(crate)` to the `vimcode_core` lib crate and this file is compiled as
/// a separate crate.
fn gap_gate_outcome<F: FnOnce()>(label: &str, body: F) -> GapOutcome {
    let listed = KNOWN_GAPS.contains(&label);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).is_err();
    match (panicked, listed) {
        (false, false) => GapOutcome::Pass,
        (true, true) => GapOutcome::ExpectedGap,
        (true, false) => GapOutcome::Regression,
        (false, true) => GapOutcome::FixLanded,
    }
}

/// Pass/fail wrapper: turns `Regression`/`FixLanded` into a test failure
/// with an actionable message; `Pass`/`ExpectedGap` return normally (an
/// ordinary passing test, or today's gap, correctly still open).
///
/// #1744 fixed every gap `KNOWN_GAPS` named at the time, so for a while no
/// `#[test]` in this file called this wrapper (each former `gap_*` test
/// became a plain, ungated assertion — see e.g.
/// `ctrl_alt_up_inserts_cursor_above_not_move_line`). #1745 adds two new
/// gaps (`gap_cmd_option_f_find_replace_unreachable`,
/// `gap_ctrl_minus_navigates_back_on_mac`), so this is live again — this is
/// this file's whole reason for existing per its own module doc
/// (deliverable 3, "a bidirectional gate for every binding
/// `VSCODE_BINDINGS` marks `Missing` or wrong"), exactly as `src/harness.rs`'s
/// `known_bug_gate` stays live infrastructure independent of how many bugs
/// `KNOWN_BUGS` currently lists.
fn gap_gate(label: &'static str, body: impl FnOnce()) {
    match gap_gate_outcome(label, body) {
        GapOutcome::Pass | GapOutcome::ExpectedGap => {}
        GapOutcome::Regression => panic!(
            "vscode_keybinding_parity: gap {label:?} panicked and is NOT \
             listed in KNOWN_GAPS — this is a regression, or a new gap that \
             needs its own KNOWN_GAPS entry"
        ),
        GapOutcome::FixLanded => panic!(
            "vscode_keybinding_parity: gap {label:?} PASSED but is still \
             listed in KNOWN_GAPS — the fix landed; delete the KNOWN_GAPS \
             entry and update the matching VSCODE_BINDINGS row to `Matches`"
        ),
    }
}

/// Test-only twin of [`gap_gate_outcome`] that takes "is this listed" as an
/// explicit parameter instead of consulting the real (now-empty)
/// [`KNOWN_GAPS`] — so the self-test below can exercise all four table rows
/// without needing a real, permanent entry just to test the mechanism.
/// Mirrors `src/harness.rs`'s own `known_bug_gate_outcome_for_test` exactly,
/// for the same reason (`known_bug_gate_outcome`'s contract is "label,
/// consult the real list"; threading a test-only bool through the real
/// function would leave a footgun parameter for one test's convenience).
fn gap_gate_outcome_for_test<F: FnOnce()>(listed: bool, body: F) -> GapOutcome {
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).is_err();
    match (panicked, listed) {
        (false, false) => GapOutcome::Pass,
        (true, true) => GapOutcome::ExpectedGap,
        (true, false) => GapOutcome::Regression,
        (false, true) => GapOutcome::FixLanded,
    }
}

/// Self-test, mirroring `src/harness.rs`'s own `known_bugs_is_empty`-style
/// check in spirit (that one asserts the list is empty; this one asserts the
/// list and the gate actually agree) — proves `gap_gate` cannot silently
/// rot in either direction before trusting it below.
#[test]
fn gap_gate_self_test_both_directions() {
    assert_eq!(
        gap_gate_outcome("NOT_LISTED_LABEL", || {}),
        GapOutcome::Pass
    );
    assert_eq!(
        gap_gate_outcome("NOT_LISTED_LABEL", || panic!("unrelated failure")),
        GapOutcome::Regression
    );
    assert_eq!(
        gap_gate_outcome_for_test(true, || panic!("still broken")),
        GapOutcome::ExpectedGap
    );
    assert_eq!(
        gap_gate_outcome_for_test(true, || {}),
        GapOutcome::FixLanded
    );
}

/// #1744, was `KNOWN_GAPS::CTRL_ALT_UP_IS_MOVE_LINE` (now deleted,
/// `FixLanded`): VS Code's `editor.action.insertCursorAbove` (`Ctrl+Alt+Up`)
/// adds a cursor one line above, leaving the buffer untouched.
/// `render::route_alt_key` now forwards `ctrl` through to
/// `Engine::handle_vscode_key` instead of hardcoding `false`, and that
/// function's `"Alt_Up" if ctrl` arm (checked before the unguarded move-line
/// arm) calls `vscode_add_cursor_above`. **Verified RED against unfixed
/// `develop`** before this change landed — this test's body is unchanged
/// from the `gap_gate`-wrapped version that asserted the same outcome and
/// reported `ExpectedGap` (i.e. panicked) there.
#[test]
fn ctrl_alt_up_inserts_cursor_above_not_move_line() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    e.handle_key("Alt_Up", None, true);
    assert_eq!(
        buf(&e),
        "aaa\nbbb\nccc\n",
        "insertCursorAbove must not move the line"
    );
    assert_eq!(
        e.view().extra_cursors.len(),
        1,
        "insertCursorAbove must add exactly one cursor above"
    );
}

/// Mirrors the above for `editor.action.insertCursorBelow` (`Ctrl+Alt+Down`).
/// Was `KNOWN_GAPS::CTRL_ALT_DOWN_IS_MOVE_LINE`.
#[test]
fn ctrl_alt_down_inserts_cursor_below_not_move_line() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    e.handle_key("Alt_Down", None, true);
    assert_eq!(
        buf(&e),
        "aaa\nbbb\nccc\n",
        "insertCursorBelow must not move the line"
    );
    assert_eq!(
        e.view().extra_cursors.len(),
        1,
        "insertCursorBelow must add exactly one cursor below"
    );
}

/// #1744, was `KNOWN_GAPS::ALT_SHIFT_UP_IS_ADD_CURSOR` (now deleted): VS
/// Code's `Shift+Alt+Up` is `editor.action.copyLinesUpAction` (duplicate line
/// upward), not add-a-cursor — that chord now belongs to Ctrl+Alt+Up (see
/// `ctrl_alt_up_inserts_cursor_above_not_move_line` above).
/// `handle_vscode_key`'s `"Alt_Shift_Up"` arm now calls the new
/// `Engine::vscode_copy_line_up`. **Verified RED against unfixed `develop`**
/// before this change landed (this test's body is unchanged from the
/// `gap_gate`-wrapped version).
#[test]
fn alt_shift_up_copies_line_up_not_add_cursor() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    e.handle_key("Alt_Shift_Up", None, false);
    assert_eq!(
        buf(&e),
        "aaa\nbbb\nbbb\nccc\n",
        "Shift+Alt+Up should duplicate the current line upward"
    );
    assert!(
        e.view().extra_cursors.is_empty(),
        "copyLinesUpAction does not add a cursor"
    );
}

/// Mirrors the above for `editor.action.copyLinesDownAction`
/// (`Shift+Alt+Down`, `Engine::vscode_copy_line_down`). Not itself a
/// `KNOWN_GAPS` conversion — the issue's gap list named only one label,
/// `ALT_SHIFT_UP_IS_ADD_CURSOR`, for the Up/Down pair — but the root cause
/// was identical in both directions: `"Alt_Shift_Down"` used to reach
/// `vscode_add_cursor_below` the same way `"Alt_Shift_Up"` reached
/// `vscode_add_cursor_above`. **Verified RED against unfixed `develop`**:
/// reverting `handle_vscode_key`'s `"Alt_Shift_Down" =>
/// self.vscode_copy_line_down(&mut changed)` arm to call
/// `vscode_add_cursor_below()` instead reproduces both assertion failures
/// here (the buffer stays `"aaa\nbbb\nccc\n"` and `extra_cursors` gains an
/// entry).
#[test]
fn alt_shift_down_copies_line_down_not_add_cursor() {
    let mut e = engine_with("aaa\nbbb\nccc\n");
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 1, col: 0 };
    e.handle_key("Alt_Shift_Down", None, false);
    assert_eq!(
        buf(&e),
        "aaa\nbbb\nbbb\nccc\n",
        "Shift+Alt+Down should duplicate the current line downward"
    );
    assert!(
        e.view().extra_cursors.is_empty(),
        "copyLinesDownAction does not add a cursor"
    );
}

/// #1744, was `KNOWN_GAPS::ALT_LEFT_RIGHT_IS_SIDEBAR_RESIZE` (now deleted):
/// VS Code's `Alt+Left`/`Alt+Right` (`workbench.action.navigateBack`/
/// `navigateForward`) behave like Vim mode's Ctrl-O/Ctrl-I — jump back to (and
/// forward from) a recorded jump-list entry. `render::route_alt_key`'s
/// mode-independent tier no longer claims `AltBase::Left`/`Right`
/// unconditionally for sidebar-resize; in VSCode mode it falls through to the
/// VSCode-mode tier, which maps the chord to `"Alt_Left"`/`"Alt_Right"` and
/// `handle_vscode_key` calls `Engine::jump_list_back`/`jump_list_forward`.
/// Seeds a real jump-list entry with the already-public
/// `Engine::push_jump_location` (the same mechanism Vim mode's motions use).
/// **Verified RED against unfixed `develop`** before this change landed
/// (this test's body is unchanged from the `gap_gate`-wrapped version).
#[test]
fn alt_left_navigates_back_not_resize_sidebar() {
    let mut e = engine_with(&"line\n".repeat(10));
    vscode_mode(&mut e);
    e.view_mut().cursor = Cursor { line: 0, col: 0 };
    // Record (0, 0) as a place Ctrl-O/Alt+Left should be able to return
    // to, then move elsewhere — exactly what a real jump (e.g. VS
    // Code's "Go to Definition") would have done first.
    e.push_jump_location();
    e.view_mut().cursor = Cursor { line: 9, col: 0 };
    e.handle_key("Alt_Left", None, false);
    assert_eq!(
        e.cursor().line,
        0,
        "Alt+Left (navigateBack) should return to the jump-list entry, \
         the same way Ctrl-O already does in Vim mode"
    );
}

/// #1744, was `KNOWN_GAPS::CTRL_SHIFT_BACKSLASH_JUMP_TO_BRACKET_UNBOUND` (now
/// deleted): VS Code's `editor.action.jumpToBracket` (`Ctrl+Shift+\`) moves
/// the cursor onto the matching bracket, exactly like Vim mode's `%` already
/// does via `Engine::find_matching_bracket`/`move_to_matching_bracket`.
/// `App::handle_dispatch`'s `Key::Char` arm (`src/app.rs`) now has a special
/// case producing `"Shift_backslash"` for the one input shape that needs
/// it, and `handle_vscode_key` has a `"Shift_backslash" | "|"` arm calling
/// `move_to_matching_bracket`. This test, like its four siblings above,
/// drives the engine with an already-decoded key name; the driver-tier
/// proof that a real Ctrl+Shift+\ keypress decodes to that name on both
/// backends is `src/gtk/testing.rs`'s `mod alt_rung_1744` and
/// `src/tui_main/app_on_tui_tests.rs`'s `mod vscode_mode_alt_rung_1744`.
/// **Verified RED against unfixed `develop`** before this change landed
/// (this test's body is unchanged from the `gap_gate`-wrapped version).
#[test]
fn ctrl_shift_backslash_jumps_to_matching_bracket() {
    let mut e = engine_with("fn f() { (1 + 2) }\n");
    vscode_mode(&mut e);
    // Cursor sits on the opening paren of `(1 + 2)`; its match is the
    // `)` at char index 15 (single-line buffer, so char index == col).
    e.view_mut().cursor = Cursor { line: 0, col: 9 };
    e.handle_key("Shift_backslash", None, true);
    assert_eq!(
        e.cursor().col,
        15,
        "Ctrl+Shift+\\ should jump to the matching ')'"
    );
}

/// #1745: `KNOWN_GAPS::CMD_OPTION_F_FIND_REPLACE_UNREACHABLE`. VS Code's
/// `editor.action.startFindReplaceAction` Mac default is Cmd+Option+F, not
/// a Ctrl-to-Cmd substitution of Ctrl+H (see this gap's `VSCODE_BINDINGS`
/// row for why Cmd+H itself is unusable — macOS's system-wide "Hide
/// application" chord). Drives the engine with the exact character a real
/// Mac keyboard produces for Option+F: `quadraui::macos::events::
/// ns_key_to_uievent` resolves a printable key's character from
/// `NSEvent.characters()`, which is Option's layout-remapped glyph
/// ('\u{192}', ƒ, U+0192 LATIN SMALL LETTER F WITH HOOK on a US keyboard),
/// not the base letter 'f' — confirmed by reading that function directly
/// (see the `VSCODE_BINDINGS` row's note for the exact call site). This
/// gap is blocked upstream in quadraui, not a vimcode binding gap — see
/// `docs/PENDING_QUADRAUI_ISSUES.md`.
///
/// Body asserts the *correct* (still unimplemented) outcome, so it panics
/// today — `gap_gate` reports `ExpectedGap` for a listed label, which is
/// this test's normal, passing result until the gap closes.
///
/// #1745 review: this body drives `'\u{192}'` — the exact glyph that will
/// **stop** arriving once the quadraui fix lands (the fix is quadraui
/// resolving Option+F to the base letter `'f'` instead). That means this
/// test can only ever report `ExpectedGap` (today) or `Regression`
/// (post-fix, since `'\u{192}'` would then reach the engine as an ordinary,
/// un-bound character and the assertions below would still fail, just for
/// an unrelated reason) — it can never report `FixLanded`, so it cannot
/// force `KNOWN_GAPS::CMD_OPTION_F_FIND_REPLACE_UNREACHABLE`'s deletion the
/// way `gap_gate`'s contract expects (same class of mistake as the
/// now-fixed `gap_ctrl_minus_navigates_back_on_mac`, which drove a key name
/// the real decoder never produces). When the quadraui fix lands, update
/// this body to drive `'f'` instead before deleting the `KNOWN_GAPS` entry.
#[test]
fn gap_cmd_option_f_find_replace_unreachable() {
    gap_gate("CMD_OPTION_F_FIND_REPLACE_UNREACHABLE", || {
        let mut e = engine_with("hello\n");
        vscode_mode(&mut e);
        // The literal glyph a real Mac keyboard's Option+F produces today
        // (`characters()`, not `charactersIgnoringModifiers()`) — see this
        // test's own doc above for why this must change to `'f'` once the
        // upstream quadraui fix lands, in the same change that deletes the
        // `KNOWN_GAPS` entry.
        e.handle_key("\u{192}", Some('\u{192}'), true);
        assert!(
            e.find_replace_open,
            "Cmd+Option+F should open Find & Replace, the same as Ctrl+H \
             on Linux/Windows"
        );
        assert!(
            e.find_replace_show_replace,
            "Cmd+Option+F should open the *replace* variant, not plain find"
        );
    });
}

/// #1745: `KNOWN_GAPS::CTRL_MINUS_NAVIGATE_BACK_FORWARD_ON_MAC_UNVERIFIED`.
/// Per this issue's own brief, VS Code's real Mac default for
/// `workbench.action.navigateBack` is Ctrl+- (not Alt+Left, which is Win/
/// Linux's default and already `Matches` — see that row). vimcode has no
/// `"-"` binding in `handle_vscode_key` at all today, on any platform, so
/// this reproduces independently of macOS: seeds a jump-list entry (the
/// same `push_jump_location` mechanism `alt_left_navigates_back_not_
/// resize_sidebar` above uses) and asserts Ctrl+- returns to it, which it
/// does not.
///
/// Not fixed in this PR: confirming the exact real chord against VS Code's
/// own `keybindings.json` needs a live web lookup this session did not
/// have access to, and shipping a guessed chord would risk being wrong in
/// a way this suite could not catch. Left for a follow-up vimcode issue
/// once verified — not quadraui-blocked, since `-` is an ordinary
/// `Key::Char` no different from the other symbol chords `#1745` fixed.
#[test]
fn gap_ctrl_minus_navigates_back_on_mac() {
    gap_gate("CTRL_MINUS_NAVIGATE_BACK_FORWARD_ON_MAC_UNVERIFIED", || {
        let mut e = engine_with(&"line\n".repeat(10));
        vscode_mode(&mut e);
        e.view_mut().cursor = Cursor { line: 0, col: 0 };
        e.push_jump_location();
        e.view_mut().cursor = Cursor { line: 9, col: 0 };
        e.handle_key("-", Some('-'), true);
        assert_eq!(
            e.cursor().line,
            0,
            "Ctrl+- (navigateBack) should return to the jump-list entry, \
             the same way Alt+Left already does on Win/Linux"
        );
    });
}

// ─── #1746: new gates, found while expanding VSCODE_BINDINGS to 100+ rows ──

/// `KNOWN_GAPS::CMD_SHIFT_Z_REDO_UNREACHABLE_ON_MAC`. See this gap's
/// `VSCODE_BINDINGS` row (`redo`, `Os::Mac`) for the full writeup: VS
/// Code's real Mac default for redo is Cmd+Shift+Z, which folds to Ctrl+
/// Shift+Z (key name `"Z"`) — `handle_vscode_key`'s ctrl match has no `"Z"`
/// arm, only lowercase `"y"`. Drives a real undo/redo roundtrip: types a
/// character, undoes it with Ctrl+Z (already `Matches`), then presses `"Z"`
/// (Ctrl+Shift+Z's engine-level shape) expecting the character back.
#[test]
fn gap_cmd_shift_z_redos_on_mac() {
    gap_gate("CMD_SHIFT_Z_REDO_UNREACHABLE_ON_MAC", || {
        let mut e = engine_with("hello\n");
        vscode_mode(&mut e);
        e.view_mut().cursor = Cursor { line: 0, col: 5 };
        e.handle_key("!", Some('!'), false);
        assert_eq!(buf(&e), "hello!\n", "test setup: typed one character");
        e.handle_key("z", Some('z'), true);
        assert_eq!(buf(&e), "hello\n", "test setup: Ctrl+Z undid the type");
        e.handle_key("Z", Some('Z'), true);
        assert_eq!(
            buf(&e),
            "hello!\n",
            "Ctrl+Shift+Z (VS Code Mac's real redo chord) should redo"
        );
    });
}

/// `KNOWN_GAPS::OPTION_BACKSPACE_DELETE_WORD_LEFT_UNBOUND_ON_MAC`. See this
/// gap's `VSCODE_BINDINGS` row (`deleteWordLeft`, `Os::Mac`) for the full
/// writeup: `render::alt_chord_base` has no case for `NamedKey::Backspace`'s
/// multi-character name, so `route_alt_key` falls through and
/// `handle_vscode_key`'s `Alt_` match (which has no `"Alt_BackSpace"` arm
/// either) never gets a chance either way. Drives the hypothetical correct
/// key name directly — confirming it is unhandled regardless of which of
/// the two functions would need the fix.
#[test]
fn gap_option_backspace_deletes_word_left_on_mac() {
    gap_gate("OPTION_BACKSPACE_DELETE_WORD_LEFT_UNBOUND_ON_MAC", || {
        let mut e = engine_with("hello world\n");
        vscode_mode(&mut e);
        e.view_mut().cursor = Cursor { line: 0, col: 11 };
        e.handle_key("Alt_BackSpace", None, false);
        assert_eq!(
            buf(&e),
            "hello \n",
            "Option+Backspace (VS Code Mac's deleteWordLeft) should delete \
             the word before the cursor, the same as Ctrl+Backspace does \
             on Linux/Windows"
        );
    });
}

/// `KNOWN_GAPS::OPTION_DELETE_WORD_RIGHT_UNBOUND_ON_MAC`. Mirrors
/// `gap_option_backspace_deletes_word_left_on_mac` above, for Delete.
#[test]
fn gap_option_delete_deletes_word_right_on_mac() {
    gap_gate("OPTION_DELETE_WORD_RIGHT_UNBOUND_ON_MAC", || {
        let mut e = engine_with("hello world\n");
        vscode_mode(&mut e);
        e.view_mut().cursor = Cursor { line: 0, col: 0 };
        e.handle_key("Alt_Delete", None, false);
        assert_eq!(
            buf(&e),
            " world\n",
            "Option+Delete (VS Code Mac's deleteWordRight) should delete \
             the word after the cursor, the same as Ctrl+Delete does on \
             Linux/Windows"
        );
    });
}

/// `KNOWN_GAPS::CTRL_K_CTRL_C_TOGGLES_INSTEAD_OF_ADD_ONLY`. See this gap's
/// `VSCODE_BINDINGS` row (`editor.action.addCommentLine`) for the full
/// writeup: `vscode_ctrl_k_dispatch`'s `"c"` arm calls the same
/// `toggle_comment` its `"u"` arm uses, so pressing Ctrl+K Ctrl+C on an
/// already-commented line uncomments it instead of being a no-op.
#[test]
fn gap_ctrl_k_ctrl_c_only_adds_never_removes() {
    gap_gate("CTRL_K_CTRL_C_TOGGLES_INSTEAD_OF_ADD_ONLY", || {
        let mut e = engine_with("# print(1)\n");
        vscode_mode(&mut e);
        e.view_mut().cursor = Cursor { line: 0, col: 0 };
        e.handle_key("k", Some('k'), true);
        assert!(
            e.vscode_pending_ctrl_k,
            "test setup: Ctrl+K armed the chord"
        );
        e.handle_key("c", Some('c'), true);
        assert!(
            buf(&e).contains('#'),
            "addCommentLine must be idempotent — it must not remove an \
             existing comment marker: {:?}",
            buf(&e)
        );
    });
}

/// `KNOWN_GAPS::CTRL_K_CTRL_U_TOGGLES_INSTEAD_OF_REMOVE_ONLY`. Mirrors
/// `gap_ctrl_k_ctrl_c_only_adds_never_removes` above, for removeCommentLine
/// on an uncommented line.
#[test]
fn gap_ctrl_k_ctrl_u_only_removes_never_adds() {
    gap_gate("CTRL_K_CTRL_U_TOGGLES_INSTEAD_OF_REMOVE_ONLY", || {
        let mut e = engine_with("print(1)\n");
        vscode_mode(&mut e);
        e.view_mut().cursor = Cursor { line: 0, col: 0 };
        e.handle_key("k", Some('k'), true);
        assert!(
            e.vscode_pending_ctrl_k,
            "test setup: Ctrl+K armed the chord"
        );
        e.handle_key("u", Some('u'), true);
        assert!(
            !buf(&e).contains('#'),
            "removeCommentLine must be idempotent — it must not add a \
             comment marker to an already-uncommented line: {:?}",
            buf(&e)
        );
    });
}

/// `KNOWN_GAPS::SHIFT_PAGEUP_PAGEDOWN_SHIFT_BIT_DROPPED_BY_DECODER`. See
/// this gap's two `VSCODE_BINDINGS` rows (`cursorPageUpSelect`/
/// `cursorPageDownSelect`) for the full writeup: unlike its `Home`/`End`
/// siblings, `engine_key_from_ui`'s `NamedKey::PageUp`/`PageDown` arms never
/// look at `shift` at all. Driver-tier (not engine-level), same idiom as
/// the Ctrl+K/Ctrl+Shift+K pair's own driver-tier tests above — this is a
/// decoder bug, not an input-triple the engine could ever usefully receive
/// today (there is no `"Shift_Page_Up"` arm in `handle_vscode_key` either,
/// since nothing can produce that name yet).
#[test]
fn gap_shift_pageup_pagedown_shift_bit_dropped_by_decoder() {
    gap_gate("SHIFT_PAGEUP_PAGEDOWN_SHIFT_BIT_DROPPED_BY_DECODER", || {
        use quadraui::{Key, Modifiers, NamedKey};
        let shift_mods = Modifiers {
            shift: true,
            ..Default::default()
        };
        let (up_name, _, _) =
            engine_key_from_ui(&Key::Named(NamedKey::PageUp), shift_mods, false).unwrap();
        assert_eq!(
            up_name, "Shift_Page_Up",
            "Shift+Page Up must decode distinctly from plain Page Up, the \
             same way Shift+Home already decodes to \"Shift_Home\" — \
             instead it collapses to {up_name:?}"
        );
        let (down_name, _, _) =
            engine_key_from_ui(&Key::Named(NamedKey::PageDown), shift_mods, false).unwrap();
        assert_eq!(
            down_name, "Shift_Page_Down",
            "Shift+Page Down must decode distinctly from plain Page Down — \
             instead it collapses to {down_name:?}"
        );
    });
}

// ─── #1746 review round: gates for the four `Matches`-claimed-but-
//     unreachable chords plus the closeAllEditors false-parity claim ───────

/// `KNOWN_GAPS::CTRL_SHIFT_HOME_END_CTRL_BIT_DROPPED_BY_DECODER`. See this
/// gap's two `VSCODE_BINDINGS` rows (`cursorTopSelect`/`cursorBottomSelect`)
/// for the full writeup: `engine_key_from_ui`'s `NamedKey::Home if shift`/
/// `NamedKey::End if shift` arms return an unconditional `ctrl: false`,
/// before the ctrl-aware arms beneath them ever run. Driver-tier, same
/// idiom as `gap_shift_pageup_pagedown_shift_bit_dropped_by_decoder` above.
#[test]
fn gap_ctrl_shift_home_end_ctrl_bit_dropped_by_decoder() {
    gap_gate("CTRL_SHIFT_HOME_END_CTRL_BIT_DROPPED_BY_DECODER", || {
        use quadraui::{Key, Modifiers, NamedKey};
        let ctrl_shift_mods = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        let (home_name, _, home_ctrl) =
            engine_key_from_ui(&Key::Named(NamedKey::Home), ctrl_shift_mods, false).unwrap();
        assert!(
            home_ctrl,
            "Ctrl+Shift+Home must preserve the ctrl bit so \
             handle_vscode_key can route to DocStart instead of \
             SmartHome — decoded as {home_name:?} with ctrl={home_ctrl}"
        );
        let (end_name, _, end_ctrl) =
            engine_key_from_ui(&Key::Named(NamedKey::End), ctrl_shift_mods, false).unwrap();
        assert!(
            end_ctrl,
            "Ctrl+Shift+End must preserve the ctrl bit so \
             handle_vscode_key can route to DocEnd instead of LineEnd — \
             decoded as {end_name:?} with ctrl={end_ctrl}"
        );
    });
}

/// `KNOWN_GAPS::CTRL_BACKSPACE_DELETE_CTRL_BIT_DROPPED_BY_DECODER`. See
/// this gap's two `VSCODE_BINDINGS` rows (`deleteWordRight`/
/// `deleteWordLeft`) for the full writeup: `engine_key_from_ui`'s
/// `NamedKey::Backspace`/`NamedKey::Delete` arms never inspect the incoming
/// `ctrl` flag at all. Driver-tier, same idiom as the Home/End gate above.
#[test]
fn gap_ctrl_backspace_delete_ctrl_bit_dropped_by_decoder() {
    gap_gate("CTRL_BACKSPACE_DELETE_CTRL_BIT_DROPPED_BY_DECODER", || {
        use quadraui::{Key, Modifiers, NamedKey};
        let ctrl_mods = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let (bs_name, _, bs_ctrl) =
            engine_key_from_ui(&Key::Named(NamedKey::Backspace), ctrl_mods, false).unwrap();
        assert!(
            bs_ctrl,
            "Ctrl+BackSpace must preserve the ctrl bit so \
             handle_vscode_key can route to vscode_delete_word_backward — \
             decoded as {bs_name:?} with ctrl={bs_ctrl}"
        );
        let (del_name, _, del_ctrl) =
            engine_key_from_ui(&Key::Named(NamedKey::Delete), ctrl_mods, false).unwrap();
        assert!(
            del_ctrl,
            "Ctrl+Delete must preserve the ctrl bit so handle_vscode_key \
             can route to vscode_delete_word_forward — decoded as \
             {del_name:?} with ctrl={del_ctrl}"
        );
    });
}

/// `KNOWN_GAPS::CTRL_K_CTRL_W_LEAVES_ONE_TAB_OPEN`. See this gap's
/// `VSCODE_BINDINGS` row (`workbench.action.closeAllEditors`) for the full
/// writeup: `Engine::close_tab` refuses to close the last tab in a single
/// group, so `vscode_ctrl_k_dispatch`'s `"w"` loop always leaves one editor
/// open, unlike VS Code's real `closeAllEditors` which leaves none. This
/// test is the same body the previous review round shipped as a plain
/// (ungated) assertion of `Some(1)` under a `Matches` row — now asserting
/// the correct `Some(0)` outcome under `gap_gate`, so it panics until the
/// divergence is actually fixed.
#[test]
fn gap_ctrl_k_ctrl_w_closes_every_tab() {
    gap_gate("CTRL_K_CTRL_W_LEAVES_ONE_TAB_OPEN", || {
        let mut e = engine_with("aaa\n");
        vscode_mode(&mut e);
        e.new_tab(None);
        e.new_tab(None);
        let group = e.active_group;
        assert_eq!(
            e.editor_groups.get(&group).map(|g| g.tabs.len()),
            Some(3),
            "test setup should have 3 tabs open"
        );
        e.handle_key("k", Some('k'), true);
        assert!(
            e.vscode_pending_ctrl_k,
            "test setup: Ctrl+K armed the chord"
        );
        e.handle_key("w", Some('w'), true);
        assert!(!e.vscode_pending_ctrl_k);
        assert_eq!(
            e.editor_groups.get(&group).map(|g| g.tabs.len()),
            Some(0),
            "Ctrl+K Ctrl+W (closeAllEditors) should close every tab, \
             unlike Ctrl+W (closeActiveEditor) which must leave at least \
             one"
        );
    });
}

// ─── #1746 deliverable 1: gates for the editor-group/tab family ────────────

/// `KNOWN_GAPS::CTRL_W_CLOSE_ACTIVE_EDITOR_UNBOUND_IN_VSCODE_MODE`. See this
/// gap's `VSCODE_BINDINGS` row (`workbench.action.closeActiveEditor`) for
/// the full writeup: `handle_vscode_key`'s ctrl match has no `"w"` arm
/// outside the Ctrl+K chord, so a plain Ctrl+W is a no-op.
#[test]
fn gap_ctrl_w_closes_active_editor_in_vscode_mode() {
    gap_gate("CTRL_W_CLOSE_ACTIVE_EDITOR_UNBOUND_IN_VSCODE_MODE", || {
        let mut e = engine_with("aaa\n");
        vscode_mode(&mut e);
        e.new_tab(None);
        let group = e.active_group;
        assert_eq!(
            e.editor_groups.get(&group).map(|g| g.tabs.len()),
            Some(2),
            "test setup should have 2 tabs open"
        );
        e.handle_key("w", Some('w'), true);
        assert_eq!(
            e.editor_groups.get(&group).map(|g| g.tabs.len()),
            Some(1),
            "Ctrl+W (closeActiveEditor) should close the active tab"
        );
    });
}

/// `KNOWN_GAPS::CTRL_BACKSLASH_SPLIT_EDITOR_UNBOUND`. See this gap's
/// `VSCODE_BINDINGS` row (`workbench.action.splitEditor`) for the full
/// writeup: `handle_vscode_key`'s ctrl match has no arm at all for the
/// literal backslash key.
#[test]
fn gap_ctrl_backslash_splits_editor() {
    gap_gate("CTRL_BACKSLASH_SPLIT_EDITOR_UNBOUND", || {
        let mut e = engine_with("aaa\n");
        vscode_mode(&mut e);
        let win_a = e.active_window_id().0;
        e.handle_key("\\", Some('\\'), true);
        let win_b = e.active_window_id().0;
        assert_ne!(
            win_a, win_b,
            "Ctrl+\\ (splitEditor) should create and focus a new window, \
             the same way Engine::split_window does"
        );
    });
}

/// `KNOWN_GAPS::CTRL_PAGEUP_PAGEDOWN_PREV_NEXT_EDITOR_UNBOUND`. See this
/// gap's two `VSCODE_BINDINGS` rows (`previousEditor`/`nextEditor`) for the
/// full writeup: `handle_vscode_key`'s ctrl match has no `"Page_Up"`/
/// `"Page_Down"` arms at all.
#[test]
fn gap_ctrl_pagedown_switches_to_next_editor() {
    gap_gate("CTRL_PAGEUP_PAGEDOWN_PREV_NEXT_EDITOR_UNBOUND", || {
        let mut e = engine_with("aaa\n");
        vscode_mode(&mut e);
        e.new_tab(None);
        let group = e.active_group;
        let before = e.editor_groups.get(&group).map(|g| g.active_tab);
        e.handle_key("Page_Down", None, true);
        let after = e.editor_groups.get(&group).map(|g| g.active_tab);
        assert_ne!(
            before, after,
            "Ctrl+PageDown (nextEditor) should switch to the next tab in \
             the group — stayed at {before:?}"
        );
    });
}

/// `KNOWN_GAPS::CTRL_1_FOCUS_FIRST_EDITOR_GROUP_UNBOUND`. See this gap's
/// `VSCODE_BINDINGS` row (`workbench.action.focusFirstEditorGroup`) for the
/// full writeup: `handle_vscode_key`'s ctrl match has no digit arms at all.
#[test]
fn gap_ctrl_1_focuses_first_editor_group() {
    gap_gate("CTRL_1_FOCUS_FIRST_EDITOR_GROUP_UNBOUND", || {
        use vimcode_core::core::window::SplitDirection;
        let mut e = engine_with("file1\n");
        vscode_mode(&mut e);
        e.new_tab(None);
        let gid = e.active_group;
        // Move the second tab into a brand-new split group, the same way
        // `tests/tab_drag.rs::move_tab_to_new_split_right` does — leaves
        // `gid` (the first group) with one tab, and focuses the new group.
        e.move_tab_to_new_split(gid, 1, gid, SplitDirection::Vertical, false);
        assert_ne!(
            e.active_group, gid,
            "test setup: split should have created and focused a new group"
        );
        e.handle_key("1", Some('1'), true);
        assert_eq!(
            e.active_group, gid,
            "Ctrl+1 (focusFirstEditorGroup) should focus the first editor \
             group"
        );
    });
}

/// `KNOWN_GAPS::CTRL_K_FOLD_ALL_UNFOLD_ALL_UNBOUND`. See this gap's two
/// `VSCODE_BINDINGS` rows (`editor.foldAll`/`editor.unfoldAll`) for the
/// full writeup: `vscode_ctrl_k_dispatch`'s match has no `"0"`/`"j"` arms.
/// Drives the chord all the way from Ctrl+K through the second key, same
/// idiom as `gap_ctrl_k_ctrl_w_closes_every_tab` above — not a decoder-tier
/// gate, since the ambiguity here is a missing dispatch arm, not a lost
/// modifier bit.
#[test]
fn gap_ctrl_k_ctrl_0_folds_all() {
    gap_gate("CTRL_K_FOLD_ALL_UNFOLD_ALL_UNBOUND", || {
        let mut e = engine_with("fn a() {\n    1\n}\nfn b() {\n    2\n}\n");
        vscode_mode(&mut e);
        e.handle_key("k", Some('k'), true);
        assert!(
            e.vscode_pending_ctrl_k,
            "test setup: Ctrl+K armed the chord"
        );
        e.handle_key("0", Some('0'), true);
        assert!(
            e.view().fold_at(0).is_some() && e.view().fold_at(3).is_some(),
            "Ctrl+K Ctrl+0 (foldAll) should fold every foldable region \
             (both fn bodies), the same way Ctrl+Shift+[ folds one"
        );
    });
}
