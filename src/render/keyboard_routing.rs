use super::*;

// ─── Modal keyboard routing (#734 slice 1) ────────────────────────────────────
//
// The keyboard twin of `route_modal_overlay_click` above, and the top rung
// of the precedence ladder #734 exists to state once. Before this, the ladder
// was transcribed per backend and had drifted:
//
//   TUI  dialog → folder picker → activity bar → sidebar (which carried its
//        OWN copy of the context-menu rung) → … → context menu → engine
//   GTK  context menu (hand-rolled, NOT `Engine::handle_context_menu_key`) →
//        activity bar → explorer (which carried a SECOND hand-rolled copy of
//        the context-menu rung, plus a dialog patch-up) → … → engine
//
// Four hand-rolled copies of one rung, and GTK had no top-level dialog rung
// at all — so a dialog opened while the activity bar / explorer / an
// extension panel held focus lost its keys to that panel.
//
// `Engine::handle_key` already sequences these three correctly for the keys
// that reach it (`core/engine/keys.rs`: spell suggestions → dialog → context
// menu). This function states the same order for the *backends*, so their
// focus ladders can no longer cut in front of it.

/// Which modal surface owns a keystroke, resolved before either backend
/// consults its own focus ladder (activity bar, sidebar, extension panel,
/// terminal, editor).
///
/// This is pure state inspection over [`Engine`] — no backend-specific
/// content, which is precisely why both backends were able to transcribe
/// it independently and then drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKeyRoute {
    /// A modal [`Engine::handle_key`] already arbitrates internally —
    /// spell-suggestion selection or a modal dialog. The backend hands the
    /// key straight to `Engine::handle_key` and consumes it.
    Engine,
    /// The context-menu popup. The backend calls
    /// [`Engine::handle_context_menu_key`] and dispatches the action it
    /// returns against [`Engine::context_menu_target_path`] — the one part
    /// that stays backend-side, because "new_file" / "open_terminal" need
    /// backend plumbing.
    ContextMenu,
    /// No modal is up; the caller continues down its own ladder.
    None,
}

/// Resolve the top rung of the *keyboard* ladder against engine state.
///
/// Order matches `Engine::handle_key`'s own: spell suggestions, then a
/// modal dialog, then the context menu.
pub fn route_modal_key(engine: &Engine) -> ModalKeyRoute {
    // Spell-suggestion selection intercepts all keys (`keys.rs`'s first
    // branch); a dialog is modal and intercepts everything below it.
    // The change-review surface (#955, shared with #525) is the same
    // shape: a full-viewport overlay that must own every keypress
    // regardless of which panel would otherwise have focus — in
    // particular, the AI panel's own focus route sends keys straight to
    // `render::route_ai_chat_event`, bypassing `Engine::handle_key`
    // entirely, so without this a keypress meant for a diff opened
    // *while* the AI panel has focus (exactly when a tool-call diff
    // arrives) would type into the chat input instead.
    // `handle_change_review_key` (inside `Engine::handle_key`, same place
    // as the other two) is the actual interception.
    if engine.spell_suggestions.is_some()
        || engine.dialog.is_some()
        || engine.change_review.is_some()
    {
        return ModalKeyRoute::Engine;
    }

    // The context menu is modal too, but unlike the two above its confirmed
    // action needs backend plumbing, so it gets its own route rather than
    // being folded into `Engine`.
    if engine.context_menu.is_some() {
        return ModalKeyRoute::ContextMenu;
    }

    ModalKeyRoute::None
}

// ─── Focus-owner keyboard routing (#757 / #734 slice 2) ──────────────────────
//
// The rung directly beneath [`route_modal_key`]: once no modal surface has
// claimed the key, the *focus owners* get their turn — the activity bar, and
// then whichever sidebar panel holds keyboard focus.
//
// Before this, the ladder was transcribed per backend and had drifted in four
// separate ways:
//
//   TUI (`handle_sidebar_focused_key`, 442 lines — now `handle_focus_owner_key`)
//        activity bar → [gate: `sidebar.has_focus && !picker_open &&
//        !terminal_has_focus`] → search → debug → ext panel → extensions →
//        settings → AI → source control → explorer (unguarded fallback),
//        with each panel selected by `active_panel_is(PANEL_*)` — the
//        *visible* panel — rather than by the engine's focus flag.
//   GTK (the block inside `handle_key_press`)
//        activity bar → explorer → ext panel → extensions → settings →
//        search → source control → debug → AI, each selected by the engine's
//        `*_has_focus` flag, with no picker/terminal gate at all.
//
// The four divergences, all of them user-visible:
//
//  1. **Picker.** TUI suppressed the whole band while `picker_open`; GTK did
//     not. Opening the command palette with the explorer focused sent j/k to
//     the explorer on GTK and to the palette on TUI.
//  2. **Terminal.** TUI suppressed the band while `terminal_has_focus` (the
//     "Press Enter to close…" state after an extension install); GTK did not,
//     so those keys were eaten by whichever panel still held a focus flag.
//  3. **Ext panel vs explorer order.** `Engine::activity_bar_activate` sets
//     `ext_panel_has_focus` *without* clearing `explorer_has_focus` (unlike
//     `focus_sidebar_panel`, which calls `clear_sidebar_focus` first), so both
//     flags can be true at once. TUI checked the ext panel first and got the
//     plugin panel; GTK checked the explorer first and sent the keys to a
//     panel that is not even on screen.
//  4. **Visible panel vs focused panel.** TUI keyed its arms off
//     `active_panel_is`, GTK off the focus flags. They agree whenever
//     `focus_sidebar_panel` put them in sync and disagree the moment anything
//     moves one without the other.
//
// This function states the ladder once. Both backends keep their own *sinks*
// (`handle_focus_owner_key` on TUI, the `match` in `handle_key_press` on GTK)
// because the per-panel dispatch needs backend-local plumbing — key-name
// translation tables, `TuiSidebar::ext_panel_name`, GTK's
// `focus_after_sidebar_key` — exactly as `route_modal_key` left
// `ContextMenu`'s action dispatch backend-side.
//
// **Verdict on two rungs deliberately *not* converged here:**
//
//  * *Completion popup* (#734 slice 1 listed it as unconverged). Re-measured
//    on `develop`: **neither backend intercepts completion keys.** There is no
//    `completion_idx` read anywhere in either keyboard ladder — both hand the
//    key to `Engine::handle_key`, whose own precedence chain
//    (`core/engine/keys.rs`) owns Ctrl-N/Ctrl-P/Tab/Up/Down over the popup.
//    The rung is already stated once, in the engine. Promoting it to
//    [`ModalKeyRoute`] would *add* behaviour — making the popup outrank the
//    sidebar band — with no bug behind it, so it is left alone.
//  * *Folder picker* (#815): `quadraui::FolderPickerController` now owns its
//    own key handling directly (`FolderPickerController::handle`), so there
//    is nothing left to state in this ladder — both backends check
//    `folder_picker.is_some()` right after `route_modal_key` declines (a
//    modal dialog still outranks the picker) and hand the raw event straight
//    to the controller instead.

/// Which focus owner owns a keystroke, resolved after [`route_modal_key`] has
/// declined it and before either backend reaches its editor/terminal tier.
///
/// Pure state inspection over [`Engine`] plus the caller's own "the sidebar
/// band holds the keyboard" latch — see [`route_focus_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusKeyRoute {
    /// The activity bar (toolbar) strip. j/k move, l/Enter activate, h/Esc
    /// leave, q collapses the sidebar.
    ActivityBar,
    /// A plugin-provided ("extension") sidebar panel —
    /// `Engine::handle_ext_panel_key` / `handle_ext_panel_input_key`.
    ExtPanel,
    /// The search/replace panel — `dispatch_search_sidebar_key_unified`.
    Search,
    /// The debug (DAP) panel — `SidebarSystem::handle` first, then
    /// `dispatch_dap_sidebar_action_key`.
    Debug,
    /// The extensions *marketplace* panel —
    /// `dispatch_ext_sidebar_key_unified`.
    ExtSidebar,
    /// The settings panel — `Engine::handle_settings_key`.
    Settings,
    /// The AI assistant panel — `route_ai_chat_event` (needs a live
    /// `Backend`, like [`FocusKeyRoute::Debug`]; see that variant's doc).
    Ai,
    /// The source-control panel — `dispatch_sc_sidebar_key_unified`.
    SourceControl,
    /// The Board panel (#521) — `Engine::dispatch_board_key_unified`.
    Board,
    /// The file explorer — `Engine::dispatch_explorer_key`. Also the
    /// terminal fallback: a key that reaches the sidebar band and matches no
    /// other panel lands here rather than falling through to the editor,
    /// mirroring the unguarded trailing block TUI's ladder ended with.
    Explorer,
    /// No focus owner claims the key; the caller continues down its own
    /// ladder (editor, terminal, …).
    None,
}

