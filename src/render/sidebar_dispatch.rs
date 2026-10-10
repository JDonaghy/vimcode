use super::*;

// ─── Sidebar panel body dispatch (#754) ───────────────────────────────────────
//
// The rung the issue names by line count: "who owns the sidebar body"
// (`sidebar_owner`, above) was already shared, but what each owner's press
// actually *did* — translate the event, feed the same widget/engine call the
// painter's own geometry lines up with — was re-derived independently on
// each backend. The five functions below are that dispatch, stated once.
// Callers keep their own gating (sidebar bounds, picker/context-menu
// precedence, drag-capture bookkeeping) — that's real per-backend plumbing,
// not duplicated business logic — and their own geometry *derivation*
// (GTK caches it from paint time; TUI recomputes it from cheap closed-form
// row math), because a pixel bounds and a cell bounds are answers to two
// different questions. What's shared is what happens once that geometry and
// the event are in hand.

/// Route an explorer-sidebar event through the shared `TreeController`
/// widget and resolve the result against the engine.
///
/// `metrics` is `(line_height, char_width)` in the caller's native unit —
/// GTK re-applies the pixel metrics the tree was painted with before
/// hit-testing (its `Backend::set_current_line_height`/`set_current_char_width`
/// are inherent, not trait methods, so the caller must set them before this
/// call); TUI's cell grid needs no such re-application and passes `(1.0,
/// 1.0)`. Both values are also used to convert a `ContextMenuRequested`'s
/// pixel/cell `position` back into the `(col, row)` `open_explorer_context_menu`
/// wants.
///
/// A `ContextMenuRequested` result is fully resolved here (opening the same
/// `engine.open_explorer_context_menu` both backends used to call
/// independently) and reported back as `None`, since it is already consumed.
/// Any other event is returned to the caller for its own DoubleClick /
/// MouseDown dispatch and focus bookkeeping, which differ enough between a
/// GTK press and a TUI scrollbar-drag lifecycle that folding them in here
/// would just move the duplication rather than remove it.
pub fn route_explorer_tree_event(
    engine: &mut Engine,
    event: &quadraui::UiEvent,
    rect: quadraui::Rect,
    metrics: (f64, f64),
    theme: &Theme,
    backend: &mut dyn quadraui::Backend,
) -> Option<quadraui::TreeControllerEvent> {
    if rect.width <= 0.0 {
        return None;
    }
    populate_explorer_tree_controller(engine, theme);
    let (lh, cw) = metrics;
    let tree_event = engine
        .explorer_tree
        .borrow_mut()
        .handle(event, backend, rect);

    if let quadraui::TreeControllerEvent::ContextMenuRequested { path, position } = &tree_event {
        if let Some(&row_idx) = path.first() {
            let idx = row_idx as usize;
            let target_info = engine
                .explorer_rows
                .get(idx)
                .map(|row| (row.path.clone(), row.is_dir));
            if let Some((target, is_dir)) = target_info {
                let cx = (position.x / (cw.max(1.0) as f32)) as u16;
                let cy = (position.y / (lh.max(1.0) as f32)) as u16;
                engine.open_explorer_context_menu(target, is_dir, cx, cy);
            }
        }
        return None;
    }
    Some(tree_event)
}

// ─── Explorer drag-and-drop (#1429) ────────────────────────────────────────
//
// TUI has carried this since before `App` existed (`explorer_drag_src`/
// `explorer_drag_active` in `tui_main/shell_app.rs`, applied by hand-rolled
// row arithmetic in `tui_main/mouse.rs`); GTK/`App` never got a twin, which
// would have silently dropped the feature the day TUI cuts over onto `App`
// (the epic this issue is part of). The three functions below are that
// twin, shared: both backends keep their own two `Option` fields (they are
// plain row indices, not GTK/TUI structures) and call these to update them.

