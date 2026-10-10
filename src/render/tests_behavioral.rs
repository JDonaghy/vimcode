use super::*;

/// Test that `open_file_preview` reuses/creates a preview tab, NOT replacing
/// a permanent buffer. This is the contract for explorer single-click.
#[test]
fn test_open_file_preview_does_not_replace_permanent() {
    let mut e = test_engine("first file\n");
    let dir = crate::harness::scratch_dir("vimcode_test_preview");
    let _ = std::fs::create_dir_all(&dir);
    let f1 = dir.join("a.txt");
    let f2 = dir.join("b.txt");
    std::fs::write(&f1, "file A\n").unwrap();
    std::fs::write(&f2, "file B\n").unwrap();

    // Open f1 permanently (simulates existing tab)
    e.open_file_in_tab(&f1);
    let buf_a = e.active_buffer_id();
    assert_eq!(e.active_group().tabs.len(), 2); // scratch + f1

    // Preview f2 (simulates explorer single-click)
    e.open_file_preview(&f2);
    let buf_b = e.active_buffer_id();
    assert_ne!(buf_a, buf_b, "Preview should show different buffer");
    assert_eq!(
        e.active_group().tabs.len(),
        3,
        "Preview should create a new tab, not replace"
    );

    // Preview another file — should reuse the preview tab
    let f3 = dir.join("c.txt");
    std::fs::write(&f3, "file C\n").unwrap();
    e.open_file_preview(&f3);
    assert_eq!(
        e.active_group().tabs.len(),
        3,
        "Second preview should reuse the preview tab"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

/// Test that `open_file_in_tab` always creates a new tab (or switches to
/// existing). This is the contract for explorer double-click / Enter.
#[test]
fn test_open_file_in_tab_creates_new_tab() {
    let mut e = test_engine("scratch\n");
    let dir = crate::harness::scratch_dir("vimcode_test_tab");
    let _ = std::fs::create_dir_all(&dir);
    let f1 = dir.join("a.txt");
    let f2 = dir.join("b.txt");
    std::fs::write(&f1, "file A\n").unwrap();
    std::fs::write(&f2, "file B\n").unwrap();

    let initial_tabs = e.active_group().tabs.len();
    e.open_file_in_tab(&f1);
    assert_eq!(e.active_group().tabs.len(), initial_tabs + 1);
    e.open_file_in_tab(&f2);
    assert_eq!(e.active_group().tabs.len(), initial_tabs + 2);

    // Opening f1 again should switch to existing tab, not create another
    e.open_file_in_tab(&f1);
    assert_eq!(e.active_group().tabs.len(), initial_tabs + 2);

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

/// Test that the default shell is platform-appropriate.
#[test]
fn test_default_shell_platform() {
    let shell = crate::core::terminal::default_shell();
    #[cfg(target_os = "windows")]
    {
        // Must NOT be /bin/bash on Windows
        assert!(
            !shell.contains("/bin/bash"),
            "Windows default shell should not be /bin/bash, got: {shell}"
        );
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Should be $SHELL or /bin/bash on Unix
        assert!(
            shell.contains("sh") || shell.contains("zsh") || shell.contains("fish"),
            "Unix default shell should be a known shell, got: {shell}"
        );
    }
}

// =====================================================================
// Phase 2d: Behavioral backend parity tests
//
// These tests simulate user interaction sequences (the same engine method
// calls that every backend must make) and assert that the engine state
// transitions are correct.  A bug here means every backend is broken;
// a missing engine call in a specific backend would pass these tests but
// fail the Phase 2c static parity check.
// =====================================================================

/// Tab click switches to the correct tab and promotes preview tabs.
#[test]
fn test_behavior_tab_click_switches_tab() {
    let mut e = test_engine("first\n");
    let dir = crate::harness::scratch_dir("vimcode_test_tab_click");
    let _ = std::fs::create_dir_all(&dir);
    let f1 = dir.join("a.txt");
    let f2 = dir.join("b.txt");
    std::fs::write(&f1, "file A\n").unwrap();
    std::fs::write(&f2, "file B\n").unwrap();

    e.open_file_in_tab(&f1);
    e.open_file_in_tab(&f2);
    // Now on tab 2 (f2).  Switch back to tab 0 (scratch).
    e.goto_tab(0);
    assert_eq!(
        e.active_group().active_tab,
        0,
        "goto_tab(0) should switch to first tab"
    );

    // Switch to tab 1 (f1)
    e.goto_tab(1);
    assert_eq!(e.active_group().active_tab, 1);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Tab close removes the tab and falls back to an adjacent tab.
#[test]
fn test_behavior_tab_close_removes_tab() {
    let mut e = test_engine("scratch\n");
    let dir = crate::harness::scratch_dir("vimcode_test_tab_close");
    let _ = std::fs::create_dir_all(&dir);
    let f1 = dir.join("a.txt");
    std::fs::write(&f1, "file A\n").unwrap();

    e.open_file_in_tab(&f1);
    assert_eq!(e.active_group().tabs.len(), 2);

    // Close the active tab (f1) — should fall back to scratch
    e.close_tab();
    assert_eq!(
        e.active_group().tabs.len(),
        1,
        "close_tab should remove the tab"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Backends must check dirty() before calling close_tab().
/// This test verifies that dirty() detects unsaved changes so backends
/// can show a confirmation dialog.  close_tab() itself is a raw operation.
#[test]
fn test_behavior_dirty_check_before_tab_close() {
    let mut e = test_engine("");
    let dir = crate::harness::scratch_dir("vimcode_test_dirty_close");
    let _ = std::fs::create_dir_all(&dir);
    let f1 = dir.join("a.txt");
    std::fs::write(&f1, "original\n").unwrap();

    e.open_file_in_tab(&f1);
    assert!(!e.dirty(), "freshly opened file should not be dirty");

    // Make the buffer dirty by inserting text
    e.handle_key("i", Some('i'), false);
    e.handle_key("x", Some('x'), false);
    e.handle_key("Escape", None, false);
    assert!(
        e.dirty(),
        "buffer should be dirty after insert — backends must check this before close_tab()"
    );

    // Verify the backend contract: if dirty() is true, do NOT call
    // close_tab() directly — show a dialog first.  We verify the raw
    // close_tab still works (backends call it after user confirms).
    let tabs_before = e.active_group().tabs.len();
    e.close_tab();
    assert_eq!(
        e.active_group().tabs.len(),
        tabs_before - 1,
        "close_tab() is a raw operation — backends gate it with dirty() check"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Context menu open/select/dismiss lifecycle.
#[test]
fn test_behavior_context_menu_lifecycle() {
    let mut e = test_engine("hello world\n");

    // Open editor context menu
    e.open_editor_context_menu(10, 5);
    assert!(
        e.context_menu.is_some(),
        "open_editor_context_menu should populate context_menu state"
    );
    let items_count = e.context_menu.as_ref().unwrap().items.len();
    assert!(items_count > 0, "context menu should have items");

    // Dismiss by clicking outside
    e.close_context_menu();
    assert!(
        e.context_menu.is_none(),
        "close_context_menu should clear the state"
    );

    // Open again and confirm an item
    e.open_editor_context_menu(10, 5);
    assert!(e.context_menu.is_some());
    let _action = e.context_menu_confirm();
    // After confirm, the menu should be closed
    assert!(
        e.context_menu.is_none(),
        "context_menu_confirm should close the menu"
    );
}

/// Explorer context menu opens with correct target type.
#[test]
fn test_behavior_explorer_context_menu() {
    let mut e = test_engine("");
    let dir = crate::harness::scratch_dir("vimcode_test_ctx_explorer");
    let _ = std::fs::create_dir_all(&dir);
    let f1 = dir.join("test.txt");
    std::fs::write(&f1, "content\n").unwrap();

    e.open_explorer_context_menu(f1.clone(), false, 5, 10);
    assert!(e.context_menu.is_some());
    let ctx = e.context_menu.as_ref().unwrap();
    assert!(
        matches!(
            ctx.target,
            crate::core::engine::ContextMenuTarget::ExplorerFile { .. }
        ),
        "file click should produce ExplorerFile target"
    );

    e.close_context_menu();

    // Directory context menu
    e.open_explorer_context_menu(dir.clone(), true, 5, 10);
    assert!(e.context_menu.is_some());
    let ctx = e.context_menu.as_ref().unwrap();
    assert!(
        matches!(
            ctx.target,
            crate::core::engine::ContextMenuTarget::ExplorerDir { .. }
        ),
        "dir click should produce ExplorerDir target"
    );

    e.close_context_menu();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Tab context menu opens with correct target.
#[test]
fn test_behavior_tab_context_menu() {
    let mut e = test_engine("hello\n");
    let gid = e.active_group;
    e.open_tab_context_menu(gid, 0, 20, 5);
    assert!(e.context_menu.is_some());
    let ctx = e.context_menu.as_ref().unwrap();
    assert!(
        matches!(
            ctx.target,
            crate::core::engine::ContextMenuTarget::Tab { .. }
        ),
        "tab right-click should produce Tab target"
    );
    e.close_context_menu();
}

/// Double-click in editor selects a word (enters visual mode).
#[test]
fn test_behavior_editor_double_click_selects_word() {
    let mut e = test_engine("hello world\n");
    let wid = e.active_window_id();
    e.mouse_double_click(wid, 0, 2); // double-click on "hello"
    assert_eq!(
        e.mode,
        crate::core::mode::Mode::Visual,
        "double-click should enter visual mode"
    );
}

/// Editor hover popup lifecycle: show → focus → scroll → dismiss.
#[test]
fn test_behavior_editor_hover_lifecycle() {
    let mut e = test_engine("fn main() {}\n");

    // Show a hover popup
    e.show_editor_hover(
        0,
        3,
        "**main** — entry point",
        crate::core::engine::EditorHoverSource::Lsp,
        false,
        false,
    );
    assert!(
        e.editor_hover.is_some(),
        "show_editor_hover should set editor_hover"
    );
    assert!(
        !e.editor_hover_has_focus,
        "hover should not auto-focus without take_focus"
    );

    // Focus the popup (simulates click on hover)
    e.editor_hover_focus();
    assert!(
        e.editor_hover_has_focus,
        "editor_hover_focus should set focus flag"
    );

    // Scroll the popup
    let scrolled = e.editor_hover_scroll(1);
    // Scroll may or may not change offset depending on content length,
    // but the method should not panic
    let _ = scrolled;

    // Dismiss
    e.dismiss_editor_hover();
    assert!(
        e.editor_hover.is_none(),
        "dismiss_editor_hover should clear popup"
    );
    assert!(!e.editor_hover_has_focus, "dismiss should clear focus flag");
}

/// Activity bar click toggles sidebar focus flags.
#[test]
fn test_behavior_sidebar_focus_toggle() {
    let mut e = test_engine("hello\n");

    // Simulate activity bar click → explorer
    e.explorer_has_focus = true;
    assert!(e.explorer_has_focus);

    // Simulate clicking editor → clear sidebar focus
    e.clear_sidebar_focus();
    assert!(
        !e.explorer_has_focus,
        "clear_sidebar_focus should clear explorer"
    );
    assert!(!e.search_has_focus);
    assert!(!e.sc_has_focus);
    assert!(!e.settings_has_focus);
    assert!(!e.ai_has_focus);
}

/// Terminal new tab / close tab lifecycle.
#[test]
fn test_behavior_terminal_new_and_close() {
    let mut e = test_engine("hello\n");

    // Create a terminal tab
    e.terminal_new_tab(80, 24);
    assert!(
        !e.terminal_panes.is_empty(),
        "terminal_new_tab should create a tab"
    );
    let count_after_new = e.terminal_panes.len();

    // Create another
    e.terminal_new_tab(80, 24);
    assert_eq!(e.terminal_panes.len(), count_after_new + 1);

    // Close active tab
    e.terminal_close_active_tab();
    assert_eq!(e.terminal_panes.len(), count_after_new);
}

/// Terminal split toggle.
#[test]
fn test_behavior_terminal_split_toggle() {
    let mut e = test_engine("hello\n");
    e.terminal_new_tab(80, 24);
    assert!(!e.terminal_split, "split should be off initially");

    e.terminal_toggle_split(80, 24);
    assert!(e.terminal_split, "toggle should enable split");

    e.terminal_toggle_split(80, 24);
    assert!(!e.terminal_split, "second toggle should disable split");
}

/// #758 / #471 (resize half): a window resize must not reflow a *split*
/// terminal's panes to the full panel width.
///
/// `Engine::terminal_resize` — what `UiEvent::WindowResized` used to call
/// directly — resizes every pane to the panel width, so both halves of a
/// split silently became as wide as the whole panel while still being
/// painted into their narrow rects; anything typed past the painted
/// boundary wrapped out of view. `route_terminal_resize` splits the width
/// instead. Asserts on the PTY's own reported `cols()`, which is what the
/// painter reads back (`build_terminal_panel` -> `TerminalPanel::
/// content_cols`).
#[test]
fn route_terminal_resize_keeps_a_split_split() {
    let mut e = test_engine("hello\n");
    e.terminal_new_tab(80, 24);
    e.terminal_toggle_split(80, 24);
    assert!(e.terminal_split, "fixture must be in split mode");
    assert_eq!(e.terminal_panes.len(), 2, "split must have two panes");

    route_terminal_resize(&mut e, 100, 24);
    assert_eq!(
        (
            e.terminal_panes[0].session.cols(),
            e.terminal_panes[1].session.cols()
        ),
        (50, 50),
        "a 100-column panel must be split between the two panes, not \
             handed to each of them in full (#471)"
    );

    // An in-progress divider drag pins the left width; the right pane
    // takes the remainder.
    e.terminal_split_set_drag_cols(30);
    route_terminal_resize(&mut e, 100, 24);
    assert_eq!(
        (
            e.terminal_panes[0].session.cols(),
            e.terminal_panes[1].session.cols()
        ),
        (30, 70),
        "a dragged divider width must survive a resize"
    );

    // Not split: every pane gets the whole panel, as before.
    e.terminal_split_set_drag_cols(0);
    e.terminal_close_split(100, 24);
    route_terminal_resize(&mut e, 90, 24);
    assert!(
        e.terminal_panes.iter().all(|s| s.session.cols() == 90),
        "outside split mode every pane still takes the full panel width"
    );
}

/// Tab drag and drop between groups creates a new split.
#[test]
fn test_behavior_tab_drag_drop_creates_split() {
    let mut e = test_engine("scratch\n");
    let dir = crate::harness::scratch_dir("vimcode_test_drag_drop");
    let _ = std::fs::create_dir_all(&dir);
    let f1 = dir.join("a.txt");
    std::fs::write(&f1, "file A\n").unwrap();

    e.open_file_in_tab(&f1);
    assert_eq!(e.editor_groups.len(), 1, "start with one group");

    let gid = e.active_group;
    // Move tab 1 (f1) to create a vertical split
    e.move_tab_to_new_split(
        gid,
        1,
        gid,
        crate::core::window::SplitDirection::Vertical,
        false,
    );
    assert_eq!(
        e.editor_groups.len(),
        2,
        "dropping into a split should create a second editor group"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Preview tab is promoted to permanent when goto_tab selects it.
#[test]
fn test_behavior_goto_tab_promotes_preview() {
    let mut e = test_engine("scratch\n");
    let dir = crate::harness::scratch_dir("vimcode_test_promote");
    let _ = std::fs::create_dir_all(&dir);
    let f1 = dir.join("a.txt");
    std::fs::write(&f1, "file A\n").unwrap();

    e.open_file_preview(&f1);
    let preview_buf = e.active_buffer_id();
    assert!(
        e.preview_buffer_id == Some(preview_buf),
        "open_file_preview should set preview_buffer_id"
    );

    // Switch away and back via goto_tab (simulates clicking the tab)
    let tab_idx = e.active_group().active_tab;
    e.goto_tab(0);
    e.goto_tab(tab_idx);
    assert!(
        e.preview_buffer_id.is_none() || e.preview_buffer_id != Some(preview_buf),
        "goto_tab should promote preview to permanent"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Editor click clears sidebar focus (backends must call clear_sidebar_focus).
#[test]
fn test_behavior_editor_click_clears_sidebar() {
    let mut e = test_engine("hello world\n");

    // Simulate various sidebar focus states
    e.explorer_has_focus = true;
    e.search_has_focus = true;
    e.sc_has_focus = true;
    e.ai_has_focus = true;
    e.settings_has_focus = true;

    // Simulate what backends do on editor click: clear sidebar, then click
    e.clear_sidebar_focus();
    let wid = e.active_window_id();
    e.mouse_click(wid, 0, 3);

    assert!(!e.explorer_has_focus);
    assert!(!e.search_has_focus);
    assert!(!e.sc_has_focus);
    assert!(!e.ai_has_focus);
    assert!(!e.settings_has_focus);
}

/// mouse_click moves cursor to the clicked position.
#[test]
fn test_behavior_mouse_click_moves_cursor() {
    let mut e = test_engine("hello world\nsecond line\n");
    let wid = e.active_window_id();
    e.mouse_click(wid, 1, 3);
    assert_eq!(e.cursor().line, 1, "click should move to line 1");
    assert_eq!(e.cursor().col, 3, "click should move to col 3");
}

/// Preview reuse: opening multiple previews reuses the same tab slot.
#[test]
fn test_behavior_preview_reuse_then_permanent() {
    let mut e = test_engine("scratch\n");
    let dir = crate::harness::scratch_dir("vimcode_test_preview_reuse");
    let _ = std::fs::create_dir_all(&dir);
    let f1 = dir.join("a.txt");
    let f2 = dir.join("b.txt");
    let f3 = dir.join("c.txt");
    std::fs::write(&f1, "A\n").unwrap();
    std::fs::write(&f2, "B\n").unwrap();
    std::fs::write(&f3, "C\n").unwrap();

    // Preview f1
    e.open_file_preview(&f1);
    assert_eq!(e.active_group().tabs.len(), 2);

    // Preview f2 — should reuse the preview slot
    e.open_file_preview(&f2);
    assert_eq!(e.active_group().tabs.len(), 2, "preview should reuse slot");

    // Open f3 permanently — should create a new tab
    e.open_file_in_tab(&f3);
    assert_eq!(
        e.active_group().tabs.len(),
        3,
        "permanent open after preview should add a tab"
    );

    // Opening yet another preview should still reuse the preview slot
    e.open_file_preview(&f1);
    // The key invariant: at most one preview buffer exists at a time
    // preview_buffer_id tracks it (None means no preview, Some means exactly one)
    assert!(
        e.preview_buffer_id.is_some(),
        "should have a preview buffer after open_file_preview"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
