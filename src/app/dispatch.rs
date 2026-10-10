use super::construction::{dedup_caret_shape, terminal_poll_rearm_delay};
use super::input::normalize_mac_cmd_as_ctrl;
use super::*;

impl App {
    /// The actual body of `ShellApp::handle`, moved to an inherent
    /// method (#813) so the trait impl can wrap it with a single
    /// `exit_requested` check that covers every early-return arm below
    /// (including ones nested inside `dispatch_engine_action` /
    /// `apply_dialog_action` / menu handling) without editing each one.
    pub(crate) fn handle_dispatch(
        &mut self,
        event: quadraui::UiEvent,
        backend: &mut dyn quadraui::Backend,
        ctx: &quadraui::ShellContext<'_>,
    ) -> quadraui::Reaction {
        use quadraui::{Key, MouseButton, UiEvent};
        // #1745: must run before anything else reads `event`'s modifiers —
        // see `normalize_mac_cmd_as_ctrl`'s own doc. The `matches!` guard
        // is a cheap hardening (#1745 review), not a behaviour change:
        // `normalize_mac_cmd_as_ctrl`'s own `match` only has arms for
        // `UiEvent::KeyPressed`, falling through every other variant via
        // `other => other` regardless of `vscode_mode`'s value — so
        // skipping the borrow entirely for a non-`KeyPressed` event (mouse
        // moves/window events, the overwhelming majority of dispatches)
        // changes nothing it would have computed. Without the guard this
        // was an *unconditional* immutable borrow at the top of the one
        // shared dispatcher, on every event; `dispatch_engine_action`
        // holds a `borrow_mut()` across the whole `apply_engine_action`
        // call, and quadraui's macOS dialogs drive nested modal loops, so
        // any nested re-entry into `handle` that previously passed through
        // a non-engine-borrowing arm would have panicked on this borrow.
        let vscode_mode =
            matches!(event, UiEvent::KeyPressed { .. }) && self.engine.borrow().is_vscode_mode();
        let event = normalize_mac_cmd_as_ctrl(event, backend, vscode_mode);

        // ── #1427: shared menu-bar reveal/hide routing ───────────────────────
        // The #318 Alt+<letter> shim (only fires while the bar is hidden —
        // GTK's is always visible, see `ShellApp::setup`'s three-way branch)
        // and the #988/#1029 stale-hamburger-corner-click guard (only ever
        // armed by a hamburger panel click, which GTK/macOS/Win never
        // register — see `Self::shell_config`'s `cell`-profile branch). A
        // no-op on those backends; see `render::route_menu_bar_reveal`'s own
        // doc. Must run before the `MenuSystem` intercept just below, since
        // the shim's reveal has to take effect in the *same* dispatch that
        // intercept reads `menu_bar_visible` from.
        let menu_bar_reveal_reaction = {
            let mut engine = self.engine.borrow_mut();
            // Nothing in `handle_dispatch` returns before this point, so
            // consuming the one-shot guard here is equivalent to TUI's "at
            // the very top of `handle`, before any early exit" placement —
            // see `consume_hamburger_stale_click_guard`'s own doc.
            let stale_hamburger_corner_click =
                render::consume_hamburger_stale_click_guard(&mut engine, &event);
            render::route_menu_bar_reveal(
                &mut engine,
                &event,
                stale_hamburger_corner_click,
                backend,
                ctx,
            )
        };
        if let Some(reaction) = menu_bar_reveal_reaction {
            return reaction;
        }

        // ── Menu system intercept (#552) ─────────────────────────────────────
        // GTK's menu bar is always visible (see `ShellApp::setup`) and its
        // dropdown overlay must intercept keys/clicks before the sidebar or
        // editor sees them — same precedence TUI uses (mod.rs "MenuSystem
        // intercept" block) via the identical shared `menu_system.handle()`.
        //
        // #955 (ACP-4, review fix): also gated on the dropdown genuinely
        // being closed OR the change-review surface being closed. The menu
        // bar/CSD row occupies the window's full top band (`bar_rect.x`
        // starts right after the app icon, `bar_rect.width` spans the rest
        // of the window) — exactly where the change-review surface's first
        // diff rows paint, since that surface is genuinely full-viewport.
        // Without this, `menu_system.handle` treated a click on those rows
        // as landing on "File" (or whichever label happens to share that
        // band) and returned `StateChanged`/`Activated`, swallowing the
        // click into a menu open/highlight instead of ever reaching
        // `handle_mouse_click_msg`. An *already-open* dropdown still wins
        // regardless (`menu_system.borrow().is_open()`), matching every
        // other "topmost modal wins" precedent in this function — only the
        // idle bar itself yields.
        let (menu_bar_visible, menu_bar_toggleable, menu_system) = {
            let eng = self.engine.borrow();
            (
                eng.menu_bar_visible,
                eng.menu_bar_toggleable,
                eng.menu_system.clone(),
            )
        };
        let menu_open = menu_system.borrow().is_open();
        let change_review_open = self.engine.borrow().change_review.is_some();
        // #1764: a *closed* dropdown must not be opened by an Alt+<mnemonic>
        // `KeyPressed` while the engine is mid-text-entry (Insert/Replace) or
        // mid-command-line (Command/Search) — see
        // `render::alt_mnemonic_open_allowed`'s own doc for why (a bugbash
        // repro traced a real macOS pty's Escape-then-letter collapsing into
        // exactly this chord, and letting it open a menu here swallows the
        // keystroke that was supposed to be the Escape, leaving every
        // following key falling through as literal Insert-mode text).
        // `menu_open` (a dropdown that's *already* open) is untouched — this
        // only gates a fresh open.
        let alt_mnemonic_open_blocked = !menu_open
            && matches!(
                event,
                UiEvent::KeyPressed { modifiers, .. } if modifiers.alt
            )
            && !render::alt_mnemonic_open_allowed(self.engine.borrow().mode, menu_bar_toggleable);
        if !alt_mnemonic_open_blocked && (menu_open || (menu_bar_visible && !change_review_open)) {
            // `menu_items_rect`, not `menu_row_rect` (#720): the app icon
            // occupies a leading slot, so the items the last frame *painted*
            // start one slot right of the band's left edge. Hit-testing
            // against the full band would resolve a click on `File` to
            // whatever label now sits a slot to its left. `render_content`
            // writes this from the same `split_menu_row_for_app_icon` call
            // that positions the paint.
            //
            // #1427: on the toggleable-menu-bar profile, `route_menu_bar_
            // reveal`'s Alt+<letter> shim (just above) can reveal the bar in
            // this very dispatch, before any paint has refreshed
            // `menu_items_rect` — `render::menu_bar_intercept_rect` falls
            // back to a synthetic full-width row for exactly that frame; see
            // its own doc. Always `cached` unchanged on GTK/macOS/Win
            // (`menu_bar_toggleable` is never `true` there).
            let bar_rect = render::menu_bar_intercept_rect(
                menu_bar_toggleable,
                self.menu_items_rect.get(),
                backend.viewport().width,
            );
            let menu_event = menu_system.borrow_mut().handle(&event, backend, bar_rect);
            match menu_event {
                quadraui::MenuEvent::Activated(id) => {
                    self.handle_menu_action(id.as_str().to_string());
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
                quadraui::MenuEvent::StateChanged | quadraui::MenuEvent::Consumed => {
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
                // #1763: a dropdown that was genuinely open (`menu_open`,
                // not just the bar revealed) and a `KeyPressed` it doesn't
                // recognise (anything but Escape/arrows/Enter/a matching
                // Alt+<letter>, all handled above) falls all the way
                // through to `Ignored` — `quadraui::MenuSystem::handle` has
                // no type-ahead/dismiss-on-any-key behaviour of its own
                // (`quadraui/src/compose/menu_system.rs`'s own `handle`:
                // the match's final arm is a bare `_ => MenuEvent::Ignored`
                // for exactly this case). Left alone, the dropdown stays
                // `is_open()` and keeps painting every subsequent frame
                // (`render_content`'s unconditional `menu_system.render()`
                // call) while this same event keeps flowing to the Vim/
                // editor dispatch below and is applied there as normal —
                // so a key sequence that happens to open a menu (Alt+<its
                // mnemonic>, or — per #1763's own bugbash repro — a
                // terminal that collapses a fast Escape-then-letter into
                // an Alt+letter chord) leaves every following keystroke
                // editing the buffer correctly *underneath* a dropdown
                // that visually never goes away, exactly the "unexpected
                // 'Go' menu dropdown" symptom. Closing here mirrors the
                // private `handle_escape`'s own whole-menu close inside
                // `MenuSystem::handle` (same `close()` call) for the one
                // case that handler can't reach: the key wasn't Escape, so
                // `MenuSystem::handle` never ran that arm itself.
                //
                // This lives in *shared* `handle_dispatch`, not TUI-side
                // wiring, so the same dismiss-on-unrecognised-key behaviour
                // lands on GTK and macOS too, where `menu_bar_visible` is
                // always `true` and `menu_open` is reachable by a plain
                // mouse click on a bar label — see GTK's
                // `unrecognised_key_closes_a_stale_open_dropdown_on_gtk_1763`
                // (`src/gtk/testing.rs`) for that side's coverage.
                //
                // Deliberately does not also return early or consume the
                // key (no `engine.menu_bar_visible` touch, no early
                // return) — the event must still fall through to the rest
                // of this dispatch unchanged. That is a conscious choice,
                // not an oversight: a dropdown open from a genuine user
                // click, dismissed by a stray letter, now also lets that
                // letter reach and mutate the buffer underneath (a
                // pre-existing leak this does not introduce — the key
                // already flowed through before this arm existed, just
                // with a stale dropdown left on top of it). Swallowing the
                // key instead would avoid that, but would also silently
                // eat whatever the user actually meant to type, which is
                // the worse failure mode of the two. A plain Escape close
                // already leaves the toggleable bar row itself visible too
                // (TUI: `escape_closes_the_dropdown_but_leaves_the_
                // toggleable_bar_row_visible_1763` below), so an
                // unrecognised key closing only the dropdown — and letting
                // the key fall through — is the narrower, consistent fix.
                //
                // NOT fixed here, and still open: #1763's own repro traces
                // the spurious dropdown open back to a real macOS pty
                // apparently collapsing a fast keystroke into an `Alt+g`
                // chord — `TuiDriver` can't carry that modifier
                // synthetically (see `alt_g_dropdown_does_not_survive_a_
                // vim_dw_1763`'s own doc), so the mis-decode itself is
                // neither reproduced nor fixed by this arm, and the
                // mis-decoded keystroke is still swallowed by whichever
                // `handle_alt_char`/menu-open arm actually consumes it
                // upstream of this one. A follow-up issue for that
                // raw-terminal-decode question (vimcode or quadraui) still
                // needs to be filed — track it from there, not from this
                // comment, once it exists.
                //
                // A generic "dismiss an open menu on any unrecognised key"
                // is also arguably `quadraui::MenuSystem::handle`'s own
                // policy to own (every quadraui consumer would want it,
                // not just vimcode) rather than a vimcode-side patch — a
                // quadraui issue for type-ahead/dismiss-on-any-key would
                // let this arm be deleted later; also not yet filed.
                quadraui::MenuEvent::Ignored => {
                    if menu_open && matches!(event, UiEvent::KeyPressed { .. }) {
                        menu_system.borrow_mut().close(backend);
                        self.draw_needed.set(true);
                    }
                }
            }
        }

        // ── Command Center: nav arrows + search box (#676) ────────────────────
        // Checked before the window-control buttons below and the CSD
        // titlebar drag-to-move fallback further down, so a click in the
        // command center (which sits inside the title-bar band the
        // drag-to-move check would otherwise claim) routes to tab-nav / the
        // picker instead of starting a window drag. Mirrors TUI's
        // `mouse.rs` "Menu bar row click — command center only" precedence.
        // The nav-arrow / search-box actions are the shared
        // `render::apply_command_center_hit` (#752) — the pre-#540 Relm4
        // `Msg::MruNavBack` / `MruNavForward` / `OpenCommandCenter` variants
        // for this exact action, already wired end-to-end but never
        // dispatched from anywhere since the cutover. This block was their
        // first live caller (#676); #732 turned them into plain methods,
        // and #752 converged those methods with TUI's identical match arm
        // into the one function in `render.rs`.
        if let UiEvent::MouseDown {
            button: MouseButton::Left,
            position,
            ..
        } = &event
        {
            // `command_center_hit_in_band`, not `CommandCenterLayout::
            // hit_test` directly (#1494 CI): the cached layout's `SearchBox`
            // rect overflows the band whenever the band is narrower than the
            // primitive's 344px content floor, and the overflow lands
            // squarely on the inline window-control buttons that start where
            // the Command Center band ends. See that function's doc.
            let cc_hit = self
                .engine
                .borrow()
                .command_center_layout
                .borrow()
                .as_ref()
                .and_then(|l| crate::render::command_center_hit_in_band(l, position.x, position.y));
            // `Bar` (command-center background, not an interactive segment)
            // and `Outside`/`None` fall through so the drag-to-move fallback
            // below still works for genuine empty-band clicks.
            if let Some(hit) = cc_hit {
                if crate::render::apply_command_center_hit(&mut self.engine.borrow_mut(), hit) {
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
            }
        }

        // ── Inline window-control buttons: minimize/maximize/close (#552) ───
        // Shared `StatusBarInteraction` hover/press/click tracker — the same
        // primitive quadraui's own `full_chrome_demo` reference title bar
        // uses (quadraui#402) — instead of a hand-rolled `StatusBarHit`
        // lookup. Gets the buttons real hover/press highlighting for free
        // and click-on-release semantics (a press that drags off the button
        // before release no longer fires it), matching native window
        // controls. Runs on every event (not just MouseDown) so hover state
        // updates as the pointer moves.
        {
            let rect = self.title_bar_rect.get();
            if rect.width > 0.0 {
                let action = self.title_bar_interaction.borrow_mut().handle(&event, rect);
                match action {
                    quadraui::StatusBarAction::Clicked(id) => {
                        match id.as_str() {
                            render::WINDOW_MINIMIZE_ACTION => self.window_minimize(backend),
                            render::WINDOW_MAXIMIZE_ACTION => self.window_toggle_maximize(backend),
                            render::WINDOW_CLOSE_ACTION => self.window_close(),
                            _ => {}
                        }
                        self.draw_needed.set(true);
                        return quadraui::Reaction::Redraw;
                    }
                    quadraui::StatusBarAction::Redraw => {
                        self.draw_needed.set(true);
                        return quadraui::Reaction::Redraw;
                    }
                    quadraui::StatusBarAction::Ignored => {}
                }
            }
        }

        // ── Outer window border: edge-resize cursor hint (quadraui#406) ──
        // Pure side effect on hover — hint the resize pointer over the outer
        // window border, default everywhere else. Falls through so the
        // editor/sidebar hover handling below still runs. Mirrors
        // `full_chrome_demo`'s `MouseMoved` arm. GTK-only; TUI `set_cursor`
        // is a documented no-op.
        //
        // #1528: `render::WINDOW_RESIZE_GRIP_PX` (a few px), not
        // `backend.line_height()` (a full title-bar/command-line row) — see
        // that constant's doc. With a thin grip there's no need to special-case
        // `ctx.in_title_bar` here the way this used to: the title bar is only
        // as wide as the whole row, and the grip only occupies its outermost
        // sliver, so a point inside the title bar but outside the grip already
        // resolves to `None` on its own. Checked on the SAME predicate the
        // press path below uses when no change-review surface is open — so
        // the cursor never promises a resize the press won't honor in that
        // (overwhelmingly common) case.
        //
        // Pre-existing quirk, NOT fixed by #1528 (review, non-blocking
        // finding #2): this `MouseMoved` arm does not check
        // `change_review_open` at all — it never has — while the press path
        // below does, for the top edge only
        // (`render::resize_edge_overlaps_change_review_band`). So while a
        // change-review surface is open, hovering near the top edge can
        // still show a resize cursor that a press in the same spot won't
        // honor (it falls through to the change-review click handling
        // instead). Narrowing this hint to match would need
        // `change_review_open` computed above the `MouseMoved` arm instead
        // of below it — out of scope for this fix; called out here so the
        // "share one predicate" guarantee above isn't read as unconditional.
        if let UiEvent::MouseMoved { position, .. } = &event {
            let shape = match ctx.window_edge(position.x, position.y, render::WINDOW_RESIZE_GRIP_PX)
            {
                Some(edge) => quadraui::PointerShape::Resize(edge),
                None => quadraui::PointerShape::Default,
            };
            backend.set_cursor(shape);
        }

        // ── Sidebar hover — #754 rung ─────────────────────────────────────
        // This backend already *painted* `screen.panel_hover` (the
        // `RichTextPopup` block in `render_content`) and already tracked the
        // popup's own rect, but nothing on this side ever set
        // `engine.panel_hover` or `engine.sc_button_hovered`: the router was
        // ~78 lines of TUI-only code. That is the #499/#484 mechanism — paint
        // without input on one backend, input without a second painter on the
        // other. `render::route_sidebar_hover` is now the single router and
        // both backends call it.
        if let UiEvent::MouseMoved { position, .. } = &event {
            if let Some(sb) = ctx.layout.sidebar_content_bounds {
                let lh = backend.line_height();
                let on_popup = self.panel_hover_popup_rect.get().is_some_and(|r| {
                    position.x >= r.x
                        && position.x < r.x + r.width
                        && position.y >= r.y
                        && position.y < r.y + r.height
                });
                let owner = render::sidebar_owner(&self.engine.borrow());
                let changed = render::route_sidebar_hover(
                    &mut self.engine.borrow_mut(),
                    &owner,
                    position.x,
                    position.y,
                    render::SidebarBodyGeometry {
                        bounds: sb,
                        row_h: lh.max(1.0),
                        header_rows: 1.0,
                    },
                    true,
                    on_popup,
                );
                if changed {
                    self.draw_needed.set(true);
                }
            }
        }

        // ── Gutter hover — #1544 (`fold_controls = "mouseover"`) ───────────
        // Shared with TUI: both backends reach this same `MouseMoved` arm in
        // `App::handle_dispatch`, so a fold-control marker's visibility can
        // never diverge between them the way a per-backend hover tracker
        // would risk. `render::route_gutter_hover` resolves against
        // `cached_screen_layout` — the same last-painted geometry
        // `handle_mouse_click_msg` already hit-tests real clicks against —
        // so "hovering" and "clicking" the gutter always agree on where it
        // is.
        if let UiEvent::MouseMoved { position, .. } = &event {
            let layout_ref = self.cached_screen_layout.borrow();
            if let Some(layout) = layout_ref.as_ref() {
                let mut engine = self.engine.borrow_mut();
                let was = engine.gutter_hover_window;
                render::route_gutter_hover(
                    &mut engine,
                    layout,
                    position.x as f64,
                    position.y as f64,
                    // #1864 review round 1: the gutter is part of the
                    // *editor* row grid — `painted_line_height()` (#555) is
                    // the cached, published-at-paint-time row pitch every
                    // other editor-row hit-test in this file already reads
                    // (`self.handle`/`apply_picker_route`/etc.), rather than
                    // `backend.line_height()`'s live, mutable field — which
                    // `render_content`'s `FrameOp::SidebarPanel` arm now
                    // transiently overwrites mid-frame for sidebar content
                    // (tree/list/form rows). A raw `backend.line_height()`
                    // read from a click/hover handler, outside any paint
                    // call, happens to still agree today (the override is
                    // restored before `render_content` returns), but
                    // `painted_line_height()` is the documented contract for
                    // this and does not depend on that ordering.
                    self.painted_line_height(),
                    backend.char_width() as f64,
                );
                if engine.gutter_hover_window != was {
                    drop(engine);
                    self.draw_needed.set(true);
                }
            }
        }

        // ── Editor hover dwell (#1750) ─────────────────────────────────────
        // Mouse movement over editor text arms the dwell timer that
        // eventually fires `textDocument/hover` (LSP) or surfaces a plugin's
        // `vimcode.editor.set_hover` content — `Engine::editor_hover_mouse_
        // move` is the sole entry point for both; `poll_editor_hover`
        // (driven by the shared tick, `render::run_shared_tick_chores`) does
        // the rest once the dwell elapses. #731 deleted the GTK-only polling
        // block that used to call this — it was gated on a permanently-
        // `None` Relm4 widget handle, so it had been dead since the #540
        // cutover — and nothing replaced the call; #1434 then deleted TUI's
        // own `mouse.rs` copy the same way, so by #1750 no backend ever
        // armed the dwell timer at all and the hover popup (LSP, and every
        // extension's own hover) stopped appearing on every backend at
        // once. This is the shared `MouseMoved` arm both backends already
        // reach (see the sidebar/gutter-hover blocks just above), not a
        // per-backend restoration — one fix covers both, same reasoning as
        // the #754/#1544 rungs above it.
        //
        // Resolved via the shared `pixel_to_click_target` (`mutate_focus:
        // false` — a pure query, the same contract `handle_mouse_drag_msg`'s
        // cross-split continuation relies on) rather than re-deriving
        // gutter-width/scroll-offset math by hand the way the pre-#731 GTK
        // code did: `ClickTarget::BufferPos`'s `(line, col)` is the exact
        // buffer position `draw_editor` painted at this pixel (via
        // `Backend::editor_col_at_x`), so hover and click can never resolve
        // to different cells (the same #560/#515 guarantee the click paths
        // above already lean on). Any other target (gutter, tab bar,
        // outside the window entirely, no cached layout yet) dismisses an
        // already-visible, unfocused popup — mirroring the pre-#731 "mouse
        // outside editor area" branch.
        //
        // No idle-tick redraw reintroduced here (the #1722 worry this issue
        // was filed against): this arm only ever touches `engine.editor_
        // hover_dwell`/`editor_hover`/`lsp_hover_text` from a real
        // `MouseMoved` event. The repaint once the LSP response actually
        // arrives is still `poll_lsp`'s job (`core/engine/panels.rs`
        // setting `redraw = true` on a hover reply), not this arm's.
        //
        // `!buttons.left`: a left-button drag (text selection, or a cross-
        // split continuation in `handle_mouse_drag_msg`) must not also arm
        // the dwell timer — matches the #751 context-menu-hover guard just
        // below in the `MouseMoved` event arm itself.
        if let UiEvent::MouseMoved { position, buttons } = &event {
            let gate_open = {
                let e = self.engine.borrow();
                !buttons.left
                    && e.settings.hover_delay > 0
                    && !e.editor_hover_has_focus
                    && !e.is_blocking_modal_open()
                    && (matches!(e.mode, core::Mode::Normal | core::Mode::Visual)
                        || e.is_vscode_mode())
            };
            if gate_open {
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(layout) = layout_ref.as_ref() {
                    let mut engine = self.engine.borrow_mut();
                    let target = pixel_to_click_target(
                        &mut engine,
                        backend,
                        position.x as f64,
                        position.y as f64,
                        self.cached_line_height,
                        self.cached_char_width,
                        layout,
                        &self.cached_group_tab_bar_layouts.borrow(),
                        self.cached_frame_hit_map.borrow().as_ref(),
                        &self.cached_tab_bar_zones.borrow(),
                        false, // pure hover query — must not steal focus or fire gutter actions
                        // scratch: never touched under mutate_focus: false — see click.rs:233-320
                        &mut quadraui::DragState::default(),
                        false, // no Alt-fine-seek context for a plain mouse-move
                    );
                    let had_hover = engine.editor_hover.is_some();
                    // Popup keep-alive is geometry-independent on purpose
                    // (#1750 review): the popup can spill over the active
                    // window's gutter column or into a neighbouring split,
                    // where `pixel_to_click_target` resolves to
                    // `ClickTarget::None`/another window id and the `_`
                    // dismiss arm below would yank the popup out from under
                    // a pointer that is physically *on* it — e.g. while
                    // travelling towards the `command:definition` link
                    // (#272/#491). Checked before the target is classified,
                    // so "the mouse is over the popup" always wins.
                    let on_popup = self.editor_hover_popup_rect.get().is_some_and(|r| {
                        position.x >= r.x
                            && position.x < r.x + r.width
                            && position.y >= r.y
                            && position.y < r.y + r.height
                    });
                    // When keep-alive hits, the only arm that could still
                    // run would be a no-op anyway (`editor_hover_mouse_move`
                    // returns without touching anything once
                    // `mouse_on_popup` is true and a popup is visible), so
                    // keeping the popup alive is simply "change nothing".
                    let keep_alive = on_popup && had_hover;
                    match target {
                        _ if keep_alive => {}
                        ClickTarget::BufferPos(wid, line, col)
                            if wid == engine.active_window_id() =>
                        {
                            engine.editor_hover_mouse_move(line, col, on_popup);
                        }
                        _ => {
                            // Not a buffer position at all, or one resolved
                            // against a *different* (unfocused) split: both
                            // mean "outside the editor area we can hover
                            // against", so dismiss any visible popup.
                            // #1750 review: the window-id guard above is
                            // load-bearing because `BufferPos`'s `(line,
                            // col)` come from the *hovered* window's scroll
                            // offset while every downstream consumer reads
                            // active-window/active-buffer state.
                            if had_hover {
                                engine.dismiss_editor_hover();
                            }
                        }
                    }
                    if had_hover != engine.editor_hover.is_some() {
                        drop(engine);
                        self.draw_needed.set(true);
                    }
                }
            }
        }

        // #955 (ACP-4, review fix): gates the title-bar drag/double-click
        // arms below on the change-review surface being closed. That surface
        // is genuinely full-viewport — its first diff row paints inside
        // `ctx.in_title_bar`'s band, underneath the (visually hidden but
        // still logically live) CSD title bar — so without this guard, a
        // click there was silently reinterpreted as "start dragging the
        // window" instead of reaching `handle_mouse_click_msg`'s
        // change-review branch further down. `ctx.in_title_bar` has no such
        // reach today for the folder picker (its popup is centred, never
        // touching row 0), which is why this wasn't already latent there in
        // a way any existing test could see.
        let change_review_open = self.engine.borrow().change_review.is_some();

        // ── Outer window border: edge-resize press (quadraui#406) ──────────
        // Checked BEFORE the CSD title-bar drag-to-move / double-click-
        // maximize arms and the #816 command-line click below (#1528, review
        // of #816/#1026/#987): `render::WINDOW_RESIZE_GRIP_PX` is a thin,
        // fixed pixel margin (see that constant's doc) — a few px, not a full
        // `line_height` row — so it sits *inside* the title bar and
        // command-line rows instead of spanning them. It has to win over
        // both to ever fire at all: a full-width CSD title bar otherwise owns
        // every pixel of the top row (dead North/NE/NW), and the command
        // line likewise owns every pixel of the bottom row (dead South/SE/
        // SW, the #816 bug this replaces the guard for). Everywhere else in
        // those rows — the overwhelming majority of both — `window_edge`
        // returns `None` and this falls through to the drag/click handling
        // below untouched, same as before #1528. The same thin margin also
        // keeps the East edge out of the vertical scrollbar's and the
        // minimap's own hit-test area with a single editor group (see the
        // constant's doc); no guard is needed for those the way the command
        // line needed one, because the margin no longer reaches them.
        //
        // #1026/#987 review: `begin_window_resize`'s own doc contract is
        // explicit — it returns `false` "when the backend owns no window
        // (TUI...)" and callers "should treat `false` as a no-op, not an
        // error". `ctx.window_edge`'s margin math is generic geometry, not
        // GTK-gated — it fires for any backend near the outer window bounds
        // — so only consume the event when the backend actually armed a
        // resize; otherwise fall through so a window's own rightmost column
        // (exactly where a vertical scrollbar or minimap gutter can sit) on
        // a backend with no window still reaches the editor's own hit-test.
        //
        // #1528 review (blocking finding #3): the base commit's guard here
        // was `!change_review_open` applied to EVERY edge — a regression
        // this PR introduced and the review caught, since the base-commit
        // pre-#1528 code had no `change_review_open` guard on edge-resize at
        // all. The change-review surface only ever overlaps the *top* of the
        // window (see the doc above), so only `North`/`NorthEast`/
        // `NorthWest` need to defer to it —
        // `render::resize_edge_overlaps_change_review_band` is the
        // (unit-tested) decision. Resizing from the bottom, left, right, or
        // the two southern corners must keep working while a change-review
        // surface is open; nothing in that surface ever paints there.
        if let UiEvent::MouseDown {
            button: MouseButton::Left,
            position,
            ..
        } = &event
        {
            if let Some(edge) =
                ctx.window_edge(position.x, position.y, render::WINDOW_RESIZE_GRIP_PX)
            {
                let blocked_by_change_review =
                    change_review_open && render::resize_edge_overlaps_change_review_band(edge);
                if !blocked_by_change_review && backend.begin_window_resize(edge) {
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
            }
        }

        // ── CSD titlebar background: drag-to-move / double-click-maximize ──
        // (quadraui#400). Runs after the menu-item intercept, the
        // window-control-button check, and the edge-resize press above, so
        // all three take priority — only a press/double-click that lands in
        // the title bar band but misses every interactive segment (menu
        // item, min/max/close button, resize grip) reaches here, matching
        // `Backend::begin_window_drag`'s documented contract. Mirrors
        // quadraui's `full_chrome_demo` reference. TUI has no window, so
        // `begin_window_drag`/`begin_window_resize`/`toggle_window_maximize`
        // are all documented no-ops there; this path is GTK-only.
        match &event {
            UiEvent::MouseDown {
                button: MouseButton::Left,
                position,
                ..
            } if !change_review_open && ctx.in_title_bar(position.x, position.y) => {
                backend.begin_window_drag();
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::DoubleClick { position, .. }
                if !change_review_open && ctx.in_title_bar(position.x, position.y) =>
            {
                backend.toggle_window_maximize();
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            _ => {}
        }

        // ── #1762: rescue a double-click that folded away an activity-bar
        // panel switch ─────────────────────────────────────────────────────
        //
        // `quadraui::AppShell::handle` (`compose/app_shell.rs`) only matches
        // a plain `UiEvent::MouseDown` for activity-bar hit-testing — every
        // other event variant, `DoubleClick` included, falls through its own
        // `_ => AppShellEvent::Ignored` arm. `quadraui::dispatch::
        // DoubleClickDetector` folds a press into a `DoubleClick` whenever
        // it lands within its radius of the previous press within
        // `DOUBLE_CLICK_MS` (400ms). Only `TuiBackend` runs the *default*
        // 1.5-*cell* radius (TUI's own character-grid unit, where adjacent
        // activity-bar rows are exactly 1.0 cell apart); `MacBackend` uses
        // `DoubleClickDetector::with_radius(MAC_DOUBLE_CLICK_RADIUS)` = 4.0
        // *points*, explicitly because the cell-tuned 1.5 is meaningless in
        // AppKit's point-precision coordinates, and GTK/Windows use their
        // own 4.0px radius — so adjacent-row folding is a TUI-only failure
        // mode. On TUI, two genuinely distinct, fast real clicks on
        // *adjacent* icons (Source Control then Debug, Debug then
        // Extensions, …) land within that 1.5-cell radius and fold into one
        // `DoubleClick`, which the activity bar has no handler for — the
        // second click is silently dropped and the sidebar stays on
        // whatever panel was already active.
        //
        // This is a real, previously-unknown latent bug in its own right,
        // confirmed by `activity_bar_adjacent_clicks_are_not_dropped_by_double_click_fold_1762`
        // in `src/tui_main/app_on_tui_tests.rs`. It is NOT a confirmed
        // explanation for vimcode#1762's reported "Explorer -> Source
        // Control -> Extensions -> Explorer" symptom: the final Explorer
        // re-click sits 4.0 cells from the row it follows, far outside the
        // 1.5-cell radius, so that specific re-click cannot be dropped by
        // this fold under any timing — see that test's doc comment and
        // `PROJECT_STATE.md` for the full accounting of what this fix does
        // and does not demonstrate about #1762's exact reported run.
        //
        // The real fix belongs in quadraui (`AppShell::handle` growing a
        // `DoubleClick` arm identical to its `MouseDown` one for the
        // activity-bar band — a double-click on an activity-bar icon has no
        // distinct meaning from a single click there, for every consumer,
        // not just vimcode). That gap is drafted in
        // `docs/PENDING_QUADRAUI_ISSUES.md` for the coordinator to file
        // verbatim (not yet filed/landed as of this commit, per CLAUDE.md's
        // Platform-Neutrality Rule). Until it lands, re-synthesize the
        // dropped click as the plain `MouseDown` it was always meant to be
        // and feed it back through the shell's own *public* `handle()` —
        // the exact dispatch a real single click takes — rather than
        // hand-rolling an activity-bar hit-test here. That keeps this "thin
        // event-to-engine wiring" against the existing public API, shared
        // once in `App` (not `src/gtk/`/`src/tui_main/`), so both backends
        // pick up the fix from one place — the same pattern
        // `render::consume_hamburger_stale_click_guard`/
        // `render::sync_runner_sidebar_visibility` already use for other
        // quadraui-shaped gaps in this exact file.
        //
        // The synthetic is hard-coded to `MouseButton::Left` because
        // `UiEvent::DoubleClick` carries no button — `DoubleClickDetector`
        // folds same-button pairs of any button, so a fast double-*right*-
        // click on the activity bar is replayed as a left click. Harmless
        // in practice (it only switches panels, same as a left click
        // would), but worth flagging since it's not quite faithful replay.
        //
        // Note this also changes behaviour on every backend, not just TUI:
        // a genuine fast double-click on an activity-bar icon that is
        // *already active* used to be swallowed as `Ignored` (the first
        // click already toggled the sidebar hidden); it is now replayed as
        // a second plain `MouseDown`, which `AppShell::handle_activity_click`
        // treats as a toggle and re-shows the sidebar. That is arguably
        // more VS-Code-like (a double-click has no special meaning on this
        // chrome), but it is a user-visible semantic change with its own
        // GTK-side coverage below (`src/gtk/testing.rs`'s
        // `activity_bar_double_click_on_active_icon_reopens_sidebar_via_gtk_driver`).
        if let UiEvent::DoubleClick { position, .. } = &event {
            let position = *position;
            let viewport = backend.viewport();
            let area = quadraui::Rect::new(0.0, 0.0, viewport.width, viewport.height);
            // Recompute the layout fresh (not `ctx.layout`, the cached
            // per-frame copy) so this gate agrees with the hit-test
            // `handle()` performs two lines down, matching
            // `render::route_menu_bar_reveal`'s own
            // `ctx.shell().layout(area, backend.line_height())` pattern for
            // the same "gate must match the hit-test" reason: both run
            // inside `handle_dispatch`, which can have already mutated
            // shell chrome earlier in this same dispatch.
            let layout = ctx.shell().layout(area, backend.line_height());
            if layout.activity_bar_bounds.contains(position) {
                let synthetic = UiEvent::MouseDown {
                    widget: None,
                    button: MouseButton::Left,
                    position,
                    modifiers: quadraui::Modifiers::default(),
                };
                let shell_ev = ctx.shell_mut().handle(&synthetic, &*backend, area);
                if !matches!(shell_ev, quadraui::AppShellEvent::Ignored) {
                    quadraui::ShellApp::on_shell_event_ctx(self, &shell_ev, ctx);
                    self.draw_needed.set(true);
                    return if self.draw_needed.get() {
                        self.draw_needed.set(false);
                        quadraui::Reaction::Redraw
                    } else {
                        quadraui::Reaction::Continue
                    };
                }
                // `shell_ev` was `Ignored` (e.g. the double-click landed in
                // the activity bar's bounds but hit no zone) — fall through
                // to the rest of the dispatch pipeline instead of
                // unconditionally consuming it, same as every other arm in
                // this function.
            }
        }

        // Pointer events over the sidebar content area are forwarded to the active
        // panel's controller before the editor click path sees them. In ShellApp
        // mode there is no per-panel DrawingArea, so without this the file explorer
        // never receives clicks. (#540 ShellApp port)
        if self.try_route_sidebar_mouse_event(&*backend, &event, ctx) {
            return if self.draw_needed.get() {
                self.draw_needed.set(false);
                quadraui::Reaction::Redraw
            } else {
                quadraui::Reaction::Continue
            };
        }

        match event {
            UiEvent::KeyPressed {
                key,
                modifiers,
                repeat,
            } => {
                // #815: kept alongside the decoded `key_name`/`unicode` below
                // so the folder-picker rung in `handle_key_press` can feed
                // `FolderPickerController::handle` the *original* event
                // instead of a re-encoded one — mirrors TUI's `dap_event`
                // (`shell_app.rs`).
                let raw_event = UiEvent::KeyPressed {
                    key: key.clone(),
                    modifiers,
                    repeat,
                };
                let (key_name, unicode) = match key {
                    // #1744: Ctrl+Shift+\ (VS Code's `editor.action.
                    // jumpToBracket`) needs to decode distinctly from plain
                    // Ctrl+\ (`open_editor_group`/split-editor). Most
                    // surfaces (GDK, and a kitty/CSI-u terminal with
                    // character-resolution keyboard enhancement) already
                    // deliver the resolved glyph `'|'` here, which matches
                    // `Engine::handle_vscode_key`'s own `"Shift_backslash" |
                    // "|"` arm unchanged below — no special case needed for
                    // that shape. A kitty/CSI-u terminal reporting the base
                    // key `'\\'` plus an explicit Shift *bit* instead (rather
                    // than the shifted glyph) needs this one extra arm to
                    // reach the same arm — `render::engine_key_from_ui` has
                    // the mirror logic for this chord too, but is never
                    // called from this `Key::Char` match (see that
                    // function's own module doc on why GTK/TUI's `Key::Char`
                    // decode stays independent of it), so it alone cannot
                    // make this shape reachable.
                    Key::Char('\\') if modifiers.ctrl && modifiers.shift => {
                        ("Shift_backslash".to_string(), Some('\\'))
                    }
                    Key::Char(c) => (c.to_string(), Some(c)),
                    Key::Named(_) => {
                        // #826: `Escape`/`Enter`->`Return`/`Backspace`->
                        // `BackSpace`/`Delete`/`Tab`/`Home`/`End`/the arrows/
                        // F-keys are byte-identical to TUI's spelling, so
                        // those go through the shared
                        // `render::engine_key_from_ui` — the same decoder
                        // TUI's dispatch now calls — instead of restating an
                        // identical table a second time here.
                        //
                        // #1060: the remaining four keys that used to keep
                        // GTK's own spelling now go through the same shared
                        // decoder too, matching TUI's spelling exactly
                        // (`render::engine_key_from_ui` spellings on the
                        // right):
                        //  * `BackTab`: `"BackTab"` -> `"ISO_Left_Tab"`.
                        //    `panels.rs`/`ext_panel.rs`'s hover-key arm
                        //    already dual-aliased both spellings; the three
                        //    sites that only recognised `"BackTab"`
                        //    (`search.rs`'s `handle_search_input_key`,
                        //    `source_control.rs`'s sidebar nav,
                        //    `ext_panel.rs`'s `dispatch_ext_sidebar_key_unified`)
                        //    now also accept `"ISO_Left_Tab"` — TUI already
                        //    sent that spelling to all three and was
                        //    silently dropping Shift+Tab there before this
                        //    fix, so this closes a live TUI bug, not just a
                        //    GTK one. The main-editor command-line wildmenu
                        //    and Ctrl+Shift+Tab tab-switcher-backward binds
                        //    (`keys.rs`) already only recognised
                        //    `"ISO_Left_Tab"`, so GTK gains working
                        //    Ctrl+Shift+Tab / Shift+Tab-in-`:`-wildmenu as a
                        //    side effect.
                        //  * `PageUp`/`PageDown`: `"PageUp"`/`"PageDown"` ->
                        //    `"Page_Up"`/`"Page_Down"`. Every consumer
                        //    (`source_control.rs`, `ext_panel.rs`,
                        //    `search.rs`, `explorer_ops.rs`,
                        //    `canonical_terminal_key_name`) already only
                        //    recognised the TUI spelling, so this closes the
                        //    pre-existing GTK gap noted at the old comment
                        //    here rather than needing its own audit.
                        //  * `Insert`: was `"Insert"`, and the shared decoder
                        //    used to return `None` for `NamedKey::Insert`
                        //    (dropping the key entirely — GTK's terminal PTY
                        //    passthrough bypassed the shared decoder just to
                        //    avoid that). `render::engine_key_from_ui` now
                        //    has an `NamedKey::Insert => Some(("Insert", ..))`
                        //    arm so both backends get the same, still-working
                        //    `"Insert"` spelling.
                        //
                        // #1422: `key_name` now reaches every engine consumer
                        // exactly as produced here — `handle_key_press` and
                        // its callees (`dispatch_sidebar_panel_key`,
                        // `dispatch_focus_owner_residual`,
                        // `activity_bar_key_action`, …) used to round-trip it
                        // through a second, GTK-local decode layer
                        // (`map_gtk_key_name` / `map_gtk_key_with_unicode` /
                        // `gtk_key_name_to_quadraui`) that this shared
                        // decoder's own spelling already made redundant — see
                        // issue #1422 for the full audit of why every
                        // consumer already accepted this decoder's spelling
                        // directly.
                        // #1428: `self.keyboard_enhanced` (read once in
                        // `setup()` from the live `BackendCaps::
                        // kitty_keyboard`) rather than a hardcoded `true` —
                        // wrong on a TUI-via-`App` construction running on a
                        // terminal without the kitty protocol (#826);
                        // unaffected on GTK/macOS/Win-GUI since that
                        // capability is always `false` there anyway, which
                        // is exactly what a literal `true` used to paper
                        // over by disabling the terminal-only fallback arms
                        // unconditionally rather than because a real
                        // capability read said so.
                        let n = render::engine_key_from_ui(&key, modifiers, self.keyboard_enhanced)
                            .map(|(name, _, _)| name)
                            .unwrap_or_default();
                        (n, None)
                    }
                };
                if !key_name.is_empty() || unicode.is_some() {
                    self.handle_key_press(
                        key_name,
                        unicode,
                        modifiers.ctrl,
                        modifiers.shift,
                        modifiers.alt,
                        &raw_event,
                        backend,
                        ctx,
                    );
                }
            }
            UiEvent::CharTyped(c) => {
                // Ctrl-modified characters arrive via KeyPressed; CharTyped is
                // for IME-composed printable characters only. Per
                // `FolderPickerController::handle`'s own contract this is
                // never the folder picker's typing source either, so passing
                // it through as the "raw event" is correct, not a stand-in.
                self.handle_key_press(
                    c.to_string(),
                    Some(c),
                    false,
                    false,
                    false,
                    &UiEvent::CharTyped(c),
                    backend,
                    ctx,
                );
            }
            UiEvent::Accelerator(id, _mods) => {
                if let Some(action) = render::dispatch_panel_accelerator(
                    id.as_str(),
                    &mut self.engine.borrow_mut(),
                    self,
                ) {
                    // `dispatch_panel_accelerator` already mutated `engine`
                    // directly for the nine pure-`Engine` actions, but every
                    // action still needs geometry recomputed before the next
                    // paint — matches the pre-#761 per-arm
                    // `deferred.send(DeferredAction::Resize)`.
                    use render::PanelAccelerator::*;
                    if matches!(
                        action,
                        FuzzyFinder | LiveGrep | CommandPalette | AddCursor | SelectAllMatches
                    ) {
                        self.deferred.send(DeferredAction::Resize);
                    }
                }
                self.draw_needed.set(true);
            }
            UiEvent::MenuActivated(id) => {
                // #901: fired by a *native* OS menu bar (macOS `NSMenu` via
                // `Backend::install_menu_bar`) — see the doc comment on
                // `UiEvent::MenuActivated` distinguishing it from the drawn
                // `MenuSystem`'s click path, which resolves to
                // `quadraui::MenuEvent::Activated` further up in this same
                // method and calls this identical dispatcher. One action
                // path for both, not two: `render::build_menu_defs` gave
                // every leaf item's `WidgetId` the same string as its
                // `MENU_STRUCTURE` `action` field, so `id.as_str()` here is
                // exactly the command string `handle_menu_action` expects.
                self.handle_menu_action(id.as_str().to_string());
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::ContextMenuItemActivated(id) => {
                // #902/#1580: fired by a *native* right-click popup (macOS
                // `NSMenu` via `Backend::show_context_menu`, opened by
                // `Self::open_context_menu_now_if_native` when
                // `Backend::effective_menu_style()` resolved `Native`).
                // `id` is one of `context_menu_panel_to_quadraui_
                // context_menu`'s synthesised `"context:N"` ids — the exact
                // same ids `route_modal_overlay_click`'s in-window hit-test
                // (`ContextMenuHit::Item` → `context_menu_hit_to_idx`)
                // resolves, so routing the activation through
                // `apply_context_menu_route` reuses that one conversion
                // instead of duplicating it. Reached from `handle_dispatch`
                // itself, i.e. event-handler time — same as every other
                // `UiEvent` arm here, and (per #1580's root-cause fix)
                // never queued from inside a paint closure any more.
                let idx = crate::core::engine::context_menu_hit_to_idx(
                    &quadraui::ContextMenuHit::Item(id),
                );
                let route = match idx {
                    Some(idx) => render::ContextMenuRoute::Item(idx),
                    None => render::ContextMenuRoute::Dismiss,
                };
                self.apply_context_menu_route(&*backend, route);
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::ContextMenuDismissed => {
                // #902: the native popup was dismissed without a selection
                // (Escape, click-away). Same close path a `ContextMenuRoute
                // ::Dismiss` from the in-window hit-test already takes.
                self.apply_context_menu_route(&*backend, render::ContextMenuRoute::Dismiss);
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::MouseDown {
                button,
                position,
                modifiers,
                ..
            } => {
                let main = ctx.layout.main_content_bounds;
                let w = main.width as f64;
                match button {
                    MouseButton::Left if modifiers.ctrl => {
                        self.handle_ctrl_mouse_click(
                            &*backend,
                            position.x as f64,
                            position.y as f64,
                        );
                    }
                    MouseButton::Left => {
                        self.handle_mouse_click_msg(
                            &*backend,
                            position.x as f64,
                            position.y as f64,
                            w,
                            modifiers.alt,
                        );
                    }
                    MouseButton::Right => {
                        let rx = position.x as f64;
                        let ry = position.y as f64;
                        // ── Modal-overlay rung (#733 review) ────────────
                        // A modal dialog eats every event, including
                        // right-clicks, so it can't be right-clicked
                        // through to the editor/tab context menu
                        // underneath — TUI's `handle_mouse` already
                        // returns unconditionally for any event kind
                        // while `engine.dialog.is_some()`. This backend's
                        // left-click path goes through
                        // `route_modal_overlay_click` via
                        // `handle_mouse_click_msg`, but the right-click
                        // path used to skip straight to tab/editor
                        // resolution below without consulting it, so a
                        // right-click on an open dialog opened the
                        // editor's context menu behind it. Route through
                        // the same shared rung (`ModalMouseAction::Other`)
                        // before doing anything else.
                        let modal_route =
                            self.route_modal_overlay(rx, ry, render::ModalMouseAction::Other);
                        if modal_route == render::ModalOverlayRoute::Swallow {
                            self.draw_needed.set(true);
                        } else {
                            // #546 FAILED-1: this used to unconditionally build
                            // `EditorRightClick`, so right-clicking a tab opened
                            // the *editor's* context menu (identical item list to
                            // right-clicking in the buffer) instead of a
                            // tab-specific one. Resolve the click against the
                            // last-painted tab-bar geometry first — read-only, no
                            // engine mutation — and only fall back to the editor
                            // menu when it isn't over a tab.
                            let tab_target = {
                                let engine = self.engine.borrow();
                                let layout_ref = self.cached_screen_layout.borrow();
                                layout_ref.as_ref().and_then(|layout| {
                                    resolve_tab_right_click(
                                        &engine,
                                        rx,
                                        ry,
                                        self.cached_line_height,
                                        self.cached_char_width,
                                        layout,
                                        &self.cached_group_tab_bar_layouts.borrow(),
                                        self.cached_frame_hit_map.borrow().as_ref(),
                                        &self.cached_tab_bar_zones.borrow(),
                                    )
                                })
                            };
                            if let Some((group_id, tab_idx)) = tab_target {
                                self.handle_tab_right_click(group_id, tab_idx, rx, ry);
                            } else {
                                self.handle_editor_right_click(&*backend, rx, ry);
                            }
                        }
                    }
                    _ => {}
                }
                // Mouse clicks always require a redraw (cursor movement, selection,
                // focus change). draw_needed may already be set by the handler
                // above, but set it unconditionally so handle() returns
                // Reaction::Redraw even when a handler takes an early-return path.
                self.draw_needed.set(true);
            }
            UiEvent::DoubleClick { position, .. } => {
                self.handle_mouse_double_click_msg(&*backend, position.x as f64, position.y as f64);
                self.draw_needed.set(true);
            }
            UiEvent::MouseMoved { position, buttons } => {
                self.mouse_pos_cell
                    .set((position.x as f64, position.y as f64));
                // ── Modal-overlay hover rung (#751) ─────────────────────
                // An open context menu tracks the pointer, exactly as TUI's
                // `handle_mouse` has always done. This backend had no hover
                // arm at all, so whichever item was selected when the menu
                // opened stayed highlighted wherever the pointer went (#373)
                // — and a keyboard Down after a mouse hover then moved from
                // the wrong row.
                if !buttons.left {
                    if let render::ModalOverlayRoute::ContextMenu(route) = self.route_modal_overlay(
                        position.x as f64,
                        position.y as f64,
                        render::ModalMouseAction::Move,
                    ) {
                        self.apply_context_menu_route(&*backend, route);
                    }
                }
                if buttons.left {
                    let main = ctx.layout.main_content_bounds;
                    // #1877: the recovered (un-shrunk) height — see
                    // `render::main_content_true_height`'s doc — so a drag
                    // that clamps against the content height (e.g. the
                    // picker popup bounds `handle_mouse_drag_msg` computes)
                    // still clamps against the window's true bottom edge,
                    // not `shell_config`'s static bottom-chrome reservation.
                    self.handle_mouse_drag_msg(
                        &*backend,
                        position.x as f64,
                        position.y as f64,
                        main.width as f64,
                        render::main_content_true_height(ctx.layout) as f64,
                    );
                }
            }
            UiEvent::MouseUp { .. } => {
                let main = ctx.layout.main_content_bounds;
                self.handle_mouse_up_msg(&*backend, main.width as f64);
            }
            UiEvent::Scroll {
                delta, position, ..
            } => {
                // #646: record where the wheel event happened before dispatching.
                // `handle_mouse_scroll_msg` takes only the delta, and reads the pointer
                // back out of `last_editor_pointer` to decide which window (or
                // registered scroll surface) the wheel targets. Nothing set that
                // cell after the #540 Relm4→ShellApp migration removed the
                // `EventControllerMotion` that used to — see the field's doc — so
                // it was permanently `None` and every wheel event fell through to
                // the *focused* window regardless of the pointer (#240 behaviour
                // dead on GTK, still live on TUI). A wheel event carries its own
                // position, so use that directly rather than depending on a
                // preceding motion event.
                self.last_editor_pointer
                    .set(Some((position.x as f64, position.y as f64)));
                // #554: **negate y back to GTK's raw polarity.**
                //
                // Two conventions meet at this line and they disagree:
                //
                // - GDK's `EventControllerScroll` reports *positive dy = wheel
                //   down*.
                // - `UiEvent::Scroll.delta` follows quadraui's convention,
                //   *positive y = up toward the top of the content*.
                //   `quadraui::gtk::events::gdk_scroll_to_uievent` is what
                //   flips one into the other — it constructs
                //   `ScrollDelta::new(dx, -dy)`.
                //
                // Everything downstream of `handle_mouse_scroll_msg` — the
                // `delta_y > 0.0 => dir = 1` viewport step, the `picker_scroll`
                // sign, `Engine::handle_terminal_scroll`'s "> 0 = toward live"
                // policy — was written against GTK's raw polarity and is
                // unchanged since before the #540 Relm4→ShellApp migration.
                // Pre-migration the Relm4 `connect_scroll` closure fed it GTK's
                // `dy` directly (as the retired `Msg::MouseScroll`'s payload
                // — the whole bus is gone as of #732)
                // and *separately* pushed the negated `gdk_scroll_to_uievent`
                // form onto the backend event queue. The migration deleted that
                // closure and left the runner's already-negated `UiEvent::Scroll`
                // as the only source, so every wheel notch reached the engine
                // with the sign flipped and the editor scrolled backwards.
                //
                // Only y is negated: `gdk_scroll_to_uievent` passes `dx`
                // through unchanged, so `delta.x` is already GTK-raw.
                self.handle_mouse_scroll_msg(&*backend, delta.x as f64, -(delta.y as f64));
            }
            UiEvent::WindowResized { .. } => {
                // Runner sets new line_height/char_width after resize.
                self.cached_line_height = backend.line_height() as f64;
                self.cached_char_width = backend.char_width() as f64;
                self.line_height_cell.set(self.cached_line_height);
                self.char_width_cell.set(self.cached_char_width);
                self.handle_resize();
                // #1428: forward the resize to any open terminal PTY —
                // mirrors TUI's identical `WindowResized` rung
                // (`route_terminal_resize`), a latent gap on GTK (and any
                // future win-gui/macOS backend hosting a terminal panel)
                // rather than TUI-only behaviour: nothing resized the PTY
                // on a window resize before this, so a shell running
                // inside the terminal panel kept painting at its stale
                // column/row count (`$COLUMNS`/`$LINES`, and any full-
                // screen program reading the real ioctl) after the window
                // — and therefore the panel — changed size.
                //
                // `terminal_panel_cols` off `painted_editor_content_width`
                // (not the just-updated `cached_char_width` against a live
                // pixel width `WindowResized` doesn't carry) — the same
                // "no live pixel width in scope" fallback every other
                // accelerator/menu/tick call site of `terminal_panel_cols`
                // already uses; the *next* `render_content` repaints the
                // terminal panel at the corrected geometry regardless of
                // which frame's width this resize computed against.
                let cols = self.terminal_panel_cols(self.painted_editor_content_width());
                let rows = self.engine.borrow().session.terminal_panel_rows;
                render::route_terminal_resize(&mut self.engine.borrow_mut(), cols, rows);
            }
            UiEvent::WindowClose => {
                self.show_quit_confirm();
            }
            // #593: quadraui's runner reads the system clipboard on Ctrl+V /
            // Ctrl+Shift+V / middle-click and delivers the text here,
            // unconditionally consuming the key — there is no raw KeyPressed
            // fallback to catch a paste with. `Engine::route_paste` is the
            // same focus-priority router TUI's `UiEvent::ClipboardPaste` arm
            // already calls (`tui_main/shell_app.rs`), so this one arm covers
            // the command line, search/replace fields, explorer rename, and
            // the editor buffer — see that fn's doc for the full priority
            // chain.
            UiEvent::ClipboardPaste(text) => {
                self.engine.borrow_mut().route_paste(&text);
                self.draw_needed.set(true);
            }
            _ => {}
        }

        if self.draw_needed.get() {
            self.draw_needed.set(false);
            quadraui::Reaction::Redraw
        } else {
            quadraui::Reaction::Continue
        }
    }

    /// The actual body of `ShellApp::tick` — see `handle_dispatch`'s
    /// doc comment (#813). `run_pending_native_dialog`, reachable from
    /// here via `apply_dialog_action`, can also request exit.
    pub(crate) fn tick_dispatch(
        &mut self,
        backend: &mut dyn quadraui::Backend,
    ) -> quadraui::Reaction {
        // ── Terminal chrome the runner doesn't own (#1428) ──────────────
        // Mirrors the pre-#1434 TUI shell's `tick`'s identical rung verbatim, including
        // the `self.live` gate — see `Self::live`'s own doc for why an
        // unconditional call would corrupt a `TuiBackend`-backed test
        // harness's real stdout. A no-op on GTK/macOS/Win-GUI either way
        // (`Backend::set_caret_shape`'s trait default), so gating this
        // costs nothing there.
        if self.live {
            let shape = {
                let engine = self.engine.borrow();
                render::caret_shape_for_mode(&engine, engine.sidebar_has_focus())
            };
            // #1634: only write when the shape actually changed since the
            // last tick — see `last_caret_shape`'s own doc (including its
            // "not confirmed as the flicker cause" caveat). Without this,
            // `TuiBackend::set_caret_shape` re-emitted the identical
            // DECSCUSR escape (`ratatui::crossterm::cursor::SetCursorStyle`)
            // to the real terminal on every idle poll cycle
            // (`quadraui::runtime::IDLE_POLL_CEILING`, ~250ms) for no reason
            // — real waste, independent of whether it's what the operator
            // saw flicker. The decision itself is `dedup_caret_shape`, a
            // free function so it's directly unit-tested without a
            // `Backend` — see that function's own doc for why a
            // `Backend`-call-count driver test isn't achievable here.
            if dedup_caret_shape(&mut self.last_caret_shape, shape) {
                backend.set_caret_shape(shape);
            }
        }

        // Keep cached metrics up to date.
        self.cached_line_height = backend.line_height() as f64;
        self.cached_char_width = backend.char_width() as f64;
        self.line_height_cell.set(self.cached_line_height);
        self.char_width_cell.set(self.cached_char_width);

        // Retry dropping the server-side titlebar until the runner's window
        // is mapped — see `capture_window_and_apply_csd` (#552). No-ops once
        // `csd_applied` is set.
        self.capture_window_and_apply_csd(backend);

        // Retry restoring the saved window size/position/maximized state
        // until the runner's window is mapped — see `restore_window_geometry`
        // (#1529). No-ops once `window_geometry_restored` is set.
        self.restore_window_geometry(backend);

        // Drain the actions async GTK callbacks queued for this frame.
        for action in self.deferred.drain() {
            match action {
                DeferredAction::Resize => self.handle_resize(),
                DeferredAction::ToggleFocusExplorer => self.toggle_focus_explorer(),
                DeferredAction::ToggleFocusSearch => self.toggle_focus_search(),
                DeferredAction::ToggleSidebar => self.toggle_sidebar_panel(),
                DeferredAction::ToggleTerminal => self.toggle_terminal(),
                DeferredAction::ToggleTerminalMaximize => self.toggle_terminal_maximize(),
            }
        }

        // Run a file dialog requested this frame — needs the runner-owned
        // `backend` handle for `PlatformServices` (#572). See
        // `PendingFileDialog` for why this can't happen in the
        // `open_file_dialog` / `save_workspace_as_dialog` handlers themselves.
        if let Some(req) = self.pending_file_dialog.take() {
            self.run_pending_file_dialog(req, backend);
        }

        // Run a native message dialog queued by `render_content`'s
        // edge-trigger check (#727) — same reason as the file dialog above:
        // needs the runner-owned `backend` for `PlatformServices`, which
        // `render_content`'s paint callback must not block inside.
        if let Some(opts) = self.pending_native_dialog.take() {
            self.run_pending_native_dialog(opts, backend);
        }

        // Periodic background work: LSP, DAP, git, search, etc. — also
        // where the yank-highlight deadline armed by `run_post_key_epilogue`
        // (#813) and the platform-action drain (open URL / reveal in file
        // manager, #1134) are polled, since #1248 folded both into the
        // chore list `render::run_shared_tick_chores` shares with TUI.
        self.handle_poll_tick(backend);

        // #1428: the one-shot nerd-font startup nudge — mirrors
        // the pre-#1434 TUI shell's `tick`'s identical drain (`pending_startup_msg`),
        // unconditional on `Self::live`, unlike the caret-shape write
        // above: this only ever writes to `engine.message`, never touches
        // the real terminal, so there is nothing here a test harness needs
        // protecting from. Always `None` on a GUI-backend `App` (see
        // `Self::pending_startup_msg`'s own doc), so this is a no-op there.
        if let Some(msg) = self.pending_startup_msg.take() {
            self.engine.borrow_mut().message = msg;
            self.draw_needed.set(true);
        }

        // #1508: while any ACP session (or the direct-curl transport, which
        // also flips `ai_streaming`) has a turn in flight, keep re-arming
        // `tick` at the cadence `quadraui::runner::Reaction::RedrawAfter`'s
        // own doc recommends for exactly this case (its "thinking-spinner
        // countdown" example) — 100ms, i.e. ≥4 Hz — rather than trusting
        // the coarser 250ms `IDLE_POLL_CEILING` fallback every backend
        // keeps regardless. `Engine::tick_ai_spinner` (called from
        // `poll_idle` above, inside `handle_poll_tick`) already advanced
        // the frame this tick and set `draw_needed`, so the `Redraw` below
        // paints the new frame now; this re-arms the *next* wake.
        if self
            .engine
            .borrow()
            .acp_sessions
            .iter()
            .any(|s| s.ai_streaming)
        {
            backend.request_frame_in(std::time::Duration::from_millis(100));
        }

        // #1668: while any terminal pane has a live PTY session, keep
        // re-arming `tick` so `Engine::poll_terminal` keeps draining its
        // output — see `terminal_poll_rearm_delay`'s own doc for why this
        // matters specifically (and only) on Win-GUI, and why it's a
        // harmless no-op re-arm on GTK/TUI/macOS (they already tick
        // regardless, via their own `IDLE_POLL_CEILING` fallback).
        if let Some(delay) =
            terminal_poll_rearm_delay(!self.engine.borrow().terminal_panes.is_empty())
        {
            backend.request_frame_in(delay);
        }

        if self.draw_needed.get() {
            self.draw_needed.set(false);
            quadraui::Reaction::Redraw
        } else {
            quadraui::Reaction::Continue
        }
    }
}

impl App {
    /// The logic behind [`Self::on_shell_event_ctx`], the real
    /// (non-deprecated, `ShellContext`-aware) hook the runner calls.
    ///
    /// #1491 pulled this out of what used to be `App`'s override of the
    /// deprecated ctx-less `ShellApp::on_shell_event` (issue #617's
    /// predecessor) and into this plain inherent method, so
    /// `on_shell_event_ctx` calls it directly instead of dispatching through
    /// the deprecated trait method — `App` no longer overrides
    /// `on_shell_event` at all (the trait's own no-op default applies),
    /// which was the last deprecation-lint suppression this file needed
    /// once the tab-bar hit-testing migration cleared the others.
    pub(crate) fn dispatch_shell_event(&mut self, event: &quadraui::AppShellEvent) {
        use quadraui::AppShellEvent;
        // #1062: the shadow-`engine.app_shell` sync, unconditionally and
        // first — see `render::sync_shell_event_shadow`'s rung comment for
        // why this call has to come before any of the id-specific branching
        // below rather than be repeated inside each arm.
        {
            let mut engine = self.engine.borrow_mut();
            render::sync_shell_event_shadow(event, &mut engine);
        }
        match event {
            AppShellEvent::PanelChanged { panel_id } => {
                // #1427: the hamburger panel only exists on the `cell`
                // profile (`Self::shell_config`) — reveal the menu bar
                // instead of switching to a nonexistent shadow panel.
                // Mirrors the pre-#1434 TUI shell's `on_shell_event`'s own
                // hamburger arm, now shared via
                // `render::route_hamburger_panel_changed`; see its own doc.
                // A no-op check on GTK/macOS/Win, which never register this
                // panel id in the first place.
                if render::route_hamburger_panel_changed(&mut self.engine.borrow_mut(), panel_id) {
                    self.last_shell_panel = Some(panel_id.clone());
                    return;
                }
                // #1064: record what the runner's own `AppShell` now
                // believes is active, whether this notification came from
                // a real click or from `take_requested_panel`'s own echo
                // below — see `Self::last_shell_panel`'s doc.
                self.last_shell_panel = Some(panel_id.clone());
                if std::mem::take(&mut self.suppress_shell_panel_echo) {
                    // Echo of our own `take_requested_panel` reconciliation:
                    // the engine already holds this state (an app-initiated
                    // switch, e.g. a DAP reveal or a panel-focus keyboard
                    // accelerator) — re-running `switch_panel` below would
                    // toggle an already-active `ext:` panel back **off**
                    // (`render::apply_activity_panel_switch`'s
                    // `already_showing` arm treats a second "click" on the
                    // active plugin panel as a close).
                    return;
                }
                // #557: plugin-provided panels are now real `PanelDefinition`s
                // in the runner's `AppShell` (`build_shell_config`), so their
                // icon clicks arrive here like any built-in panel's. They are
                // *not* engine-`AppShell` panels though — `render_content`
                // dispatches on `engine.ext_panel_active`, which
                // `sync_shell_event_shadow` deliberately leaves untouched for
                // an `ext:` id (see that function's doc) — so route them
                // through the existing `switch_panel` handler that owns the
                // ext-panel focus/toggle bookkeeping.
                if is_ext_panel_id(panel_id.as_str()) {
                    // #1427: a real activity-bar click is exactly as much
                    // "the user moved on" as a keystroke — spend the guard
                    // (mirrors TUI's identical call in this arm). A no-op
                    // when never armed (GTK/macOS/Win).
                    render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut());
                    self.switch_panel(panel_id.as_str().to_string());
                    return;
                }
                render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut());
                // #1360: a built-in panel's activity-bar icon click must move
                // keyboard focus into that panel, exactly as TUI's own
                // `PanelChanged` arm does (`focus_sidebar_panel` +
                // `sidebar.has_focus = true` in the pre-#1434 TUI shell's `on_shell_event`)
                // — before this, GTK only redrew, so `render::route_focus_key`
                // (which every keystroke passes through, see
                // `Self::handle_key_press`'s "Shared focus-owner keyboard
                // rung") kept reading `sidebar_has_focus() == false` and sent
                // every subsequent key straight to the editor. `sidebar.
                // has_focus`/`ext_panel_name` have no GTK equivalent to set —
                // GTK's `route_focus_key` call passes
                // `engine.sidebar_has_focus()` itself as the "band" (see
                // that call site's own comment), so the one engine call
                // below is the whole fix, and it is the same
                // already-shared `Engine::focus_sidebar_panel` this method's
                // own `toggle_focus_search`/`toggle_focus_explorer` already
                // call for the keyboard-accelerator path.
                self.engine
                    .borrow_mut()
                    .focus_sidebar_panel(panel_id.as_str());
                self.draw_needed.set(true);
            }
            AppShellEvent::SidebarHidden => {
                // #1427: a real panel's own second click — the hamburger's
                // is intercepted by `Self::on_shell_event_ctx` before it
                // ever reaches this ctx-less method (see that method's own
                // doc) — so spend the guard here too (mirrors TUI).
                render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut());
                // #557: this is also how a *second* click on an open
                // extension panel's icon arrives — `sync_shell_event_shadow`
                // already dropped the plugin panel's claim (its
                // `SidebarHidden` arm clears the same two fields
                // unconditionally). Re-opening still works:
                // `AppShell::handle_activity_click` reports a click on the
                // active panel as `PanelChanged`, not `SidebarHidden`, once
                // the sidebar is hidden.
                //
                // #1427: `Engine::clear_sidebar_focus()` — a real second
                // click must clear the sidebar's own keyboard focus the
                // same way the keyboard-driven `ActivityBarKeyAction::
                // Collapse` arm above already does (via the wider
                // `Engine::collapse_sidebar`, which also touches
                // `session.explorer_visible` — not appropriate here, since
                // this arm fires for *any* panel's close, not just
                // Explorer's), or a subsequent keystroke keeps routing to
                // whichever sidebar panel last held focus instead of the
                // editor. A pre-existing gap on every backend (this arm
                // never cleared focus before), invisible until #1427's own
                // `App`-on-TUI driver tests started clicking a real panel's
                // icon twice in a row (reveal, then collapse) — the
                // pre-#1427 "tui" arm never needed a first click at all,
                // since Explorer was already the runner's default-active
                // panel.
                self.engine.borrow_mut().clear_sidebar_focus();
                self.draw_needed.set(true);
            }
            AppShellEvent::SidebarResized { .. } => {
                render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut());
            }
            AppShellEvent::BottomItemClicked { id } => {
                render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut());
                // The runner treats bottom activity-bar items as action
                // buttons (not sidebar panels), so it never toggles or
                // hides on its own — it only ever reports the click.
                // #1057: this used to unconditionally `show_panel`, so a
                // second click on an already-open bottom item (e.g.
                // "bottom:settings") re-showed it instead of collapsing the
                // sidebar like VS Code does for an active-tab click — while
                // the old, hand-rolled the pre-#1434 TUI shell's `on_shell_event` (same
                // arm) already ran the toggle. Route through `switch_panel`,
                // the same shared `render::apply_activity_panel_switch`
                // call site `PanelChanged`'s ext-panel arm above already
                // uses, so both backends make the identical toggle decision
                // from one place instead of drifting again. #1433: this is
                // no longer "two arms that happen to agree" — `App` is what
                // TUI dispatches through in production now too, so this one
                // arm *is* both backends' Settings-bottom-item-toggle
                // behaviour; there was no remaining fork to reconcile when
                // auditing this for the flip.
                self.switch_panel(id.as_str().to_string());
            }
            // #1427: every remaining `AppShellEvent` variant is likewise a
            // shell-consumed user interaction — spend the guard and let any
            // variant quadraui adds later default to the safe direction
            // (mirrors TUI's identical catch-all).
            _ => render::disarm_hamburger_stale_click_guard(&mut self.engine.borrow_mut()),
        }
    }
}

/// #1549: the macOS report ("command-line row below the status bar is
/// clipped to about half its height by the window's bottom edge")
/// reduces to one portable geometric invariant on the painted rect: it
/// must fit entirely inside the real window content area (`viewport`,
/// the exact portable equivalent of AppKit's `contentView.bounds`
/// on every backend — see `quadraui::Backend::viewport`) and be at
/// least one full line tall.
///
/// This should hold on *every* backend by construction: quadraui's
/// `compose::app_shell::compute_layout` derives `AppShellLayout::
/// main_content_bounds` from this exact `viewport` (title bar carved off
/// the top). #1877: `shell_config` now also opts into quadraui's own
/// `with_command_line()`/`with_status_bar()` bands — but only so
/// `activity_bar_bounds`/`sidebar_*_bounds` stop above them; `h` here is
/// `render::main_content_true_height(layout)`, which adds that
/// reservation straight back, so `render_content`'s own `status_bar_h`/
/// `cmd_y` locals still lay both rows out as if nothing were reserved
/// below `main_content_bounds`, and `cmd_y + lh` always resolves to
/// `main.y + h`, i.e. `viewport.height` (see the `FrameOp::CommandLine`
/// arm below). A live macOS run is the one environment this fleet cannot
/// check that construction against directly — no macOS cross-toolchain is
/// installed (`src/macos/mod.rs`'s "Verifying this file without a Mac").
/// `debug_assert!` (not a hard `assert!`) so a violation surfaces loudly
/// in a debug build's log/console — exactly the signal a future live-
/// macOS debug run needs to confirm or rule out this shape of bug —
/// instead of crashing a release build over a cosmetic clip.
pub(crate) fn debug_assert_command_line_fits_viewport(
    cmd_rect: quadraui::Rect,
    viewport: quadraui::Viewport,
    line_height: f64,
) {
    debug_assert!(
        (cmd_rect.y + cmd_rect.height) as f64 <= viewport.height as f64 + 0.5,
        "#1549: command-line row bottom ({}) exceeds the real window's \
         content height ({}) — the row will be clipped by the window edge",
        cmd_rect.y + cmd_rect.height,
        viewport.height,
    );
    debug_assert!(
        cmd_rect.height as f64 + 0.5 >= line_height,
        "#1549: command-line row height ({}) is shorter than one full \
         line ({})",
        cmd_rect.height,
        line_height,
    );
}

#[cfg(test)]
mod command_line_viewport_geometry_tests {
    //! #1549: coverage for `debug_assert_command_line_fits_viewport` in
    //! isolation — no live window (or even a constructed `App`) needed,
    //! since the check is a pure function of the three values the bug
    //! report's own "next step" asked to be logged. `cargo test`'s
    //! default debug profile keeps `debug_assert!` live, so these panics
    //! are real, observable failures, not silently-compiled-out no-ops.

    use super::*;

    fn viewport(height: f32) -> quadraui::Viewport {
        quadraui::Viewport::new(800.0, height, 1.0)
    }

    /// The exact shape `render_content`'s `FrameOp::CommandLine` arm
    /// produces on a healthy frame: the row ends precisely at the
    /// viewport's bottom edge. Must not panic.
    #[test]
    fn row_flush_with_viewport_bottom_does_not_panic() {
        let rect = quadraui::Rect::new(0.0, 576.0, 800.0, 24.0);
        debug_assert_command_line_fits_viewport(rect, viewport(600.0), 24.0);
    }

    /// RED-verified: with the assertion's first check removed, this test
    /// still passes (proving it *can* fail) — see the reasoning in the
    /// function's own doc for why this shape is exactly #1549's report.
    /// Mirrors a real window whose usable content height is ~12px
    /// shorter than the layout assumed (half of a 24px line).
    #[test]
    #[should_panic(expected = "#1549: command-line row bottom")]
    fn row_extending_past_viewport_bottom_panics() {
        let rect = quadraui::Rect::new(0.0, 576.0, 800.0, 24.0);
        debug_assert_command_line_fits_viewport(rect, viewport(588.0), 24.0);
    }

    /// The second, independent invariant: a row shorter than one full
    /// line (even if it does fit inside the viewport) still reproduces
    /// the reported symptom — only part of a line of glyphs has room to
    /// paint.
    #[test]
    #[should_panic(expected = "is shorter than one full")]
    fn row_shorter_than_line_height_panics() {
        let rect = quadraui::Rect::new(0.0, 576.0, 800.0, 12.0);
        debug_assert_command_line_fits_viewport(rect, viewport(600.0), 24.0);
    }
}