/// Row index under `(x, y)` inside the explorer tree's own painted `rect`,
/// in the caller's native unit (`row_height` is `1.0` cell on TUI, the
/// measured line height in pixels on GTK — the same first element of the
/// `metrics` tuple [`route_explorer_tree_event`]'s callers already re-apply
/// before dispatch). `None` when the point is outside `rect`'s columns, above
/// its first row, or past the last populated row — "no row here", used by
/// both the drag-and-drop rung below and the empty-space right-click
/// fallback ([`route_tree_empty_space_context_menu`]) to tell "over a real
/// row" (owned by [`route_explorer_tree_event`]) from "over empty tree
/// space".
pub fn explorer_row_at(
    engine: &Engine,
    rect: quadraui::Rect,
    row_height: f64,
    x: f64,
    y: f64,
) -> Option<usize> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    if x < rect.x as f64 || x >= (rect.x + rect.width) as f64 {
        return None;
    }
    let rel_y = y - rect.y as f64;
    if rel_y < 0.0 {
        return None;
    }
    let row_height = row_height.max(1.0);
    let row_in_view = (rel_y / row_height) as usize;
    let idx = row_in_view + engine.explorer_tree.borrow().scroll_offset();
    if idx < engine.explorer_rows.len() {
        Some(idx)
    } else {
        None
    }
}

/// Apply one [`MouseDragRoute::ExplorerDnd`] move: promote a pending
/// `explorer_drag_src` into an active `(src, target)` pair once the pointer
/// has moved off the source row, slide the target row while it stays over
/// the tree, or clear the target (keeping the drag itself armed) once the
/// pointer strays outside `rect` — mirrors what `tui_main::mouse`'s
/// hand-rolled version used to do inline, now shared so `App` gets the same
/// behaviour without re-deriving it.
pub fn apply_explorer_drag_move(
    engine: &Engine,
    rect: quadraui::Rect,
    row_height: f64,
    x: f64,
    y: f64,
    explorer_drag_src: &mut Option<usize>,
    explorer_drag_active: &mut Option<(usize, Option<usize>)>,
) {
    match explorer_row_at(engine, rect, row_height, x, y) {
        Some(idx) => {
            if let Some(src_row) = *explorer_drag_src {
                // Only activate the drag once the target differs from the
                // source — a press-then-tiny-jitter must not fire a move.
                if idx != src_row {
                    *explorer_drag_active = Some((src_row, Some(idx)));
                    *explorer_drag_src = None;
                }
            } else if let Some((src, _)) = explorer_drag_active {
                *explorer_drag_active = Some((*src, Some(idx)));
            }
        }
        None => {
            if let Some((src, _)) = explorer_drag_active {
                // Dragged outside the tree (or over empty space below the
                // last row) — clear the target but keep the drag active.
                *explorer_drag_active = Some((*src, None));
            }
        }
    }
}

/// Apply an explorer drag-and-drop release: move `src_row` into
/// `target_row`'s directory (or `target_row`'s own parent, if it is a file)
/// via [`Engine::confirm_move_file`]. A no-op if either index is out of
/// range (the tree can shrink mid-drag, e.g. a watcher-driven refresh) or
/// `target_row` is `None` (dropped outside the tree).
pub fn apply_explorer_drop(engine: &mut Engine, src_row: usize, target_row: Option<usize>) {
    let Some(target_row) = target_row else {
        return;
    };
    if src_row >= engine.explorer_rows.len() || target_row >= engine.explorer_rows.len() {
        return;
    }
    let src_path = engine.explorer_rows[src_row].path.clone();
    let target = &engine.explorer_rows[target_row];
    let dest_dir = if target.is_dir {
        target.path.clone()
    } else {
        target
            .path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf()
    };
    engine.confirm_move_file(&src_path, &dest_dir);
}

/// Right-click fallback for empty tree space below the explorer's last row
/// (#1429). Temporary shared workaround for quadraui#1045 item 4 (upstream:
/// `TreeController::right_click` resolving a `TreeViewHit::Empty` to a
/// container-level `ContextMenuRequested` instead of plain `Consumed`) —
/// delete this function and its two call sites (`tui_main::mouse`,
/// `App::explorer_ui_event`) once that lands, per this repo's
/// Platform-Neutrality Rule (shared code now, not per-backend, but still a
/// stopgap for a gap that belongs upstream).
///
/// #1025 intentionally dropped the pre-existing "right-click below the last
/// row opens the root folder's context menu" fallback when it moved the
/// row/chevron case onto `route_explorer_tree_event` (which only resolves
/// `Row`/`Chevron` hits — an `Empty` hit reaches here instead, on both
/// backends alike, restoring the fallback without re-diverging them).
///
/// `rect`/`metrics`/`pos` share `route_explorer_tree_event`'s own contract:
/// `rect` is the tree's last painted rect, `metrics` is `(line_height,
/// char_width)` in the caller's native unit, `pos` is in that same space.
/// Returns `true` if the fallback fired (opened the cwd/root context menu);
/// `false` for a hit that either missed `rect` entirely or landed on a real
/// row (`route_explorer_tree_event` already owns that case).
pub fn route_tree_empty_space_context_menu(
    engine: &mut Engine,
    rect: quadraui::Rect,
    metrics: (f64, f64),
    pos: quadraui::Point,
) -> bool {
    let (lh, cw) = metrics;
    if explorer_row_at(engine, rect, lh, pos.x as f64, pos.y as f64).is_some() {
        return false;
    }
    let inside = pos.x >= rect.x
        && pos.x < rect.x + rect.width
        && pos.y >= rect.y
        && pos.y < rect.y + rect.height;
    if !inside {
        return false;
    }
    let cx = (pos.x / (cw.max(1.0) as f32)) as u16;
    let cy = (pos.y / (lh.max(1.0) as f32)) as u16;
    let root = engine.cwd.clone();
    engine.open_explorer_context_menu(root, true, cx, cy);
    true
}

