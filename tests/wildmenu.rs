mod common;
use common::*;
use vimcode_core::Mode;

// ── Single match auto-completes ─────────────────────────────────────────

#[test]
fn tab_single_match_completes() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "Explor");
    press_key(&mut e, "Tab");

    assert_eq!(e.command_buffer, "Explore");
    // No wildmenu shown for single match
    assert!(e.wildmenu_items.is_empty());
}

// ── Multiple matches show wildmenu ──────────────────────────────────────

#[test]
fn tab_multiple_matches_shows_wildmenu() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "Exp");
    press_key(&mut e, "Tab");

    // Should show wildmenu with Explore
    // "Exp" matches "Explore" and "Ex" should not match since "Ex" doesn't start with "Exp"
    // Actually "Exp" only matches "Explore", so single match
    assert_eq!(e.command_buffer, "Explore");
}

#[test]
fn tab_shows_wildmenu_for_multiple() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "Ext");
    press_key(&mut e, "Tab");

    // "Ext" matches ExtInstall, ExtList, ExtEnable, ExtDisable, ExtRemove, ExtRefresh
    assert!(!e.wildmenu_items.is_empty(), "wildmenu should have items");
    assert!(
        e.wildmenu_items.iter().all(|i| i.starts_with("Ext")),
        "all items should start with Ext"
    );
}

// ── Tab cycles through items ────────────────────────────────────────────

#[test]
fn tab_cycles_through_items() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "Ext");
    press_key(&mut e, "Tab"); // opens wildmenu

    let items = e.wildmenu_items.clone();
    assert!(items.len() > 1);

    // First Tab should select index 0
    press_key(&mut e, "Tab");
    assert_eq!(e.wildmenu_selected, Some(0));
    assert_eq!(e.command_buffer, items[0]);

    // Second Tab should select index 1
    press_key(&mut e, "Tab");
    assert_eq!(e.wildmenu_selected, Some(1));
    assert_eq!(e.command_buffer, items[1]);
}

// ── Shift-Tab cycles backwards ──────────────────────────────────────────

#[test]
fn shift_tab_cycles_backwards() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "Ext");
    press_key(&mut e, "Tab"); // opens wildmenu

    let items = e.wildmenu_items.clone();

    // Tab to select first item
    press_key(&mut e, "Tab");
    assert_eq!(e.wildmenu_selected, Some(0));

    // Shift-Tab should wrap to last item
    e.handle_key("ISO_Left_Tab", None, false);
    assert_eq!(e.wildmenu_selected, Some(items.len() - 1));
    assert_eq!(e.command_buffer, items[items.len() - 1]);
}

// ── Tab wraps around ────────────────────────────────────────────────────

#[test]
fn tab_wraps_around() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "Ext");
    press_key(&mut e, "Tab"); // opens wildmenu

    let count = e.wildmenu_items.len();

    // Tab through all items plus one more
    for _ in 0..count + 1 {
        press_key(&mut e, "Tab");
    }
    // Should wrap back to index 0
    assert_eq!(e.wildmenu_selected, Some(0));
}

// ── Typing clears wildmenu ──────────────────────────────────────────────

#[test]
fn typing_clears_wildmenu() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "Ext");
    press_key(&mut e, "Tab"); // opens wildmenu
    assert!(!e.wildmenu_items.is_empty());

    // Type a character — should dismiss wildmenu
    press(&mut e, 'I');
    assert!(e.wildmenu_items.is_empty());
}

// ── Backspace clears wildmenu ───────────────────────────────────────────

#[test]
fn backspace_clears_wildmenu() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "Ext");
    press_key(&mut e, "Tab");
    assert!(!e.wildmenu_items.is_empty());

    press_key(&mut e, "BackSpace");
    assert!(e.wildmenu_items.is_empty());
}

// ── Escape clears wildmenu ──────────────────────────────────────────────

#[test]
fn escape_clears_wildmenu() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "Ext");
    press_key(&mut e, "Tab");
    assert!(!e.wildmenu_items.is_empty());

    press_key(&mut e, "Escape");
    assert!(e.wildmenu_items.is_empty());
    assert_mode(&e, Mode::Normal);
}

// ── Return executes and clears wildmenu ─────────────────────────────────

#[test]
fn return_clears_wildmenu_and_executes() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "Ext");
    press_key(&mut e, "Tab"); // opens wildmenu
    press_key(&mut e, "Tab"); // select first

    press_key(&mut e, "Return");
    assert!(e.wildmenu_items.is_empty());
    assert_mode(&e, Mode::Normal);
}

// ── Common prefix expansion ─────────────────────────────────────────────

#[test]
fn common_prefix_expanded() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "tab");
    press_key(&mut e, "Tab");

    // "tab" should expand to "tab" (common prefix of tabnew, tabnext, tabprev, tabclose, tabmove)
    // The common prefix is "tab" itself since they diverge after that
    assert!(e.command_buffer.starts_with("tab"));
    // Wildmenu should be open since there are multiple matches
    assert!(!e.wildmenu_items.is_empty());
}

// ── No matches = no wildmenu ────────────────────────────────────────────

#[test]
fn no_matches_no_wildmenu() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "zzzzz");
    press_key(&mut e, "Tab");

    assert!(e.wildmenu_items.is_empty());
    assert!(e.wildmenu_selected.is_none());
}