/// Resolve the focus-owner rung against engine state.
///
/// `sidebar_band_focused` is the caller's own latch for "the sidebar band, as
/// opposed to the editor, currently holds the keyboard". TUI passes
/// `TuiSidebar::has_focus`; GTK, which keeps no such latch, passes
/// [`Engine::sidebar_has_focus`] — the disjunction of the very flags the arms
/// below test, so the gate is a no-op there and GTK's per-flag behaviour is
/// preserved exactly.
///
/// [`FocusKeyRoute::ActivityBar`] is resolved *above* that gate on purpose:
/// `Engine::dispatch_explorer_key`'s `FocusToolbar` result calls
/// `activity_bar_focus_in_at` while the backend clears its band latch, so the
/// activity bar is routinely focused with the latch already false.
pub fn route_focus_key(engine: &Engine, sidebar_band_focused: bool) -> FocusKeyRoute {
    // A picker modal (command palette / fuzzy finder / live grep) and the
    // terminal PTY both outrank every focus owner below. TUI gated on both;
    // GTK gated on neither, which is divergences 1 and 2 above.
    if engine.picker_open || engine.terminal_has_focus {
        return FocusKeyRoute::None;
    }

    if engine.activity_bar_focused {
        return FocusKeyRoute::ActivityBar;
    }

    if !sidebar_band_focused {
        return FocusKeyRoute::None;
    }

    // Ext panel first: it is the one flag that can be set alongside a stale
    // built-in panel flag (divergence 3).
    if engine.ext_panel_has_focus {
        return FocusKeyRoute::ExtPanel;
    }

    // Each built-in panel matches on its focus flag *or* on being the visible
    // panel — the union of the two predicates the backends used to disagree
    // over (divergence 4). They coincide whenever `focus_sidebar_panel` set
    // them together, which is the overwhelmingly common case.
    if engine.search_has_focus || engine.active_panel_is(PANEL_SEARCH) {
        return FocusKeyRoute::Search;
    }
    if engine.dap_sidebar_has_focus || engine.active_panel_is(PANEL_DEBUG) {
        return FocusKeyRoute::Debug;
    }
    if engine.ext_sidebar_has_focus || engine.active_panel_is(PANEL_EXTENSIONS) {
        return FocusKeyRoute::ExtSidebar;
    }
    if engine.settings_has_focus || engine.active_panel_is(PANEL_SETTINGS) {
        return FocusKeyRoute::Settings;
    }
    if engine.ai_has_focus || engine.active_panel_is(PANEL_AI) {
        return FocusKeyRoute::Ai;
    }
    if engine.sc_has_focus || engine.active_panel_is(PANEL_GIT) {
        return FocusKeyRoute::SourceControl;
    }
    if engine.board_has_focus || engine.active_panel_is(PANEL_BOARD) {
        return FocusKeyRoute::Board;
    }

    // The explorer is the unguarded fallback, not a guarded arm — see the
    // variant doc.
    FocusKeyRoute::Explorer
}

/// Outcome of feeding a keystroke to [`route_sidebar_chord_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarChordAction {
    /// Ctrl-W armed the latch (`Engine::sidebar_ctrl_w_pending`); nothing
    /// else to do for this keypress but redraw.
    Armed,
    /// The chord's follow-up key moved focus out of the sidebar band —
    /// `h`/`Left` to the activity bar toolbar, `l`/`Right` to the editor.
    /// `Engine::clear_sidebar_focus` (and, for `h`/`Left`,
    /// `Engine::activity_bar_focus_in_at`) already ran; the caller only
    /// needs to clear its own "sidebar band holds the keyboard" latch, if it
    /// keeps one (TUI's `TuiSidebar::has_focus` does; GTK has none).
    FocusOut,
    /// The chord's follow-up key was consumed but wasn't a recognised
    /// direction — ignored, like an unmapped Ctrl-W combo in Vim's own
    /// window commands.
    Consumed,
}

/// Ctrl-W sidebar chord: arm on `Ctrl-W`, then on the very next keypress
/// navigate `h`/`Left` to the activity bar toolbar or `l`/`Right` to the
/// editor, consuming any other follow-up. `None` means the key was neither
/// the arming chord nor a pending follow-up — the caller's own ladder
/// continues unclaimed.
///
/// A Vim chord, so — like [`route_debug_fkey`] and the other shared rungs in
/// this file — it stays a plain key-name match rather than a declared
/// accelerator. Only meaningful while some sidebar panel (not the activity
/// bar itself) holds focus; both callers only reach this after their own
/// focus-owner routing has already claimed the key for such a panel, so
/// there is no separate "is the sidebar band focused" gate here.
///
/// Promoted out of TUI-only `TuiSidebar::pending_ctrl_w` (#1419, closing
/// #406): GTK kept no per-keypress chord latch of its own to hang that field
/// on, so Ctrl-W h/l silently did nothing there. `Engine::sidebar_ctrl_w_pending`
/// is the shared latch both backends now read and write through this
/// function, so GTK gets the same navigation TUI already had.
///
/// `key_name`/`unicode` are [`engine_key_from_ui`]'s own output: a plain
/// letter like `h`/`l`/`w` arrives as `unicode`, with `key_name` empty
/// (`engine_key_from_ui` only fills `key_name` for `Ctrl`-held or named
/// keys), while `Left`/`Right` arrive as `key_name` with `unicode: None` —
/// so both must be checked, matching every other rung in this file
/// ([`dispatch_sidebar_panel_key`], [`route_terminal_key`], …).
pub fn route_sidebar_chord_key(
    engine: &mut Engine,
    key_name: &str,
    unicode: Option<char>,
    ctrl: bool,
) -> Option<SidebarChordAction> {
    if ctrl && key_name.eq_ignore_ascii_case("w") {
        engine.sidebar_ctrl_w_pending = true;
        return Some(SidebarChordAction::Armed);
    }
    if !engine.sidebar_ctrl_w_pending {
        return None;
    }
    engine.sidebar_ctrl_w_pending = false;
    let to_toolbar = key_name == "Left" || unicode == Some('h');
    let to_editor = key_name == "Right" || unicode == Some('l');
    Some(if to_toolbar {
        // Panel -> activity bar toolbar.
        let idx = engine.activity_bar_toolbar_idx_for_active_panel();
        engine.clear_sidebar_focus();
        engine.activity_bar_focus_in_at(idx);
        SidebarChordAction::FocusOut
    } else if to_editor {
        // Panel -> editor.
        engine.clear_sidebar_focus();
        SidebarChordAction::FocusOut
    } else {
        SidebarChordAction::Consumed // Unknown Ctrl-W combo: ignore.
    })
}

/// What a key does once [`FocusKeyRoute::ActivityBar`] has claimed it.
///
/// The activity bar's key table was the one part of that arm both backends
/// transcribed *identically enough to look fine* and still disagreed on: GTK
/// guarded activation and focus-out with `!ctrl`, TUI did not, so Ctrl-L in
/// the toolbar activated the selected panel on TUI and did nothing on GTK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityBarKeyAction {
    /// j / Down — move the toolbar cursor down.
    MoveDown,
    /// k / Up — move the toolbar cursor up.
    MoveUp,
    /// l / Right / Enter — activate the selected item (`activity_bar_activate`).
    Activate,
    /// h / Left / Esc — leave the toolbar, focus returns to the editor.
    FocusOut,
    /// q — leave the toolbar *and* collapse the sidebar.
    Collapse,
    /// Anything else — swallowed, the toolbar stays as it is.
    Ignore,
}

/// Map an engine-space key name (both backends' `engine_key_from_ui` output,
/// or a bare character) to its activity-bar action. `ctrl` suppresses
/// activation and focus-out, matching GTK.
pub fn activity_bar_key_action(key: &str, ctrl: bool) -> ActivityBarKeyAction {
    match key {
        "j" | "Down" => ActivityBarKeyAction::MoveDown,
        "k" | "Up" => ActivityBarKeyAction::MoveUp,
        "l" | "Right" | "Return" if !ctrl => ActivityBarKeyAction::Activate,
        "h" | "Left" | "Escape" if !ctrl => ActivityBarKeyAction::FocusOut,
        "q" => ActivityBarKeyAction::Collapse,
        _ => ActivityBarKeyAction::Ignore,
    }
}

