mod common;
use common::*;
use vimcode_core::Mode;

// ── Helper ───────────────────────────────────────────────────────────────────

fn win_count(e: &vimcode_core::Engine) -> usize {
    e.active_tab().layout.window_ids().len()
}

// ── :wincmd ex command ───────────────────────────────────────────────────────

#[test]
fn wincmd_v_creates_vertical_split() {
    let mut e = engine_with("hello\n");
    assert_eq!(win_count(&e), 1);
    exec(&mut e, "wincmd v");
    assert_eq!(win_count(&e), 2);
}

#[test]
fn wincmd_s_creates_horizontal_split() {
    let mut e = engine_with("hello\n");
    assert_eq!(win_count(&e), 1);
    exec(&mut e, "wincmd s");
    assert_eq!(win_count(&e), 2);
}

#[test]
fn wincmd_c_closes_window() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "wincmd v");
    assert_eq!(win_count(&e), 2);
    exec(&mut e, "wincmd c");
    assert_eq!(win_count(&e), 1);
}

#[test]
fn wincmd_q_closes_window() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "wincmd v");
    assert_eq!(win_count(&e), 2);
    exec(&mut e, "wincmd q");
    assert_eq!(win_count(&e), 1);
}

#[test]
fn wincmd_o_closes_other_windows() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "wincmd v");
    exec(&mut e, "wincmd v");
    assert!(win_count(&e) >= 3);
    exec(&mut e, "wincmd o");
    assert_eq!(win_count(&e), 1);
}

// Note: focus_window_direction (h/j/k/l) needs window rects from rendering,
// so directional focus can't be tested in headless mode. The :wincmd h/j/k/l
// code path is verified by the Ctrl-W refactor regression tests + wincmd_w test.

#[test]
fn wincmd_w_cycles_windows() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "vsplit");
    let w1 = e.active_window_id();
    exec(&mut e, "wincmd w");
    assert_ne!(
        e.active_window_id(),
        w1,
        "wincmd w should cycle to another window"
    );
}

#[test]
fn wincmd_n_creates_new_window() {
    let mut e = engine_with("hello\n");
    assert_eq!(win_count(&e), 1);
    exec(&mut e, "wincmd n");
    assert_eq!(win_count(&e), 2);
}

#[test]
fn wincmd_equalize() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "vsplit");
    exec(&mut e, "wincmd > 5");
    exec(&mut e, "wincmd =");
    assert_eq!(win_count(&e), 2);
}

#[test]
fn wincmd_resize_with_count() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "vsplit");
    exec(&mut e, "wincmd + 3");
    exec(&mut e, "wincmd - 2");
    exec(&mut e, "wincmd > 3");
    exec(&mut e, "wincmd < 1");
    assert_eq!(win_count(&e), 2);
}

#[test]
fn wincmd_maximize() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "vsplit");
    exec(&mut e, "wincmd _");
    exec(&mut e, "wincmd |");
    assert_eq!(win_count(&e), 2);
}

// ── Abbreviation ─────────────────────────────────────────────────────────────

#[test]
fn wincmd_abbreviation_winc() {
    use vimcode_core::core::engine::normalize_ex_command;
    let result = normalize_ex_command("winc v");
    assert_eq!(&*result, "wincmd v");
}

#[test]
fn wincmd_abbreviation_wincm() {
    use vimcode_core::core::engine::normalize_ex_command;
    let result = normalize_ex_command("wincm h");
    assert_eq!(&*result, "wincmd h");
}

#[test]
fn wincmd_via_command_mode_abbreviation() {
    let mut e = engine_with("hello\n");
    assert_eq!(win_count(&e), 1);
    run_cmd(&mut e, "winc v");
    assert_eq!(win_count(&e), 2);
}

// ── Missing argument / unknown ───────────────────────────────────────────────

#[test]
fn wincmd_no_arg_shows_error() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "wincmd");
    assert_msg_contains(&e, "E471");
}

#[test]
fn wincmd_unknown_char_shows_error() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "wincmd z");
    assert_msg_contains(&e, "Unknown wincmd");
}

// ── Ctrl-W still works (regression) ─────────────────────────────────────────

#[test]
fn ctrl_w_v_still_splits() {
    let mut e = engine_with("hello\n");
    assert_eq!(win_count(&e), 1);
    ctrl(&mut e, 'w');
    press(&mut e, 'v');
    assert_eq!(win_count(&e), 2);
}

#[test]
fn ctrl_w_w_still_cycles() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "vsplit");
    let w1 = e.active_window_id();
    ctrl(&mut e, 'w');
    press(&mut e, 'w');
    assert_ne!(e.active_window_id(), w1, "Ctrl-W w should cycle windows");
}

