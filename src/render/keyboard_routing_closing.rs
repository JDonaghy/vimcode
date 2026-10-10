use super::*;

// ─── Closing rungs (#762 / #734 slice 7) ────────────────────────────────────
//
// Slices 1–6 lifted the ladder's big tiers into this module. What was left
// behind in the two surviving entry points — the pre-#1434 TUI shell's
// `handle_key_pressed` and `App::handle_key_press` — was the residue: five
// small tiers that each backend still stated for itself, three of them only
// on one backend and therefore silently missing on the other. They are stated
// here, once, in ladder order:
//
// 1. [`is_force_redraw_key`]  — Ctrl+L. TUI-only; GTK had no rung for it, so
//    the chord fell through to whatever tier was next — `Engine::handle_key`
//    with the editor focused, or the focused sidebar panel's own key table
//    otherwise — instead of being consumed as a repaint request.
// 2. The modal folder picker (`quadraui::FolderPickerController`, #815) — key
//    handling lives entirely in the controller now (`FolderPickerController::
//    handle`), so there is no `render.rs` rung to state for it; both backends
//    just feed it the raw `UiEvent` while the picker is open.
// 3. [`route_debug_fkey`] — F5/F9/F10/F11 and their Shift twins. Each backend
//    had **half** of this rung: TUI had Shift+F5/Shift+F11 but no unshifted
//    global F-key tier (a focused non-editor panel swallowed them), GTK had
//    the unshifted tier but ignored `shift` entirely, so Shift+F5 ran
//    *continue* instead of *stop* and Shift+F11 ran *step-in* instead of
//    *step-out*.
// 4. [`route_hover_popup_copy`] — y / Y / Ctrl+C while the editor hover popup
//    holds focus. GTK-only; on TUI the same keys fell through to the editor
//    and moved the cursor / yanked the buffer instead of the popup.
// 5. [`route_cmdline_selection_key`] — Ctrl+C copies the command-line /
//    message-line mouse selection, any other key clears it. Shared by both
//    backends (#816): TUI has populated `Engine::cmd_sel` since #602, and GTK
//    now arms the same field from its own press/drag handlers via
//    `quadraui::CommandLineLayout::hit_test`.
// 6. [`post_key_epilogue`] — the after-every-editor-keypress bookkeeping.
//    TUI ran seven behaviours, GTK ran three of them; the four GTK was
//    missing (sidebar autohide, explorer refresh after a file move, quickfix
//    scroll clamping, and the "no sidebar visible → focus the activity bar"
//    half of Ctrl-W overflow) are now shared.

/// The engine call each [`FocusKeyRoute`] arm makes once the resolver has
/// named the focus owner — the *dispatch* half of slice 2's rung, which only
/// shared the *routing* half (#762 / #734 slice 7).
///
/// Call with the backend's key name already normalised to the engine's own
/// spelling (`Return`/`Escape`/`BackSpace`/`Up`/… and the bare character for
/// printables). Returns `Some(panel_still_focused)` — the flag the caller
/// feeds to its "did focus leave the sidebar?" bookkeeping — or `None` for
/// the four routes this cannot own:
///
/// - [`FocusKeyRoute::Debug`] needs a live `Backend` and the un-round-tripped
///   `UiEvent` to drive `dap_sidebar_system`,
/// - [`FocusKeyRoute::Ai`] needs the same — a live `Backend` and the
///   un-round-tripped `UiEvent` — to drive `ai_chat` (`ChatController`, #819);
///   see [`route_ai_chat_event`],
/// - [`FocusKeyRoute::Explorer`] is a backend widget on GTK and a `TuiSidebar`
///   on TUI,
/// - [`FocusKeyRoute::ActivityBar`] / [`FocusKeyRoute::None`] are not sidebar
///   panels at all.
///
/// `sc_unicode` exists because the source-control panel's key table reads
/// printables that its siblings read as named keys (`?`, `/`), so a backend
/// whose translation layer distinguishes the two has a second spelling to
/// hand over. #1422 deleted GTK's own such layer (`map_gtk_key_with_unicode`
/// vs `map_gtk_key_name`) once its spelling turned out identical to
/// `unicode`'s — both backends now pass `unicode` twice.
pub fn dispatch_sidebar_panel_key(
    engine: &mut Engine,
    route: FocusKeyRoute,
    key_name: &str,
    unicode: Option<char>,
    sc_unicode: Option<char>,
    ctrl: bool,
    alt: bool,
) -> Option<bool> {
    Some(match route {
        FocusKeyRoute::ExtPanel => {
            if engine.ext_panel_input_active {
                engine.handle_ext_panel_input_key(key_name, false, unicode);
            } else {
                // h/Left moves focus to the activity bar; other exits go to
                // the editor. Either way `ext_panel_has_focus` goes false.
                engine.handle_ext_panel_key(key_name, false, unicode);
            }
            engine.ext_panel_has_focus && engine.dialog.is_none()
        }
        FocusKeyRoute::ExtSidebar => {
            engine.dispatch_ext_sidebar_key_unified(key_name, unicode);
            engine.ext_sidebar_has_focus && engine.dialog.is_none()
        }
        FocusKeyRoute::Settings => {
            engine.handle_settings_key(key_name, ctrl, unicode);
            engine.settings_has_focus && engine.dialog.is_none()
        }
        FocusKeyRoute::Search => {
            // Ctrl+V does not reach here on GTK: quadraui's runner intercepts
            // it and delivers `UiEvent::ClipboardPaste` straight to
            // `ShellApp::handle`, which routes through `Engine::route_paste`
            // (covering the search/replace fields) before any key event is
            // dispatched (#593).
            engine.dispatch_search_sidebar_key_unified(key_name, ctrl, alt, unicode);
            engine.search_has_focus
        }
        FocusKeyRoute::SourceControl => {
            engine.dispatch_sc_sidebar_key_unified(key_name, ctrl, sc_unicode);
            engine.sc_has_focus
        }
        FocusKeyRoute::Board => engine.dispatch_board_key_unified(key_name),
        FocusKeyRoute::Debug | FocusKeyRoute::Explorer | FocusKeyRoute::Ai => return None,
        FocusKeyRoute::ActivityBar | FocusKeyRoute::None => return None,
    })
}

/// Ctrl+L — "repaint everything".
///
/// Consuming it here (rather than letting it fall through) is the behaviour
/// the legacy TUI loop had and GTK never did — GTK carried no Ctrl+L tier at
/// all, so the chord was dispatched like any other Ctrl-modified `l`.
///
/// #1243: TUI now honours the *full* semantics via
/// `quadraui::Backend::request_full_repaint` (quadraui#1037) —
/// the pre-#1434 TUI shell's `handle_key_pressed` calls it on this chord before returning
/// `Reaction::Redraw`, and `tui::run::run_inner`'s frame loop clears
/// `ratatui::Terminal`'s previous-frame buffer the next time it paints. GTK's
/// `DrawingArea` repaints in full every frame via Cairo (no incremental diff
/// to desync in the first place), so `GtkBackend` never overrides the hook
/// and `App::handle_key_press` requests only an ordinary redraw — see
/// `Backend::request_full_repaint`'s own doc for why that default is
/// correct rather than a gap. The same hook also covers the
/// popup-disappearance clear this comment used to point at —
/// the pre-#1434 TUI shell's `render_content`'s `had_popup_overlay` transition check.
///
/// **#1393 (quadraui#1060 consume side): the Ctrl+L call site now has a
/// driver test that proves the repaint itself**, not just the decision
/// logic — `tui_main::shell_app`'s
/// `ctrl_l_repaints_a_stale_cell_an_incremental_diff_would_skip_via_vt_driver`,
/// RED-verified by disabling this rung's `backend.request_full_repaint()`
/// call. A `TestBackend`-based driver (`quadraui::tui::testing::TuiDriver`)
/// genuinely cannot observe this — see [`popup_overlay_closed_this_frame`]'s
/// doc for the full proof — so that test uses the vt100-backed
/// `quadraui::tui::vt_testing::TuiVtDriver` instead (quadraui#1060,
/// `driver_with_shell` + `inject_raw`, landed at this repo's pinned rev).
/// The decision logic here is additionally unit-tested in
/// `slice7_router_tests` below; the wiring at the call site is a one-line
/// delegation.
///
/// `insert_ctrl_x_pending` is `engine.insert_ctrl_x_pending` (only ever true
/// in the one-keystroke window right after `<C-x>` in Insert mode): right
/// after `<C-x>`, `<C-x><C-l>` is the whole-line completion sub-mode
/// (`:h i_CTRL-X_CTRL-L`, #1160), not a repaint request. This rung runs
/// *before* `Engine::handle_key` is ever called on either backend
/// (`app.rs`/`shell_app.rs`), so without this carve-out the keystroke never
/// reaches the engine at all for `<C-x><C-l>` to see it — the same shape of
/// bug the `<C-x><C-f>` (find/replace) and `<C-x><C-s>` (save) carve-outs
/// fix inside `Engine::handle_key` itself, just one layer further out.
pub fn is_force_redraw_key(
    key_name: &str,
    unicode: Option<char>,
    ctrl: bool,
    insert_ctrl_x_pending: bool,
) -> bool {
    if insert_ctrl_x_pending {
        return false;
    }
    ctrl && (matches!(unicode, Some('l') | Some('L')) || key_name == "l" || key_name == "L")
}

