mod common;
use common::*;
use vimcode_core::EngineAction;

// ═══════════════════════════════════════════════════════════════════════════════
//  Group 1: Normalizer abbreviation tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn normalizer_sor_to_sort() {
    let mut e = engine_with("banana\napple\ncherry\n");
    exec(&mut e, "sor");
    assert_eq!(get_lines(&e)[0], "apple");
}

#[test]
fn normalizer_colo_to_colorscheme() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "colo onedark");
    assert_msg_contains(&e, "onedark");
}

#[test]
fn normalizer_di_to_display() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "di");
    assert_msg_contains(&e, "Registers");
}

#[test]
fn normalizer_te_to_terminal() {
    let mut e = engine_with("hello\n");
    let act = exec(&mut e, "te");
    assert_eq!(act, EngineAction::OpenTerminal);
}

#[test]
fn normalizer_u_to_undo() {
    let mut e = engine_with("hello\n");
    // Make a change then undo via abbreviated command
    press(&mut e, 'x'); // delete 'h'
    assert_eq!(get_lines(&e)[0], "ello");
    exec(&mut e, "u");
    assert_eq!(get_lines(&e)[0], "hello");
}

#[test]
fn normalizer_red_to_redo() {
    let mut e = engine_with("hello\n");
    press(&mut e, 'x');
    exec(&mut e, "u");
    exec(&mut e, "red");
    assert_eq!(get_lines(&e)[0], "ello");
}

#[test]
fn normalizer_se_to_set() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "se tabstop=2");
    assert_eq!(e.settings.tabstop, 2);
}

#[test]
fn normalizer_gr_to_grep() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "gr"); // bare grep shows usage
    assert_msg_contains(&e, "Usage");
}

#[test]
fn normalizer_vs_to_vsplit() {
    let mut e = engine_with("hello\n");
    let before = e.windows.len();
    exec(&mut e, "vs");
    assert!(e.windows.len() > before);
}

#[test]
fn normalizer_cq_to_cquit() {
    let mut e = engine_with("hello\n");
    let act = exec(&mut e, "cq");
    assert_eq!(act, EngineAction::QuitWithError);
}

#[test]
fn normalizer_up_to_update() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "up");
    assert_msg_contains(&e, "no changes");
}

#[test]
fn normalizer_ve_to_version() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "ve");
    assert_msg_contains(&e, "VimCode");
}

#[test]
fn normalizer_ret_to_retab() {
    let mut e = engine_with("\thello\n");
    e.settings.expand_tab = true;
    e.settings.tabstop = 4;
    exec(&mut e, "ret");
    assert!(get_lines(&e)[0].starts_with("    "));
}

#[test]
fn normalizer_vne_to_vnew() {
    let mut e = engine_with("hello\n");
    let before = e.windows.len();
    exec(&mut e, "vne");
    assert!(e.windows.len() > before);
}

#[test]
fn normalizer_full_forms_still_work() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "version");
    assert_msg_contains(&e, "VimCode");
}

#[test]
fn normalizer_quit_bang() {
    let mut e = engine_with("hello\n");
    // Single window → quit
    let act = exec(&mut e, "q!");
    assert_eq!(act, EngineAction::Quit);
}

#[test]
fn normalizer_qall_bang() {
    let mut e = engine_with("hello\n");
    let act = exec(&mut e, "qa!");
    assert_eq!(act, EngineAction::Quit);
}

#[test]
fn normalizer_skips_uppercase_commands() {
    // VimCode-specific commands starting with uppercase should not be normalized
    let mut e = engine_with("hello\n");
    exec(&mut e, "DapInfo");
    // Should not error about "Not an editor command"
    // DapInfo should work or give a DAP message
    assert!(!e.message.starts_with("Not an editor command"));
}

#[test]
fn normalizer_skips_substitute() {
    let mut e = engine_with("hello world\n");
    exec(&mut e, "s/hello/hi/");
    assert_eq!(get_lines(&e)[0], "hi world");
}

#[test]
fn normalizer_skips_global() {
    let mut e = engine_with("foo\nbar\nfoo\n");
    exec(&mut e, "g/foo/d");
    assert_eq!(get_lines(&e), vec!["bar"]);
}