// ─── Terminal (PTY) keyboard rung (#758 / #734 slice 3, #351, #471) ──────────
//
// The rung directly beneath [`route_focus_key`]: once no modal overlay and no
// focus owner has claimed the key, a focused embedded terminal takes it before
// the editor ever sees it.
//
// **Where the encoder lives.** quadraui's `TerminalSession` already owns the
// *mouse* encoder (`encode_mouse` / `forward_mouse`) and, since quadraui#343,
// bracketed paste (`paste` / `bracketed_paste_enabled`) — but quadraui#342
// ("lift the keyboard → PTY encoder out of the example into the engine") has
// **not** landed on the pinned rev, so there is no upstream
// `TerminalSession::key_bytes`. The key encoder therefore stays where it
// already is: [`crate::core::engine::terminal_ops::key_to_pty_bytes`], which is
// platform-neutral `core` code and satisfies `CLAUDE.md`'s neutrality rule
// exactly as well. When quadraui#342 lands, `key_to_pty_bytes` is the single
// call site to swap; nothing in either backend changes.
//
// **What diverged.** TUI hand-rolled this rung inside `handle_key_pressed`
// (~80 lines). GTK's twin was deleted outright by the #540 Relm4→ShellApp
// cutover — the whole `if engine.borrow().terminal_has_focus { … }` block lived
// in the per-window `EventControllerKey` closure that went with the Relm4
// `view!`, and nothing replaced it. Since #540, GTK keys typed into a focused
// terminal have fallen through to `Engine::handle_key` and edited the *buffer*
// instead of reaching the PTY. Three concrete disagreements this router ends:
//
//  1. **GTK forwarded nothing at all.** Typing in the terminal ran vim normal
//     mode commands on the editor buffer. This is the live half of **#471** —
//     the other half was the old GTK arm's `sender.input(Msg::Resize)` after
//     every terminal keypress, whose handler called
//     `terminal_resize(full_panel_cols, …)` on *every* pane. In split mode that
//     reflowed the half-width panes to the full panel width on each keystroke
//     while they were still painted into their narrow rects, so freshly typed
//     text in the right pane wrapped off the painted area and "disappeared".
//     This router performs no resize, and [`route_terminal_resize`] is
//     split-aware, so neither half can come back.
//  2. **Key-name spelling.** TUI reached past `translate_key` and re-derived
//     names from the raw crossterm `KeyCode` (`"Page_Up"`, `"ISO_Left_Tab"`)
//     because `translate_key`'s editor-facing names (`"Shift_Up"`,
//     `"Shift_Return"`) have no PTY encoding; GTK speaks `"PageUp"` /
//     `"BackTab"`. [`canonical_terminal_key_name`] accepts both spellings, so
//     PageUp scrolls the scrollback on GTK too (it previously did not exist,
//     and would have fallen through to a raw `ESC[5~` write had it).
//  3. **`SendToPty` follow-through.** TUI polled the PTY immediately after the
//     write so the echo landed in the same frame; the old GTK arm did not, and
//     relied on the next poll tick. The router always polls.
//
// Both backends now call [`route_terminal_key`] and do nothing else for this
// rung.

/// Canonicalise a backend key name into the spelling
/// [`crate::core::engine::terminal_ops::key_to_pty_bytes`] and
/// `Engine::handle_terminal_key` expect.
///
/// The two backends name the same physical keys differently, and TUI's
/// `translate_key` additionally prefixes shifted navigation keys with `Shift_`
/// for the *editor*'s benefit — a prefix the PTY encoder has no arm for, which
/// is why TUI used to bypass `translate_key` entirely here. Shift is already
/// carried as its own `shift` argument, so the prefix is pure noise on this
/// rung and is stripped.
pub fn canonical_terminal_key_name(key_name: &str) -> &str {
    let base = key_name.strip_prefix("Shift_").unwrap_or(key_name);
    match base {
        // GTK's `NamedKey::Enter` mapping and the GDK keypad name.
        "Enter" | "KP_Enter" => "Return",
        // GTK says "PageUp"; X11/GDK says "Page_Up"/"Prior"; TUI says "Page_Up".
        "PageUp" | "Prior" | "KP_Page_Up" => "Page_Up",
        "PageDown" | "Next" | "KP_Page_Down" => "Page_Down",
        // GTK's `NamedKey::BackTab`; TUI/X11 spell it "ISO_Left_Tab".
        "BackTab" => "ISO_Left_Tab",
        other => other,
    }
}