/// Did an editor-anchored popup (the completions/hover-doc picker or the
/// modal folder picker) that was visible last frame close this frame?
/// (#1243, TUI-only — the pre-#1434 TUI shell's `render_content`'s `had_popup_overlay`.)
///
/// The transition that must call `quadraui::Backend::request_full_repaint`
/// (quadraui#1037): a popup staying open, staying closed, or newly opening
/// all paint their own content this frame regardless of ratatui's diff
/// cache, so only the *closing* transition can leave stale glyphs — ones
/// the popup itself painted last frame, in cells nothing repaints this
/// frame — for that cache to wrongly believe are still correct and skip.
/// Pulled out as its own pure function (mirroring [`is_force_redraw_key`]
/// just above) because it is the one piece of this wiring that *is*
/// directly unit-testable from here.
///
/// **Why `tui_main::shell_app`'s `TestBackend`-driven
/// `picker_dismiss_leaves_no_popup_glyphs_on_the_grid_via_shell_app` cannot
/// assert on the *repaint* itself, only on this edge-detection predicate —
/// verified directly against quadraui checkout rev `215e9e4`
/// (`Cargo.toml`'s pin), not assumed:**
/// `quadraui::tui::testing::TuiDriver` (`TestBackend`-backed) already
/// consumes `request_full_repaint` in its own `render()` (it calls
/// `Terminal::clear()` when the flag is set, exactly like the live
/// runner) — that part isn't the blocker. The blocker is that
/// `ratatui`'s `Terminal::clear()` is *provably output-identical* under a
/// `TestBackend`: it blanks the backend buffer and resets the back
/// buffer, so the following `draw` diffs a fully-desired frame against a
/// blank previous frame and writes every non-blank cell — landing on
/// byte-for-byte the same buffer the incremental path lands on, because
/// `Terminal::draw`'s own contract already requires the render callback
/// to repaint the whole frame. There is therefore no stale cell for a
/// `TestBackend`-based driver to observe, and no `screen()` /
/// `style_at()` / `terminal_cursor_position()` assertion that can
/// distinguish the two paths. Only content written *outside* ratatui's
/// `Buffer`/diff tracking (e.g. an embedded PTY writing raw bytes
/// straight into the terminal) can produce the "diff believes this cell
/// is unchanged" condition the hook exists to fix, and only
/// [`TuiVtDriver`] (vt100-backed, real ANSI byte stream) can model that —
/// see its own `render_actually_clears_stale_content_outside_the_diff_cache`
/// test, which proves the *mechanism* works upstream.
///
/// **#1393 (quadraui#1060 consume side) closed the two seams that used to
/// block reusing `TuiVtDriver` from a vimcode `ShellApp` impl**:
/// `quadraui::tui::vt_testing::driver_with_shell` (mirroring
/// `quadraui::tui::testing::driver_with_shell`) now wraps a `ShellApp` in
/// the same `ShellAdapter` stack the live runner uses, and the public
/// `TuiVtDriver::inject_raw` hook exposes the out-of-band byte injection
/// that test's technique relies on — both landed at this repo's pinned
/// rev. `tui_main::shell_app`'s
/// `ctrl_l_repaints_a_stale_cell_an_incremental_diff_would_skip_via_vt_driver`
/// and
/// `popup_dismiss_repaints_a_stale_cell_an_incremental_diff_would_skip_via_vt_driver`
/// use them to assert on the actual repaint this function's transition
/// exists to trigger, RED-verified against unfixed `develop`-shaped wiring
/// by disabling each call site in turn.
///
/// [`TuiVtDriver`]: https://github.com/JDonaghy/quadraui/blob/215e9e4/quadraui/src/tui/vt_testing.rs
pub fn popup_overlay_closed_this_frame(was_open: bool, is_open_now: bool) -> bool {
    was_open && !is_open_now
}

/// Popup rect for the folder-picker modal (`quadraui::FolderPickerController`,
/// #815), in the caller's own units.
///
/// Both backends use the same proportions the controller's own
/// `tui_folder_picker` / `gtk_folder_picker` demo apps use — 60% of the
/// viewport width (min 50), 55% of the height (min 15 lines) — computed
/// purely from `Backend::viewport()` / `Backend::line_height()`, which is
/// what lets one function serve TUI (cells, `line_height() == 1.0`) and GTK
/// (pixels) without either backend branching on its own identity.
pub fn folder_picker_popup_rect(viewport: quadraui::Rect, line_height: f32) -> quadraui::Rect {
    let w = (viewport.width * 0.6).max(50.0);
    let h = (viewport.height * 0.55).max(15.0 * line_height);
    let x = viewport.x + (viewport.width - w) / 2.0;
    let y = viewport.y + (viewport.height - h) / 2.0;
    quadraui::Rect::new(x, y, w, h)
}

/// Visible list rows inside an open folder picker's popup — the popup height
/// minus the `Palette` chrome (title + query + borders,
/// `quadraui::PALETTE_CHROME_ROWS`) that `FolderPickerController::handle`
/// needs to keep the selection's scroll position in sync.
pub fn folder_picker_visible_rows(popup_rect: quadraui::Rect, line_height: f32) -> usize {
    let rows = (popup_rect.height / line_height.max(f32::EPSILON)) as usize;
    rows.saturating_sub(quadraui::PALETTE_CHROME_ROWS)
}

/// Where a mouse press against an open folder-picker popup lands.
///
/// Mirrors [`PickerHitGeometry::resolve`]'s shape for the unified picker, but
/// this rung is deliberately *not* folded into [`route_modal_overlay_click`] /
/// [`MOUSE_ARBITRATION_ORDER`]: the folder picker swallows every input while
/// open (like a modal dialog) rather than competing for z-order with the
/// other overlays, so both backends check it first and return early — the
/// same shape the pre-#815 TUI-only code already had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderPickerClickRoute {
    /// Landed on a selectable results row — index into `filtered()`.
    SelectRow(usize),
    /// Landed inside the popup, but not on a selectable row (title/query/
    /// border chrome, or past the last row).
    Consume,
    /// Landed outside the popup — dismiss.
    Dismiss,
}

/// Resolve a mouse press at `(x, y)` against the folder picker's painted
/// popup `rect`. `row_height` is the backend's line height (`1.0` on TUI, the
/// painted pixel line height on GTK); `scroll_top`/`total_filtered` come
/// straight off the live `FolderPickerController`.
pub fn route_folder_picker_click(
    rect: quadraui::Rect,
    x: f32,
    y: f32,
    row_height: f32,
    scroll_top: usize,
    total_filtered: usize,
) -> FolderPickerClickRoute {
    let inside = x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height;
    if !inside {
        return FolderPickerClickRoute::Dismiss;
    }
    let row_height = row_height.max(f32::EPSILON);
    let rows_top = rect.y + quadraui::PALETTE_CHROME_ROWS as f32 * row_height;
    let rows_bottom = rect.y + rect.height;
    if y >= rows_top && y < rows_bottom {
        let row = ((y - rows_top) / row_height) as usize;
        let idx = scroll_top + row;
        if idx < total_filtered {
            return FolderPickerClickRoute::SelectRow(idx);
        }
    }
    FolderPickerClickRoute::Consume
}

/// Move an open folder picker's selection to `idx` by repeated
/// `move_up`/`move_down` calls.
///
/// `FolderPickerController` (quadraui) exposes no direct "set selected index"
/// setter — only relative movement, clamped at both ends — so a mouse click
/// on a specific row has to walk there one step at a time. The list is capped
/// at 50 entries (`FolderPickerController`'s own `filter_dir_entries` cap),
/// so this is at most 50 calls, all pure index arithmetic.
pub fn set_folder_picker_selected(picker: &mut quadraui::FolderPickerController, idx: usize) {
    while picker.selected() < idx {
        picker.move_down();
    }
    while picker.selected() > idx {
        picker.move_up();
    }
}

/// Where a mouse press against the open change-review surface (#955,
/// shared with #525) lands, resolved against the *exact* geometry
/// [`paint_change_review_rung`] last painted
/// (`diff_rect`/`line_height` — both backends read these from
/// `Engine::change_review_diff_rect`/`Backend::line_height`, same
/// "paint writes it, click routing reads it" contract as
/// `command_line_rect`).
///
/// Deliberately *not* folded into [`route_modal_overlay_click`] /
/// [`MOUSE_ARBITRATION_ORDER`] — same call [`route_folder_picker_click`]
/// makes and for the same reason: this surface swallows every click while
/// open (like a modal dialog) rather than competing for z-order with the
/// other overlays, so both backends check it first and return early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeReviewClickRoute {
    /// Landed on a real diff row — jump to the file/line it represents
    /// (`Engine::change_review_jump_to_hit`).
    Jump(quadraui::DiffViewHit),
    /// Landed inside the surface but not on a row (a unified hunk header,
    /// the status footer, or empty space) — swallow, no navigation.
    Consume,
}

/// Resolve a click at `(x, y)` (ABSOLUTE, backend-native units) against the
/// currently-shown entry's `DiffView`, re-deriving the same geometry
/// [`paint_change_review_rung`] painted from (`entry.view.layout(diff_rect,
/// line_height)`) rather than trusting a cached one, so paint and
/// hit-testing can never disagree (`DiffViewGeometry::hit_test`'s own
/// contract).
pub fn route_change_review_click(
    diff_rect: quadraui::Rect,
    view: &quadraui::DiffView,
    line_height: f32,
    x: f32,
    y: f32,
) -> ChangeReviewClickRoute {
    let geometry = view.layout(diff_rect, line_height);
    match geometry.hit_test(x, y) {
        hit @ quadraui::DiffViewHit::Row { .. } => ChangeReviewClickRoute::Jump(hit),
        _ => ChangeReviewClickRoute::Consume,
    }
}