/// Dispatch a mouse/scroll event to the debug ("run and debug") sidebar's
/// *body* — everything below its title + action-button chrome — through the
/// shared `SidebarSystem` widget. The chrome band itself is
/// [`dap_sidebar_action_click_at`], a separate function because it needs
/// only a local point, not a whole event.
pub fn dispatch_dap_sidebar_body_event(
    engine: &mut Engine,
    event: &quadraui::UiEvent,
    rect: quadraui::Rect,
    backend: &mut dyn quadraui::Backend,
) {
    populate_dap_sidebar_system(engine);
    let sidebar_event = engine
        .dap_sidebar_system
        .borrow_mut()
        .handle(event, backend, rect);
    engine.dispatch_dap_sidebar_event(sidebar_event);
}

/// Resolve a press against the debug sidebar's title + action-button chrome
/// row, given `pos` in the same absolute space `SidebarPanelBody::
/// render_with` painted the chrome into — i.e. whatever space the `rect`
/// passed to that call was in (TUI cell coordinates, GTK pixels). Both
/// backends now store that same space on `Engine::dap_sidebar_action_hits`
/// (populated straight from `SidebarPanelBodyLayout::status_bar_hit_regions`
/// — see that field's doc), so there is no per-backend translation step left
/// here (issue #1392; GTK used to subtract its own separately-cached
/// `action_rect`'s origin, which is exactly the "paint and click can
/// disagree" risk the issue called out). Returns whether a segment was
/// actually hit — the caller claims the whole chrome row regardless,
/// matching both backends' pre-#754 behaviour.
pub fn dap_sidebar_action_click_at(engine: &mut Engine, pos: quadraui::Point) -> bool {
    let matched = {
        let hits = engine.dap_sidebar_action_hits.borrow();
        hits.iter().any(|(rect, hit)| {
            rect.contains(pos) && matches!(hit, quadraui::StatusBarHit::Segment(_))
        })
    };
    if matched {
        engine.handle_dap_sidebar_action_click();
    }
    matched
}

/// Which band of the git sidebar a [`route_sc_sidebar_click`] press landed
/// on. Both variants mean "consumed" — the split only exists because TUI's
/// double-click synthesis (crossterm has no native double-click) must fire
/// only for a genuine content-row click, matching what GTK's toolkit-native
/// `DoubleClick` event would have hit had it landed in the same place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScSidebarClickOutcome {
    /// Header, commit-input box, or a toolbar button — chrome consumed the
    /// press directly.
    Chrome,
    /// Fell through to `handle_sc_sidebar_ui_event` — a section/content-row
    /// click (or a non-`starts_interaction` follow-through, e.g. a drag).
    Content,
}