/// The shared terminal (PTY) keyboard rung — one implementation, both backends
/// (#758 / #734 slice 3).
///
/// Returns `true` when the focused terminal claimed the key, in which case the
/// caller must stop dispatching and repaint. Returns `false` when no terminal
/// has focus, leaving the key to the rungs below.
///
/// `key_name` may be spelled in either backend's dialect — see
/// [`canonical_terminal_key_name`]. `unicode` is the resolved character (for
/// Ctrl combos, the *unshifted* letter, matching both backends' translation
/// layers).
///
/// The engine decides *what* the key means
/// ([`Engine::handle_terminal_key`](crate::core::Engine::handle_terminal_key));
/// this function performs the side effects that used to be duplicated in the
/// backends — clipboard read/write through the engine's own callbacks, the PTY
/// write, and the follow-up poll.
pub fn route_terminal_key(
    engine: &mut Engine,
    key_name: &str,
    unicode: Option<char>,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> bool {
    use crate::core::engine::TerminalKeyAction;

    if !engine.terminal_has_focus {
        return false;
    }

    let canon = canonical_terminal_key_name(key_name);
    match engine.handle_terminal_key(canon, unicode, ctrl, shift, alt) {
        TerminalKeyAction::CopySelection => {
            let text = engine.active_terminal().and_then(|t| t.selected_text());
            if let Some(ref text) = text {
                if let Some(ref cb) = engine.clipboard_write {
                    let _ = cb(text);
                }
                engine.message = "Copied".to_string();
            }
        }
        TerminalKeyAction::PasteClipboard => {
            // System clipboard first, then the `+` and unnamed registers —
            // the fallback chain TUI had and GTK never did.
            let paste_text = engine
                .clipboard_read
                .as_ref()
                .and_then(|cb| cb().ok())
                .filter(|t| !t.is_empty())
                .or_else(|| {
                    engine
                        .registers
                        .get(&'+')
                        .map(|(t, _)| t.clone())
                        .filter(|t| !t.is_empty())
                })
                .or_else(|| {
                    engine
                        .registers
                        .get(&'"')
                        .map(|(t, _)| t.clone())
                        .filter(|t| !t.is_empty())
                });
            if let Some(text) = paste_text {
                engine.terminal_paste(&text);
            } else {
                engine.message = "Nothing to paste".to_string();
            }
        }
        TerminalKeyAction::SendToPty(data) => {
            engine.terminal_write(&data);
            engine.poll_terminal();
        }
        TerminalKeyAction::Handled | TerminalKeyAction::Ignore => {}
    }
    true
}

/// The shared "window resized → resize the PTYs" rung (#758 / #734 slice 3).
///
/// `panel_cols` is the *whole* terminal panel's width in cells.
///
/// `Engine::terminal_resize` resizes **every** pane to `panel_cols`, which is
/// right for tabs and wrong for a split: the two visible panes are painted at
/// roughly half the panel width each, so resizing them to the full width
/// reflows their contents off the painted area — the resize half of **#471**.
/// This router keeps a split's per-pane widths, honouring an in-progress
/// divider drag (`terminal_split_left_cols`) when one is set.
pub fn route_terminal_resize(engine: &mut Engine, panel_cols: u16, rows: u16) {
    let panel_cols = panel_cols.max(2);
    if engine.terminal_split && engine.terminal_panes.len() >= 2 {
        let left = if engine.terminal_split_left_cols > 0 {
            engine
                .terminal_split_left_cols
                .clamp(1, panel_cols.saturating_sub(1))
        } else {
            panel_cols / 2
        };
        let right = panel_cols.saturating_sub(left).max(1);
        engine.terminal_panes[0].session.resize(left, rows);
        engine.terminal_panes[1].session.resize(right, rows);
        // Panes 3+ are hidden tabs; they get the full panel width they will
        // be painted at once the split closes.
        for slot in engine.terminal_panes.iter_mut().skip(2) {
            slot.session.resize(panel_cols, rows);
        }
    } else {
        engine.terminal_resize(panel_cols, rows);
    }
}

// ─── Alt-modifier / VSCode-mode keyboard rung (#759 / #734 slice 4) ──────────
//
// An Alt chord can mean three different things, and the decision between them
// is a *sequence*, not a lookup: menu accelerator → editor/shell chord →
// pass-through to `Engine::handle_key`. The first tier is already shared
// (`quadraui::MenuSystem::handle` + `MenuBar::find_alt_target`, gated
// identically on both backends by #695). The second tier is what this rung
// converges.
//
// **What diverged.** TUI hand-rolled the whole tier as a ~66-line
// `if key_event.modifiers.contains(KeyModifiers::ALT) { match key_event.code
// { … } }` block inside `handle_key_pressed`. GTK had **no Alt tier at all**:
// `handle_key_press` took an `alt: bool` and used it for exactly two things —
// forwarding it to the terminal router and suppressing the debug F-keys — and
// then dropped it on the floor, because `Engine::handle_key` has no `alt`
// parameter. So since the #540 Relm4→ShellApp cutover, **every chord below was
// silently dead on GTK**:
//
//   Alt+Left / Alt+Right (resize the sidebar), Alt+M (toggle Vim ↔ VSCode
//   editing mode), Alt+, / Alt+. (resize the editor-group split), Alt+] /
//   Alt+[ (cycle AI ghost-text alternatives), and — the VSCode-mode half —
//   Alt+Up / Alt+Down (move line), Alt+Shift+Up / Alt+Shift+Down (add a
//   cursor above/below) and Alt+Z (toggle word wrap).
//
// That is the #499/#484 shape again: `Engine::vscode_move_line_up`,
// `add_cursor_at_pos` and the `Alt_*` arm of `handle_vscode_key` are all
// platform-neutral `core` code with a full unit-test suite, reachable from
// exactly one of the two backends.
//
// **The VSCode-mode statement.** The reason the second half diverged is that
// neither backend wrote down *what VSCode mode means for a keystroke*; each
// had (or, on GTK, lacked) its own ad-hoc branch. [`vscode_alt_key_name`] is
// that statement, in one place: in VSCode mode these Alt chords are VS Code
// editor commands, spelled in the `Alt_*` dialect
// `Engine::handle_vscode_key` decodes; in Vim mode they are not, and fall
// through to the vim mapping layer. Both backends now ask that one function.
//
// **Shift+Alt+F is deliberately absent from the "GTK gained it" list.** It is
// in the resolver (LSP format document), but on both backends the menu tier
// above claims it first whenever the menu bar is live: `find_alt_target`
// lower-cases the chord and `&File` triggers on `f`. GTK's menu bar is always
// visible and TUI's is visible in VSCode mode, so the reachable case is TUI in
// Vim mode. Converging the rung does not change that ordering — it is the same
// on both backends, which is the point.
//
// #1764 (non-blocking review note) narrows this slightly on the *toggleable*
// (TUI) profile only: `alt_mnemonic_open_allowed` now refuses a fresh menu
// open while mid-text-entry (Insert/Replace) or mid-command-line
// (Command/Search), so on TUI a Shift+Alt+F pressed in one of those modes no
// longer gets swallowed by the menu tier — it reaches this arm and runs
// `lsp_format_current()` instead. GTK (never toggleable) is unaffected: its
// menu tier still claims the chord first in every mode, exactly as before.
// Plausibly an improvement (formatting while typing is a reasonable thing to
// want), but it is a real, if narrow, ordering change worth knowing about
// when reading the "reachable case is TUI in Vim mode" sentence above — that
// sentence is now true only for the menu-open half of the story, not for
// where this specific chord ends up executing on every TUI mode.

/// Lower bound Alt+Left clamps the sidebar width to, in `AppShell` width units
/// (columns on TUI, line-heights on GTK — see
/// `quadraui::AppShell::set_sidebar_width`).
pub const ALT_SIDEBAR_WIDTH_MIN: u16 = 15;
/// Upper bound Alt+Right clamps the sidebar width to. See
/// [`ALT_SIDEBAR_WIDTH_MIN`].
pub const ALT_SIDEBAR_WIDTH_MAX: u16 = 150;

/// Apply an [`AltKeyOutcome::ResizeSidebar`] delta to a backend-owned sidebar
/// width, clamped to [`ALT_SIDEBAR_WIDTH_MIN`]..=[`ALT_SIDEBAR_WIDTH_MAX`].
///
/// The width itself cannot live in the resolver: TUI's authoritative copy is
/// the pre-#1434 TUI shell's `sidebar_width` (which its end-of-dispatch sync pushes into the
/// runner's `AppShell`), while GTK's is the runner `AppShell` itself, reached
/// through `ShellContext::shell_mut`. The *clamp* is shared so the two cannot
/// drift apart — which is the only part a user can observe.
///
/// Both backends additionally configure their `AppShell` with these same two
/// bounds (the pre-#1434 TUI shell's `shell_config`, `gtk::build_shell_config`), so
/// `AppShell::set_sidebar_width`'s own internal clamp cannot narrow the rung's
/// range on one backend and not the other. That was live on GTK before #759:
/// quadraui's generic default is `8.0..=50.0`, which TUI had already overridden
/// for exactly this reason (#634) and GTK had not.
pub fn alt_resized_sidebar_width(current: u16, delta: i32) -> u16 {
    let next = current as i32 + delta;
    next.clamp(ALT_SIDEBAR_WIDTH_MIN as i32, ALT_SIDEBAR_WIDTH_MAX as i32) as u16
}

/// #1764: whether the engine's current mode is safe for an Alt+<mnemonic>
/// `KeyPressed` to open a *closed* `quadraui::MenuSystem` dropdown.
///
/// Vim mode has no `<M-x>` mapping support at all — nothing in
/// `src/core/engine` spells a `"<M-"` binding — so the only two things an
/// Alt-modified keypress reaching the menu-bar intercept can plausibly be
/// are a genuine (currently unbound, meaningless) Meta-key chord, or — per
/// the #1763 bugbash finding this issue (#1764) is the follow-up to — a real
/// pty collapsing a fast Escape-then-letter into exactly this chord, because
/// that is the standard xterm 8-bit-meta encoding for Alt and a raw
/// terminal reader cannot always tell the two apart from bytes alone.
///
/// That ambiguity is **terminal-only**: it exists because a pty delivers Alt
/// as a byte-level encoding a raw reader has to *infer* rather than a
/// discrete, unambiguous modifier bit. `menu_bar_toggleable` (true only on
/// the `cell`/TUI profile — `App::setup`'s own doc) is this crate's existing,
/// non-`cfg` discriminator for exactly that backend shape (see e.g.
/// `App::handle_dispatch`'s `menu_bar_intercept_rect` call and
/// `render::menu_bar_intercept_rect`'s own doc). On every other backend
/// (GTK/macOS/Win — always `false` here) a `Modifiers { alt: true }`
/// `KeyPressed` is a real, unambiguous keystroke from a real keyboard; there
/// is no pty to fuse anything, so the standard Alt+<mnemonic> menu gesture
/// stays allowed in every mode there, matching pre-#1764 behaviour exactly.
///
/// On the toggleable (TUI) profile, letting the fused chord open a dropdown
/// while the engine is mid-text-entry (`Insert`/`Replace`) or
/// mid-command-line (`Command`/`Search`) is strictly worse than not: it
/// swallows the keystroke that — if this really was a collapsed Escape —
/// was supposed to return to `Normal` mode, and does so silently, with no
/// error and no visible cue beyond the dropdown itself. Every subsequent
/// keystroke then falls through as literal Insert-mode text (or
/// Command-line text), which is exactly #1764's reported
/// `foo bar bazg0wdw:%d`-shaped corruption. `Normal` and the `Visual*`
/// family are where a menu action is conventionally meaningful there (and
/// where an unclaimed bare key is a harmless pending-key latch, not literal
/// insertion), so only those allow the open on that profile.
///
/// The other half of this fix is `crate::app::App::handle_key_press`'s own
/// `AltKeyOutcome::Fallthrough` arm: once this function has kept the menu
/// from stealing the chord, that arm treats an unclaimed *printable-char*
/// Alt chord (see `alt_chord_is_printable_char`) as an implicit Escape
/// instead of letting `Engine::handle_key` see it as the bare, unmodified
/// key (which has no `alt` parameter to even know the difference). That
/// substitution is not itself gated on `menu_bar_toggleable` — see its own
/// call site for why a bare-letter fallthrough is equally wrong to redeliver
/// on every backend, pty or not.
pub fn alt_mnemonic_open_allowed(mode: crate::core::Mode, menu_bar_toggleable: bool) -> bool {
    use crate::core::Mode;
    if !menu_bar_toggleable {
        return true;
    }
    matches!(
        mode,
        Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::VisualBlock
    )
}

/// What [`route_alt_key`] decided about an Alt chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AltKeyOutcome {
    /// No Alt chord claimed the key — keep dispatching down the ladder.
    Fallthrough,
    /// The rung applied its effect to the engine; repaint and stop.
    Handled,
    /// Resize the backend-owned sidebar width by this many units, then repaint
    /// and stop. Apply with [`alt_resized_sidebar_width`].
    ResizeSidebar(i32),
}

/// The base key of an Alt chord, in a spelling both backends agree on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AltBase {
    Left,
    Right,
    Up,
    Down,
    Char(char),
}

/// Canonicalise a backend key name + resolved character into an [`AltBase`].
///
/// The two backends disagree on both halves. GTK spells a printable key in
/// *both* `key_name` (`"m"`) and `unicode` (`Some('m')`); TUI's `translate_key`
/// leaves `key_name` empty for an unmodified printable and puts the character
/// in `unicode` alone. For shifted navigation keys TUI additionally prefixes
/// the name (`"Shift_Up"`) — shift is carried separately on this rung, so the
/// prefix is stripped, exactly as [`canonical_terminal_key_name`] does.
fn alt_chord_base(key_name: &str, unicode: Option<char>) -> Option<AltBase> {
    let name = key_name.strip_prefix("Shift_").unwrap_or(key_name);
    match name {
        "Left" => return Some(AltBase::Left),
        "Right" => return Some(AltBase::Right),
        "Up" => return Some(AltBase::Up),
        "Down" => return Some(AltBase::Down),
        _ => {}
    }
    // `unicode` first (TUI's only channel for a printable), then a
    // single-character `key_name` (GTK's). Multi-character names that reach
    // here are named keys this rung has no chord for.
    let mut chars = name.chars();
    let from_name = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    unicode.or(from_name).map(AltBase::Char)
}