/// What a debugger function key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugFKey {
    /// Feed this name to `Engine::handle_key` (the unshifted tier).
    EngineKey(&'static str),
    /// Run this `:`-command through `Engine::execute_command` (the Shift
    /// tier, which has no key binding of its own).
    Command(&'static str),
}

/// The shared debugger F-key rung — global, above every focus owner.
///
/// F5 / F9 / F10 / F11 are debugger commands regardless of which panel holds
/// focus, and Shift+F5 / Shift+F11 are *different* debugger commands rather
/// than shifted spellings of the same one. Before this rung each backend had
/// one half of that and neither had both:
///
/// - TUI reached the Shift tier only after the focus owners had declined the
///   key, and had no unshifted global tier at all — so with the debug panel
///   focused, F5 went to the panel's own action-key table.
/// - GTK had the unshifted global tier but tested only `!ctrl && !alt`, never
///   `shift`, so **Shift+F5 ran `continue` instead of `stop`** and Shift+F11
///   ran `step-in` instead of `step-out` — the exact opposite of the intent.
///
/// Returns `None` for anything that is not one of these six chords, including
/// every Ctrl- or Alt-modified F-key (those belong to the accelerator table).
pub fn route_debug_fkey(key_name: &str, ctrl: bool, shift: bool, alt: bool) -> Option<DebugFKey> {
    if ctrl || alt {
        return None;
    }
    if shift {
        return Some(match key_name {
            "F5" => DebugFKey::Command("stop"),
            "F11" => DebugFKey::Command("stepout"),
            _ => return None,
        });
    }
    Some(match key_name {
        "F5" => DebugFKey::EngineKey("F5"),
        "F9" => DebugFKey::EngineKey("F9"),
        "F10" => DebugFKey::EngineKey("F10"),
        "F11" => DebugFKey::EngineKey("F11"),
        _ => return None,
    })
}

/// Copy the editor hover popup's selection when the popup holds focus.
///
/// `y`, `Y` and Ctrl+C are the three chords; they mean "copy" only while
/// `editor_hover_has_focus`, and must not reach `Engine::handle_key` (where
/// `y` would start a vim yank against the *buffer*). GTK had this rung and
/// TUI did not, so on TUI a focused hover popup leaked all three to the
/// editor.
///
/// Returns the text to place on the clipboard, or `None` when the chord does
/// not apply and the caller should keep dispatching.
pub fn route_hover_popup_copy(engine: &Engine, key_name: &str, ctrl: bool) -> Option<String> {
    if !engine.editor_hover_has_focus {
        return None;
    }
    let is_copy =
        key_name == "y" || key_name == "Y" || (ctrl && (key_name == "c" || key_name == "C"));
    if !is_copy {
        return None;
    }
    engine.hover_selection_text()
}

/// What a key does to a live command-line / message-line mouse selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmdSelKeyRoute {
    /// Ctrl+C over a selection — put this text on the clipboard, then clear
    /// the selection.
    Copy(String),
    /// Any other key while a selection (or a command-line) is live — drop the
    /// selection but keep dispatching the key.
    Clear,
    /// Nothing to do.
    Keep,
}

/// Resolve one key against the command-line / message-line selection.
///
/// `sel` is the half-open-inclusive `(anchor, head)` column pair the mouse
/// drag left behind, in *either* order. In `Command`/`Search` mode column 0
/// is the `:`/`/` prefix and columns 1.. index `Engine::command_buffer`; in
/// every other mode the selection indexes `Engine::message` with no offset.
/// That one-column skew is the whole reason this is worth stating once.
///
/// Both backends populate `sel` from `Engine::cmd_sel` (#816): TUI's
/// the pre-#1434 TUI shell's `handle_mouse_event` (#602) and GTK's press/drag handlers
/// (`handle_mouse_click_msg` / `handle_mouse_drag_msg`, driven through
/// `quadraui::CommandLineLayout::hit_test`) both arm the same engine-level
/// field, and `CommandLineState::command_line_selecting` mirrors
/// `Engine::cmd_dragging` for GTK's paint layout.
pub fn route_cmdline_selection_key(
    engine: &Engine,
    unicode: Option<char>,
    ctrl: bool,
    sel: Option<(usize, usize)>,
) -> CmdSelKeyRoute {
    let in_cmdline = matches!(
        engine.mode,
        crate::core::Mode::Command | crate::core::Mode::Search
    );
    let Some((start, end)) = sel else {
        return if in_cmdline {
            CmdSelKeyRoute::Clear
        } else {
            CmdSelKeyRoute::Keep
        };
    };
    if ctrl && matches!(unicode, Some('c') | Some('C')) {
        let lo = start.min(end);
        let hi = start.max(end);
        let (source, offset) = if in_cmdline {
            // col 0 = the ':' / '/' prefix, col 1+ = buffer chars.
            (&engine.command_buffer, 1usize)
        } else {
            (&engine.message, 0usize)
        };
        let lo = lo.saturating_sub(offset);
        let hi = hi.saturating_sub(offset);
        let text: String = source
            .chars()
            .enumerate()
            .filter(|(i, _)| *i >= lo && *i <= hi)
            .map(|(_, c)| c)
            .collect();
        return CmdSelKeyRoute::Copy(text);
    }
    CmdSelKeyRoute::Clear
}

/// The residue of the post-key epilogue that the *backend* still has to act
/// on, after [`post_key_epilogue`] has applied everything `Engine` owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PostKeyEpilogue {
    /// Ctrl-W h/l overflowed left with a sidebar panel visible — give the
    /// sidebar band the keyboard. Also set when a panel reveal from inside
    /// `Engine::handle_key` asked for the keyboard via
    /// `Engine::sidebar_focus_requested` (#1450).
    pub focus_sidebar: bool,
    /// Ctrl-W h/l overflowed left with **no** sidebar panel visible — put the
    /// cursor on the activity bar toolbar instead. GTK never did this: its
    /// overflow arm unconditionally called `focus_sidebar_panel`, so with the
    /// sidebar hidden the keypress went nowhere.
    pub focus_activity_bar: bool,
    /// A yank just happened — arm the 200 ms highlight expiry.
    pub arm_yank_highlight: bool,
}

/// The shared after-every-editor-keypress epilogue (#762 / #734 slice 7).
///
/// Seven behaviours ran after each key that reached `Engine::handle_key`.
/// TUI ran all seven inline; GTK ran three (nav overflow, clipboard sync,
/// yank timer). Of the rest, two are genuinely new GTK behaviour and two are
/// not:
///
/// - **sidebar autohide** (new) — `autohide_panels` never fired on GTK when
///   focus returned to the editor, so the panel stayed pinned open.
/// - **activity-bar overflow** (new) — see
///   [`PostKeyEpilogue::focus_activity_bar`].
/// - **explorer refresh** (latency, not new) — `explorer_needs_refresh`, set
///   by a completed file move/rename/create, was already drained on GTK from
///   at least four other call sites, including `handle_poll_tick`, which runs
///   independently of key presses. Routing it through this rung too just
///   closes the window between a move completing and the next poll tick —
///   it was never simply missing.
/// - **quickfix scroll clamp** (inapplicable on GTK) — GTK never carried
///   `quickfix_scroll_top` as per-keypress state to begin with (see that
///   parameter's doc below), so this arm is a no-op there; it exists here
///   purely for TUI, which does carry the field.
///
/// Macro playback and the unnamed-register→clipboard sync stay with the
/// caller: draining `advance_macro_playback` yields `EngineAction`s that only
/// the backend's own dispatcher can apply (and one of them is *quit*), and
/// the clipboard write is a backend callback.
///
/// `quickfix_scroll_top` is `None` for backends that recompute the quickfix
/// scroll offset statelessly each frame instead of carrying it across key
/// events — GTK's `draw_bottom_chrome` does exactly that, so it has no field
/// to hand in.
pub fn post_key_epilogue(
    engine: &mut Engine,
    quickfix_scroll_top: Option<&mut usize>,
) -> PostKeyEpilogue {
    let mut out = PostKeyEpilogue::default();

    // #1450: a programmatic panel reveal from inside `Engine::handle_key`
    // (Visual `<leader>ai`, `:{range}AI` with no message) asked for the
    // keyboard. One-shot — taken here so the *next* keypress routes normally
    // again. Shared, so TUI's cached `sidebar.has_focus` and GTK's
    // re-sync both go through the one field they already honour.
    if std::mem::take(&mut engine.sidebar_focus_requested) {
        out.focus_sidebar = true;
    }

    // Ctrl-W h/l overflow: move focus to the sidebar, or — when no panel is
    // visible to receive it — to the activity bar.
    if let Some(false) = engine.handle_nav_overflow() {
        if engine.app_shell.sidebar_visible() {
            out.focus_sidebar = true;
        } else {
            let idx = engine.activity_bar_toolbar_idx_for_active_panel();
            engine.activity_bar_focus_in_at(idx);
            out.focus_activity_bar = true;
        }
    }

    // Auto-hide the sidebar when focus returns to the editor.
    // (`sidebar_has_focus()` includes `activity_bar_focused`, so autohide is
    // suppressed while the user navigates the toolbar.)
    if engine.should_autohide_sidebar() {
        engine.app_shell.hide_sidebar();
    }

    // Rebuild the explorer tree if a file move just completed.
    if engine.explorer_needs_refresh {
        engine.explorer_needs_refresh = false;
        engine.explorer_rebuild_rows();
    }

    // Keep the selected entry inside the six-row window — the active
    // window's location list shares this same scroll state when it (rather
    // than the global quickfix list) is the one occupying the bottom "list
    // rung" (#1155).
    if let Some(scroll_top) = quickfix_scroll_top {
        let selected = if engine.quickfix.open {
            Some(engine.quickfix.selected)
        } else {
            engine
                .location_lists
                .get(&engine.active_window_id())
                .filter(|l| l.open)
                .map(|l| l.selected)
        };
        if let Some(selected) = selected {
            const QF_VISIBLE: usize = 5; // 6 rows − 1 header
            if selected < *scroll_top {
                *scroll_top = selected;
            } else if selected >= *scroll_top + QF_VISIBLE {
                *scroll_top = selected + 1 - QF_VISIBLE;
            }
        } else {
            *scroll_top = 0;
        }
    }

    out.arm_yank_highlight = engine.yank_highlight.is_some();
    out
}