/// Route a press/scroll to the git ("source control") sidebar, given the
/// [`ScSidebarBands`] the shared painter laid the panel out into and the
/// event's position in that *same* absolute space. Mirrors the GTK-only
/// `route_sc_sidebar_event` this replaced: header row clears the commit
/// input, the commit-input band activates it, a toolbar-button hit routes
/// through `sc_button_hit`/`sc_activate_button`, and anything else in the
/// slab (or a non-`starts_interaction` follow-through, e.g. a drag) falls
/// through to `handle_sc_sidebar_ui_event`.
pub fn route_sc_sidebar_click(
    engine: &mut Engine,
    event: &quadraui::UiEvent,
    pos: quadraui::Point,
    bands: &ScSidebarBands,
    starts_interaction: bool,
) -> ScSidebarClickOutcome {
    if starts_interaction {
        engine.sc_set_focus(true);
        let commit_bottom = bands.commit_input.y + bands.commit_input.height;
        if pos.y < bands.header.y + bands.header.height {
            engine.sc_commit_input_active = false;
            return ScSidebarClickOutcome::Chrome;
        }
        if pos.y < commit_bottom {
            engine.sc_commit_input_active = true;
            engine.sc_commit_cursor = engine.sc_commit_message.len();
            return ScSidebarClickOutcome::Chrome;
        }
        engine.sc_commit_input_active = false;
        let hit = {
            let layout = engine.sc_panel_layout.borrow();
            layout.as_ref().map(|l| l.hit_test(pos.x, pos.y))
        };
        if let Some(quadraui::SidebarPanelHit::ToolbarButton(_)) = hit {
            if let Some(idx) = engine.sc_button_hit(pos.x, pos.y) {
                engine.sc_activate_button(idx);
            }
            return ScSidebarClickOutcome::Chrome;
        }
    }
    engine.handle_sc_sidebar_ui_event(event.clone());
    ScSidebarClickOutcome::Content
}