/// #1764: whether an unclaimed Alt chord's base key is a printable
/// character (letter/digit/punctuation) rather than a named key
/// (`Enter`/`BackSpace`/`Up`/`Down`/`Home`/`Delete`/`Page_Up`/…). Only the
/// printable-char shape is what a real pty's Escape-then-letter fusion (the
/// #1763/#1764 mechanism — the standard xterm 8-bit-meta encoding collapsing
/// a fast `Escape` + letter into one `Alt+<letter>` byte sequence) can
/// produce; a named key can never be the second half of that fusion.
///
/// Exposed for `App::handle_key_press`'s `AltKeyOutcome::Fallthrough` arm,
/// which must not treat *every* unclaimed Alt chord as an implicit Escape —
/// doing so swallowed pre-existing, meaningful bare-key fallthroughs for
/// named keys (`Alt+Enter`/`Alt+BackSpace` inserting/deleting in Insert
/// mode, `Alt+Up`/`Alt+Down` moving the cursor in Vim mode outside VSCode
/// mode, `Alt+]`/`Alt+[` falling through outside Insert mode per
/// [`route_alt_key`]'s own comment on that arm) that have nothing to do with
/// the pty-fusion bug this function exists to let that arm recognise.
pub(crate) fn alt_chord_is_printable_char(key_name: &str, unicode: Option<char>) -> bool {
    matches!(alt_chord_base(key_name, unicode), Some(AltBase::Char(_)))
}

/// The single statement of what **VSCode mode** means for an Alt chord: the
/// `Alt_*` key name `Engine::handle_vscode_key` decodes, or `None` when the
/// chord is not a VS Code editor command.
///
/// In Vim mode this function is not consulted at all — the chord falls through
/// to the vim mapping layer instead. That asymmetry *is* the mode semantics,
/// and it now exists once rather than once per backend.
///
/// `Left`/`Right` were added by #1744: in VSCode mode a plain Alt+Left/Right
/// is `workbench.action.navigateBack`/`navigateForward`
/// (`Engine::jump_list_back`/`jump_list_forward`, the same mechanism Vim
/// mode's Ctrl-O/Ctrl-I already use), not sidebar resize — see
/// [`route_alt_key`]'s mode-independent tier, which now only claims
/// `Left`/`Right` for resize outside VSCode mode (or with both `ctrl` *and*
/// `shift` held, VSCode mode's own alternate home for the same resize — see
/// that tier's own doc for why plain `ctrl` alone was the wrong choice).
fn vscode_alt_key_name(base: AltBase, shift: bool) -> Option<&'static str> {
    Some(match (base, shift) {
        (AltBase::Up, true) => "Alt_Shift_Up",
        (AltBase::Down, true) => "Alt_Shift_Down",
        (AltBase::Up, false) => "Alt_Up",
        (AltBase::Down, false) => "Alt_Down",
        (AltBase::Left, false) => "Alt_Left",
        (AltBase::Right, false) => "Alt_Right",
        (AltBase::Char('z') | AltBase::Char('Z'), false) => "Alt_z",
        _ => return None,
    })
}

/// The shared Alt-modifier / VSCode-mode keyboard rung — one implementation,
/// both backends (#759 / #734 slice 4).
///
/// Call it with the backend's own key spelling (see [`alt_chord_base`]) once
/// the menu tier above has declined the event. Returns [`AltKeyOutcome`];
/// `Fallthrough` means nothing here claimed the chord. Through #1763, the
/// caller always kept dispatching it as if `alt` had never been set — #1764
/// changed that for a chord whose base is a printable character (see
/// [`alt_chord_is_printable_char`]): `App::handle_key_press`'s
/// `Fallthrough` arm now treats *that* shape as an implicit Escape instead
/// of redelivering the bare key, so this rung is no longer a pure filter for
/// printable-char chords — only for the named-key chords (`Enter`,
/// `BackSpace`, `Home`/`End`/`Delete`/`Page_Up`/`Page_Down`, and `Left`/
/// `Right`/`Up`/`Down` outside the arms above) that still fall all the way
/// through exactly as before.
///
/// `alt == false` is an immediate `Fallthrough`, so a caller may invoke this
/// unconditionally rather than wrapping it in its own modifier test.
///
/// `ctrl` (#1744) distinguishes two chords that otherwise share a base/shift
/// shape: Ctrl+Alt+Up/Down is VS Code's real `insertCursorAbove`/
/// `insertCursorBelow` (forwarded to `Engine::handle_vscode_key` as `Alt_Up`/
/// `Alt_Down` with `ctrl` now passed through instead of hardcoded `false`,
/// letting that function's own `if ctrl` guard tell it apart from plain
/// Alt+Up/Down's move-line); and Ctrl+Shift+Alt+Left/Right is VSCode mode's
/// alternate home for sidebar resize, now that plain Alt+Left/Right means
/// navigate back/forward in that mode (see [`vscode_alt_key_name`]'s doc).
///
/// That alternate home is Ctrl+**Shift**+Alt+Left/Right, not plain
/// Ctrl+Alt+Left/Right: the latter is already the shipped default binding
/// for `panel_keys.nav_back`/`nav_forward`
/// (`Settings::PanelKeys::nav_back`/`nav_forward`, `"<C-A-Left>"`/
/// `"<C-A-Right>"`), registered as a `AcceleratorScope::Global` accelerator
/// by [`register_panel_accelerators`] below. quadraui's accelerator tier
/// sits *above* this rung and **replaces** a matched key event with the
/// fired accelerator rather than letting it fall through
/// (`accelerator_match_replaces_keypressed_with_accelerator`, quadraui's own
/// `tui/backend.rs`), so a plain Ctrl+Alt+Left/Right arm here would never
/// actually run in the live app — it would be silently shadowed by tab
/// history nav on every real keypress, reachable only when a test calls
/// this function directly below the accelerator tier. Ctrl+Shift+Alt is
/// free on both backends and is not itself a VS Code default binding either
/// (unlike Ctrl+Alt+Left/Right, which upstream binds to
/// `workbench.action.moveEditorToPreviousGroup`/`NextGroup`).
pub fn route_alt_key(
    engine: &mut Engine,
    key_name: &str,
    unicode: Option<char>,
    shift: bool,
    ctrl: bool,
    alt: bool,
) -> AltKeyOutcome {
    if !alt {
        return AltKeyOutcome::Fallthrough;
    }
    let Some(base) = alt_chord_base(key_name, unicode) else {
        return AltKeyOutcome::Fallthrough;
    };

    // ── Mode-independent chords ──────────────────────────────────────────
    // The shifted spellings (`<`, `>`, `}`, `{`) are listed alongside the
    // unshifted ones because TUI's old block matched crossterm's raw
    // `KeyCode::Char(',')` — the *physical* key, before the shift map — while
    // this rung sees the resolved character. Accepting both keeps Alt+Shift+,
    // working exactly as it did, and gives GTK (which never had the chord at
    // all) the same tolerance.
    match base {
        // #1744: in VSCode mode, Ctrl+Shift+Alt+Left/Right is sidebar
        // resize's alternate home — plain Alt+Left/Right now means navigate
        // back/forward in that mode (the arm below, via
        // `vscode_alt_key_name`), and plain Ctrl+Alt+Left/Right is already
        // claimed, above this rung, by the `panel_keys.nav_back`/
        // `nav_forward` global accelerator — see this function's own doc
        // for why `ctrl` alone is not a usable alternate home. Outside
        // VSCode mode `ctrl`/`shift` are ignored, same as every other
        // mode-independent arm here, so Vim mode keeps resizing on plain
        // Alt+Left/Right (with or without Shift, as before #1744) exactly
        // as before — Shift+Alt+Left/Right's own VSCode-mode fate (nothing:
        // deliberately left unbound, since Ctrl+Shift+Alt is the chord that
        // now owns this) is documented on the arm below that doesn't match
        // it.
        AltBase::Left if ctrl && shift && engine.is_vscode_mode() => {
            return AltKeyOutcome::ResizeSidebar(-1);
        }
        AltBase::Right if ctrl && shift && engine.is_vscode_mode() => {
            return AltKeyOutcome::ResizeSidebar(1);
        }
        AltBase::Left if !engine.is_vscode_mode() => return AltKeyOutcome::ResizeSidebar(-1),
        AltBase::Right if !engine.is_vscode_mode() => return AltKeyOutcome::ResizeSidebar(1),
        // Shift+Alt+F: LSP format document. Reachable only when the menu tier
        // above is not live — see this rung's header comment.
        AltBase::Char('F') if shift => {
            engine.lsp_format_current();
            return AltKeyOutcome::Handled;
        }
        // Alt+M: toggle Vim ↔ VSCode editing mode.
        AltBase::Char('m') | AltBase::Char('M') => {
            engine.toggle_editor_mode();
            return AltKeyOutcome::Handled;
        }
        // Alt+, / Alt+. — resize the editor group split.
        AltBase::Char(',') | AltBase::Char('<') => {
            engine.group_resize(-0.05);
            return AltKeyOutcome::Handled;
        }
        AltBase::Char('.') | AltBase::Char('>') => {
            engine.group_resize(0.05);
            return AltKeyOutcome::Handled;
        }
        // Alt+] / Alt+[ — cycle AI ghost-text alternatives (insert mode only;
        // outside insert mode there is no ghost text to cycle, so the chord
        // falls through rather than being swallowed).
        AltBase::Char(']') | AltBase::Char('}') if engine.mode == crate::core::Mode::Insert => {
            engine.ai_ghost_next_alt();
            return AltKeyOutcome::Handled;
        }
        AltBase::Char('[') | AltBase::Char('{') if engine.mode == crate::core::Mode::Insert => {
            engine.ai_ghost_prev_alt();
            return AltKeyOutcome::Handled;
        }
        // Alt+T (tab switcher) is claimed by the accelerator tier above this
        // one on both backends, so it deliberately has no arm here.
        _ => {}
    }

    // ── VSCode-mode chords ───────────────────────────────────────────────
    if engine.is_vscode_mode() {
        if let Some(name) = vscode_alt_key_name(base, shift) {
            engine.handle_key(name, None, ctrl);
            return AltKeyOutcome::Handled;
        }
    }

    AltKeyOutcome::Fallthrough
}