/// Sync the system clipboard from the engine's registers
/// (`clipboard=unnamedplus` semantics), if the mirrored content changed.
///
/// Checks the explicit `+` register first — an explicit write (`"+yy`, a
/// plugin's `vimcode.state.set_register('+', ...)`) always wins — and falls
/// back to the unnamed `"` register so a plain `yy` still reaches the
/// clipboard. `last` is the caller's cache of what was last pushed, so an
/// unchanged register is a no-op rather than a clipboard write on every call.
///
/// #1239: GTK (`App::sync_plus_register_to_clipboard`) and TUI
/// (`sync_tui_clipboard`) used to be two near-identical copies of this that
/// had drifted — TUI mirrored `"` only, so on TUI a subsequent plain
/// yank/delete (which only ever touches `"`, never `+`) could clobber the
/// clipboard mirror of an earlier explicit `+` write instead of leaving it
/// alone (see the `..._1239` tests in `gtk/testing.rs` and
/// `tui_main/shell_app.rs` for the exact repro — the latter deleted by
/// #1434). Both called this one function; GTK's own name survives as a
/// thin wrapper (`App::sync_plus_register_to_clipboard`) so its existing
/// call sites don't need to change — TUI's `sync_tui_clipboard` wrapper
/// was deleted along with the rest of the pre-#1434 TUI shell, its callers
/// now going through `App`. (The issue that raised
/// this bug illustrated the explicit-write case as `:let @+='...'` — that
/// ex command isn't actually implemented in vimcode, `VIM_COMPATIBILITY.md`
/// marks `:let` N/A; every real write path to `+`, from `"+yy` to the Lua
/// plugin API, goes through `Engine::set_register_typed`, which this
/// function's `+`-first priority now matches on both backends.)
///
/// Cadence: called after every editor keypress that might have yanked/cut
/// text, plus a few early-return tiers that skip the main post-key epilogue
/// (terminal-focused keys, an ext-panel key, TUI's bracketed-paste event) —
/// same reason on both backends: whichever tier consumes the key returns
/// before reaching the shared epilogue tail, so it syncs for itself on the
/// way out. That per-keypress cadence is still needed now that the register
/// priority is fixed; it isn't a workaround for the priority bug, so there's
/// nothing to collapse into a single trigger.
pub fn sync_register_to_clipboard(engine: &mut Engine, last: &mut Option<String>) {
    let new_content = engine
        .registers
        .get(&'+')
        .filter(|(s, _)| !s.is_empty())
        .map(|(s, _)| s.clone())
        .or_else(|| {
            engine
                .registers
                .get(&'"')
                .filter(|(s, _)| !s.is_empty())
                .map(|(s, _)| s.clone())
        });

    if new_content != *last {
        if let (Some(ref content), Some(ref cb)) = (&new_content, &engine.clipboard_write) {
            let _ = cb(content.as_str());
        }
        *last = new_content;
    }
}

// ─── Panel-accelerator dispatch rung (#761 / #734 slice 6) ──────────────────
//
// Both backends register the same 15-entry (#1577 added `focus_notifications`) `panel_keys` accelerator set
// (`register_panel_accelerators` in `gtk/mod.rs` / `tui_main/mod.rs`) and,
// until now, each restated its own ~100-line `match id { ... }` translating
// a fired accelerator into the engine call it makes. `PanelAccelerator`
// states the 15-entry id table once; [`dispatch_panel_accelerator`] states
// the nine actions that are pure `Engine` mutations once too.
//
// The other five (`ToggleSidebar`, `FocusExplorer`, `FocusSearch`,
// `OpenTerminal`, `TerminalToggleMax`) queue onto `App`'s `DeferredQueue`
// instead of being inlined here, because their effect genuinely depends on
// state this module has no business owning:
//
// - GTK's `UiEvent::Accelerator` arm has no engine-mutation seam of its own
//   for these — `toggle_sidebar_panel`/`toggle_focus_explorer`/
//   `toggle_focus_search`/`toggle_terminal`/`toggle_terminal_maximize` are
//   `&mut App` methods that also re-sync GTK widgets (file-tree population,
//   focus-chain), so GTK queues a `DeferredAction` and lets `tick()` (which
//   *does* have `&mut App`) run the real method next frame — the same
//   `DeferredQueue` seam every other App-only GTK callback uses (see its
//   doc comment in `gtk/mod.rs`).
// - TUI has no such seam (its `handle()` already owns `&mut self` end to
//   end) but instead carries a second piece of state GTK doesn't have at
//   all: `TuiSidebar::has_focus`, the single input-focus token a terminal
//   has to track by hand where GTK gets real widget focus for free from the
//   toolkit. `focus_explorer`/`focus_search`'s toggle condition reads it
//   *together with* `engine.explorer_has_focus`/`search_has_focus`, so the
//   two backends' conditions are not the same expression over the same
//   state — collapsing them into one body would be papering over a real
//   platform difference, not deleting duplication.
//
// #1499: this used to take a `host: &mut impl PanelAcceleratorHost` argument
// — each backend supplied a thumbnail struct (`GtkAccelHost` / `TuiAccelHost`,
// defined next to their call sites) implementing the five hook methods, so
// the dispatcher itself never needed to know which backend was calling it.
// With the TUI backend gone, `App` is the only implementation left, so
// [`dispatch_panel_accelerator`] takes `app: &App` directly and queues onto
// its `DeferredQueue` inline instead of through a trait object.
//
// #823 item 1: the *registration* half (the 15-entry `(id, binding)` table
// and the loop that (un)registers each one) was, until now, a byte-identical
// 44-line function pasted into `app.rs` and `tui_main/mod.rs`. It has no
// backend-specific step at all — every call goes through the trait object
// `&mut dyn quadraui::Backend` — so unlike the five deferred-queue actions
// above, it needed no per-backend seam to collapse:
// [`register_panel_accelerators`] below is the whole function, called
// verbatim from both `App::setup` and `tui_main`'s startup path.

/// Register the panel-keys accelerator set on the backend. Re-runs on each
/// settings reload so live rebinding takes effect.
///
/// Shared by both backends (#823 item 1) — called from GTK's `ShellApp::setup`
/// and TUI's startup path, each passing their own `&mut dyn quadraui::Backend`.
pub fn register_panel_accelerators(
    backend: &mut dyn quadraui::Backend,
    pk: &crate::core::settings::PanelKeys,
) {
    let entries: [(&str, &str); 15] = [
        (ACC_TOGGLE_SIDEBAR, &pk.toggle_sidebar),
        (ACC_FOCUS_EXPLORER, &pk.focus_explorer),
        (ACC_FOCUS_SEARCH, &pk.focus_search),
        (ACC_FUZZY_FINDER, &pk.fuzzy_finder),
        (ACC_LIVE_GREP, &pk.live_grep),
        (ACC_COMMAND_PALETTE, &pk.command_palette),
        (ACC_OPEN_TERMINAL, &pk.open_terminal),
        (ACC_TERMINAL_TOGGLE_MAX, &pk.toggle_terminal_maximize),
        (ACC_ADD_CURSOR, &pk.add_cursor),
        (ACC_SELECT_ALL_MATCHES, &pk.select_all_matches),
        (ACC_SPLIT_EDITOR_RIGHT, &pk.split_editor_right),
        (ACC_SPLIT_EDITOR_DOWN, &pk.split_editor_down),
        (ACC_NAV_BACK, &pk.nav_back),
        (ACC_NAV_FORWARD, &pk.nav_forward),
        (ACC_FOCUS_NOTIFICATIONS, &pk.focus_notifications),
    ];
    for (id, binding) in entries {
        let acc_id = quadraui::AcceleratorId::new(id);
        if binding.is_empty() {
            // Empty string = unbound (e.g. split_editor_right defaults to ""). Drop
            // any prior registration so a settings reload removing a binding
            // doesn't leave a stale entry.
            backend.unregister_accelerator(&acc_id);
            continue;
        }
        backend.register_accelerator(&quadraui::Accelerator {
            id: acc_id,
            binding: quadraui::KeyBinding::Literal(binding.to_string()),
            scope: quadraui::AcceleratorScope::Global,
            label: None,
        });
    }
}

/// The 15 panel-key accelerator actions, shared by both backends' registries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelAccelerator {
    ToggleSidebar,
    FocusExplorer,
    FocusSearch,
    FuzzyFinder,
    LiveGrep,
    CommandPalette,
    OpenTerminal,
    TerminalToggleMax,
    AddCursor,
    SelectAllMatches,
    SplitEditorRight,
    SplitEditorDown,
    NavBack,
    NavForward,
    /// Give the toast/notification stack keyboard focus (#1577). Same
    /// effect as `:Notifications` — see `Engine::focus_toast_stack`.
    FocusNotifications,
}