/// Dispatch a mouse/scroll/keyboard [`quadraui::UiEvent`] to the AI assistant
/// sidebar's `ChatController` (#819 — the adoption that replaced the
/// hand-painted `draw_ai_sidebar_panel`/`route_ai_sidebar_click` pair this
/// function and [`populate_ai_chat_controller`] took over from). Shared by
/// GTK and TUI, mirroring [`route_explorer_tree_event`] and
/// [`dispatch_dap_sidebar_body_event`]: `rect` must be the same rect the
/// caller's last `ai_chat.borrow().render(backend, rect)` used (both
/// backends cache it in `Engine::ai_chat_rect`), so `ChatController::handle`
/// re-derives the identical layout `render()` painted rather than risking
/// the #544/#582/#646 drift a second hand-rolled geometry pass invites.
///
/// Returns whether the panel should keep keyboard focus — see
/// [`Engine::dispatch_ai_chat_event`], which this delegates the semantic
/// event to after `ChatController::handle` resolves it.
///
/// On GTK, `backend`'s "current" `line_height`/`char_width` — what
/// [`quadraui::Backend::line_height`]/`char_width` actually read — are
/// mutable and can be left at whatever some *other* widget's render pass
/// last set them to by the time an event reaches here. Callers must
/// re-apply the metrics `render()` painted this panel with (GTK's `App`
/// caches them in `cached_ai_chat_metrics` for exactly this) *before*
/// calling this function, the same way [`route_explorer_tree_event`]'s
/// callers re-apply `cached_explorer_metrics`. Skipping this doesn't panic
/// or misroute the event — it silently feeds `ChatController::handle` a
/// different `total_rows`/`visible_rows` than `render()` computed, which
/// desyncs the scrollbar clamp from the very first scroll notch (caught by
/// `ai_panel_scrolls_transcript`, #819 review).
pub fn route_ai_chat_event(
    engine: &mut Engine,
    event: &quadraui::UiEvent,
    rect: quadraui::Rect,
    theme: &Theme,
    backend: &mut dyn quadraui::Backend,
) -> bool {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return engine.ai_has_focus;
    }
    populate_ai_chat_controller(engine, theme, backend);

    // #1511: plain `Enter` while a transcript turn (not the input) has
    // keyboard focus is the "keyboard toggle without a mouse" the issue
    // asks for — `quadraui::ChatController::handle` already implements
    // exactly this gesture (see its module doc's Enter arm), but it acts on
    // its own *internal* `turn_collapsed` map, which `populate_ai_chat_
    // controller` overwrites from `AcpSession::thought_expanded`/
    // `tool_call_expanded` every frame (that function's doc explains why
    // those two, not `ChatController`'s own map, are the source of truth).
    // Left alone, an internal-only toggle would be invisible: the very next
    // frame's populate call stomps it straight back to whatever the
    // vimcode-side maps say. Intercepted here, ahead of `ChatController::
    // handle`, with the identical guard `ChatController` itself uses, so
    // `Engine::ai_chat_toggle_turn` flips the persistent state instead.
    if let quadraui::UiEvent::KeyPressed { key, modifiers, .. } = event {
        if matches!(key, quadraui::Key::Named(quadraui::NamedKey::Enter)) && !modifiers.ctrl {
            let focused = {
                let chat = engine.ai_chat.borrow();
                (!chat.input_has_focus())
                    .then(|| chat.focused_turn())
                    .flatten()
            };
            if let Some(turn_idx) = focused {
                engine.ai_chat_toggle_turn(turn_idx);
                return engine.ai_has_focus;
            }
        }
    }

    // #1507: the `<leader>ai` focus-toggle gesture (see
    // `Engine::ai_leader_toggle_key`'s doc for why this has to be
    // intercepted here, ahead of the ordinary `ChatController::handle`
    // call, rather than reusing `Engine::handle_leader_key`). Checked before
    // the completion-popup intercepts below: `ai_leader_toggle_key` only
    // ever engages while the input is empty, which is also the one state a
    // completion popup can never be open in, so the two can't race.
    if let quadraui::UiEvent::KeyPressed { key, modifiers, .. } = event {
        let no_modifiers = !modifiers.shift && !modifiers.ctrl && !modifiers.alt && !modifiers.cmd;
        match key {
            quadraui::Key::Char(ch) if no_modifiers => {
                if engine.ai_leader_toggle_key(*ch) {
                    return engine.ai_has_focus;
                }
                // A mismatched plain char already had any pending prefix
                // replayed by `ai_leader_toggle_key` itself — nothing more
                // to do here.
            }
            // Every other key shape (a modified `Char` — e.g. Shift held for
            // a capital letter — or any `Named` key: Enter, Tab, Backspace,
            // arrows, Escape, …) can never be part of the `<leader>ai`
            // gesture and so never reaches `ai_leader_toggle_key` at all. If
            // a partial match is still buffered when one of these arrives,
            // it must be resolved right here or it dangles silently (#1507
            // review) — replayed into the input for an ordinary interrupting
            // key, or discarded for `Escape`, which is about to end the
            // session via `Cancelled` below (see `dispatch_ai_chat_event`'s
            // `Cancelled` arm for why that one path discards instead of
            // replaying).
            _ if !engine.ai_leader_toggle_pending.is_empty() => {
                if matches!(key, quadraui::Key::Named(quadraui::NamedKey::Escape)) {
                    engine.ai_leader_toggle_pending.clear();
                } else {
                    engine.ai_leader_toggle_flush();
                }
            }
            _ => {}
        }
    }

    // #956 (ACP-5): while the slash-command completion popup is showing,
    // steal Tab (cycle selection) and Enter (accept) before handing the
    // event to `ChatController::handle` — the same "intercept the
    // accept/cycle keys, let everything else fall through unchanged" shape
    // the editor's own word-completion popup uses
    // (`Engine::insert_completion_intercepts_key`). This one shared call
    // site is what makes it zero-backend-specific: GTK and TUI both route
    // every AI-panel key through here already.
    if let quadraui::UiEvent::KeyPressed { key, modifiers, .. } = event {
        let no_modifiers = !modifiers.shift && !modifiers.ctrl && !modifiers.alt && !modifiers.cmd;
        if no_modifiers && engine.ai_command_completions().is_some() {
            match key {
                quadraui::Key::Named(quadraui::NamedKey::Tab) => {
                    engine.ai_command_completion_cycle();
                    return engine.ai_has_focus;
                }
                quadraui::Key::Named(quadraui::NamedKey::Enter) => {
                    engine.ai_command_accept_selected();
                    return engine.ai_has_focus;
                }
                _ => {}
            }
        // #1449: same intercept, for the `@`-mention popup — mutually
        // exclusive with the slash-command one above (a slash command only
        // ever occupies the *whole* input, a mention only the trailing
        // word, so `ai_command_completions` being `Some` already claims
        // this key).
        } else if no_modifiers && engine.ai_mention_completions().is_some() {
            match key {
                quadraui::Key::Named(quadraui::NamedKey::Tab) => {
                    engine.ai_mention_completion_cycle();
                    return engine.ai_has_focus;
                }
                quadraui::Key::Named(quadraui::NamedKey::Enter) => {
                    engine.ai_mention_accept_selected();
                    return engine.ai_has_focus;
                }
                _ => {}
            }
        }
    }

    let chat_event = engine.ai_chat.borrow_mut().handle(event, backend, rect);
    engine.dispatch_ai_chat_event(chat_event)
}