#[test]
fn normalizer_w_to_write() {
    let mut e = engine_with("hello\n");
    // :w on buffer with no path shows message (no crash)
    exec(&mut e, "w");
    // Should not be "Not an editor command"
    assert!(!e.message.starts_with("Not an editor command"));
}

#[test]
fn normalizer_e_to_edit() {
    let mut e = engine_with("hello\n");
    let path = std::env::temp_dir().join("vimcode_test_e_cmd.txt");
    std::fs::write(&path, "test content\n").unwrap();
    let act = exec(&mut e, &format!("e {}", path.display()));
    assert_eq!(act, EngineAction::OpenFile(path.clone()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn normalizer_sp_to_split() {
    let mut e = engine_with("hello\n");
    let before = e.windows.len();
    exec(&mut e, "sp");
    assert!(e.windows.len() > before);
}

// ═══════════════════════════════════════════════════════════════════════════════
//  Group 2: New command tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn cmd_display_shows_registers() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "display");
    assert_msg_contains(&e, "Registers");
}

#[test]
fn cmd_join_lines() {
    let mut e = engine_with("hello\nworld\n");
    exec(&mut e, "join");
    assert_eq!(get_lines(&e)[0], "hello world");
}

#[test]
fn cmd_j_abbreviation() {
    let mut e = engine_with("foo\nbar\n");
    exec(&mut e, "j");
    assert_eq!(get_lines(&e)[0], "foo bar");
}

#[test]
fn cmd_yank_default_register() {
    let mut e = engine_with("hello\nworld\n");
    exec(&mut e, "yank");
    assert_register(&e, '"', "hello\n", true);
}

#[test]
fn cmd_yank_named_register() {
    let mut e = engine_with("hello\nworld\n");
    exec(&mut e, "yank a");
    assert_register(&e, 'a', "hello\n", true);
}

#[test]
fn cmd_yank_linewise() {
    let mut e = engine_with("test line\n");
    exec(&mut e, "y");
    let (_, lw) = e.registers.get(&'"').unwrap();
    assert!(*lw, "yank should be linewise");
}

#[test]
fn cmd_put_default_register() {
    let mut e = engine_with("line1\nline2\n");
    e.registers.insert('"', ("inserted\n".to_string(), true));
    exec(&mut e, "put");
    let lines = get_lines(&e);
    assert_eq!(lines[1], "inserted");
}

#[test]
fn cmd_put_named_register() {
    let mut e = engine_with("line1\nline2\n");
    e.registers.insert('a', ("from_a\n".to_string(), true));
    exec(&mut e, "put a");
    let lines = get_lines(&e);
    assert_eq!(lines[1], "from_a");
}

#[test]
fn cmd_put_empty_register() {
    let mut e = engine_with("line1\n");
    exec(&mut e, "put z");
    assert_msg_contains(&e, "empty");
}

#[test]
fn cmd_shift_right() {
    let mut e = engine_with("hello\n");
    exec(&mut e, ">");
    assert!(get_lines(&e)[0].starts_with(' ') || get_lines(&e)[0].starts_with('\t'));
}

#[test]
fn cmd_shift_left() {
    let mut e = engine_with("    hello\n");
    exec(&mut e, "<");
    // Should have fewer leading spaces
    let line = &get_lines(&e)[0];
    assert!(line.len() < "    hello".len());
}

#[test]
fn cmd_equals_line_count() {
    let mut e = engine_with("a\nb\nc\nd\ne\n");
    exec(&mut e, "=");
    assert_eq!(e.message, "5");
}

#[test]
fn cmd_hash_number() {
    let mut e = engine_with("hello\nworld\n");
    exec(&mut e, "#");
    assert_msg_contains(&e, "1");
    assert_msg_contains(&e, "hello");
}

#[test]
fn cmd_mark_local() {
    let mut e = engine_with("line1\nline2\nline3\n");
    // Move to line 2
    press(&mut e, 'j');
    exec(&mut e, "mark a");
    assert_msg_contains(&e, "Mark 'a' set");
    let buf_id = e.active_buffer_id();
    let marks = e.marks.get(&buf_id).unwrap();
    let cursor = marks.get(&'a').unwrap();
    assert_eq!(cursor.line, 1);
}

#[test]
fn cmd_k_shorthand_mark() {
    let mut e = engine_with("line1\nline2\n");
    press(&mut e, 'j');
    exec(&mut e, "kb");
    assert_msg_contains(&e, "Mark 'b' set");
}

#[test]
fn cmd_pwd() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "pwd");
    // Message should contain a path (has a / on unix)
    assert!(e.message.contains('/'));
}