pub const ACC_TOGGLE_SIDEBAR: &str = "panel.toggle_sidebar";
pub const ACC_FOCUS_EXPLORER: &str = "panel.focus_explorer";
pub const ACC_FOCUS_SEARCH: &str = "panel.focus_search";
pub const ACC_FUZZY_FINDER: &str = "panel.fuzzy_finder";
pub const ACC_LIVE_GREP: &str = "panel.live_grep";
pub const ACC_COMMAND_PALETTE: &str = "panel.command_palette";
pub const ACC_OPEN_TERMINAL: &str = "panel.open_terminal";
/// Also matched by name in `Engine::handle_ui_event` — keep this string
/// literal in sync with that one if it ever changes.
pub const ACC_TERMINAL_TOGGLE_MAX: &str = "terminal.toggle_maximize";
pub const ACC_ADD_CURSOR: &str = "panel.add_cursor";
pub const ACC_SELECT_ALL_MATCHES: &str = "panel.select_all_matches";
pub const ACC_SPLIT_EDITOR_RIGHT: &str = "panel.split_editor_right";
pub const ACC_SPLIT_EDITOR_DOWN: &str = "panel.split_editor_down";
pub const ACC_NAV_BACK: &str = "panel.nav_back";
pub const ACC_NAV_FORWARD: &str = "panel.nav_forward";
pub const ACC_FOCUS_NOTIFICATIONS: &str = "panel.focus_notifications";

impl PanelAccelerator {
    /// Resolve a registered accelerator id to the action it represents.
    /// Single source of truth for the id table both `register_panel_accelerators`
    /// (gtk/mod.rs, tui_main/mod.rs) and [`dispatch_panel_accelerator`] key off.
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            ACC_TOGGLE_SIDEBAR => Self::ToggleSidebar,
            ACC_FOCUS_EXPLORER => Self::FocusExplorer,
            ACC_FOCUS_SEARCH => Self::FocusSearch,
            ACC_FUZZY_FINDER => Self::FuzzyFinder,
            ACC_LIVE_GREP => Self::LiveGrep,
            ACC_COMMAND_PALETTE => Self::CommandPalette,
            ACC_OPEN_TERMINAL => Self::OpenTerminal,
            ACC_TERMINAL_TOGGLE_MAX => Self::TerminalToggleMax,
            ACC_ADD_CURSOR => Self::AddCursor,
            ACC_SELECT_ALL_MATCHES => Self::SelectAllMatches,
            ACC_SPLIT_EDITOR_RIGHT => Self::SplitEditorRight,
            ACC_SPLIT_EDITOR_DOWN => Self::SplitEditorDown,
            ACC_NAV_BACK => Self::NavBack,
            ACC_NAV_FORWARD => Self::NavForward,
            ACC_FOCUS_NOTIFICATIONS => Self::FocusNotifications,
            _ => return None,
        })
    }
}

/// Resolve `id` and apply the corresponding panel-accelerator action.
/// Returns the resolved [`PanelAccelerator`] (so callers can layer their own
/// residual bookkeeping — e.g. GTK's per-action `DeferredAction::Resize`) or
/// `None` if `id` isn't a registered panel accelerator.
///
/// Five actions (`ToggleSidebar`, `FocusExplorer`, `FocusSearch`,
/// `OpenTerminal`, `TerminalToggleMax`) queue onto `app`'s `DeferredQueue`
/// instead of mutating `engine` directly — `App`'s own `UiEvent::Accelerator`
/// arm has no engine-mutation seam of its own for these
/// (`toggle_sidebar_panel`/`toggle_focus_explorer`/`toggle_focus_search`/
/// `toggle_terminal`/`toggle_terminal_maximize` are `&mut App` methods that
/// also re-sync GTK widgets), so `tick()` (which does have `&mut App`) runs
/// the real method next frame instead — see [`crate::app::DeferredQueue`]'s
/// own doc comment.
pub(crate) fn dispatch_panel_accelerator(
    id: &str,
    engine: &mut Engine,
    app: &crate::app::App,
) -> Option<PanelAccelerator> {
    use crate::app::DeferredAction;
    let action = PanelAccelerator::from_id(id)?;
    match action {
        PanelAccelerator::ToggleSidebar => app.deferred.send(DeferredAction::ToggleSidebar),
        PanelAccelerator::FocusExplorer => app.deferred.send(DeferredAction::ToggleFocusExplorer),
        PanelAccelerator::FocusSearch => app.deferred.send(DeferredAction::ToggleFocusSearch),
        PanelAccelerator::OpenTerminal => app.deferred.send(DeferredAction::ToggleTerminal),
        PanelAccelerator::TerminalToggleMax => {
            app.deferred.send(DeferredAction::ToggleTerminalMaximize)
        }
        PanelAccelerator::FuzzyFinder => {
            engine.open_picker(crate::core::engine::PickerSource::Files)
        }
        PanelAccelerator::LiveGrep => engine.open_picker(crate::core::engine::PickerSource::Grep),
        PanelAccelerator::CommandPalette => {
            engine.open_picker(crate::core::engine::PickerSource::Commands)
        }
        PanelAccelerator::AddCursor => {
            engine.add_cursor_at_next_match();
        }
        PanelAccelerator::SelectAllMatches => engine.select_all_occurrences(),
        PanelAccelerator::SplitEditorRight => engine.open_editor_group(SplitDirection::Vertical),
        PanelAccelerator::SplitEditorDown => engine.open_editor_group(SplitDirection::Horizontal),
        PanelAccelerator::NavBack => engine.tab_nav_back(),
        PanelAccelerator::NavForward => engine.tab_nav_forward(),
        PanelAccelerator::FocusNotifications => {
            engine.focus_toast_stack();
        }
    }
    Some(action)
}

/// #762 / #734 slice 7 — the closing rungs' router tables.
///
/// These are the *spelling-identity* tier of the cross-backend assertion: one
/// function, called by both backends with their own key spellings, resolving
/// to one route. The behavioural tier is the pair of black-box tests that
/// drive the same chord against the same engine state on each backend and
/// assert the same rendered result —
/// `gtk::testing::slice7_debug_fkey_tests::shift_f5_stops_instead_of_continuing_on_gtk`
/// and `tui_main::shell_app`'s
/// `shift_f5_stops_the_debug_session_via_shell_app` /
/// `f5_reaches_the_debugger_from_a_focused_panel_via_shell_app`.
#[cfg(test)]
mod slice7_router_tests {
    use super::*;

    /// The ladder both entry points now walk, in order. Each rung is a shared
    /// function; the *order* is the contract this test pins, because a
    /// backend that reorders one silently diverges while every unit table
    /// below still passes.
    ///
    /// **Verified RED by reordering one rung on one backend:** move
    /// `render::route_debug_fkey` above `render::route_terminal_key` in
    /// the pre-#1434 TUI shell's `handle_key_pressed` and
    /// `terminal_keeps_its_own_function_keys` below fails — a focused PTY
    /// stops receiving F5 and the debugger starts a session instead.
    #[test]
    fn terminal_keeps_its_own_function_keys() {
        // The terminal rung sits *above* the debug F-key rung on both
        // backends, so a focused PTY takes F5 to the process (vim/htop bind
        // it) rather than to the debugger.
        let mut engine = Engine::new_for_test();
        assert!(
            route_debug_fkey("F5", false, false, false).is_some(),
            "precondition: F5 is a debug chord when nothing above claims it"
        );
        // With no terminal focused the terminal rung declines, so the debug
        // rung is what runs.
        assert!(
            !route_terminal_key(&mut engine, "F5", None, false, false, false),
            "no focused terminal: the PTY rung must decline F5"
        );
    }

    #[test]
    fn debug_fkeys_resolve_the_same_from_either_backends_spelling() {
        // Unshifted: the four global debugger keys.
        for name in ["F5", "F9", "F10", "F11"] {
            assert_eq!(
                route_debug_fkey(name, false, false, false),
                Some(DebugFKey::EngineKey(name)),
                "{name} must reach the engine unchanged"
            );
        }
        // Shifted: two *different* commands, not shifted spellings of the
        // same one. This is the arm GTK was missing entirely.
        assert_eq!(
            route_debug_fkey("F5", false, true, false),
            Some(DebugFKey::Command("stop")),
            "Shift+F5 is `stop`, not `continue`"
        );
        assert_eq!(
            route_debug_fkey("F11", false, true, false),
            Some(DebugFKey::Command("stepout")),
            "Shift+F11 is `stepout`, not `stepin`"
        );
        // Shift over a key with no shifted twin declines rather than falling
        // back to the unshifted arm.
        assert_eq!(route_debug_fkey("F9", false, true, false), None);
        assert_eq!(route_debug_fkey("F10", false, true, false), None);
        // Ctrl / Alt belong to the accelerator table.
        assert_eq!(route_debug_fkey("F5", true, false, false), None);
        assert_eq!(route_debug_fkey("F5", false, false, true), None);
        assert_eq!(route_debug_fkey("a", false, false, false), None);
    }

    #[test]
    fn ctrl_l_is_a_force_redraw_from_either_backends_spelling() {
        // TUI hands a `unicode` char; GTK hands a one-character `key_name`.
        assert!(is_force_redraw_key("", Some('l'), true, false));
        assert!(is_force_redraw_key("", Some('L'), true, false));
        assert!(is_force_redraw_key("l", None, true, false));
        assert!(is_force_redraw_key("L", None, true, false));
        // Without Ctrl, `l` is vim's cursor-right and must fall through —
        // the bug GTK had, where Ctrl+L moved the cursor.
        assert!(!is_force_redraw_key("l", Some('l'), false, false));
        assert!(!is_force_redraw_key("k", Some('k'), true, false));
    }

    /// #1160: right after `<C-x>` in Insert mode, `<C-x><C-l>` is the
    /// whole-line completion sub-mode (`:h i_CTRL-X_CTRL-L`), not a repaint
    /// request — `insert_ctrl_x_pending = true` must make the same Ctrl+L
    /// chord that `ctrl_l_is_a_force_redraw_from_either_backends_spelling`
    /// asserts *is* a force-redraw fall through instead, on both backends'
    /// key spellings.
    #[test]
    fn ctrl_l_falls_through_when_ctrl_x_completion_is_pending() {
        assert!(!is_force_redraw_key("", Some('l'), true, true));
        assert!(!is_force_redraw_key("l", None, true, true));
    }