// ── Empty command = no wildmenu ─────────────────────────────────────────

#[test]
fn empty_command_no_wildmenu() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    press_key(&mut e, "Tab");

    assert!(e.wildmenu_items.is_empty());
}

// ── Expanded command list includes new commands ─────────────────────────

#[test]
fn command_list_includes_git_commands() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "G");
    press_key(&mut e, "Tab");

    // Should have git commands
    assert!(
        e.wildmenu_items.iter().any(|i| i == "Gdiff"),
        "should include Gdiff"
    );
    assert!(
        e.wildmenu_items.iter().any(|i| i == "Gblame"),
        "should include Gblame"
    );
}

#[test]
fn command_list_includes_lsp_commands() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "Lsp");
    press_key(&mut e, "Tab");

    assert!(!e.wildmenu_items.is_empty());
    assert!(
        e.wildmenu_items.iter().all(|i| i.starts_with("Lsp")),
        "all should start with Lsp"
    );
}

// =========================================================================
// Argument completion tests
// =========================================================================

// ── :set <tab> shows setting names ──────────────────────────────────────

#[test]
fn set_tab_shows_settings() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "set ");
    press_key(&mut e, "Tab");

    assert!(!e.wildmenu_items.is_empty(), "should show setting names");
    // All items should start with "set "
    assert!(
        e.wildmenu_items.iter().all(|i| i.starts_with("set ")),
        "all items should start with 'set '"
    );
    // Should include common settings
    assert!(
        e.wildmenu_items.iter().any(|i| i == "set wrap"),
        "should include 'set wrap'"
    );
    assert!(
        e.wildmenu_items.iter().any(|i| i == "set number"),
        "should include 'set number'"
    );
}

// ── :set w<tab> filters to settings starting with w ─────────────────────

#[test]
fn set_partial_arg_filters() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "set w");
    press_key(&mut e, "Tab");

    // "set w" should match "set wrap" (single match → auto-complete)
    assert_eq!(e.command_buffer, "set wrap");
}

// ── :set no<tab> shows noXxx variants ───────────────────────────────────

#[test]
fn set_no_prefix_completes() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "set no");
    press_key(&mut e, "Tab");

    // Should have multiple "noXxx" items
    assert!(
        !e.wildmenu_items.is_empty(),
        "should have no-prefixed items"
    );
    assert!(
        e.wildmenu_items.iter().all(|i| i.starts_with("set no")),
        "all should start with 'set no'"
    );
    assert!(
        e.wildmenu_items.iter().any(|i| i == "set nowrap"),
        "should include 'set nowrap'"
    );
}

// ── :set tab<tab> completes value options ───────────────────────────────

#[test]
fn set_tabstop_completes() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "set tabs");
    press_key(&mut e, "Tab");

    assert_eq!(e.command_buffer, "set tabstop");
}

// ── :colorscheme <tab> shows themes ─────────────────────────────────────

#[test]
fn colorscheme_tab_shows_themes() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "colorscheme ");
    press_key(&mut e, "Tab");

    assert!(!e.wildmenu_items.is_empty(), "should show theme names");
    assert!(
        e.wildmenu_items.iter().any(|i| i == "colorscheme onedark"),
        "should include onedark"
    );
    assert!(
        e.wildmenu_items
            .iter()
            .any(|i| i == "colorscheme gruvbox-dark"),
        "should include gruvbox-dark"
    );
}

// ── :colorscheme t<tab> filters themes ──────────────────────────────────

#[test]
fn colorscheme_partial_filters() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "colorscheme t");
    press_key(&mut e, "Tab");

    // Should match tokyo-night and tokyonight
    assert!(
        e.wildmenu_items
            .iter()
            .any(|i| i == "colorscheme tokyo-night"),
        "should include tokyo-night"
    );
}

// ── Tab through first word then argument ────────────────────────────────

#[test]
fn tab_through_command_then_args() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "se");
    press_key(&mut e, "Tab");

    // "se" should match "set " — single match auto-complete
    assert_eq!(e.command_buffer, "set ");

    // Now Tab again should open argument completion
    press_key(&mut e, "Tab");
    assert!(
        !e.wildmenu_items.is_empty(),
        "should show settings after 'set '"
    );
}

// ── Selected item with trailing space clears wildmenu ───────────────────

#[test]
fn trailing_space_item_clears_wildmenu_for_next_tab() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "s");
    press_key(&mut e, "Tab"); // opens wildmenu (set , s/, split, sort, ...)

    // Find "set " in wildmenu
    let set_idx = e.wildmenu_items.iter().position(|i| i == "set ");
    assert!(set_idx.is_some(), "wildmenu should contain 'set '");

    // Tab to "set " item
    for _ in 0..=set_idx.unwrap() {
        press_key(&mut e, "Tab");
    }
    assert_eq!(e.command_buffer, "set ");
    // Wildmenu should be cleared (trailing space)
    assert!(
        e.wildmenu_items.is_empty(),
        "wildmenu should clear after selecting item with trailing space"
    );

    // Tab again → argument completions
    press_key(&mut e, "Tab");
    assert!(
        !e.wildmenu_items.is_empty(),
        "should show setting completions"
    );
}