#[test]
fn cmd_file_info() {
    let mut e = engine_with("hello\nworld\n");
    exec(&mut e, "file");
    assert_msg_contains(&e, "2 lines");
}

#[test]
fn cmd_f_abbreviation() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "f");
    assert_msg_contains(&e, "1 lines");
}

#[test]
fn cmd_enew() {
    let mut e = engine_with("hello\n");
    let old_buf = e.active_buffer_id();
    exec(&mut e, "enew");
    assert_ne!(e.active_buffer_id(), old_buf);
    assert_eq!(e.buffer().len_chars(), 0);
}

#[test]
fn cmd_update_dirty() {
    let path = std::env::temp_dir().join("vimcode_test_update.txt");
    std::fs::write(&path, "original\n").unwrap();
    let mut e = engine_with("");
    let _ = e.open_file_with_mode(&path, vimcode_core::core::engine::OpenMode::Permanent);
    // Make buffer dirty
    press(&mut e, 'x');
    assert!(e.dirty());
    exec(&mut e, "update");
    // Should have saved (message not "no changes")
    assert!(!e.message.contains("no changes"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn cmd_update_clean() {
    let mut e = engine_with("hello\n");
    e.set_dirty(false);
    exec(&mut e, "update");
    assert_msg_contains(&e, "no changes");
}

#[test]
fn cmd_version() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "version");
    assert_msg_contains(&e, "VimCode");
}

#[test]
fn cmd_print() {
    let mut e = engine_with("hello world\n");
    exec(&mut e, "print");
    assert_eq!(e.message, "hello world");
}

#[test]
fn cmd_p_abbreviation() {
    let mut e = engine_with("test line\n");
    exec(&mut e, "p");
    assert_eq!(e.message, "test line");
}

#[test]
fn cmd_number() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "number");
    assert_msg_contains(&e, "1");
    assert_msg_contains(&e, "hello");
}

#[test]
fn cmd_nu_abbreviation() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "nu");
    assert_msg_contains(&e, "1");
}

#[test]
fn cmd_new_hsplit() {
    let mut e = engine_with("hello\n");
    let before = e.windows.len();
    exec(&mut e, "new");
    assert!(e.windows.len() > before);
    // New buffer should be empty
    assert_eq!(e.buffer().len_chars(), 0);
}

#[test]
fn cmd_vnew_vsplit() {
    let mut e = engine_with("hello\n");
    let before = e.windows.len();
    exec(&mut e, "vnew");
    assert!(e.windows.len() > before);
    assert_eq!(e.buffer().len_chars(), 0);
}

#[test]
fn cmd_retab_tabs_to_spaces() {
    let mut e = engine_with("\thello\n\tworld\n");
    e.settings.expand_tab = true;
    e.settings.tabstop = 4;
    exec(&mut e, "retab");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "    hello");
    assert_eq!(lines[1], "    world");
}

#[test]
fn cmd_retab_spaces_to_tabs() {
    let mut e = engine_with("    hello\n    world\n");
    e.settings.expand_tab = false;
    e.settings.tabstop = 4;
    exec(&mut e, "retab");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "\thello");
    assert_eq!(lines[1], "\tworld");
}

#[test]
fn cmd_retab_with_tabstop_arg() {
    let mut e = engine_with("\thello\n");
    e.settings.expand_tab = true;
    exec(&mut e, "retab 2");
    assert_eq!(e.settings.tabstop, 2);
    assert_eq!(get_lines(&e)[0], "  hello");
}