    /// #1243: only the popup-was-up-last-frame-and-is-gone-this-frame edge
    /// must fire `Backend::request_full_repaint` — every other transition
    /// (staying open, staying closed, or newly opening) repaints its own
    /// content this frame regardless of the diff cache, so requesting a
    /// full repaint there would just be wasted work, not a correctness bug,
    /// but pinning all four keeps the predicate from drifting into
    /// "request it whenever a popup isn't open" (which would fire on every
    /// popup-free frame forever).
    #[test]
    fn popup_overlay_closed_this_frame_fires_only_on_the_closing_edge() {
        assert!(
            popup_overlay_closed_this_frame(true, false),
            "open → closed must fire"
        );
        assert!(
            !popup_overlay_closed_this_frame(true, true),
            "staying open must not fire"
        );
        assert!(
            !popup_overlay_closed_this_frame(false, false),
            "staying closed must not fire"
        );
        assert!(
            !popup_overlay_closed_this_frame(false, true),
            "newly opening must not fire"
        );
    }

    #[test]
    fn folder_picker_popup_rect_is_60_by_55_percent_centred() {
        let vp = quadraui::Rect::new(0.0, 0.0, 200.0, 100.0);
        let rect = folder_picker_popup_rect(vp, 1.0);
        assert!((rect.width - 120.0).abs() < 0.01, "{}", rect.width);
        assert!((rect.height - 55.0).abs() < 0.01, "{}", rect.height);
        assert!((rect.x - 40.0).abs() < 0.01, "{}", rect.x);
        assert!((rect.y - 22.5).abs() < 0.01, "{}", rect.y);
    }

    #[test]
    fn folder_picker_popup_rect_clamps_to_minimums() {
        // Tiny viewport: width/height clamp to the documented minimums
        // (50 units wide, 15 lines tall) rather than shrinking further.
        let vp = quadraui::Rect::new(0.0, 0.0, 40.0, 10.0);
        let rect = folder_picker_popup_rect(vp, 1.0);
        assert_eq!(rect.width, 50.0);
        assert_eq!(rect.height, 15.0);
    }

    #[test]
    fn folder_picker_click_outside_popup_dismisses() {
        let rect = quadraui::Rect::new(10.0, 10.0, 40.0, 20.0);
        assert_eq!(
            route_folder_picker_click(rect, 0.0, 0.0, 1.0, 0, 10),
            FolderPickerClickRoute::Dismiss
        );
        assert_eq!(
            route_folder_picker_click(rect, 100.0, 100.0, 1.0, 0, 10),
            FolderPickerClickRoute::Dismiss
        );
    }

    #[test]
    fn folder_picker_click_on_chrome_consumes_without_selecting() {
        let rect = quadraui::Rect::new(0.0, 0.0, 40.0, 20.0);
        // Row 0 is inside the title-row chrome (PALETTE_CHROME_ROWS == 4).
        assert_eq!(
            route_folder_picker_click(rect, 5.0, 0.0, 1.0, 0, 10),
            FolderPickerClickRoute::Consume
        );
    }

    #[test]
    fn folder_picker_click_on_a_row_selects_scroll_top_plus_offset() {
        let rect = quadraui::Rect::new(0.0, 0.0, 40.0, 20.0);
        let chrome = quadraui::PALETTE_CHROME_ROWS as f32;
        // Second visible row past the chrome, with a nonzero scroll offset.
        assert_eq!(
            route_folder_picker_click(rect, 5.0, chrome + 1.0, 1.0, 3, 10),
            FolderPickerClickRoute::SelectRow(4)
        );
    }

    #[test]
    fn folder_picker_click_past_last_row_consumes() {
        let rect = quadraui::Rect::new(0.0, 0.0, 40.0, 20.0);
        let chrome = quadraui::PALETTE_CHROME_ROWS as f32;
        // Only 2 filtered entries — clicking row 5 lands past the end.
        assert_eq!(
            route_folder_picker_click(rect, 5.0, chrome + 5.0, 1.0, 0, 2),
            FolderPickerClickRoute::Consume
        );
    }

    #[test]
    fn cmdline_selection_ctrl_c_copies_across_the_prefix_skew() {
        let mut engine = Engine::new_for_test();
        // Command mode: column 0 is the `:` prefix, so a selection of
        // columns 1..=3 is buffer chars 0..=2.
        engine.mode = crate::core::Mode::Command;
        engine.command_buffer = "abcdef".to_string();
        assert_eq!(
            route_cmdline_selection_key(&engine, Some('c'), true, Some((1, 3))),
            CmdSelKeyRoute::Copy("abc".to_string())
        );
        // Reversed drag order resolves identically.
        assert_eq!(
            route_cmdline_selection_key(&engine, Some('c'), true, Some((3, 1))),
            CmdSelKeyRoute::Copy("abc".to_string())
        );
        // Any other key clears.
        assert_eq!(
            route_cmdline_selection_key(&engine, Some('x'), false, Some((1, 3))),
            CmdSelKeyRoute::Clear
        );
        // Normal mode message line: no prefix offset.
        let mut engine = Engine::new_for_test();
        engine.message = "abcdef".to_string();
        assert_eq!(
            route_cmdline_selection_key(&engine, Some('C'), true, Some((0, 2))),
            CmdSelKeyRoute::Copy("abc".to_string())
        );
        // No selection outside the command line: nothing to do at all. This
        // is the arm GTK takes today (it has no command-line mouse drag), so
        // wiring the rung there is a guaranteed no-op.
        assert_eq!(
            route_cmdline_selection_key(&engine, Some('x'), false, None),
            CmdSelKeyRoute::Keep
        );
    }

    /// Hover-popup copy only claims y / Y / Ctrl+C, and only while the popup
    /// holds focus — otherwise `y` must reach the editor as a vim yank.
    #[test]
    fn hover_popup_copy_only_claims_its_chords_while_focused() {
        let mut engine = Engine::new_for_test();
        assert!(!engine.editor_hover_has_focus);
        assert_eq!(route_hover_popup_copy(&engine, "y", false), None);
        engine.editor_hover_has_focus = true;
        // Focused but with no popup content there is nothing to copy, so the
        // rung still declines rather than swallowing the key.
        assert_eq!(route_hover_popup_copy(&engine, "y", false), None);
        assert_eq!(route_hover_popup_copy(&engine, "j", false), None);
    }

    /// The epilogue applies what `Engine` owns and reports back only what the
    /// backend must do itself.
    #[test]
    fn post_key_epilogue_clamps_quickfix_and_drains_the_explorer_flag() {
        let mut engine = Engine::new_for_test();
        engine.explorer_needs_refresh = true;
        let mut scroll_top = 0usize;
        let out = post_key_epilogue(&mut engine, Some(&mut scroll_top));
        assert!(
            !engine.explorer_needs_refresh,
            "the epilogue must drain `explorer_needs_refresh` — GTK never did"
        );
        assert!(!out.focus_sidebar && !out.focus_activity_bar);
        assert_eq!(scroll_top, 0, "quickfix closed: scroll resets");

        // Selection below the six-row window scrolls it down.
        engine.quickfix.open = true;
        engine.quickfix.selected = 9;
        let mut scroll_top = 0usize;
        post_key_epilogue(&mut engine, Some(&mut scroll_top));
        assert_eq!(scroll_top, 5, "9 must be the last of five visible rows");
        // Selection above it scrolls back up.
        engine.quickfix.selected = 2;
        post_key_epilogue(&mut engine, Some(&mut scroll_top));
        assert_eq!(scroll_top, 2);

        // `None` is the GTK call: no scroll field, and no panic.
        post_key_epilogue(&mut engine, None);
    }

    /// The five routes [`dispatch_sidebar_panel_key`] declines are exactly
    /// the ones a backend must still handle itself.
    #[test]
    fn sidebar_panel_dispatch_declines_only_the_backend_owned_routes() {
        let mut engine = Engine::new_for_test();
        for route in [
            FocusKeyRoute::Debug,
            FocusKeyRoute::Ai,
            FocusKeyRoute::Explorer,
            FocusKeyRoute::ActivityBar,
            FocusKeyRoute::None,
        ] {
            assert_eq!(
                dispatch_sidebar_panel_key(
                    &mut engine,
                    route,
                    "j",
                    Some('j'),
                    Some('j'),
                    false,
                    false
                ),
                None,
                "{route:?} needs backend-owned state and must be handed back"
            );
        }
        // A panel route that this function *does* own is still claimed even
        // when the panel is not focused — the resolver above is what decides
        // *whether* to call this.
        assert!(dispatch_sidebar_panel_key(
            &mut engine,
            FocusKeyRoute::Settings,
            "j",
            Some('j'),
            Some('j'),
            false,
            false
        )
        .is_some());
    }

    // ── #901: menu defs → native MenuBar conversion ─────────────────────

    /// `build_menu_defs` must populate `key_equivalent` from any parseable
    /// shortcut, alongside the pre-existing `detail` display text — RED
    /// against the pre-#901 body (which only ever set `detail`, so this
    /// field was `None` for every item, and the native macOS menu installer
    /// had no accelerator to wire).
    #[test]
    fn build_menu_defs_populates_key_equivalent_from_shortcut() {
        let defs = build_menu_defs(false);
        let file = defs.iter().find(|d| d.id.as_str() == "File").unwrap();
        let save = file
            .items
            .iter()
            .find(|i| i.id.as_ref().map(|id| id.as_str()) == Some("w"))
            .expect("Save item present");
        let acc = save
            .key_equivalent
            .as_ref()
            .expect("Save's \"Ctrl+S\" shortcut must parse into key_equivalent");
        assert_eq!(acc.binding, quadraui::KeyBinding::Literal("Ctrl+S".into()));
        // `detail` (the drawn dropdown's display text) is untouched.
        assert!(save
            .detail
            .as_ref()
            .unwrap()
            .spans
            .iter()
            .any(|s| s.text == "Ctrl+S"));
    }

