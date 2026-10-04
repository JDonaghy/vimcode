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
//!    row marked `Matches` / `MacDiverges` / `Missing` against what vimcode
//!    does. Every `Matches` row that the existing 62 tests don't already
//!    exercise gets a new engine-level test here (same style as
//!    `tests/vscode_mode.rs`'s own `vk`/`handle_key` idiom — this is
//!    deliberately *not* a driver-tier test: the gap this table closes is
//!    "is every implemented binding asserted", not "does a real keypress
//!    reach the engine", which is (2) below).
//! 2. **Reachability matrix** — [`REACHABILITY_TABLE`], recording whether
//!    each ambiguous or OS-sensitive chord actually reaches the engine
//!    through real input on TUI-legacy-xterm, TUI-kitty, ConPTY, GTK and a
//!    hypothetical macOS GUI backend — plus driver-tier
//!    (`render::engine_key_from_ui`, the one shared decoder both GTK and TUI
//!    call per its own module doc) tests for the cases that are reachable or
//!    unreachable for a provable reason.
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
//! this repo's platform-neutrality rule for no benefit. The one place Cmd
//! *does* matter is the still-unbuilt macOS **GUI** backend (`src/macos/`,
//! gated out entirely on this non-Darwin host) — see [`REACHABILITY_TABLE`]'s
//! `macos_gui` column and the gap list in this issue's closing report for
//! what that backend will need to do differently from the terminal.
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
    /// exactly (for the `Os` the row is cited for).
    Matches,
    /// Implemented correctly for Win/Linux, but VS Code's Mac default for
    /// this command id is not a plain Ctrl-to-Cmd substitution — it is a
    /// genuinely different chord or is unreachable in a terminal. Not
    /// gated: there is no macOS GUI backend in this repo to regress against
    /// (see module doc), so there is nothing to bidirectionally gate yet.
    MacDiverges,
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
    // `gap_ctrl_alt_up_is_move_line_not_insert_cursor_above` and its four
    // siblings below for the now-`FixLanded`, deleted `KNOWN_GAPS` entries,
    // and `render::alt_key_router_tests` for the driver-adjacent coverage of
    // `route_alt_key` itself.
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
               gap_ctrl_alt_up_is_move_line_not_insert_cursor_above (its body \
               asserts the fix, forcing `KNOWN_GAPS` deletion) and \
               `render::alt_key_router_tests::\
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
               function Vim mode's Ctrl-O already uses. Ctrl+Alt+Left is \
               VSCode mode's new alternate home for the sidebar-resize this \
               chord used to always perform (Vim mode keeps plain \
               Alt+Left/Right for that, unchanged).",
    },
    VscodeBinding {
        command_id: "workbench.action.navigateForward",
        os: Os::WinLinux,
        vscode_chord: "Alt+Right",
        vimcode_key: "Alt_Right",
        status: Status::Matches,
        note: "Same as navigateBack, mirrored for `jump_list_forward` / \
               Ctrl-I; Ctrl+Alt+Right is the sidebar-resize alternate home.",
    },
    VscodeBinding {
        command_id: "editor.action.jumpToBracket",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+\\",
        vimcode_key: "Shift_backslash",
        status: Status::Matches,
        note: "`render::engine_key_from_ui` now has shift-aware arms for \
               Ctrl+\\ (the literal shifted glyph `'|'`, and the base key \
               plus an explicit Shift bit for kitty/CSI-u) producing \
               `\"Shift_backslash\"`, distinct from plain Ctrl+\\'s \
               `\"backslash\"` (still bound to `open_editor_group`, Vim \
               mode's own Ctrl+\\ meaning, unaffected). \
               `handle_vscode_key`'s new `\"Shift_backslash\" | \"|\"` arm \
               calls `Engine::move_to_matching_bracket` — the same search \
               Vim's `%` already uses. A legacy (non-keyboard-enhanced) \
               terminal still cannot report the Shift bit for this chord at \
               all (same ANSI-C0 shift-blindness as the Ctrl+K/Ctrl+P family \
               — see REACHABILITY_TABLE's new row below), so it stays \
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
        note: "Covered by tests/vscode_mode.rs::test_vscode_ctrl_d_*.",
    },
    VscodeBinding {
        command_id: "editor.action.selectHighlights",
        os: Os::WinLinux,
        vscode_chord: "Ctrl+Shift+L",
        vimcode_key: "L",
        status: Status::Matches,
        note: "Covered by tests/vscode_mode.rs::test_vscode_ctrl_shift_l_*.",
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
    // ── Confirmed Mac divergence, not gated (no macOS GUI backend yet) ──
    VscodeBinding {
        command_id: "workbench.action.toggleSidebarVisibility",
        os: Os::Mac,
        vscode_chord: "Cmd+B",
        vimcode_key: "b (Ctrl only)",
        status: Status::MacDiverges,
        note: "A plain Ctrl-to-Cmd substitution — reachable once a macOS GUI \
               backend exists to decode Cmd at all (none does today, see \
               module doc); on macOS *TUI* this stays Ctrl+B per this \
               issue's own decision (Cmd never reaches a terminal).",
    },
    VscodeBinding {
        command_id: "workbench.action.togglePanel",
        os: Os::Mac,
        vscode_chord: "Cmd+J",
        vimcode_key: "j (Ctrl only)",
        status: Status::MacDiverges,
        note: "Same shape as toggleSidebarVisibility.",
    },
    VscodeBinding {
        command_id: "workbench.action.openSettings",
        os: Os::Mac,
        vscode_chord: "Cmd+,",
        vimcode_key: "comma (Ctrl only)",
        status: Status::MacDiverges,
        note: "Same shape as toggleSidebarVisibility; also the universal \
               macOS \"Preferences\" chord in every native Mac app.",
    },
    VscodeBinding {
        command_id: "editor.action.commentLine",
        os: Os::Mac,
        vscode_chord: "Cmd+/",
        vimcode_key: "slash (Ctrl only)",
        status: Status::MacDiverges,
        note: "Same shape as toggleSidebarVisibility.",
    },
    VscodeBinding {
        command_id: "actions.find",
        os: Os::Mac,
        vscode_chord: "Cmd+F",
        vimcode_key: "f (Ctrl only)",
        status: Status::MacDiverges,
        note: "Same shape as toggleSidebarVisibility.",
    },
    VscodeBinding {
        command_id: "editor.action.clipboardCutAction",
        os: Os::Mac,
        vscode_chord: "Cmd+X",
        vimcode_key: "x (Ctrl only)",
        status: Status::MacDiverges,
        note: "Same shape as toggleSidebarVisibility (also Cmd+C/Cmd+V/Cmd+A \
               for copy/paste/select-all).",
    },
    VscodeBinding {
        command_id: "editor.action.startFindReplaceAction",
        os: Os::Mac,
        vscode_chord: "Cmd+Option+F",
        vimcode_key: "h (Ctrl only)",
        status: Status::MacDiverges,
        note: "NOT a Ctrl-to-Cmd substitution: Cmd+H is macOS's system-wide \
               \"Hide application\" chord, so VS Code's Mac default for \
               Find & Replace is a different chord entirely \
               (Cmd+Option+F), not Cmd+H. A future macOS GUI backend must \
               bind that chord explicitly rather than assuming the Linux \
               key name's Cmd equivalent.",
    },
    VscodeBinding {
        command_id: "cursorWordEndRight / cursorWordLeft",
        os: Os::Mac,
        vscode_chord: "Option+Right / Option+Left",
        vimcode_key: "Right/Left (Ctrl only)",
        status: Status::MacDiverges,
        note: "NOT a Ctrl-to-Cmd substitution: on Mac, word-wise navigation \
               is Option+arrow, while Cmd+arrow is mapped to \
               cursorHome/cursorEnd (line start/end) — the two modifiers \
               swap roles relative to Linux/Windows' Ctrl=word, \
               Home/End=line split. A future macOS GUI backend's Cmd/Option \
               handling needs this restated explicitly; it is not safe to \
               synthesize from vimcode's existing Ctrl+Right/Left == \
               word-move binding by substitution.",
    },
    VscodeBinding {
        command_id: "cursorTop / cursorBottom",
        os: Os::Mac,
        vscode_chord: "Cmd+Up / Cmd+Down",
        vimcode_key: "(no vscode.rs binding for Ctrl+Up/Down at all)",
        status: Status::MacDiverges,
        note: "Mac's document-start/end chord is Cmd+Up/Down, mirroring the \
               cursorWordEndRight/cursorWordLeft split above; vimcode has no \
               Ctrl+Up/Down binding on Linux/Windows either (`Engine::
               handle_vscode_key`'s ctrl match has no \"Up\"/\"Down\" arm), \
               so there is nothing to even substitute from today.",
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
    // Select "hello" and copy it.
    e.visual_anchor = Some(Cursor { line: 0, col: 0 });
    e.mode = Mode::Visual;
    e.view_mut().cursor = Cursor { line: 0, col: 4 };
    e.handle_key("c", Some('c'), true);
    // Clear selection, move to end of buffer, paste.
    e.visual_anchor = None;
    e.mode = Mode::Insert;
    e.view_mut().cursor = Cursor { line: 0, col: 11 };
    e.handle_key("v", Some('v'), true);
    assert!(
        buf(&e).starts_with("hello world") && buf(&e).len() > "hello world\n".len(),
        "paste did not insert the copied text: {:?}",
        buf(&e)
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

// ═══════════════════════════════════════════════════════════════════════════
// 2. Reachability matrix — does real input ever produce the key the table
//    above says `handle_vscode_key` wants?
// ═══════════════════════════════════════════════════════════════════════════

/// One surface vimcode ships (or will ship — `macos_gui` names a backend
/// that does not exist in this repo yet, see the module doc).
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
        chord: "Ctrl+K (chord prefix) vs Ctrl+Shift+K (delete line)",
        tui_legacy_xterm: Reach::No,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::No,
        gtk: Reach::Yes,
        macos_tui: Reach::No,
        macos_gui: Reach::NotApplicable,
        reason: "Ctrl+<letter> is an ANSI C0 control code (`letter & 0x1F`) \
                 — the byte stream is shift-invariant for letters, so a \
                 legacy terminal cannot send Shift information for this \
                 chord at all; crossterm's legacy decoder can only report \
                 Char('k')+CONTROL, with no SHIFT bit, regardless of \
                 whether Shift was physically held. Only the kitty keyboard \
                 protocol / CSI-u (TUI) or GDK's always-resolved keysym \
                 (GTK) carries that bit.",
    },
    ReachabilityRow {
        chord: "Ctrl+P (quick open) vs Ctrl+Shift+P (command palette)",
        tui_legacy_xterm: Reach::No,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::No,
        gtk: Reach::Yes,
        macos_tui: Reach::No,
        macos_gui: Reach::NotApplicable,
        reason: "Same C0-control-code argument as Ctrl+K/Ctrl+Shift+K, \
                 substituting 'p' for 'k'.",
    },
    ReachabilityRow {
        chord: "Ctrl+[ / Ctrl+Shift+[ (outdent vs fold)",
        tui_legacy_xterm: Reach::Yes,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::Yes,
        gtk: Reach::Yes,
        macos_tui: Reach::Yes,
        macos_gui: Reach::NotApplicable,
        reason: "Not actually ambiguous: `engine_key_from_ui` has an \
                 explicit `!keyboard_enhanced` fallback that recognises the \
                 legacy byte for Ctrl+Shift+[ (which a non-enhanced \
                 terminal sends as Ctrl+3's byte) and resolves it to \
                 `Shift_bracketleft` — see that function's `lower == '3'` \
                 arm. Listed here precisely to document that this pair is \
                 *not* in the same unresolved state as Ctrl+K/Ctrl+P above.",
    },
    ReachabilityRow {
        chord: "Alt+Up / Alt+Down (move line)",
        tui_legacy_xterm: Reach::Yes,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::Yes,
        gtk: Reach::Yes,
        macos_tui: Reach::Yes,
        macos_gui: Reach::NotApplicable,
        reason: "Alt+arrow is a dedicated escape sequence with no C0-style \
                 ambiguity; reachable everywhere Alt itself is deliverable, \
                 which includes every surface vimcode ships today (macOS \
                 Option key behaves as Alt in a terminal).",
    },
    ReachabilityRow {
        chord: "Ctrl+Alt+Up / Ctrl+Alt+Down (VS Code's real insertCursorAbove/Below)",
        tui_legacy_xterm: Reach::Yes,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::Yes,
        gtk: Reach::Yes,
        macos_tui: Reach::Yes,
        macos_gui: Reach::NotApplicable,
        reason: "#1744: fixed, and was never actually a terminal-encoding \
                 limitation like the Ctrl+K/Ctrl+P family above — Ctrl+Alt+ \
                 arrow has a dedicated, unambiguous escape sequence on every \
                 surface, same as plain Alt+arrow. The old `Reach::No` row \
                 here recorded a gap in vimcode's own `render::route_alt_key` \
                 (no `ctrl` parameter at all), not a wire-protocol ambiguity; \
                 that parameter now exists and forwards `ctrl` through to \
                 `Engine::handle_vscode_key`, which tells Ctrl+Alt+Up/Down \
                 apart from plain Alt+Up/Down via its own `if ctrl` guard.",
    },
    ReachabilityRow {
        chord: "Ctrl+\\ (split editor) vs Ctrl+Shift+\\ (jump to matching bracket)",
        tui_legacy_xterm: Reach::No,
        tui_kitty_or_csiu: Reach::Yes,
        conpty_legacy: Reach::No,
        gtk: Reach::Yes,
        macos_tui: Reach::No,
        macos_gui: Reach::NotApplicable,
        reason: "Same C0-control-code argument as Ctrl+K/Ctrl+Shift+K above: \
                 Ctrl+\\ is the ANSI C0 byte 0x1C regardless of Shift (the \
                 physical key's shifted glyph, '|', XORs down to the exact \
                 same control byte), so a legacy terminal has no way to \
                 report the Shift bit for this chord at all. kitty/CSI-u \
                 (explicit Shift modifier alongside the base key) and GTK \
                 (GDK hands over the literal shifted glyph '|' directly) can \
                 both report it; `render::engine_key_from_ui` has arms for \
                 both shapes, producing `\"Shift_backslash\"`.",
    },
    ReachabilityRow {
        chord: "Cmd+<key> (any VS Code Mac-default chord)",
        tui_legacy_xterm: Reach::NotApplicable,
        tui_kitty_or_csiu: Reach::NotApplicable,
        conpty_legacy: Reach::NotApplicable,
        gtk: Reach::NotApplicable,
        macos_tui: Reach::No,
        macos_gui: Reach::NotApplicable,
        reason: "Cmd is a GUI-only modifier; no terminal emulator on any OS \
                 forwards it to the foreground application, so it is \
                 structurally unreachable on macOS TUI — this is the fact \
                 behind this issue's own macOS-terminal decision (module \
                 doc). `macos_gui` is NotApplicable rather than `No` \
                 because no macOS GUI backend exists in this repo to even \
                 ask the question of (`src/macos/` is a thin shell wrapper, \
                 gated out entirely on this non-Darwin host).",
    },
];