#[test]
fn cmd_cquit() {
    let mut e = engine_with("hello\n");
    let act = exec(&mut e, "cquit");
    assert_eq!(act, EngineAction::QuitWithError);
}

#[test]
fn cmd_cquit_bang() {
    let mut e = engine_with("hello\n");
    let act = exec(&mut e, "cquit!");
    assert_eq!(act, EngineAction::QuitWithError);
}

#[test]
fn cmd_saveas() {
    let path = std::env::temp_dir().join("vimcode_test_saveas.txt");
    let _ = std::fs::remove_file(&path);
    let mut e = engine_with("saveas content\n");
    exec(&mut e, &format!("saveas {}", path.display()));
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("saveas content"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn cmd_copy_bare_shows_usage() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "copy");
    assert_msg_contains(&e, "Usage");
}

#[test]
fn cmd_copy_with_address() {
    let mut e = engine_with("line1\nline2\nline3\n");
    // :t 1 copies current line (line1) after line index 1
    exec(&mut e, "t 1");
    let lines = get_lines(&e);
    assert_eq!(lines.len(), 4);
    // After copy: line1, line2, line1(copy), line3
    // Address 1 → 0-based index 1 → insert after it
    assert_eq!(lines[0], "line1");
    assert_eq!(lines[2], "line1"); // the copy
}

#[test]
fn cmd_windo() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "vsplit");
    // Both windows should be affected (set tabstop as a verifiable side effect)
    exec(&mut e, "windo set tabstop=3");
    assert_eq!(e.settings.tabstop, 3);
}

// ═══════════════════════════════════════════════════════════════════════════════
//  Group 3: Existing command abbreviation tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn abbrev_wri_writes() {
    let mut e = engine_with("hello\n");
    // :wri should normalize to :write (no crash)
    exec(&mut e, "wri");
    assert!(!e.message.starts_with("Not an editor command"));
}

#[test]
fn abbrev_qui_quits() {
    let mut e = engine_with("hello\n");
    e.set_dirty(false);
    let act = exec(&mut e, "qui");
    assert_eq!(act, EngineAction::Quit);
}

#[test]
fn abbrev_ed_opens() {
    let mut e = engine_with("hello\n");
    let path = std::env::temp_dir().join("vimcode_test_ed_cmd.txt");
    std::fs::write(&path, "test\n").unwrap();
    let act = exec(&mut e, &format!("ed {}", path.display()));
    assert_eq!(act, EngineAction::OpenFile(path.clone()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn abbrev_se_set() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "se tabstop=8");
    assert_eq!(e.settings.tabstop, 8);
}

#[test]
fn abbrev_colo_colorscheme() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "colo onedark");
    assert_msg_contains(&e, "onedark");
}

#[test]
fn abbrev_tabm_tabmove() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "tabnew");
    exec(&mut e, "tabm 0");
    // Should not error
    assert!(!e.message.starts_with("Not an editor command"));
}

#[test]
fn abbrev_noh_nohlsearch() {
    let mut e = engine_with("hello world\n");
    search_fwd(&mut e, "hello");
    assert!(!e.search_matches.is_empty());
    exec(&mut e, "noh");
    assert!(e.search_matches.is_empty());
}

#[test]
fn abbrev_bd_bdelete() {
    let mut e = engine_with("hello\n");
    // Open a second buffer first
    exec(&mut e, "enew");
    let buf_count_before = e.buffer_manager.list().len();
    exec(&mut e, "bd");
    assert!(e.buffer_manager.list().len() < buf_count_before);
}

#[test]
fn abbrev_bn_bnext() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "enew");
    let buf1 = e.active_buffer_id();
    exec(&mut e, "bn");
    // Should have switched buffer
    assert_ne!(e.active_buffer_id(), buf1);
}

#[test]
fn abbrev_bp_bprevious() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "enew");
    exec(&mut e, "bp");
    // Should not error
    assert!(!e.message.starts_with("Not an editor command"));
}

#[test]
fn abbrev_h_help() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "h");
    // Help opens a split with help content
    assert!(!e.message.starts_with("Not an editor command"));
}