    /// Items with no shortcut (most of the menu) get no `key_equivalent` —
    /// this isn't a blanket accelerator grab, only real shortcuts convert.
    #[test]
    fn build_menu_defs_leaves_key_equivalent_none_without_a_shortcut() {
        let defs = build_menu_defs(false);
        let file = defs.iter().find(|d| d.id.as_str() == "File").unwrap();
        let open = file
            .items
            .iter()
            .find(|i| i.id.as_ref().map(|id| id.as_str()) == Some("open_file_dialog"))
            .expect("Open File item present");
        assert!(open.key_equivalent.is_none());
    }

    /// `menu_defs_to_menu_bar` preserves top-level order and reuses each
    /// `MenuDef`'s `ContextMenuItem` list verbatim as the `MenuBarItem`'s
    /// submenu — the native NSMenu installer and the drawn `MenuSystem`
    /// dropdown must never be able to disagree about what's in the menu.
    #[test]
    fn menu_defs_to_menu_bar_preserves_order_and_submenu_contents() {
        let defs = build_menu_defs(false);
        let bar = menu_defs_to_menu_bar(&defs);

        assert_eq!(bar.items.len(), defs.len());
        for (bar_item, def) in bar.items.iter().zip(defs.iter()) {
            assert_eq!(bar_item.id, def.id);
            assert_eq!(bar_item.label, def.label);
            assert_eq!(bar_item.disabled, def.disabled);
            assert_eq!(bar_item.submenu.as_deref(), Some(def.items.as_slice()));
        }
    }

    /// A separator in `MENU_STRUCTURE` round-trips into the `MenuBar`
    /// conversion as a genuine separator (`id: None`), not a blank action
    /// item — the native installer treats `id: None` as
    /// `NSMenuItem::separatorItem`, so getting this wrong would either drop
    /// the divider or install a dead clickable row in its place.
    #[test]
    fn menu_defs_to_menu_bar_keeps_separators_as_separators() {
        let defs = build_menu_defs(false);
        let bar = menu_defs_to_menu_bar(&defs);
        let file = bar.items.iter().find(|i| i.id.as_str() == "File").unwrap();
        let submenu = file.submenu.as_ref().unwrap();
        assert!(
            submenu.iter().any(|item| item.id.is_none()),
            "File menu should still contain at least one separator"
        );
    }

    // ── #1580: menu_style → quadraui::MenuStyle, native-vs-in-window ─────

    fn caps_with_native_menu(native_menu: bool) -> quadraui::BackendCaps {
        quadraui::BackendCaps {
            native_menu,
            ..Default::default()
        }
    }

    /// `to_quadraui_menu_style` is a straight 1:1 translation — no
    /// capability logic lives here any more (that's
    /// `quadraui::MenuStyle::resolve`, exercised below against a real
    /// `BackendCaps`).
    #[test]
    fn to_quadraui_menu_style_maps_every_variant() {
        use crate::core::settings::MenuStyle;

        assert_eq!(
            to_quadraui_menu_style(MenuStyle::Auto),
            quadraui::MenuStyle::Auto
        );
        assert_eq!(
            to_quadraui_menu_style(MenuStyle::Native),
            quadraui::MenuStyle::Native
        );
        assert_eq!(
            to_quadraui_menu_style(MenuStyle::Custom),
            quadraui::MenuStyle::Custom
        );
    }

    /// `Auto` and `Native` both defer to the backend's own capability via
    /// `quadraui::MenuStyle::resolve` — they never force a native popup
    /// onto a backend that can't paint one (GTK, TUI), matching #901's
    /// identical gate for the menu bar. This is quadraui's own contract
    /// (covered by its own unit tests), pinned here so a future
    /// `to_quadraui_menu_style` regression that mis-maps a variant would
    /// still be caught end-to-end.
    #[test]
    fn menu_style_resolves_native_only_with_capability() {
        use crate::core::settings::MenuStyle;

        for style in [MenuStyle::Auto, MenuStyle::Native] {
            assert_eq!(
                to_quadraui_menu_style(style).resolve(&caps_with_native_menu(true)),
                quadraui::ResolvedMenuStyle::Native,
                "{style:?} must resolve to native when the backend has one"
            );
            assert_eq!(
                to_quadraui_menu_style(style).resolve(&caps_with_native_menu(false)),
                quadraui::ResolvedMenuStyle::Custom,
                "{style:?} must fall back to in-window when the backend has \
                 no native context menu"
            );
        }
    }

    /// `Custom` always paints in-window, even on a backend that could show
    /// a native popup — the escape hatch VS Code's `window.menuStyle:
    /// custom` provides.
    #[test]
    fn menu_style_custom_never_resolves_native() {
        use crate::core::settings::MenuStyle;

        assert_eq!(
            to_quadraui_menu_style(MenuStyle::Custom).resolve(&caps_with_native_menu(true)),
            quadraui::ResolvedMenuStyle::Custom
        );
        assert_eq!(
            to_quadraui_menu_style(MenuStyle::Custom).resolve(&caps_with_native_menu(false)),
            quadraui::ResolvedMenuStyle::Custom
        );
    }

    /// `paint_context_menu_rung` always paints in-window now (#1580) — the
    /// native path moved to `show_context_menu_now`, called at
    /// event-handler time instead of from this render rung. This is the
    /// RED-able regression pin for that split: it proves the rung this
    /// file owns still calls `draw_context_menu` and returns the painted
    /// layout.
    #[test]
    fn paint_context_menu_rung_paints_in_window() {
        let panel = ContextMenuPanel {
            items: vec![ContextMenuRenderItem {
                label: "Copy".to_string(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            }],
            selected_idx: 0,
            screen_col: 3,
            screen_row: 2,
            trigger_height: 0.0,
        };
        let viewport = quadraui::Rect::new(0.0, 0.0, 800.0, 600.0);

        let mut backend = quadraui::testing::RecordingBackend::new();
        let _layout = paint_context_menu_rung(&mut backend, &panel, viewport, 8.0, 16.0, 0.0);
        assert!(
            backend.calls.contains(&"draw_context_menu"),
            "paint_context_menu_rung must call draw_context_menu; calls were {:?}",
            backend.calls
        );
    }

    // ─── 'list' glyph substitution (#1190, 'listchars' #1206) ──────────────

    /// A `Settings` with `listchars` overridden and `tabstop` at its default
    /// (8) — the shape every `apply_list_glyphs`/`compute_list_glyph_expansion`
    /// test below needs, since #1206 made both option-driven.
    fn list_glyphs_settings(listchars: &str) -> Settings {
        let mut settings = Settings::default();
        settings.listchars = listchars.to_string();
        settings
    }

    #[test]
    fn apply_list_glyphs_marks_eol_with_no_tabs() {
        let settings = list_glyphs_settings("eol:$");
        let (text, spans, diags, spells) = apply_list_glyphs(
            "hello".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            true,
            &settings,
        );
        assert_eq!(text, "hello$");
        assert!(spans.is_empty());
        assert!(diags.is_empty());
        assert!(spells.is_empty());
    }

    #[test]
    fn apply_list_glyphs_marks_eol_before_trailing_newline() {
        let settings = list_glyphs_settings("eol:$");
        let (text, ..) = apply_list_glyphs(
            "hello\n".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            true,
            &settings,
        );
        assert_eq!(
            text, "hello$\n",
            "the $ must land before the newline, not after it"
        );
    }

    #[test]
    fn apply_list_glyphs_skips_eol_for_non_final_wrap_segment() {
        let settings = list_glyphs_settings("eol:$");
        let (text, ..) = apply_list_glyphs(
            "hello".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            &settings,
        );
        assert_eq!(text, "hello", "mark_eol=false must not append $");
    }

    /// RED against unfixed `develop` (#1206): before this change,
    /// `apply_list_glyphs` unconditionally appended `$` regardless of
    /// `'listchars'` — this asserts the *opposite* (default `'listchars'`
    /// has no `eol` item, so nothing is appended), which fails against the
    /// pre-#1206 hardcoded-`$` implementation.
    #[test]
    fn apply_list_glyphs_default_listchars_has_no_eol_marker() {
        let settings = Settings::default();
        let (text, ..) = apply_list_glyphs(
            "hello".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            true,
            &settings,
        );
        assert_eq!(
            text, "hello",
            "Neovim's real default 'listchars' has no eol item"
        );
    }

    #[test]
    fn apply_list_glyphs_expands_tab_to_caret_i_when_no_tab_item() {
        let settings = list_glyphs_settings("");
        let (text, ..) = apply_list_glyphs(
            "a\tb".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            &settings,
        );
        assert_eq!(text, "a^Ib");
    }

    /// RED against unfixed `develop` (#1206): before this change, `'list'`
    /// always rendered a tab as literal `^I`. Neovim's real default
    /// `'listchars'` (`"tab:> ,trail:-,nbsp:+"`) instead fills to the next
    /// `'tabstop'` stop with `>` then spaces — this fails against the
    /// pre-#1206 hardcoded-`^I` implementation.
    #[test]
    fn apply_list_glyphs_default_listchars_renders_tab_as_arrow_fill() {
        let settings = Settings::default();
        // vimcode's default 'tabstop' is 4 (not Vim's classic 8 — see
        // `default_tabstop`); a tab right after "a" (column 1) fills
        // columns 1..4 — '>' then 2 more spaces (3 cells total).
        let (text, ..) = apply_list_glyphs(
            "a\tb".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            &settings,
        );
        assert_eq!(text, "a>  b");
    }

    /// RED against unfixed `develop` (#1206): 'trail' had no implementation
    /// at all before this change (trailing spaces just rendered as spaces).
    #[test]
    fn apply_list_glyphs_trailing_spaces_use_trail_glyph() {
        let settings = list_glyphs_settings("trail:-");
        let (text, ..) = apply_list_glyphs(
            "ab  ".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            &settings,
        );
        assert_eq!(text, "ab--");
    }

    /// A leading/mid-line space must NOT be treated as trailing.
    #[test]
    fn apply_list_glyphs_trail_glyph_does_not_touch_non_trailing_spaces() {
        let settings = list_glyphs_settings("trail:-");
        let (text, ..) = apply_list_glyphs(
            "a  b  ".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            &settings,
        );
        assert_eq!(text, "a  b--");
    }

    /// RED against unfixed `develop` (#1206): 'nbsp' had no implementation
    /// at all before this change.
    #[test]
    fn apply_list_glyphs_nbsp_uses_nbsp_glyph() {
        let settings = list_glyphs_settings("nbsp:+");
        let (text, ..) = apply_list_glyphs(
            "a\u{a0}b".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            &settings,
        );
        assert_eq!(text, "a+b");
    }

    fn plain_style() -> Style {
        Style {
            fg: quadraui::Color::rgb(0, 0, 0),
            bg: None,
            bold: false,
            italic: false,
            font_scale: 1.0,
        }
    }

    #[test]
    fn apply_list_glyphs_remaps_span_offsets_past_a_tab() {
        // "a\tbc" — a span covering "bc" (source bytes 2..4) must land on
        // "^Ibc"'s "bc" (bytes 3..5) once the tab becomes the 2-byte `^I`.
        let settings = list_glyphs_settings("");
        let spans = vec![StyledSpan {
            start_byte: 2,
            end_byte: 4,
            style: plain_style(),
        }];
        let (text, spans, ..) = apply_list_glyphs(
            "a\tbc".to_string(),
            spans,
            Vec::new(),
            Vec::new(),
            false,
            &settings,
        );
        assert_eq!(text, "a^Ibc");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start_byte, 3);
        assert_eq!(spans[0].end_byte, 5);
        assert_eq!(&text[spans[0].start_byte..spans[0].end_byte], "bc");
    }