#[test]
fn reachability_table_rows_are_well_formed() {
    for row in REACHABILITY_TABLE {
        assert!(!row.chord.is_empty());
        assert!(!row.reason.is_empty());
    }
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
/// `CTRL_SHIFT_BACKSLASH_JUMP_TO_BRACKET_UNBOUND`), so it is empty — the
/// `gap_*` tests below are now plain (ungated) assertions of the fixed
/// behaviour, same as `src/harness.rs`'s own `KNOWN_BUGS` empty-list state.
const KNOWN_GAPS: &[&str] = &[];

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
/// #1744 fixed every gap `KNOWN_GAPS` ever named, so no `#[test]` in this
/// file calls this wrapper today (each former `gap_*` test is now a plain,
/// ungated assertion — see e.g. `ctrl_alt_up_inserts_cursor_above_not_move_line`).
/// Kept, not deleted: this is this file's whole reason for existing per its
/// own module doc (deliverable 3, "a bidirectional gate for every binding
/// `VSCODE_BINDINGS` marks `Missing` or wrong") — a future mismatch gets
/// pinned by calling this, exactly as `src/harness.rs`'s `known_bug_gate`
/// stays live infrastructure independent of how many bugs `KNOWN_BUGS`
/// currently lists.
#[allow(dead_code)]
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
/// (`Shift+Alt+Down`, `Engine::vscode_copy_line_down`).
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
/// `render::engine_key_from_ui` now has shift-aware arms producing
/// `"Shift_backslash"`, and `handle_vscode_key` has a
/// `"Shift_backslash" | "|"` arm calling `move_to_matching_bracket`.
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