#[test]
fn ctrl_w_c_still_closes() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "vsplit");
    assert_eq!(win_count(&e), 2);
    ctrl(&mut e, 'w');
    press(&mut e, 'c');
    assert_eq!(win_count(&e), 1);
}

// ── Editor group commands ────────────────────────────────────────────────────

#[test]
fn wincmd_e_opens_editor_group() {
    let mut e = engine_with("hello\n");
    let groups_before = e.editor_groups.len();
    exec(&mut e, "wincmd e");
    assert_eq!(e.editor_groups.len(), groups_before + 1);
}

#[test]
fn wincmd_upper_e_opens_editor_group_down() {
    let mut e = engine_with("hello\n");
    let groups_before = e.editor_groups.len();
    exec(&mut e, "wincmd E");
    assert_eq!(e.editor_groups.len(), groups_before + 1);
}

// ── Keybindings reference includes command names ─────────────────────────────

#[test]
fn keybindings_reference_shows_wincmd() {
    let mut e = engine_with("");
    exec(&mut e, "Keybindings vim");
    let content = e.buffer().to_string();
    assert!(
        content.contains(":wincmd h"),
        "reference should show :wincmd h"
    );
    assert!(content.contains(":close"), "reference should show :close");
    assert!(content.contains(":split"), "reference should show :split");
    assert!(content.contains(":vsplit"), "reference should show :vsplit");
    assert!(content.contains(":only"), "reference should show :only");
    assert!(content.contains(":new"), "reference should show :new");
    assert!(
        content.contains(":wincmd p"),
        "reference should show :wincmd p"
    );
}

#[test]
fn keybindings_reference_shows_g_command_names() {
    let mut e = engine_with("");
    exec(&mut e, "Keybindings vim");
    let content = e.buffer().to_string();
    assert!(content.contains(":def"), "g-commands should show :def");
    assert!(content.contains(":refs"), "g-commands should show :refs");
    assert!(
        content.contains(":LspTypedef"),
        "g-commands should show :LspTypedef"
    );
}

#[test]
fn keybindings_reference_shows_leader_command_names() {
    let mut e = engine_with("");
    exec(&mut e, "Keybindings vim");
    let content = e.buffer().to_string();
    assert!(content.contains(":Rename"), "leader should show :Rename");
    assert!(content.contains(":Lformat"), "leader should show :Lformat");
    assert!(content.contains(":LspImpl"), "leader should show :LspImpl");
}

#[test]
fn keybindings_reference_shows_bracket_nav_command_names() {
    let mut e = engine_with("");
    exec(&mut e, "Keybindings vim");
    let content = e.buffer().to_string();
    assert!(
        content.contains(":nexthunk"),
        "bracket nav should show :nexthunk"
    );
    assert!(
        content.contains(":prevhunk"),
        "bracket nav should show :prevhunk"
    );
    assert!(
        content.contains(":nextdiag"),
        "bracket nav should show :nextdiag"
    );
    assert!(
        content.contains(":prevdiag"),
        "bracket nav should show :prevdiag"
    );
}

#[test]
fn keybindings_reference_shows_panel_command_names() {
    let mut e = engine_with("");
    exec(&mut e, "Keybindings vim");
    let content = e.buffer().to_string();
    assert!(content.contains(":fuzzy"), "panel should show :fuzzy");
    assert!(content.contains(":sidebar"), "panel should show :sidebar");
    assert!(content.contains(":palette"), "panel should show :palette");
    assert!(content.contains(":hover"), "panel should show :hover");
    assert!(content.contains(":terminal"), "panel should show :terminal");
    assert!(content.contains(":debug"), "panel should show :debug");
}

// ── New ex commands are recognized ──────────────────────────────────────────

#[test]
fn hover_command_recognized() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "hover");
    // Should not produce "Unknown command" error
    assert!(
        !e.message.contains("Unknown command"),
        "hover should be recognized, got: {:?}",
        e.message
    );
}

#[test]
fn lsp_impl_command_recognized() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "LspImpl");
    assert!(
        !e.message.contains("Unknown command"),
        "LspImpl should be recognized, got: {:?}",
        e.message
    );
}

#[test]
fn lsp_typedef_command_recognized() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "LspTypedef");
    assert!(
        !e.message.contains("Unknown command"),
        "LspTypedef should be recognized, got: {:?}",
        e.message
    );
}

#[test]
fn nextdiag_command_recognized() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "nextdiag");
    assert!(
        !e.message.contains("Unknown command"),
        "nextdiag should be recognized, got: {:?}",
        e.message
    );
}