    #[test]
    fn apply_list_glyphs_remaps_spans_across_two_tabs() {
        // "a\tb\tc" — a span on the trailing "c" (source byte 4..5) must
        // shift by +2 (one extra byte from each of the two tabs).
        let settings = list_glyphs_settings("");
        let spans = vec![StyledSpan {
            start_byte: 4,
            end_byte: 5,
            style: plain_style(),
        }];
        let (text, spans, ..) = apply_list_glyphs(
            "a\tb\tc".to_string(),
            spans,
            Vec::new(),
            Vec::new(),
            false,
            &settings,
        );
        assert_eq!(text, "a^Ib^Ic");
        assert_eq!(&text[spans[0].start_byte..spans[0].end_byte], "c");
    }

    #[test]
    fn apply_list_glyphs_combines_tab_and_eol() {
        let settings = list_glyphs_settings("eol:$");
        let (text, ..) = apply_list_glyphs(
            "a\tb".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            true,
            &settings,
        );
        assert_eq!(text, "a^Ib$");
    }

    #[test]
    fn apply_list_glyphs_remaps_diagnostic_and_spell_marks_past_a_tab() {
        // "\tfoo" — a diagnostic/spell mark on "foo" (char cols 1..4) must
        // land on "^Ifoo"'s "foo" (char cols 2..5) once the tab expands to
        // the 2-char `^I` (#1208 bug 1: these are char-index based, unlike
        // `spans`, so they need their own remap through the same table).
        let settings = list_glyphs_settings("");
        let diags = vec![DiagnosticMark {
            start_col: 1,
            end_col: 4,
            severity: crate::core::lsp::DiagnosticSeverity::Error,
            message: "oops".to_string(),
        }];
        let spells = vec![SpellMark {
            start_col: 1,
            end_col: 4,
        }];
        let (text, _, diags, spells) = apply_list_glyphs(
            "\tfoo".to_string(),
            Vec::new(),
            diags,
            spells,
            false,
            &settings,
        );
        assert_eq!(text, "^Ifoo");
        assert_eq!(diags[0].start_col, 2);
        assert_eq!(diags[0].end_col, 5);
        assert_eq!(spells[0].start_col, 2);
        assert_eq!(spells[0].end_col, 5);
        let chars: Vec<char> = text.chars().collect();
        let marked: String = chars[diags[0].start_col..diags[0].end_col].iter().collect();
        assert_eq!(
            marked, "foo",
            "diagnostic mark must land on 'foo', not shifted left"
        );
    }

    // ── #146: plugin-declared view → `quadraui::Form` ───────────────────

    fn fixture_view() -> crate::core::plugin_ui::PluginView {
        use crate::core::plugin_ui::{PluginView, ViewField, ViewFieldKind, VIEW_SCHEMA_VERSION};
        let f = |id: &str, label: &str, kind: ViewFieldKind| ViewField {
            id: id.to_string(),
            label: label.to_string(),
            hint: String::new(),
            disabled: false,
            error: None,
            warning: None,
            kind,
        };
        PluginView {
            id: "main".to_string(),
            schema_version: VIEW_SCHEMA_VERSION,
            body: None,
            fields: vec![
                f("hdr", "Header", ViewFieldKind::Label),
                f(
                    "url",
                    "URL",
                    ViewFieldKind::Text {
                        value: "https://x".to_string(),
                        placeholder: String::new(),
                    },
                ),
                f("send", "Send", ViewFieldKind::Button),
            ],
        }
    }

    #[test]
    fn plugin_view_form_namespaces_every_widget_id() {
        let form = plugin_view_to_form("my-ext", &fixture_view(), 1, 0, true, None);
        let ids: Vec<String> = form
            .fields
            .iter()
            .map(|f| f.id.as_str().to_string())
            .collect();
        assert_eq!(
            ids,
            vec![
                "plugin:my-ext:hdr".to_string(),
                "plugin:my-ext:url".to_string(),
                "plugin:my-ext:send".to_string(),
            ]
        );
        // Interaction state is vimcode-owned, taken from the arguments rather
        // than from anything the plugin declared.
        assert_eq!(
            form.focused_field.as_ref().map(|w| w.as_str()),
            Some("plugin:my-ext:url")
        );
        assert!(form.has_focus);
    }

    #[test]
    fn plugin_view_form_never_hands_a_plugin_the_text_cursor() {
        // `cursor`/`selection_anchor` are byte offsets into the value; letting a
        // plugin set them is how you get a caret mid-codepoint. #146's ABI
        // decision keeps them out of the vocabulary entirely — assert the
        // adapter does not invent one either.
        let form = plugin_view_to_form("my-ext", &fixture_view(), 0, 0, false, None);
        match &form.fields[1].kind {
            quadraui::FieldKind::TextInput {
                cursor,
                selection_anchor,
                ..
            } => {
                assert!(cursor.is_none());
                assert!(selection_anchor.is_none());
            }
            other => panic!("expected a TextInput field, got {other:?}"),
        }
    }

    #[test]
    fn a_settings_form_event_is_not_mistaken_for_a_plugin_one() {
        // The Settings panel paints through the *same* `FormController` shape,
        // with ids like `cat-0` / `core-tabstop`. If the namespace check were
        // dropped, a Settings click would be dispatched into a plugin.
        assert!(
            plugin_view_event_from_form_event(&quadraui::FormEvent::ButtonClicked {
                id: quadraui::WidgetId::new("save"),
            })
            .is_none()
        );
        let plugin = plugin_view_event_from_form_event(&quadraui::FormEvent::ButtonClicked {
            id: quadraui::WidgetId::new("plugin:my-ext:send"),
        })
        .expect("a namespaced id must resolve");
        assert_eq!(plugin.0, "my-ext");
        assert_eq!(plugin.1, "send");
        assert_eq!(
            plugin.2,
            crate::core::plugin_ui::ViewEventKind::ButtonClicked
        );
    }

    #[test]
    fn a_toggle_group_entry_keeps_its_own_id_through_the_round_trip() {
        use crate::core::plugin_ui::{
            PluginView, ViewEventKind, ViewField, ViewFieldKind, ViewToggle, VIEW_SCHEMA_VERSION,
        };
        let view = PluginView {
            id: "f".to_string(),
            schema_version: VIEW_SCHEMA_VERSION,
            body: None,
            fields: vec![ViewField {
                id: "flags".to_string(),
                label: "Flags".to_string(),
                hint: String::new(),
                disabled: false,
                error: None,
                warning: None,
                kind: ViewFieldKind::Toggles {
                    toggles: vec![ViewToggle {
                        id: "case".to_string(),
                        label: "Aa".to_string(),
                        value: false,
                    }],
                },
            }],
        };
        let form = plugin_view_to_form("my-ext", &view, 0, 0, false, None);
        let inner = match &form.fields[0].kind {
            quadraui::FieldKind::ToggleGroup { toggles } => toggles[0].id.clone(),
            other => panic!("expected a ToggleGroup, got {other:?}"),
        };
        assert_eq!(inner.as_str(), "plugin:my-ext:case");
        let resolved = plugin_view_event_from_form_event(&quadraui::FormEvent::ToggleChanged {
            id: inner,
            value: true,
        })
        .expect("the sub-widget id must resolve back to the plugin");
        assert_eq!(resolved.1, "case");
        assert_eq!(resolved.2, ViewEventKind::ToggleChanged { value: true });
    }
}