// ─── Clipboard-paste pre-load / Ctrl+Shift+V rung (#760 / #734 slice 5) ─────
//
// Two small tiers that sit directly below the Alt rung above and directly
// above `Engine::handle_key` on both backends.
//
// **Pre-load.** Before a `p`/`P` (Vim mode) or Ctrl+V (VSCode mode) keypress
// reaches `Engine::handle_key`, the system clipboard has to be read and
// copied into the paste registers first — `handle_key`'s `p` has no clipboard
// access of its own. Both backends called the same two engine methods
// (`needs_clipboard_for_paste` / `prepare_paste_clipboard`, shared since
// #381) but each restated the four-line "if it needs it, read it, load it"
// glue around them; [`preload_paste_clipboard`] is that glue, once.
//
// **Ctrl+Shift+V.** #593 wired GTK's Ctrl+V (and, incidentally, Ctrl+Shift+V —
// quadraui's GTK runner intercepts both and redelivers them as
// `UiEvent::ClipboardPaste`, per its own `ctrl_shift_v_delivers_the_clipboard_
// selection_as_a_paste` / `ctrl_shift_v_is_intercepted_not_forwarded_as_raw_v`
// tests at the pinned rev) straight through `Engine::route_paste` — the same
// arm this crate's `ClipboardPaste` handler already uses for TUI's bracketed
// paste. TUI's terminal side never got the same treatment: quadraui's TUI
// runner only synthesizes `ClipboardPaste` for a real bracketed-paste escape
// sequence (`CtEvent::Paste`), not for a Ctrl+Shift+V keypress, so that chord
// arrives as an ordinary `KeyEvent` and TUI hand-rolled a second paste path —
// read the clipboard, `load_clipboard_for_paste`, then either replay `p` or
// splice characters into insert mode one at a time — instead of calling
// `route_paste` like every other paste destination does. That hand-rolled
// path covered only Normal/Visual/Insert/Replace, so a Ctrl+Shift+V while the
// terminal, picker, explorer rename, search box, SC commit message, extension
// sidebar or AI chat had focus fell through to whatever `handle_key` does
// with a bare `V`, instead of pasting into that context the way plain Ctrl+V
// (via `prepare_paste_clipboard` + `p`) already does.
// [`route_ctrl_shift_v_paste`] replaces it with the one-line call the other
// destinations use, so the whole `route_paste` priority chain (terminal →
// picker → explorer rename → search → SC commit → ext sidebar → AI chat →
// mode dispatch) applies to Ctrl+Shift+V too. It has no GTK caller — GTK
// never sees the raw keystroke to route in the first place — which is the
// point: both backends now reach `Engine::route_paste`, GTK via quadraui's
// `ClipboardPaste` interception and TUI via this function, rather than one
// going through `UiEvent::ClipboardPaste` and the other through a bespoke key
// branch that duplicated (and under-covered) what `route_paste` already does.

/// Pre-load the system clipboard into the paste registers before a paste
/// keystroke reaches `Engine::handle_key` — shared by both backends (#381
/// gave them the two engine methods; this states the glue between them once).
///
/// `key_name`/`unicode`/`ctrl` are the backend's own spelling of the key about
/// to be dispatched; this is a no-op unless [`Engine::needs_clipboard_for_paste`]
/// says that key is a paste.
pub fn preload_paste_clipboard(
    engine: &mut Engine,
    key_name: &str,
    unicode: Option<char>,
    ctrl: bool,
) {
    if engine.needs_clipboard_for_paste(key_name, unicode, ctrl) {
        let text = engine.clipboard_read.as_ref().and_then(|cb| cb().ok());
        engine.prepare_paste_clipboard(text);
    }
}

/// Ctrl+Shift+V: paste the system clipboard verbatim through
/// `Engine::route_paste`'s priority chain. TUI-only — see this rung's header
/// comment above for why GTK never calls it. Returns `true` when the chord
/// matched (and was therefore consumed) regardless of whether the clipboard
/// actually produced text, matching the pre-#760 behaviour this replaces.
pub fn route_ctrl_shift_v_paste(engine: &mut Engine, key_name: &str, ctrl: bool) -> bool {
    if !ctrl || key_name != "V" || engine.is_vscode_mode() {
        return false;
    }
    if let Some(text) = engine.clipboard_read.as_ref().and_then(|cb| cb().ok()) {
        if !text.is_empty() {
            engine.route_paste(&text);
        }
    }
    true
}

#[cfg(test)]
mod alt_key_router_tests {
    //! #759 / #734 slice 4 — the Alt-modifier / VSCode-mode rung.
    //!
    //! The two backends spell the *same physical chord* differently, and that
    //! is precisely how the old duplicated blocks were able to drift without
    //! anyone noticing. These tests feed both spellings of one chord into
    //! [`route_alt_key`] and assert the two resolve identically; the
    //! rendered-output halves live in `src/tui_main/app_on_tui_tests.rs`
    //! (`alt_*_via_shell_app`, plus #1744's `mod vscode_mode_alt_rung_1744`)
    //! and `src/gtk/testing.rs` (`alt_*_on_gtk`, plus #1744's
    //! `mod alt_rung_1744`).
    use super::*;
    use crate::core::Mode;

    fn engine() -> Engine {
        crate::core::session::suppress_disk_saves();
        let mut e = Engine::new_for_test();
        e.buffer_mut().insert(0, "alpha\nbravo\ncharlie\n");
        e
    }