#[test]
fn prevdiag_command_recognized() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "prevdiag");
    assert!(
        !e.message.contains("Unknown command"),
        "prevdiag should be recognized, got: {:?}",
        e.message
    );
}

#[test]
fn nexthunk_command_recognized() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "nexthunk");
    assert!(
        !e.message.contains("Unknown command"),
        "nexthunk should be recognized, got: {:?}",
        e.message
    );
}

#[test]
fn prevhunk_command_recognized() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "prevhunk");
    assert!(
        !e.message.contains("Unknown command"),
        "prevhunk should be recognized, got: {:?}",
        e.message
    );
}

// ── Tab completion for new commands ─────────────────────────────────────────

#[test]
fn hover_appears_in_tab_completion() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "hove");
    press_key(&mut e, "Tab");
    assert!(
        e.command_buffer.contains("hover"),
        "tab should complete 'hove' to 'hover', got: {:?}",
        e.command_buffer
    );
}

#[test]
fn nextdiag_appears_in_tab_completion() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "nextd");
    press_key(&mut e, "Tab");
    assert!(
        e.command_buffer.contains("nextdiag"),
        "tab should complete 'nextd' to 'nextdiag', got: {:?}",
        e.command_buffer
    );
}

// ── Tab completion ───────────────────────────────────────────────────────────

#[test]
fn wincmd_appears_in_tab_completion() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "winc");
    press_key(&mut e, "Tab");
    assert!(
        e.command_buffer.contains("wincmd"),
        "tab should complete 'winc' to 'wincmd', got: {:?}",
        e.command_buffer
    );
}

#[test]
fn close_appears_in_tab_completion() {
    let mut e = engine_with("hello\n");
    press(&mut e, ':');
    type_chars(&mut e, "clos");
    press_key(&mut e, "Tab");
    assert!(
        e.command_buffer.contains("close"),
        "tab should complete 'clos' to 'close', got: {:?}",
        e.command_buffer
    );
}

// ── VSCode mode keymaps ─────────────────────────────────────────────────────

#[test]
fn vscode_mode_user_keymap_works() {
    let mut e = engine_with("hello world\n");
    // Switch to VSCode mode
    e.settings.editor_mode = vimcode_core::core::settings::EditorMode::Vscode;
    e.mode = Mode::Insert;
    // Add a keymap: Ctrl+Shift+D → :def (just testing the command runs)
    e.settings.keymaps = vec!["n <C-D> :nexthunk".to_string()];
    e.rebuild_user_keymaps();
    assert_eq!(e.user_keymaps.len(), 1);
    // Fire the mapped key (Ctrl+D in VSCode mode)
    e.handle_key("D", Some('D'), true);
    // Should NOT produce "Unknown command" — the keymap ran :nexthunk
    assert!(
        !e.message.contains("Unknown command"),
        "VSCode mode should honor user keymaps, got: {:?}",
        e.message
    );
}

#[test]
fn vscode_mode_unmapped_key_falls_through() {
    let mut e = engine_with("ab\n");
    e.settings.editor_mode = vimcode_core::core::settings::EditorMode::Vscode;
    e.mode = Mode::Insert;
    // No keymaps — plain typing should still insert text
    let buf_before = e.buffer().to_string();
    e.handle_key("x", Some('x'), false);
    let buf_after = e.buffer().to_string();
    assert_ne!(
        buf_before, buf_after,
        "typing should still insert in VSCode mode"
    );
    assert!(buf_after.contains('x'), "character 'x' should be inserted");
}

#[test]
fn vscode_keybindings_reference_shows_command_names() {
    let mut e = engine_with("");
    exec(&mut e, "Keybindings vscode");
    let content = e.buffer().to_string();
    assert!(content.contains(":def"), "vscode ref should show :def");
    assert!(content.contains(":refs"), "vscode ref should show :refs");
    assert!(
        content.contains(":Rename"),
        "vscode ref should show :Rename"
    );
    assert!(content.contains(":debug"), "vscode ref should show :debug");
    assert!(content.contains(":fuzzy"), "vscode ref should show :fuzzy");
    assert!(
        content.contains(":map n"),
        "vscode ref should mention :map n for remapping"
    );
}

#[test]
fn keymaps_editor_mentions_vscode_mode() {
    let mut e = engine_with("");
    exec(&mut e, "Keymaps");
    let content = e.buffer().to_string();
    assert!(
        content.contains("VSCode mode"),
        "keymaps editor should mention VSCode mode, got header: {:?}",
        content.lines().take(7).collect::<Vec<_>>()
    );
}