    /// The same chord as each backend actually spells it.
    ///
    /// TUI's `translate_key` leaves `key_name` empty for an unmodified
    /// printable and reports the character in `unicode` alone, and prefixes
    /// shifted navigation keys with `Shift_`. GTK's `ShellApp::handle` puts
    /// the character in *both* slots and never prefixes.
    struct Chord {
        tui: (&'static str, Option<char>),
        gtk: (&'static str, Option<char>),
        shift: bool,
        ctrl: bool,
    }

    const ALT_Z: Chord = Chord {
        tui: ("", Some('z')),
        gtk: ("z", Some('z')),
        shift: false,
        ctrl: false,
    };
    const ALT_SHIFT_UP: Chord = Chord {
        tui: ("Shift_Up", None),
        gtk: ("Up", None),
        shift: true,
        ctrl: false,
    };
    const ALT_SHIFT_DOWN: Chord = Chord {
        tui: ("Shift_Down", None),
        gtk: ("Down", None),
        shift: true,
        ctrl: false,
    };
    const ALT_DOWN: Chord = Chord {
        tui: ("Down", None),
        gtk: ("Down", None),
        shift: false,
        ctrl: false,
    };
    const ALT_LEFT: Chord = Chord {
        tui: ("Left", None),
        gtk: ("Left", None),
        shift: false,
        ctrl: false,
    };
    const ALT_RIGHT: Chord = Chord {
        tui: ("Right", None),
        gtk: ("Right", None),
        shift: false,
        ctrl: false,
    };
    const CTRL_ALT_UP: Chord = Chord {
        tui: ("Up", None),
        gtk: ("Up", None),
        shift: false,
        ctrl: true,
    };
    const CTRL_ALT_DOWN: Chord = Chord {
        tui: ("Down", None),
        gtk: ("Down", None),
        shift: false,
        ctrl: true,
    };
    const CTRL_SHIFT_ALT_RIGHT: Chord = Chord {
        tui: ("Right", None),
        gtk: ("Right", None),
        shift: true,
        ctrl: true,
    };

    /// Run `chord` through the rung twice — once in each backend's spelling,
    /// each against its own fresh engine — and return
    /// `(outcome, resulting buffer text, resulting message)` once, after
    /// asserting the two runs agreed on all three.
    fn resolves_identically(chord: &Chord, vscode: bool) -> (AltKeyOutcome, String, String) {
        let run = |(name, unicode): (&str, Option<char>)| {
            let mut e = engine();
            if vscode {
                e.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
                e.mode = Mode::Insert;
            }
            let outcome = route_alt_key(&mut e, name, unicode, chord.shift, chord.ctrl, true);
            (outcome, e.buffer().to_string(), e.message.clone())
        };
        let tui = run(chord.tui);
        let gtk = run(chord.gtk);
        assert_eq!(
            tui, gtk,
            "the same chord must resolve identically whichever backend spelled \
             it: TUI said {tui:?}, GTK said {gtk:?}"
        );
        tui
    }

    /// Alt+Z is a VS Code editor command (toggle word wrap) in VSCode mode and
    /// nothing at all in Vim mode. Before #759 that decision existed only
    /// inside TUI's `handle_key_pressed`; GTK dropped `alt` on the floor, so
    /// the chord was inert there in *both* modes.
    #[test]
    fn alt_z_is_a_vscode_command_and_a_vim_passthrough() {
        let (outcome, _, message) = resolves_identically(&ALT_Z, true);
        assert_eq!(outcome, AltKeyOutcome::Handled);
        assert!(
            message.starts_with("Word wrap "),
            "VSCode-mode Alt+Z must reach `Engine::handle_vscode_key`'s \
             `Alt_z` arm; message was {message:?}"
        );

        let (outcome, _, message) = resolves_identically(&ALT_Z, false);
        assert_eq!(
            outcome,
            AltKeyOutcome::Fallthrough,
            "Vim mode must leave Alt+Z to the vim mapping layer"
        );
        assert_eq!(message, "", "Vim-mode Alt+Z must not toggle word wrap");
    }

    /// Alt+Down moves the current line down in VSCode mode; Alt+Shift+Down
    /// duplicates it downward instead (#1744 — it used to add a cursor,
    /// VS Code's real chord for *that* is Ctrl+Alt+Down, see
    /// [`ctrl_alt_up_down_add_a_cursor_distinct_from_plain_alt_up_down`]
    /// below). The shift discrimination is the part TUI wrote against
    /// crossterm's raw `KeyCode` + modifier flags and GTK never wrote at
    /// all, so it is the arm most likely to drift again.
    #[test]
    fn alt_down_moves_a_line_and_alt_shift_down_duplicates_it() {
        let (outcome, text, _) = resolves_identically(&ALT_DOWN, true);
        assert_eq!(outcome, AltKeyOutcome::Handled);
        assert!(
            text.starts_with("bravo\nalpha\n"),
            "VSCode-mode Alt+Down must swap the cursor line with the one \
             below; buffer was {text:?}"
        );

        // Alt+Shift+Up from line 0 duplicates "alpha" above itself — proving
        // the *decode* differs from plain Alt+Up (which would have swapped
        // lines instead, and there is nothing above line 0 to swap with).
        let (outcome, text, _) = resolves_identically(&ALT_SHIFT_UP, true);
        assert_eq!(outcome, AltKeyOutcome::Handled);
        assert!(
            text.starts_with("alpha\nalpha\nbravo\n"),
            "Alt+Shift+Up must decode as `Alt_Shift_Up` (duplicate line up), \
             not `Alt_Up` (move line); buffer was {text:?}"
        );

        // Alt+Shift+Down duplicates "alpha" below itself, mirrored.
        let (outcome, text, _) = resolves_identically(&ALT_SHIFT_DOWN, true);
        assert_eq!(outcome, AltKeyOutcome::Handled);
        assert!(
            text.starts_with("alpha\nalpha\nbravo\n"),
            "Alt+Shift+Down must duplicate the line downward; buffer was \
             {text:?}"
        );
    }

    /// #1744: VS Code's real `insertCursorAbove`/`insertCursorBelow` is
    /// Ctrl+Alt+Up/Down, not plain Alt+Up/Down — and must leave the buffer
    /// untouched (unlike the move-line chord it shares a base key with).
    /// Verified RED against unfixed `develop`: before this change
    /// `route_alt_key` had no `ctrl` parameter at all, so Ctrl+Alt+Up was
    /// indistinguishable from plain Alt+Up and moved the line instead of
    /// adding a cursor (`tests/vscode_keybinding_parity.rs`'s
    /// `KNOWN_GAPS::CTRL_ALT_UP_IS_MOVE_LINE` pinned exactly this).
    #[test]
    fn ctrl_alt_up_down_add_a_cursor_distinct_from_plain_alt_up_down() {
        // `insertCursorAbove` has nowhere to add a cursor from line 0, so run
        // this half directly (not through `resolves_identically`'s default-
        // cursor fixture) with the cursor seeded on line 1, once per backend
        // spelling.
        let run_up = |(name, unicode): (&str, Option<char>)| {
            let mut e = engine();
            e.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
            e.mode = Mode::Insert;
            e.view_mut().cursor.line = 1;
            let outcome = route_alt_key(&mut e, name, unicode, false, true, true);
            (
                outcome,
                e.buffer().to_string(),
                e.view().extra_cursors.len(),
            )
        };
        let tui_up = run_up(CTRL_ALT_UP.tui);
        let gtk_up = run_up(CTRL_ALT_UP.gtk);
        assert_eq!(
            tui_up, gtk_up,
            "Ctrl+Alt+Up must resolve identically whichever backend spelled \
             it: TUI said {tui_up:?}, GTK said {gtk_up:?}"
        );
        let (outcome, text, cursors) = tui_up;
        assert_eq!(outcome, AltKeyOutcome::Handled);
        assert_eq!(
            text, "alpha\nbravo\ncharlie\n",
            "Ctrl+Alt+Up (insertCursorAbove) must not move any line"
        );
        assert_eq!(cursors, 1, "Ctrl+Alt+Up must add exactly one cursor");

        // `insertCursorBelow` has room from line 0 (there are lines below),
        // so the default-cursor fixture is fine here.
        let (outcome, text, _) = resolves_identically(&CTRL_ALT_DOWN, true);
        assert_eq!(outcome, AltKeyOutcome::Handled);
        assert_eq!(
            text, "alpha\nbravo\ncharlie\n",
            "Ctrl+Alt+Down (insertCursorBelow) must not move any line"
        );
        let mut e = engine();
        e.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
        e.mode = Mode::Insert;
        route_alt_key(&mut e, "Down", None, false, true, true);
        assert_eq!(
            e.view().extra_cursors.len(),
            1,
            "Ctrl+Alt+Down must add exactly one cursor"
        );
    }

    /// Alt+Left / Alt+Right resize the sidebar in Vim mode (mode-independent,
    /// handing the width change back to the caller because the two backends
    /// store the sidebar width in different places — the *clamp* is shared).
    /// #1744: in VSCode mode the plain chord means navigate back/forward
    /// instead (see [`vscode_mode_alt_left_right_navigate_instead_of_resize`]
    /// below), so Ctrl+**Shift**+Alt+Right is VSCode mode's alternate resize
    /// home — plain Ctrl+Alt+Right is already the shipped
    /// `panel_keys.nav_forward` accelerator, which the accelerator tier
    /// claims *above* this rung in the live app (see [`route_alt_key`]'s own
    /// doc), so it cannot be the alternate home.
    #[test]
    fn alt_arrows_resize_in_vim_mode_and_vscode_mode_ctrl_shift_alt() {
        let (outcome, ..) = resolves_identically(&ALT_RIGHT, false);
        assert_eq!(
            outcome,
            AltKeyOutcome::ResizeSidebar(1),
            "Vim-mode Alt+Right must widen the sidebar"
        );

        let (outcome, ..) = resolves_identically(&CTRL_SHIFT_ALT_RIGHT, true);
        assert_eq!(
            outcome,
            AltKeyOutcome::ResizeSidebar(1),
            "VSCode-mode Ctrl+Shift+Alt+Right must widen the sidebar, now \
             that plain Alt+Right means navigate-forward in that mode and \
             plain Ctrl+Alt+Right is the shipped tab-history accelerator"
        );

        // Ctrl+Alt+Right alone (no Shift) must NOT resize in VSCode mode —
        // the resize arms above require both `ctrl` and `shift`, so this
        // falls through to the same `vscode_alt_key_name` lookup plain
        // Alt+Right uses (navigate-forward; `ctrl` is not part of that
        // lookup's key, only its *shift* half is). This function is never
        // actually called with this exact shape in the live app — the
        // accelerator tier above it claims plain Ctrl+Alt+Right first (see
        // this function's own doc) — but pinning what it *would* do keeps
        // the resize arms' `ctrl && shift` guard from silently degrading to
        // `ctrl` alone again.
        let mut e = engine();
        e.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
        e.mode = Mode::Insert;
        assert_eq!(
            route_alt_key(&mut e, "Right", None, false, true, true),
            AltKeyOutcome::Handled,
            "plain Ctrl+Alt+Right (no Shift) must not resize — it falls to \
             the navigate-forward arm instead, same as plain Alt+Right"
        );

        assert_eq!(alt_resized_sidebar_width(30, 1), 31);
        assert_eq!(alt_resized_sidebar_width(30, -1), 29);
        assert_eq!(
            alt_resized_sidebar_width(ALT_SIDEBAR_WIDTH_MIN, -1),
            ALT_SIDEBAR_WIDTH_MIN,
            "Alt+Left must clamp at the shared floor"
        );
        assert_eq!(
            alt_resized_sidebar_width(ALT_SIDEBAR_WIDTH_MAX, 1),
            ALT_SIDEBAR_WIDTH_MAX,
            "Alt+Right must clamp at the shared ceiling"
        );
    }

    /// #1798: the sidebar's opening width is a line-height *multiple*, so one
    /// number means a 20-column strip on TUI and a ~460px slab on a GUI
    /// backend — which left an 800px window no room for a second editor tab.
    /// [`UnitProfile::sidebar_width_lh`] carries the per-unit value; this
    /// pins the three properties that split has to keep.
    ///
    /// The behavioural proof is in the driver tests — `gtk::testing`'s
    /// `explorer_double_click_opens_second_file_in_a_second_tab_1798` runs at
    /// the reported 800x480 and fails if the GUI profile goes back to `20.0`.
    /// This guards the *invariants* a future edit to either profile could
    /// break while leaving that test green (or while breaking it for a reason
    /// its message would not explain).
    #[test]
    fn unit_profiles_scale_the_sidebar_width_per_unit_1798() {
        let (px, cell) = (UnitProfile::px(), UnitProfile::cell());

        // 1. The TUI is untouched: 20 *cells*, the value it already shipped
        //    (`ShellConfig::new`'s own default, which `App::shell_config`
        //    used to leave alone on every backend).
        assert_eq!(cell.sidebar_width_lh, 20.0);

        // 2. The GUI opens narrower than that, because one GUI line height
        //    is ~23 device pixels rather than one terminal row.
        assert!(
            px.sidebar_width_lh < cell.sidebar_width_lh,
            "the GUI profile must open narrower than the TUI's cell count, or \
             #1798's 800px window still has no room for a second tab: {} vs {}",
            px.sidebar_width_lh,
            cell.sidebar_width_lh
        );

        // 3. Neither profile may open *below* the shared Alt rung's floor.
        //    Two independent reasons, both load-bearing:
        //      * `AppShell::compute_layout` clamps the opening width through
        //        `min_sidebar_width` (still `ALT_SIDEBAR_WIDTH_MIN` on every
        //        backend), so a narrower value is silently discarded; and
        //      * the user's first Alt+Right would jump straight to the floor
        //        with no way back, since `alt_resized_sidebar_width` clamps to
        //        the same bound (measured at 10.0 on GTK: 230px -> 345px on
        //        Alt+Right, then stuck at 345px on Alt+Left).
        //    Lowering that floor per-unit is a change to the *shared* rung's
        //    cross-backend contract (#759) and wants its own issue; until
        //    then this is the real lower bound on `sidebar_width_lh`.
        for (name, p) in [("px", px), ("cell", cell)] {
            assert!(
                p.sidebar_width_lh >= ALT_SIDEBAR_WIDTH_MIN as f32,
                "{name} profile opens the sidebar at {}, below the shared Alt \
                 rung's floor of {} — compute_layout would clamp it back up \
                 and Alt+Left could never return to it",
                p.sidebar_width_lh,
                ALT_SIDEBAR_WIDTH_MIN
            );
            assert!(
                p.sidebar_width_lh <= ALT_SIDEBAR_WIDTH_MAX as f32,
                "{name} profile opens the sidebar at {}, above the shared Alt \
                 rung's ceiling of {}",
                p.sidebar_width_lh,
                ALT_SIDEBAR_WIDTH_MAX
            );
        }
    }

    /// #1744: VS Code's `workbench.action.navigateBack` (Alt+Left) returns the
    /// cursor to the last jump-list entry — the same mechanism Vim mode's
    /// Ctrl-O already uses (`Engine::jump_list_back`) — rather than resizing
    /// the sidebar, which is where this chord used to land unconditionally
    /// (`AltBase::Left`'s old mode-independent arm, with no VSCode-mode
    /// exception at all). Verified RED against unfixed `develop`: the old
    /// unconditional `AltBase::Left => ResizeSidebar(-1)` arm ran before the
    /// VSCode-mode tier ever got a chance, so this chord could never reach
    /// `jump_list_back` (`KNOWN_GAPS::ALT_LEFT_RIGHT_IS_SIDEBAR_RESIZE`).
    #[test]
    fn vscode_mode_alt_left_right_navigate_instead_of_resize() {
        let mut e = engine();
        e.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
        e.mode = Mode::Insert;
        e.push_jump_location();
        e.view_mut().cursor.line = 2;

        let outcome = route_alt_key(&mut e, "Left", None, false, false, true);
        assert_eq!(
            outcome,
            AltKeyOutcome::Handled,
            "VSCode-mode Alt+Left must be handled here, not handed back as \
             ResizeSidebar"
        );
        assert_eq!(
            e.view().cursor.line,
            0,
            "Alt+Left (navigateBack) must return to the jump-list entry"
        );

        e.view_mut().cursor.line = 2;
        let outcome = route_alt_key(&mut e, "Right", None, false, false, true);
        assert_eq!(
            outcome,
            AltKeyOutcome::Handled,
            "VSCode-mode Alt+Right must be handled here, not handed back as \
             ResizeSidebar"
        );
        assert_eq!(
            e.view().cursor.line,
            2,
            "Alt+Right (navigateForward) must move forward in the jump \
             list, back to the line Alt+Left just left — not leave the \
             cursor on line 0 where Alt+Left's own assertion above left it"
        );

        // Sanity: the same chords still resize in Vim mode.
        let (outcome, ..) = resolves_identically(&ALT_LEFT, false);
        assert_eq!(outcome, AltKeyOutcome::ResizeSidebar(-1));
    }

    /// A chord with no arm must fall through untouched — this rung itself
    /// never sinks it. `Alt+q` is deliberately not bound to anything.
    ///
    /// #1764: this rung's own `Fallthrough` return is no longer the end of
    /// the story for a printable-char base like `'q'` — the caller
    /// (`App::handle_key_press`'s `Fallthrough` arm) now substitutes an
    /// implicit Escape for it instead of redelivering the bare key. That
    /// substitution lives in `app.rs`, one layer above what this function
    /// and this test can see; what this test still pins is this rung's own
    /// contract, that it does not claim the chord itself.
    #[test]
    fn an_unclaimed_alt_chord_falls_through_on_both_spellings() {
        let unbound = Chord {
            tui: ("", Some('q')),
            gtk: ("q", Some('q')),
            shift: false,
            ctrl: false,
        };
        for vscode in [false, true] {
            let (outcome, ..) = resolves_identically(&unbound, vscode);
            assert_eq!(outcome, AltKeyOutcome::Fallthrough);
        }
    }

    /// Without Alt held the rung must decline immediately, so callers can
    /// invoke it unconditionally rather than re-testing the modifier
    /// themselves (which is how the two backends ended up with two different
    /// gates in the first place).
    #[test]
    fn no_alt_modifier_is_an_immediate_fallthrough() {
        let mut e = engine();
        e.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
        assert_eq!(
            route_alt_key(&mut e, "Down", None, false, false, false),
            AltKeyOutcome::Fallthrough
        );
        assert!(
            e.buffer().to_string().starts_with("alpha\n"),
            "a plain Down must not have been decoded as `Alt_Down`"
        );
    }

    /// #1764 (review nit, round 1): every other predicate on this rung has
    /// its own exhaustive-over-`Mode` unit test here; `alt_mnemonic_open_
    /// allowed` had none. Pins both halves of its contract: on the
    /// toggleable (TUI) profile only Normal/Visual* allow a fresh open, and
    /// on every other (GTK/macOS/Win) profile every mode allows it — there
    /// is no pty-fusion ambiguity to defend against there.
    #[test]
    fn alt_mnemonic_open_allowed_is_normal_and_visual_only_on_the_toggleable_profile() {
        let toggleable_allowed = [
            Mode::Normal,
            Mode::Visual,
            Mode::VisualLine,
            Mode::VisualBlock,
        ];
        let toggleable_blocked = [Mode::Insert, Mode::Replace, Mode::Command, Mode::Search];
        for mode in toggleable_allowed {
            assert!(
                alt_mnemonic_open_allowed(mode, true),
                "{mode:?} must allow a fresh mnemonic open on the toggleable profile"
            );
            // Every mode is allowed on a non-toggleable (GTK/macOS/Win)
            // profile — there is no pty to fuse an Escape into the chord.
            assert!(alt_mnemonic_open_allowed(mode, false));
        }
        for mode in toggleable_blocked {
            assert!(
                !alt_mnemonic_open_allowed(mode, true),
                "{mode:?} must block a fresh mnemonic open on the toggleable profile"
            );
            assert!(alt_mnemonic_open_allowed(mode, false));
        }
    }
}
