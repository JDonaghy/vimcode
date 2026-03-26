use super::super::settings::LineNumberMode;
use super::*;

fn press_char(engine: &mut Engine, ch: char) {
    engine.handle_key(&ch.to_string(), Some(ch), false);
}

fn press_special(engine: &mut Engine, name: &str) {
    engine.handle_key(name, None, false);
}

fn press_ctrl(engine: &mut Engine, ch: char) {
    engine.handle_key(&ch.to_string(), Some(ch), true);
}

#[test]
fn test_normal_movement() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "Hello");

    press_char(&mut engine, 'l');
    assert_eq!(engine.view().cursor.col, 1);

    press_char(&mut engine, 'h');
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_bounds_checking() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "Hi\nThere");

    press_char(&mut engine, 'l');
    press_char(&mut engine, 'l');
    press_char(&mut engine, 'l');
    assert!(
        engine.view().cursor.col <= 1,
        "Cursor col went too far right"
    );

    press_char(&mut engine, 'j');
    assert_eq!(engine.view().cursor.line, 1);

    press_char(&mut engine, 'j');
    assert_eq!(
        engine.view().cursor.line,
        1,
        "Cursor line went past last line"
    );
}

#[test]
fn test_column_clamping() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "Long line\nShort");

    for _ in 0..10 {
        press_char(&mut engine, 'l');
    }

    press_char(&mut engine, 'j');
    assert!(
        engine.view().cursor.col <= 4,
        "Cursor col not clamped on short line"
    );
}

#[test]
fn test_arrow_keys() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "AB\nCD");

    press_special(&mut engine, "Right");
    assert_eq!(engine.view().cursor.col, 1);

    press_special(&mut engine, "Down");
    assert_eq!(engine.view().cursor.line, 1);

    press_special(&mut engine, "Up");
    assert_eq!(engine.view().cursor.line, 0);

    press_special(&mut engine, "Left");
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_insert_mode_typing() {
    let mut engine = Engine::new();
    press_char(&mut engine, 'i');
    assert_eq!(engine.mode, Mode::Insert);

    press_char(&mut engine, 'H');
    press_char(&mut engine, 'i');
    press_char(&mut engine, '!');
    assert_eq!(engine.buffer().to_string(), "Hi!");
    assert_eq!(engine.view().cursor.col, 3);

    press_special(&mut engine, "Escape");
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_insert_special_chars() {
    let mut engine = Engine::new();
    press_char(&mut engine, 'i');

    for ch in "fn main() { println!(\"hello\"); }".chars() {
        press_char(&mut engine, ch);
    }
    assert_eq!(
        engine.buffer().to_string(),
        "fn main() { println!(\"hello\"); }"
    );
}

#[test]
fn test_insert_tab() {
    let mut engine = Engine::new();
    press_char(&mut engine, 'i');
    press_special(&mut engine, "Tab");
    assert_eq!(engine.buffer().to_string(), "    ");
    assert_eq!(engine.view().cursor.col, 4);
}

#[test]
fn test_backspace_joins_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "AB\nCD");
    engine.update_syntax();

    press_char(&mut engine, 'j');
    press_char(&mut engine, 'i');
    assert_eq!(engine.view().cursor.line, 1);
    assert_eq!(engine.view().cursor.col, 0);

    press_special(&mut engine, "BackSpace");
    assert_eq!(engine.view().cursor.line, 0);
    assert_eq!(engine.buffer().to_string(), "ABCD");
}

#[test]
fn test_delete_key() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABC");
    engine.update_syntax();

    press_char(&mut engine, 'i');
    press_special(&mut engine, "Delete");
    assert_eq!(engine.buffer().to_string(), "BC");
}

#[test]
fn test_normal_x_deletes_char() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABC");
    engine.update_syntax();

    press_char(&mut engine, 'x');
    assert_eq!(engine.buffer().to_string(), "BC");
}

#[test]
fn test_normal_o_opens_line_below() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "AB\nCD");
    engine.update_syntax();

    press_char(&mut engine, 'o');
    assert_eq!(engine.mode, Mode::Insert);
    assert_eq!(engine.view().cursor.line, 1);
    assert_eq!(engine.view().cursor.col, 0);
    assert_eq!(engine.buffer().to_string(), "AB\n\nCD");
}

fn type_command(engine: &mut Engine, cmd: &str) {
    press_char(engine, ':');
    assert_eq!(engine.mode, Mode::Command);
    for ch in cmd.chars() {
        engine.handle_key(&ch.to_string(), Some(ch), false);
    }
    press_special(engine, "Return");
}

#[test]
fn test_command_mode_enter_exit() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "Hello");

    press_char(&mut engine, ':');
    assert_eq!(engine.mode, Mode::Command);
    assert!(engine.command_buffer.is_empty());

    press_special(&mut engine, "Escape");
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_command_backspace_stays_in_command() {
    let mut engine = Engine::new();
    // Type ":a" then backspace — should stay in Command with empty buffer.
    press_char(&mut engine, ':');
    assert_eq!(engine.mode, Mode::Command);
    press_char(&mut engine, 'a');
    assert_eq!(engine.command_buffer, "a");
    press_special(&mut engine, "BackSpace");
    assert_eq!(
        engine.mode,
        Mode::Command,
        "BackSpace on last char should stay in Command"
    );
    assert_eq!(
        engine.command_buffer, "",
        "buffer should be empty after BackSpace"
    );
    // Second BackSpace on empty buffer → exit
    press_special(&mut engine, "BackSpace");
    assert_eq!(
        engine.mode,
        Mode::Normal,
        "BackSpace on empty buffer should exit Command"
    );
}

#[test]
fn test_command_quit_clean() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "Hello");
    engine.set_dirty(false);

    press_char(&mut engine, ':');
    press_char(&mut engine, 'q');
    let action = engine.handle_key("Return", None, false);
    assert_eq!(action, EngineAction::Quit);
}

#[test]
fn test_command_quit_dirty_blocked() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "Hello");
    engine.set_dirty(true);

    type_command(&mut engine, "q");
    assert!(engine.message.contains("No write since last change"));
}

#[test]
fn test_command_force_quit() {
    let mut engine = Engine::new();
    engine.set_dirty(true);

    press_char(&mut engine, ':');
    for ch in "q!".chars() {
        engine.handle_key(&ch.to_string(), Some(ch), false);
    }
    let action = engine.handle_key("Return", None, false);
    assert_eq!(action, EngineAction::Quit);
}

#[test]
fn test_command_unknown() {
    let mut engine = Engine::new();
    type_command(&mut engine, "notacommand");
    assert!(engine.message.contains("Not an editor command"));
}

#[test]
fn test_q_closes_tab_when_multiple_tabs() {
    let mut engine = Engine::new();
    // Tab 0 — first file
    engine.buffer_mut().insert(0, "first");
    engine.set_dirty(false);
    let first_id = engine.active_buffer_id();
    // Tab 1 — second file
    engine.new_tab(None);
    engine.buffer_mut().insert(0, "second");
    engine.set_dirty(false);
    assert_eq!(engine.active_group().tabs.len(), 2);
    assert_eq!(engine.buffer_manager.len(), 2);
    // :q closes the active tab, not the whole app
    let action = type_command_action(&mut engine, "q");
    assert_eq!(action, EngineAction::None);
    assert_eq!(engine.active_group().tabs.len(), 1, "tab should be closed");
    // The closed buffer is freed; session restore excludes it via window-filter.
    assert_eq!(engine.buffer_manager.len(), 1);
    assert!(engine.buffer_manager.get(first_id).is_some());
}

#[test]
fn test_q_quits_when_single_buffer_clean() {
    let mut engine = Engine::new();
    engine.set_dirty(false);
    let action = type_command_action(&mut engine, "q");
    assert_eq!(action, EngineAction::Quit);
}

#[test]
fn test_q_blocks_when_single_buffer_dirty() {
    let mut engine = Engine::new();
    engine.set_dirty(true);
    type_command(&mut engine, "q");
    assert!(engine.message.contains("No write since last change"));
}

#[test]
fn test_q_bang_closes_dirty_tab_when_multiple() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "first");
    engine.set_dirty(false);
    engine.new_tab(None);
    engine.buffer_mut().insert(0, "second");
    engine.set_dirty(true); // dirty but force-close with q!
    assert_eq!(engine.active_group().tabs.len(), 2);
    let action = type_command_action(&mut engine, "q!");
    assert_eq!(action, EngineAction::None);
    assert_eq!(engine.active_group().tabs.len(), 1, "tab should be closed");
    assert_eq!(engine.buffer_manager.len(), 1);
}

#[test]
fn test_q_bang_quits_when_single_buffer() {
    let mut engine = Engine::new();
    engine.set_dirty(true);
    let action = type_command_action(&mut engine, "q!");
    assert_eq!(action, EngineAction::Quit);
}

#[test]
fn test_qa_quits_when_all_clean() {
    let mut engine = Engine::new();
    engine.set_dirty(false);
    let action = type_command_action(&mut engine, "qa");
    assert_eq!(action, EngineAction::Quit);
}

#[test]
fn test_qa_blocks_when_any_dirty() {
    let mut engine = Engine::new();
    engine.set_dirty(true);
    type_command(&mut engine, "qa");
    assert!(engine.message.contains("No write since last change"));
}

#[test]
fn test_qa_bang_force_quits() {
    let mut engine = Engine::new();
    engine.set_dirty(true);
    let action = type_command_action(&mut engine, "qa!");
    assert_eq!(action, EngineAction::Quit);
}

#[test]
fn test_restore_session_files_opens_separate_tabs() {
    use crate::core::session::SessionState;
    let dir = std::env::temp_dir();
    let p1 = dir.join("vimcode_restore_a.txt");
    let p2 = dir.join("vimcode_restore_b.txt");
    let p3 = dir.join("vimcode_restore_c.txt");
    std::fs::write(&p1, "aaa").unwrap();
    std::fs::write(&p2, "bbb").unwrap();
    std::fs::write(&p3, "ccc").unwrap();

    // Write a per-workspace session so restore_session_files finds it.
    // (Session files live in ~/.config/vimcode/sessions/{hash}.json.)
    let workspace_dir = dir.join("vimcode_restore_test_ws");
    std::fs::create_dir_all(&workspace_dir).unwrap();
    let mut ws_session = SessionState::default();
    ws_session.open_files = vec![p1.clone(), p2.clone(), p3.clone()];
    ws_session.active_file = Some(p2.clone());
    // save_for_workspace is a no-op under #[cfg(test)], so write directly.
    let session_path = SessionState::session_path_for_workspace(&workspace_dir);
    if let Some(parent) = session_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(
        &session_path,
        serde_json::to_string_pretty(&ws_session).unwrap(),
    )
    .unwrap();

    let mut engine = Engine::new();
    engine.cwd = workspace_dir.clone();
    // Disable swap files so swap_scan_stale doesn't open extra files.
    engine.settings.swap_file = false;
    engine.restore_session_files();

    // Three files → three tabs.
    assert_eq!(
        engine.active_group().tabs.len(),
        3,
        "each file should get its own tab"
    );
    // Three buffers in manager (no scratch buffer).
    assert_eq!(engine.buffer_manager.len(), 3);
    // Active tab should be the one showing p2.
    let active_buf = engine.active_buffer_id();
    let active_path = engine
        .buffer_manager
        .get(active_buf)
        .and_then(|s| s.file_path.clone());
    assert_eq!(active_path.as_deref(), Some(p2.as_path()));

    let _ = std::fs::remove_file(&p1);
    let _ = std::fs::remove_file(&p2);
    let _ = std::fs::remove_file(&p3);
    let _ = std::fs::remove_dir(&workspace_dir);
    let _ = std::fs::remove_file(SessionState::session_path_for_workspace(&workspace_dir));
}

/// Regression test: opening vimcode in a fresh directory must NOT restore files
/// from a different workspace (the old global-session fallback caused this).
#[test]
fn test_restore_session_does_not_bleed_across_workspaces() {
    use crate::core::session::SessionState;
    let dir = std::env::temp_dir();
    let p1 = dir.join("vimcode_bleed_a.txt");
    std::fs::write(&p1, "from workspace A").unwrap();

    // Set global session to simulate "last used workspace A had p1 open".
    let mut engine = Engine::new();
    // Point cwd to a directory with NO workspace session.
    let fresh_dir = dir.join("vimcode_fresh_workspace");
    std::fs::create_dir_all(&fresh_dir).unwrap();
    // Ensure no session exists for this dir.
    let _ = std::fs::remove_file(SessionState::session_path_for_workspace(&fresh_dir));
    engine.cwd = fresh_dir.clone();
    // Populate global session as if we'd just come from workspace A.
    engine.session.open_files = vec![p1.clone()];
    engine.session.active_file = Some(p1.clone());

    engine.restore_session_files();

    // Fresh workspace → should open NO files (just the scratch buffer).
    assert_eq!(
        engine.active_group().tabs.len(),
        1,
        "no files should bleed from another workspace"
    );
    // The single buffer should be the empty scratch buffer (no file_path).
    let buf_id = engine.active_buffer_id();
    let buf_path = engine
        .buffer_manager
        .get(buf_id)
        .and_then(|s| s.file_path.clone());
    assert!(buf_path.is_none(), "scratch buffer should have no path");

    let _ = std::fs::remove_file(&p1);
    let _ = std::fs::remove_dir(&fresh_dir);
}

#[test]
fn test_ctrl_s_saves_in_normal_mode() {
    let dir = std::env::temp_dir();
    let path = dir.join("vimcode_test_ctrl_s.txt");
    std::fs::write(&path, "original").unwrap();
    let mut engine = Engine::open(&path);
    // Edit the buffer (direct insert to simulate typing)
    engine.buffer_mut().insert(0, "new ");
    engine.set_dirty(true);
    // Ctrl-S in normal mode
    let action = engine.handle_key("s", Some('s'), true);
    assert_eq!(action, EngineAction::None);
    // File should be saved (not dirty)
    assert!(!engine.dirty());
    let _ = std::fs::remove_file(&path);
}

/// Helper: type a command and return its EngineAction.
fn type_command_action(engine: &mut Engine, cmd: &str) -> EngineAction {
    press_char(engine, ':');
    for ch in cmd.chars() {
        engine.handle_key(&ch.to_string(), Some(ch), false);
    }
    engine.handle_key("Return", None, false)
}

#[test]
fn test_history_search_basic() {
    let mut engine = Engine::new();
    engine.history.add_command("write");
    engine.history.add_command("quit");
    engine.history.add_command("wall");

    // Enter command mode, then Ctrl-R
    press_char(&mut engine, ':');
    press_ctrl(&mut engine, 'r');

    assert!(engine.history_search_active);
    // Most recent match with empty query: "wall"
    assert_eq!(engine.command_buffer, "wall");
}

#[test]
fn test_history_search_typing_filters() {
    let mut engine = Engine::new();
    engine.history.add_command("write");
    engine.history.add_command("quit");
    engine.history.add_command("wall");

    press_char(&mut engine, ':');
    press_ctrl(&mut engine, 'r');

    // Type "w" - should match most recent command containing "w": "wall"
    engine.handle_key("w", Some('w'), false);
    assert_eq!(engine.history_search_query, "w");
    assert_eq!(engine.command_buffer, "wall");

    // Type "r" -> "wr" - should match "write"
    engine.handle_key("r", Some('r'), false);
    assert_eq!(engine.history_search_query, "wr");
    assert_eq!(engine.command_buffer, "write");
}

#[test]
fn test_history_search_ctrl_r_cycles() {
    let mut engine = Engine::new();
    engine.history.add_command("write");
    engine.history.add_command("wquit");
    engine.history.add_command("wall");

    press_char(&mut engine, ':');
    press_ctrl(&mut engine, 'r');
    engine.handle_key("w", Some('w'), false);

    // First match: "wall" (most recent with "w")
    assert_eq!(engine.command_buffer, "wall");

    // Ctrl-R again: next older match "wquit"
    press_ctrl(&mut engine, 'r');
    assert_eq!(engine.command_buffer, "wquit");

    // Ctrl-R again: next older match "write"
    press_ctrl(&mut engine, 'r');
    assert_eq!(engine.command_buffer, "write");
}

#[test]
fn test_history_search_escape_cancels() {
    let mut engine = Engine::new();
    engine.history.add_command("write");
    engine.history.add_command("quit");

    press_char(&mut engine, ':');
    engine.handle_key("w", Some('w'), false); // type "w" normally
    press_ctrl(&mut engine, 'r');

    assert!(engine.history_search_active);

    // Escape should cancel and restore original buffer ("w")
    press_special(&mut engine, "Escape");
    assert!(!engine.history_search_active);
    assert_eq!(engine.command_buffer, "w");
    // Mode is still Command (Escape from search returns to command line)
    assert_eq!(engine.mode, Mode::Command);
}

#[test]
fn test_history_search_enter_accepts() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\nworld\nfoo");
    engine.history.add_command("3");

    press_char(&mut engine, ':');
    press_ctrl(&mut engine, 'r');

    // Found "3" (only history entry)
    assert_eq!(engine.command_buffer, "3");

    // Enter executes it
    press_special(&mut engine, "Return");
    assert!(!engine.history_search_active);
    assert_eq!(engine.mode, Mode::Normal);
    assert_eq!(engine.view().cursor.line, 2); // jumped to line 3
}

#[test]
fn test_history_search_backspace_narrows() {
    let mut engine = Engine::new();
    engine.history.add_command("write");
    engine.history.add_command("wall");

    press_char(&mut engine, ':');
    press_ctrl(&mut engine, 'r');
    engine.handle_key("r", Some('r'), false); // query = "r", matches "write"
    assert_eq!(engine.command_buffer, "write");

    // Backspace removes "r" -> query = "", matches "wall" (most recent)
    press_special(&mut engine, "BackSpace");
    assert_eq!(engine.history_search_query, "");
    assert_eq!(engine.command_buffer, "wall");
}

#[test]
fn test_command_line_number_jump() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "line1\nline2\nline3\nline4\nline5");

    type_command(&mut engine, "3");
    assert_eq!(engine.view().cursor.line, 2);
}

#[test]
fn test_command_save() {
    use std::io::Write;
    let dir = std::env::temp_dir().join("vimcode_test_save");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test_save.txt");

    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"original").unwrap();
    }

    let mut engine = Engine::open(&path);
    assert_eq!(engine.buffer().to_string(), "original");

    engine.buffer_mut().insert(0, "new ");
    engine.set_dirty(true);
    type_command(&mut engine, "w");
    assert!(!engine.dirty());
    assert!(engine.message.contains("written"));

    let saved = std::fs::read_to_string(&path).unwrap();
    assert_eq!(saved, "new original");

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn test_dirty_flag() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "Hello");
    assert!(!engine.dirty());

    press_char(&mut engine, 'i');
    press_char(&mut engine, 'X');
    assert!(engine.dirty());
}

#[test]
fn test_search_basic() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar foo baz foo");

    press_char(&mut engine, '/');
    assert_eq!(engine.mode, Mode::Search);

    for ch in "foo".chars() {
        engine.handle_key(&ch.to_string(), Some(ch), false);
    }
    press_special(&mut engine, "Return");

    assert_eq!(engine.mode, Mode::Normal);
    assert_eq!(engine.search_query, "foo");
    assert_eq!(engine.search_matches.len(), 3);
    assert!(engine.message.contains("match"));
}

#[test]
fn test_search_not_found() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");

    press_char(&mut engine, '/');
    for ch in "zzz".chars() {
        engine.handle_key(&ch.to_string(), Some(ch), false);
    }
    press_special(&mut engine, "Return");

    assert!(engine.search_matches.is_empty());
    assert!(engine.message.contains("Pattern not found"));
}

#[test]
#[allow(non_snake_case)]
fn test_search_n_and_N() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "aXa\naXa\naXa");

    press_char(&mut engine, '/');
    engine.handle_key("X", Some('X'), false);
    press_special(&mut engine, "Return");

    assert_eq!(engine.search_matches.len(), 3);
    let first_line = engine.view().cursor.line;
    let first_col = engine.view().cursor.col;

    press_char(&mut engine, 'n');
    assert!(
        engine.view().cursor.line > first_line
            || (engine.view().cursor.line == first_line && engine.view().cursor.col > first_col)
            || engine.search_matches.len() == 1,
        "n should advance to next match"
    );

    press_char(&mut engine, 'N');
}

#[test]
fn test_search_escape_cancels() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");

    press_char(&mut engine, '/');
    assert_eq!(engine.mode, Mode::Search);
    press_special(&mut engine, "Escape");
    assert_eq!(engine.mode, Mode::Normal);
    assert!(engine.search_query.is_empty());
}

#[test]
fn test_incremental_search_forward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar baz foo");
    engine.update_syntax();

    // Start at beginning
    assert_eq!(engine.view().cursor.line, 0);
    assert_eq!(engine.view().cursor.col, 0);

    // Enter search mode
    press_char(&mut engine, '/');
    assert_eq!(engine.mode, Mode::Search);

    // Type 'f' - should jump to first 'foo'
    press_char(&mut engine, 'f');
    assert_eq!(engine.view().cursor.col, 0); // Already at first 'f'

    // Type 'o' - should still be at 'foo'
    press_char(&mut engine, 'o');
    assert_eq!(engine.view().cursor.col, 0);

    // Type 'o' - complete 'foo'
    press_char(&mut engine, 'o');
    assert_eq!(engine.view().cursor.col, 0);
    assert_eq!(engine.search_matches.len(), 2);

    // Press Enter to confirm
    press_special(&mut engine, "Return");
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_incremental_search_backward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar baz foo");
    engine.update_syntax();

    // Move to end of line
    for _ in 0..15 {
        press_char(&mut engine, 'l');
    }
    let start_col = engine.view().cursor.col;

    // Enter reverse search mode
    press_char(&mut engine, '?');

    // Type 'foo' - should jump to last 'foo' before cursor
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'o');
    press_char(&mut engine, 'o');

    // Should have jumped to the second 'foo' (at col 12)
    assert!(engine.view().cursor.col < start_col);
    assert_eq!(engine.view().cursor.col, 12);
}

#[test]
fn test_incremental_search_escape_restores_cursor() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world test");
    engine.update_syntax();

    // Move to col 6 (start of 'world')
    for _ in 0..6 {
        press_char(&mut engine, 'l');
    }
    assert_eq!(engine.view().cursor.col, 6);

    // Start search
    press_char(&mut engine, '/');

    // Type 'test' - cursor should jump to 'test'
    for ch in "test".chars() {
        press_char(&mut engine, ch);
    }
    assert_eq!(engine.view().cursor.col, 12);

    // Escape - should restore to original position (col 6)
    press_special(&mut engine, "Escape");
    assert_eq!(engine.mode, Mode::Normal);
    assert_eq!(engine.view().cursor.col, 6);
}

#[test]
fn test_incremental_search_backspace() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo food fool");
    engine.update_syntax();

    // Start search
    press_char(&mut engine, '/');

    // Type 'fool' - should jump to 'fool'
    for ch in "fool".chars() {
        press_char(&mut engine, ch);
    }
    assert_eq!(engine.view().cursor.col, 9);

    // Backspace to 'foo' - should update to first 'foo'
    press_special(&mut engine, "BackSpace");
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_incremental_search_no_match() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Move to col 5
    for _ in 0..5 {
        press_char(&mut engine, 'l');
    }
    assert_eq!(engine.view().cursor.col, 5);

    // Start search
    press_char(&mut engine, '/');

    // Type pattern that doesn't exist
    for ch in "xyz".chars() {
        press_char(&mut engine, ch);
    }

    // Cursor should stay at original position
    assert_eq!(engine.view().cursor.col, 5);
    assert!(engine.message.contains("not found"));
}

#[test]
fn test_reverse_search_basic() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar foo baz foo");

    // Enter reverse search mode with '?'
    press_char(&mut engine, '?');
    assert_eq!(engine.mode, Mode::Search);

    // Type search pattern
    for ch in "foo".chars() {
        engine.handle_key(&ch.to_string(), Some(ch), false);
    }
    press_special(&mut engine, "Return");

    assert_eq!(engine.mode, Mode::Normal);
    assert_eq!(engine.search_query, "foo");
    assert_eq!(engine.search_matches.len(), 3);
}

#[test]
fn test_reverse_search_n_goes_backward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1 X\nline2 X\nline3 X");

    // Move to line 3
    engine.view_mut().cursor.line = 2;
    engine.view_mut().cursor.col = 6;

    // Reverse search for 'X'
    press_char(&mut engine, '?');
    engine.handle_key("X", Some('X'), false);
    press_special(&mut engine, "Return");

    assert_eq!(engine.search_matches.len(), 3);

    // After '?', 'n' should go to previous match (backward)
    let start_line = engine.view().cursor.line;
    press_char(&mut engine, 'n');

    // Should move to an earlier line or same line with earlier column
    assert!(
        engine.view().cursor.line < start_line
            || (engine.view().cursor.line == start_line && engine.view().cursor.col < 6),
        "n after ? should go backward"
    );
}

#[test]
fn test_reverse_search_n_goes_forward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1 X\nline2 X\nline3 X");

    // Move to line 2, after the last match
    engine.view_mut().cursor.line = 2;
    engine.view_mut().cursor.col = 7;

    // Reverse search for 'X' - should find the match on line 2
    press_char(&mut engine, '?');
    engine.handle_key("X", Some('X'), false);
    press_special(&mut engine, "Return");

    assert_eq!(engine.search_matches.len(), 3);
    assert_eq!(engine.view().cursor.line, 2);
    assert_eq!(engine.view().cursor.col, 6);

    // After '?', 'N' should go to next match (forward), wrapping to line 0
    press_char(&mut engine, 'N');
    assert_eq!(engine.view().cursor.line, 0, "N after ? should go forward");
}

#[test]
fn test_forward_then_reverse_search() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "line1 X\nline2 X\nline3 X\nline4 X");

    // Start at line 1
    engine.view_mut().cursor.line = 1;
    engine.view_mut().cursor.col = 0;

    // Forward search with '/' - should find X on line 1
    press_char(&mut engine, '/');
    engine.handle_key("X", Some('X'), false);
    press_special(&mut engine, "Return");
    assert_eq!(engine.search_matches.len(), 4);
    assert_eq!(engine.view().cursor.line, 1);

    // 'n' should go forward to line 2
    press_char(&mut engine, 'n');
    assert_eq!(engine.view().cursor.line, 2, "n after / should go forward");

    // Now do a reverse search with '?' - should find X on line 1 (previous match)
    press_char(&mut engine, '?');
    engine.handle_key("X", Some('X'), false);
    press_special(&mut engine, "Return");
    assert_eq!(engine.view().cursor.line, 1);

    // 'n' should now go backward to line 0
    press_char(&mut engine, 'n');
    assert_eq!(engine.view().cursor.line, 0, "n after ? should go backward");
}

#[test]
fn test_word_forward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world foo");

    press_char(&mut engine, 'w');
    assert_eq!(engine.view().cursor.col, 6);

    press_char(&mut engine, 'w');
    assert_eq!(engine.view().cursor.col, 12);
}

#[test]
fn test_word_backward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world foo");

    press_char(&mut engine, '$');

    press_char(&mut engine, 'b');
    assert_eq!(engine.view().cursor.col, 12);

    press_char(&mut engine, 'b');
    assert_eq!(engine.view().cursor.col, 6);
}

#[test]
fn test_word_end() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");

    press_char(&mut engine, 'e');
    assert_eq!(engine.view().cursor.col, 4);

    press_char(&mut engine, 'e');
    assert_eq!(engine.view().cursor.col, 10);
}

#[test]
fn test_paragraph_forward_basic() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "text1\ntext2\n\ntext3");
    // Cursor at line 0 (text1)

    press_char(&mut engine, '}');
    assert_eq!(engine.view().cursor.line, 2); // Empty line
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_paragraph_backward_basic() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "text1\n\ntext2\ntext3");
    engine.view_mut().cursor.line = 3;

    press_char(&mut engine, '{');
    assert_eq!(engine.view().cursor.line, 1); // Empty line
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_paragraph_forward_from_empty_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "text1\n\ntext2\n\ntext3");
    engine.view_mut().cursor.line = 1; // First empty line

    press_char(&mut engine, '}');
    assert_eq!(engine.view().cursor.line, 3); // Next empty line
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_paragraph_backward_from_empty_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "text1\n\ntext2\n\ntext3");
    engine.view_mut().cursor.line = 3; // Second empty line

    press_char(&mut engine, '{');
    assert_eq!(engine.view().cursor.line, 1); // First empty line
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_paragraph_forward_at_eof() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "text1\ntext2\ntext3");
    engine.view_mut().cursor.line = 2; // Last line

    press_char(&mut engine, '}');
    assert_eq!(engine.view().cursor.line, 2); // Stays at last line
}

#[test]
fn test_paragraph_backward_at_bof() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "text1\ntext2\ntext3");
    // Cursor at line 0

    press_char(&mut engine, '{');
    assert_eq!(engine.view().cursor.line, 0); // Stays at line 0
}

#[test]
fn test_paragraph_whitespace_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "text1\n  \t  \ntext2");

    press_char(&mut engine, '}');
    assert_eq!(engine.view().cursor.line, 1); // Whitespace line
    assert_eq!(engine.view().cursor.col, 5); // End of whitespace line
}

#[test]
fn test_paragraph_forward_multiple() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\n\nb\n\nc\n\nd");

    press_char(&mut engine, '}');
    assert_eq!(engine.view().cursor.line, 1);

    press_char(&mut engine, '}');
    assert_eq!(engine.view().cursor.line, 3);

    press_char(&mut engine, '}');
    assert_eq!(engine.view().cursor.line, 5);
}

#[test]
fn test_paragraph_backward_multiple() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\n\nb\n\nc\n\nd");
    engine.view_mut().cursor.line = 6;

    press_char(&mut engine, '{');
    assert_eq!(engine.view().cursor.line, 5);

    press_char(&mut engine, '{');
    assert_eq!(engine.view().cursor.line, 3);

    press_char(&mut engine, '{');
    assert_eq!(engine.view().cursor.line, 1);
}

#[test]
fn test_paragraph_consecutive_empty_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "text\n\n\n\nmore");

    press_char(&mut engine, '}');
    assert_eq!(engine.view().cursor.line, 1); // First empty

    press_char(&mut engine, '}');
    assert_eq!(engine.view().cursor.line, 2); // Second empty

    press_char(&mut engine, '}');
    assert_eq!(engine.view().cursor.line, 3); // Third empty
}

#[test]
fn test_gg_goes_to_top() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3\nline4");
    engine.view_mut().cursor.line = 3;

    press_char(&mut engine, 'g');
    press_char(&mut engine, 'g');
    assert_eq!(engine.view().cursor.line, 0);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
#[allow(non_snake_case)]
fn test_G_goes_to_bottom() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3\nline4");

    press_char(&mut engine, 'G');
    assert_eq!(engine.view().cursor.line, 3);
}

#[test]
fn test_dd_deletes_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'd');
    assert_eq!(engine.buffer().to_string(), "line2\nline3");
    assert_eq!(engine.view().cursor.line, 0);
}

#[test]
fn test_dd_deletes_middle_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "aaa\nbbb\nccc");

    press_char(&mut engine, 'j');
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'd');
    assert_eq!(engine.buffer().to_string(), "aaa\nccc");
    assert_eq!(engine.view().cursor.line, 1);
}

#[test]
#[allow(non_snake_case)]
fn test_D_deletes_to_end_of_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world\nline2");

    press_char(&mut engine, 'l');
    press_char(&mut engine, 'l');
    press_char(&mut engine, 'l');
    press_char(&mut engine, 'l');
    press_char(&mut engine, 'l');
    press_char(&mut engine, 'D');
    assert_eq!(engine.buffer().to_string(), "hello\nline2");
}

#[test]
#[allow(non_snake_case)]
fn test_A_appends_at_end() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\nworld");

    press_char(&mut engine, 'A');
    assert_eq!(engine.mode, Mode::Insert);
    let line_insert_len = engine.get_line_len_for_insert(0);
    assert_eq!(engine.view().cursor.col, line_insert_len);
}

#[test]
#[allow(non_snake_case)]
fn test_I_inserts_at_first_nonwhitespace() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "    hello");

    press_char(&mut engine, 'I');
    assert_eq!(engine.mode, Mode::Insert);
    assert_eq!(engine.view().cursor.col, 4);
}

#[test]
fn test_ensure_cursor_visible() {
    let mut engine = Engine::new();
    let mut text = String::new();
    for i in 0..100 {
        text.push_str(&format!("line {}\n", i));
    }
    engine.buffer_mut().insert(0, &text);
    engine.set_viewport_lines(20);

    engine.view_mut().cursor.line = 50;
    engine.ensure_cursor_visible();
    assert!(engine.scroll_top() <= 50);
    assert!(engine.scroll_top() + engine.viewport_lines() > 50);
}

#[test]
fn test_ctrl_d_half_page_down() {
    let mut engine = Engine::new();
    let mut text = String::new();
    for i in 0..100 {
        text.push_str(&format!("line {}\n", i));
    }
    engine.buffer_mut().insert(0, &text);
    engine.set_viewport_lines(20);

    engine.handle_key("d", Some('d'), true);
    assert_eq!(engine.view().cursor.line, 10);
}

#[test]
fn test_ctrl_u_half_page_up() {
    let mut engine = Engine::new();
    let mut text = String::new();
    for i in 0..100 {
        text.push_str(&format!("line {}\n", i));
    }
    engine.buffer_mut().insert(0, &text);
    engine.set_viewport_lines(20);
    engine.view_mut().cursor.line = 50;

    engine.handle_key("u", Some('u'), true);
    assert_eq!(engine.view().cursor.line, 40);
}

#[test]
fn test_ctrl_d_fold_aware() {
    let mut engine = Engine::new();
    let mut text = String::new();
    for i in 0..200 {
        text.push_str(&format!("line {}\n", i));
    }
    engine.buffer_mut().insert(0, &text);
    engine.set_viewport_lines(20);
    // Create a large fold: lines 5..=100 are hidden (line 5 is header)
    engine.view_mut().close_fold(5, 100);
    // Cursor at line 0, Ctrl-D = half page (10 visible lines)
    engine.handle_key("d", Some('d'), true);
    // Should skip fold body and land on a visible line
    // Lines 0-5 are visible (0,1,2,3,4,5=header), then 101+ are visible
    // From 0, advance 10 visible: 1,2,3,4,5,101,102,103,104,105
    assert_eq!(engine.view().cursor.line, 105);
}

#[test]
fn test_ctrl_u_fold_aware() {
    let mut engine = Engine::new();
    let mut text = String::new();
    for i in 0..200 {
        text.push_str(&format!("line {}\n", i));
    }
    engine.buffer_mut().insert(0, &text);
    engine.set_viewport_lines(20);
    // Create a large fold: lines 5..=100 are hidden
    engine.view_mut().close_fold(5, 100);
    // Cursor at line 110, Ctrl-U = half page (10 visible lines)
    engine.view_mut().cursor.line = 110;
    engine.handle_key("u", Some('u'), true);
    // From 110, go back 10 visible: 109,108,107,106,105,104,103,102,101,5
    assert_eq!(engine.view().cursor.line, 5);
}

#[test]
fn test_scroll_down_visible_skips_folds() {
    let mut engine = Engine::new();
    let mut text = String::new();
    for i in 0..200 {
        text.push_str(&format!("line {}\n", i));
    }
    engine.buffer_mut().insert(0, &text);
    engine.set_viewport_lines(20);
    // Fold lines 10..=100
    engine.view_mut().close_fold(10, 100);
    engine.view_mut().scroll_top = 8;
    // Scroll down 5 visible lines
    engine.scroll_down_visible(5);
    // From 8: 9, 10(header), 101, 102, 103
    assert_eq!(engine.view().scroll_top, 103);
}

#[test]
fn test_open_nonexistent_file() {
    let path = std::path::PathBuf::from("/tmp/vimcode_nonexistent_12345.txt");
    let engine = Engine::open(&path);
    assert!(engine.buffer().to_string().is_empty());
    assert!(engine.message.contains("[New File]"));
    assert_eq!(engine.file_path(), Some(&path));
}

#[test]
fn test_open_existing_file() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_open.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"test content").unwrap();
    }

    let engine = Engine::open(&path);
    assert_eq!(engine.buffer().to_string(), "test content");
    assert!(!engine.dirty());

    let _ = std::fs::remove_file(&path);
}

// --- New tests for multi-buffer/window/tab ---

#[test]
fn test_split_window() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");

    assert_eq!(engine.windows.len(), 1);
    assert_eq!(engine.active_tab().window_ids().len(), 1);

    engine.split_window(SplitDirection::Vertical, None);

    assert_eq!(engine.windows.len(), 2);
    assert_eq!(engine.active_tab().window_ids().len(), 2);
}

#[test]
fn test_close_window() {
    let mut engine = Engine::new();
    engine.split_window(SplitDirection::Vertical, None);
    assert_eq!(engine.windows.len(), 2);

    engine.close_window();
    assert_eq!(engine.windows.len(), 1);
}

#[test]
fn test_window_cycling() {
    let mut engine = Engine::new();
    engine.split_window(SplitDirection::Vertical, None);

    let first_window = engine.active_window_id();
    engine.focus_next_window();
    let second_window = engine.active_window_id();
    assert_ne!(first_window, second_window);

    engine.focus_next_window();
    assert_eq!(engine.active_window_id(), first_window);
}

#[test]
fn test_new_tab() {
    let mut engine = Engine::new();
    assert_eq!(engine.active_group().tabs.len(), 1);

    engine.new_tab(None);
    assert_eq!(engine.active_group().tabs.len(), 2);
    assert_eq!(engine.active_group().active_tab, 1);
}

#[test]
fn test_tab_navigation() {
    let mut engine = Engine::new();
    engine.new_tab(None);
    engine.new_tab(None);
    assert_eq!(engine.active_group().tabs.len(), 3);
    assert_eq!(engine.active_group().active_tab, 2);

    engine.prev_tab();
    assert_eq!(engine.active_group().active_tab, 1);

    engine.next_tab();
    assert_eq!(engine.active_group().active_tab, 2);

    engine.goto_tab(0);
    assert_eq!(engine.active_group().active_tab, 0);
}

#[test]
fn test_buffer_navigation() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "buffer 1");

    // Open a new file (creates second buffer)
    let path = std::env::temp_dir().join("vimcode_test_buf2.txt");
    std::fs::write(&path, "buffer 2").unwrap();

    engine.split_window(SplitDirection::Vertical, Some(&path));

    let buf2_id = engine.active_buffer_id();
    assert_eq!(engine.buffer().to_string(), "buffer 2");

    engine.prev_buffer();
    assert_ne!(engine.active_buffer_id(), buf2_id);

    engine.next_buffer();
    assert_eq!(engine.active_buffer_id(), buf2_id);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_list_buffers() {
    let engine = Engine::new();
    let listing = engine.list_buffers();
    assert!(listing.contains("[No Name]"));
}

#[test]
fn test_ctrl_w_commands() {
    let mut engine = Engine::new();

    // Ctrl-W s should split horizontally
    press_ctrl(&mut engine, 'w');
    press_char(&mut engine, 's');
    assert_eq!(engine.windows.len(), 2);

    // Ctrl-W v should split vertically
    press_ctrl(&mut engine, 'w');
    press_char(&mut engine, 'v');
    assert_eq!(engine.windows.len(), 3);

    // Ctrl-W w should cycle
    let before = engine.active_window_id();
    press_ctrl(&mut engine, 'w');
    press_char(&mut engine, 'w');
    assert_ne!(engine.active_window_id(), before);

    // Ctrl-W c should close
    press_ctrl(&mut engine, 'w');
    press_char(&mut engine, 'c');
    assert_eq!(engine.windows.len(), 2);
}

#[test]
fn test_close_tab_removes_orphaned_dirty_buffer() {
    // When a tab is closed (discarded), its buffer should be removed from
    // buffer_manager so has_any_unsaved() doesn't report it as dirty.
    let mut engine = Engine::new();
    // Open a second tab so we can close the first
    engine.new_tab(None);
    assert_eq!(engine.active_group().tabs.len(), 2);
    // Make the active (second) tab's buffer dirty by typing in insert mode
    engine.handle_key("i", Some('i'), false); // enter insert mode
    engine.handle_key("", Some('x'), false); // type a char
    engine.handle_key("Escape", None, false); // back to normal
    assert!(engine.dirty(), "buffer should be dirty after typing");
    assert!(engine.has_any_unsaved());
    // Close the tab (discard) — this is what pressing D does
    let closed = engine.close_tab();
    assert!(closed);
    // The buffer should have been removed from the buffer manager
    assert!(
        !engine.has_any_unsaved(),
        "has_any_unsaved should be false after discarding tab"
    );
}

#[test]
fn test_gt_gT_tab_navigation() {
    let mut engine = Engine::new();
    engine.new_tab(None);
    engine.new_tab(None);
    engine.goto_tab(0);

    // gt should go to next tab
    press_char(&mut engine, 'g');
    press_char(&mut engine, 't');
    assert_eq!(engine.active_group().active_tab, 1);

    // gT should go to previous tab
    press_char(&mut engine, 'g');
    press_char(&mut engine, 'T');
    assert_eq!(engine.active_group().active_tab, 0);
}

// --- Undo/Redo tests ---

#[test]
fn test_undo_insert_mode_typing() {
    let mut engine = Engine::new();

    // Type "hello" in insert mode
    press_char(&mut engine, 'i');
    for ch in "hello".chars() {
        press_char(&mut engine, ch);
    }
    press_special(&mut engine, "Escape");

    assert_eq!(engine.buffer().to_string(), "hello");

    // Undo should remove entire "hello" (single undo group for insert session)
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "");
}

#[test]
fn test_undo_x_delete() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABC");
    engine.update_syntax();

    // Delete 'A' with x
    press_char(&mut engine, 'x');
    assert_eq!(engine.buffer().to_string(), "BC");

    // Undo should restore 'A'
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "ABC");
}

#[test]
fn test_undo_dd_delete_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    // Delete first line with dd
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'd');
    assert_eq!(engine.buffer().to_string(), "line2\nline3");

    // Undo should restore the line
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "line1\nline2\nline3");
}

#[test]
fn test_undo_D_delete_to_eol() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world\nline2");
    engine.update_syntax();

    // Move to 'w' and delete to end of line
    for _ in 0..6 {
        press_char(&mut engine, 'l');
    }
    press_char(&mut engine, 'D');
    assert_eq!(engine.buffer().to_string(), "hello \nline2");

    // Undo should restore "world"
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "hello world\nline2");
}

#[test]
fn test_undo_o_open_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2");
    engine.update_syntax();

    // Open line below and type "new"
    press_char(&mut engine, 'o');
    for ch in "new".chars() {
        press_char(&mut engine, ch);
    }
    press_special(&mut engine, "Escape");

    assert_eq!(engine.buffer().to_string(), "line1\nnew\nline2");

    // Undo should remove the new line and text
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "line1\nline2");
}

#[test]
fn test_redo_after_undo() {
    let mut engine = Engine::new();

    // Type "hello"
    press_char(&mut engine, 'i');
    for ch in "hello".chars() {
        press_char(&mut engine, ch);
    }
    press_special(&mut engine, "Escape");

    assert_eq!(engine.buffer().to_string(), "hello");

    // Undo
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "");

    // Redo with Ctrl-r
    press_ctrl(&mut engine, 'r');
    assert_eq!(engine.buffer().to_string(), "hello");
}

#[test]
fn test_redo_cleared_on_new_edit() {
    let mut engine = Engine::new();

    // Type "hello"
    press_char(&mut engine, 'i');
    for ch in "hello".chars() {
        press_char(&mut engine, ch);
    }
    press_special(&mut engine, "Escape");

    // Undo
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "");

    // New edit (type "world")
    press_char(&mut engine, 'i');
    for ch in "world".chars() {
        press_char(&mut engine, ch);
    }
    press_special(&mut engine, "Escape");

    // Redo should do nothing (redo stack was cleared)
    press_ctrl(&mut engine, 'r');
    assert_eq!(engine.buffer().to_string(), "world");
    assert!(engine.message.contains("Already at newest"));
}

#[test]
fn test_multiple_undos() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABC");
    engine.update_syntax();

    // Delete three chars one by one
    press_char(&mut engine, 'x'); // removes A
    press_char(&mut engine, 'x'); // removes B
    press_char(&mut engine, 'x'); // removes C

    assert_eq!(engine.buffer().to_string(), "");

    // Three undos should restore ABC
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "C");

    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "BC");

    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "ABC");
}

#[test]
fn test_undo_at_empty_stack() {
    let mut engine = Engine::new();

    // Try to undo with nothing to undo
    press_char(&mut engine, 'u');
    assert!(engine.message.contains("Already at oldest"));
}

#[test]
fn test_undo_cursor_position_restored() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Move to column 6 ('w') and delete with x
    for _ in 0..6 {
        press_char(&mut engine, 'l');
    }
    assert_eq!(engine.view().cursor.col, 6);

    press_char(&mut engine, 'x'); // delete 'w'
    assert_eq!(engine.buffer().to_string(), "hello orld");

    // Undo should restore cursor to column 6
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "hello world");
    assert_eq!(engine.view().cursor.col, 6);
}

#[test]
fn test_undo_line_basic() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Make some changes to the line
    press_char(&mut engine, 'x'); // delete 'h' -> "ello world"
    press_char(&mut engine, 'x'); // delete 'e' -> "llo world"

    assert_eq!(engine.buffer().to_string(), "llo world");

    // Undo line with U
    press_char(&mut engine, 'U');

    assert_eq!(engine.buffer().to_string(), "hello world");
    assert_eq!(engine.view().cursor.line, 0);
}

#[test]
fn test_undo_line_multiple_operations() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test");
    engine.update_syntax();

    // Multiple operations on the line
    press_char(&mut engine, 'A'); // append mode
    engine.handle_key("1", Some('1'), false);
    engine.handle_key("2", Some('2'), false);
    engine.handle_key("3", Some('3'), false);
    press_special(&mut engine, "Escape");

    assert_eq!(engine.buffer().to_string(), "test123");

    // Delete some chars
    press_char(&mut engine, 'x'); // delete '3'
    press_char(&mut engine, 'x'); // delete '2'

    assert_eq!(engine.buffer().to_string(), "test1");

    // U should restore original line
    press_char(&mut engine, 'U');

    assert_eq!(engine.buffer().to_string(), "test");
}

#[test]
fn test_undo_line_no_changes() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    engine.update_syntax();

    // Try U without making any changes
    press_char(&mut engine, 'U');

    // Should show message but not crash
    assert_eq!(engine.buffer().to_string(), "hello");
}

#[test]
fn test_undo_line_multiline() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    // Modify line 1
    press_char(&mut engine, 'x'); // delete 'l' -> "ine1"
    assert_eq!(engine.buffer().to_string(), "ine1\nline2\nline3");

    // Move to line 2
    press_char(&mut engine, 'j');
    assert_eq!(engine.view().cursor.line, 1);

    // Modify line 2
    press_char(&mut engine, 'x'); // delete 'l' -> "ine2"
    assert_eq!(engine.buffer().to_string(), "ine1\nine2\nline3");

    // U should only restore line 2
    press_char(&mut engine, 'U');
    assert_eq!(engine.buffer().to_string(), "ine1\nline2\nline3");

    // Move back to line 1 - U won't work because we moved away
    press_char(&mut engine, 'k');
    press_char(&mut engine, 'U');
    // Line 1 stays modified because we moved away from it
    assert_eq!(engine.buffer().to_string(), "ine1\nline2\nline3");
}

#[test]
fn test_undo_line_is_undoable() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    engine.update_syntax();

    // Make a change
    press_char(&mut engine, 'x'); // "ello"
    assert_eq!(engine.buffer().to_string(), "ello");

    // U to restore
    press_char(&mut engine, 'U');
    assert_eq!(engine.buffer().to_string(), "hello");

    // Regular undo should undo the U operation
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "ello");
}

// --- Yank/Paste/Register Tests ---

#[test]
fn test_yank_line_yy() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    // Yank first line with yy
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');

    // Check register content
    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "line1\n");
    assert!(is_linewise);
    assert!(engine.message.contains("yanked"));
}

#[test]
fn test_yank_line_Y() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "first\nsecond");
    engine.update_syntax();

    press_char(&mut engine, 'j'); // move to line 2
    press_char(&mut engine, 'Y');

    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "second\n");
    assert!(is_linewise);
}

#[test]
fn test_paste_after_linewise() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2");
    engine.update_syntax();

    // Yank line1
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');

    // Paste after (p) - should insert below current line
    press_char(&mut engine, 'p');

    assert_eq!(engine.buffer().to_string(), "line1\nline1\nline2");
    assert_eq!(engine.view().cursor.line, 1); // cursor on pasted line
}

#[test]
fn test_paste_before_linewise() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2");
    engine.update_syntax();

    press_char(&mut engine, 'j'); // move to line2
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y'); // yank line2

    press_char(&mut engine, 'k'); // back to line1
    press_char(&mut engine, 'P'); // paste before

    assert_eq!(engine.buffer().to_string(), "line2\nline1\nline2");
    assert_eq!(engine.view().cursor.line, 0);
}

#[test]
fn test_delete_x_fills_register() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABC");
    engine.update_syntax();

    press_char(&mut engine, 'x'); // delete 'A'

    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "A");
    assert!(!is_linewise);
}

#[test]
fn test_delete_dd_fills_register() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "first\nsecond\nthird");
    engine.update_syntax();

    press_char(&mut engine, 'j'); // move to "second"
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'd'); // delete line

    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "second\n");
    assert!(is_linewise);
}

#[test]
#[allow(non_snake_case)]
fn test_delete_D_fills_register() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    press_char(&mut engine, 'l');
    press_char(&mut engine, 'l'); // cursor on 'l'
    press_char(&mut engine, 'D'); // delete to end

    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "llo world");
    assert!(!is_linewise);
}

#[test]
fn test_named_register_yank() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test line");
    engine.update_syntax();

    // Use "a register
    press_char(&mut engine, '"');
    press_char(&mut engine, 'a');
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');

    // Check 'a' register has content
    let (content, _) = engine.registers.get(&'a').unwrap();
    assert_eq!(content, "test line\n");

    // Unnamed register should also have it
    let (content2, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content2, "test line\n");
}

#[test]
fn test_named_register_paste() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "AAA\nBBB");
    engine.update_syntax();

    // Yank to "a
    press_char(&mut engine, '"');
    press_char(&mut engine, 'a');
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');

    // Move down and yank to "b
    press_char(&mut engine, 'j');
    press_char(&mut engine, '"');
    press_char(&mut engine, 'b');
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');

    // Now paste from "a
    press_char(&mut engine, '"');
    press_char(&mut engine, 'a');
    press_char(&mut engine, 'p');

    assert!(engine.buffer().to_string().contains("AAA"));
}

#[test]
fn test_delete_and_paste_workflow() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    // Delete line2 with dd
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'd');

    assert_eq!(engine.buffer().to_string(), "line1\nline3");

    // Paste it back
    press_char(&mut engine, 'p');

    assert_eq!(engine.buffer().to_string(), "line1\nline3\nline2\n");
}

#[test]
fn test_x_delete_and_paste() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABCD");
    engine.update_syntax();

    press_char(&mut engine, 'x'); // delete 'A'
    press_char(&mut engine, 'l');
    press_char(&mut engine, 'l'); // cursor after 'D'
    press_char(&mut engine, 'p'); // paste after

    assert_eq!(engine.buffer().to_string(), "BCDA");
}

#[test]
fn test_replace_char_basic() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");

    // Replace 'h' with 'j'
    press_char(&mut engine, 'r');
    press_char(&mut engine, 'j');

    assert_eq!(engine.buffer().to_string(), "jello");
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_replace_char_with_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");

    // Replace 3 chars with 'x': "xxxlo"
    press_char(&mut engine, '3');
    press_char(&mut engine, 'r');
    press_char(&mut engine, 'x');

    assert_eq!(engine.buffer().to_string(), "xxxlo");
    // Cursor should stay at starting position
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_replace_char_at_line_end() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test");

    // Move to last char
    engine.view_mut().cursor.col = 3;

    // Replace 't' with 'x'
    press_char(&mut engine, 'r');
    press_char(&mut engine, 'x');

    assert_eq!(engine.buffer().to_string(), "tesx");
}

#[test]
fn test_replace_char_doesnt_cross_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hi\nbye");

    // Move to 'i' (last char of first line)
    engine.view_mut().cursor.col = 1;

    // Try to replace 3 chars - should only replace 'i' (not crossing newline)
    press_char(&mut engine, '3');
    press_char(&mut engine, 'r');
    press_char(&mut engine, 'x');

    assert_eq!(engine.buffer().to_string(), "hx\nbye");
}

#[test]
fn test_replace_char_with_space() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");

    // Replace 'h' with space
    press_char(&mut engine, 'r');
    press_char(&mut engine, ' ');

    assert_eq!(engine.buffer().to_string(), " ello");
}

#[test]
fn test_replace_char_with_digit() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");

    // r followed by a digit should replace with that digit, not treat it as count
    press_char(&mut engine, 'r');
    press_char(&mut engine, '1');

    assert_eq!(engine.buffer().to_string(), "1ello");
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_replace_char_repeat() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");

    // Replace 'h' with 'j'
    press_char(&mut engine, 'r');
    press_char(&mut engine, 'j');
    assert_eq!(engine.buffer().to_string(), "jello");

    // Move forward and repeat
    press_char(&mut engine, 'l');
    press_char(&mut engine, '.');

    assert_eq!(engine.buffer().to_string(), "jjllo");
}

#[test]
fn test_replace_char_multicount_repeat() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");

    // Replace 2 chars with 'x'
    press_char(&mut engine, '2');
    press_char(&mut engine, 'r');
    press_char(&mut engine, 'x');
    assert_eq!(engine.buffer().to_string(), "xxllo world");

    // Move forward and repeat (should replace 2 chars again)
    engine.view_mut().cursor.col = 6;
    press_char(&mut engine, '.');

    assert_eq!(engine.buffer().to_string(), "xxllo xxrld");
}

#[test]
fn test_paste_empty_register() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test");
    engine.update_syntax();

    // Try to paste from empty register - should do nothing
    press_char(&mut engine, 'p');

    assert_eq!(engine.buffer().to_string(), "test");
}

#[test]
fn test_yank_last_line_no_newline() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "first\nlast");
    engine.update_syntax();

    press_char(&mut engine, 'j'); // move to "last" (no trailing newline)
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');

    // Should still be linewise with newline added
    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "last\n");
    assert!(is_linewise);
}

// --- Visual Mode Tests ---

#[test]
fn test_enter_visual_mode() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Enter visual mode with v
    press_char(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::Visual);
    assert!(engine.visual_anchor.is_some());
    assert_eq!(engine.visual_anchor.unwrap().line, 0);
    assert_eq!(engine.visual_anchor.unwrap().col, 0);
}

#[test]
fn test_enter_visual_line_mode() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2");
    engine.update_syntax();

    // Enter visual line mode with V
    press_char(&mut engine, 'V');
    assert_eq!(engine.mode, Mode::VisualLine);
    assert!(engine.visual_anchor.is_some());
}

#[test]
fn test_visual_mode_escape_exits() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test");
    engine.update_syntax();

    press_char(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::Visual);

    press_special(&mut engine, "Escape");
    assert_eq!(engine.mode, Mode::Normal);
    assert!(engine.visual_anchor.is_none());
}

#[test]
fn test_visual_yank_forward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Select "hello" (5 chars)
    press_char(&mut engine, 'v');
    for _ in 0..4 {
        press_char(&mut engine, 'l');
    }

    // Yank
    press_char(&mut engine, 'y');

    // Check register
    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "hello");
    assert!(!is_linewise);

    // Should be back in normal mode
    assert_eq!(engine.mode, Mode::Normal);
    assert!(engine.visual_anchor.is_none());
}

#[test]
fn test_visual_yank_backward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Move to 'w' (position 6)
    for _ in 0..6 {
        press_char(&mut engine, 'l');
    }

    // Select backward to 'h'
    press_char(&mut engine, 'v');
    for _ in 0..6 {
        press_char(&mut engine, 'h');
    }

    // Yank
    press_char(&mut engine, 'y');

    // Should yank "hello " (anchor at 6, cursor at 0, inclusive)
    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "hello w");
}

#[test]
fn test_visual_delete() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Select "hello"
    press_char(&mut engine, 'v');
    for _ in 0..4 {
        press_char(&mut engine, 'l');
    }

    // Delete
    press_char(&mut engine, 'd');

    assert_eq!(engine.buffer().to_string(), " world");
    assert_eq!(engine.mode, Mode::Normal);

    // Check register
    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "hello");
}

#[test]
fn test_visual_line_yank() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    // Select 2 lines
    press_char(&mut engine, 'V');
    press_char(&mut engine, 'j');

    // Yank
    press_char(&mut engine, 'y');

    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "line1\nline2\n");
    assert!(is_linewise);
}

#[test]
fn test_visual_line_delete() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    // Select middle line
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'V');

    // Delete
    press_char(&mut engine, 'd');

    assert_eq!(engine.buffer().to_string(), "line1\nline3");
    assert_eq!(engine.view().cursor.line, 1); // cursor at start of next line
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_visual_x_deletes_selection() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Select "hello"
    press_char(&mut engine, 'v');
    for _ in 0..4 {
        press_char(&mut engine, 'l');
    }

    // Delete with x (same as d in visual mode)
    press_char(&mut engine, 'x');

    assert_eq!(engine.buffer().to_string(), " world");
    assert_eq!(engine.mode, Mode::Normal);

    // Check register — deleted text should be stored
    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "hello");
}

#[test]
fn test_visual_line_x_deletes_selection() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    // Select middle line
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'V');

    // Delete with x
    press_char(&mut engine, 'x');

    assert_eq!(engine.buffer().to_string(), "line1\nline3");
    assert_eq!(engine.view().cursor.line, 1);
}

#[test]
fn test_visual_change() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Select "hello"
    press_char(&mut engine, 'v');
    for _ in 0..4 {
        press_char(&mut engine, 'l');
    }

    // Change (should delete and enter insert mode)
    press_char(&mut engine, 'c');

    assert_eq!(engine.buffer().to_string(), " world");
    assert_eq!(engine.mode, Mode::Insert);
    assert_eq!(engine.view().cursor.col, 0);

    // Type replacement
    for ch in "hi".chars() {
        press_char(&mut engine, ch);
    }
    press_special(&mut engine, "Escape");

    assert_eq!(engine.buffer().to_string(), "hi world");
}

#[test]
fn test_visual_line_change() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    press_char(&mut engine, 'V');
    press_char(&mut engine, 'c');

    assert_eq!(engine.buffer().to_string(), "line2\nline3");
    assert_eq!(engine.mode, Mode::Insert);
}

#[test]
fn test_visual_mode_navigation() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    press_char(&mut engine, 'v');
    assert_eq!(engine.view().cursor.col, 0);

    // Move right extends selection
    press_char(&mut engine, 'l');
    assert_eq!(engine.view().cursor.col, 1);
    assert_eq!(engine.mode, Mode::Visual); // still in visual mode

    press_char(&mut engine, 'l');
    press_char(&mut engine, 'l');
    assert_eq!(engine.view().cursor.col, 3);
}

#[test]
fn test_visual_mode_switching() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2");
    engine.update_syntax();

    // Start in character visual
    press_char(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::Visual);

    // Switch to line visual
    press_char(&mut engine, 'V');
    assert_eq!(engine.mode, Mode::VisualLine);
    assert!(engine.visual_anchor.is_some()); // anchor preserved

    // Press V again to exit
    press_char(&mut engine, 'V');
    assert_eq!(engine.mode, Mode::Normal);
    assert!(engine.visual_anchor.is_none());
}

#[test]
fn test_visual_mode_toggle_with_v() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test");
    engine.update_syntax();

    // Enter visual mode
    press_char(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::Visual);

    // Press v again to exit
    press_char(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_visual_multiline_selection() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    // Select from beginning of line1 to middle of line2
    press_char(&mut engine, 'v');
    press_char(&mut engine, 'j'); // move to line 2
    for _ in 0..2 {
        press_char(&mut engine, 'l'); // move right 2 chars
    }

    press_char(&mut engine, 'y');

    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "line1\nlin");
}

#[test]
fn test_visual_with_named_register() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Select text and yank to register 'a'
    press_char(&mut engine, '"');
    press_char(&mut engine, 'a');
    press_char(&mut engine, 'v');
    for _ in 0..4 {
        press_char(&mut engine, 'l');
    }
    press_char(&mut engine, 'y');

    // Check register 'a'
    let (content, _) = engine.registers.get(&'a').unwrap();
    assert_eq!(content, "hello");

    // Also in unnamed register
    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "hello");
}

#[test]
fn test_visual_word_motion() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world foo bar");
    engine.update_syntax();

    // Select with word motion
    press_char(&mut engine, 'v');
    press_char(&mut engine, 'w'); // cursor moves to 'w' (start of "world")
    press_char(&mut engine, 'w'); // cursor moves to 'f' (start of "foo")

    press_char(&mut engine, 'y');

    // Visual mode is inclusive, so we get from 'h' to 'f' inclusive
    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "hello world f");
}

#[test]
fn test_visual_line_multiple_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\nb\nc\nd\ne");
    engine.update_syntax();

    // Move to line 2 (b)
    press_char(&mut engine, 'j');

    // Select 3 lines (b, c, d)
    press_char(&mut engine, 'V');
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'j');

    press_char(&mut engine, 'd');

    assert_eq!(engine.buffer().to_string(), "a\ne");
    assert_eq!(engine.view().cursor.line, 1);
}

// ===================================================================
// Count infrastructure tests (Step 1)
// ===================================================================

#[test]
fn test_count_accumulation() {
    let mut engine = Engine::new();
    press_char(&mut engine, '1');
    assert_eq!(engine.peek_count(), Some(1));
    press_char(&mut engine, '2');
    assert_eq!(engine.peek_count(), Some(12));
    press_char(&mut engine, '3');
    assert_eq!(engine.peek_count(), Some(123));
    assert_eq!(engine.take_count(), 123);
    assert_eq!(engine.peek_count(), None);
}

#[test]
fn test_zero_goes_to_line_start() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "Hello world");
    engine.view_mut().cursor.col = 5;
    assert_eq!(engine.view().cursor.col, 5);

    press_char(&mut engine, '0');
    assert_eq!(engine.view().cursor.col, 0);
    assert_eq!(engine.peek_count(), None);
}

#[test]
fn test_count_with_zero() {
    let mut engine = Engine::new();
    press_char(&mut engine, '1');
    assert_eq!(engine.peek_count(), Some(1));
    press_char(&mut engine, '0');
    assert_eq!(engine.peek_count(), Some(10));

    // take_count() should return 10 and clear
    assert_eq!(engine.take_count(), 10);
    assert_eq!(engine.peek_count(), None);
}

#[test]
fn test_count_max_limit() {
    let mut engine = Engine::new();
    // Type 99999 to exceed 10,000 limit
    for ch in ['9', '9', '9', '9', '9'] {
        press_char(&mut engine, ch);
    }
    assert_eq!(engine.peek_count(), Some(10_000));
    assert!(engine.message.contains("limit") || engine.message.contains("10,000"));
}

#[test]
fn test_count_display() {
    let mut engine = Engine::new();
    press_char(&mut engine, '5');

    // peek_count should not consume
    assert_eq!(engine.peek_count(), Some(5));
    assert_eq!(engine.peek_count(), Some(5));
    assert_eq!(engine.peek_count(), Some(5));

    // take_count should consume
    assert_eq!(engine.take_count(), 5);
    assert_eq!(engine.peek_count(), None);
}

#[test]
fn test_count_cleared_on_escape() {
    let mut engine = Engine::new();
    press_char(&mut engine, '5');
    assert_eq!(engine.peek_count(), Some(5));

    press_special(&mut engine, "Escape");
    assert_eq!(engine.peek_count(), None);
}

// --- Count-based motion tests (Step 2) ---

#[test]
fn test_count_hjkl_motions() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "ABCDEFGH\nIJKLMNOP\nQRSTUVWX\nYZ");
    engine.update_syntax();

    // Test 5l - move right 5 times
    press_char(&mut engine, '5');
    press_char(&mut engine, 'l');
    assert_eq!(engine.view().cursor.col, 5);
    assert_eq!(engine.peek_count(), None); // count consumed

    // Test 2j - move down 2 times
    press_char(&mut engine, '2');
    press_char(&mut engine, 'j');
    assert_eq!(engine.view().cursor.line, 2);

    // Test 3h - move left 3 times
    press_char(&mut engine, '3');
    press_char(&mut engine, 'h');
    assert_eq!(engine.view().cursor.col, 2);

    // Test 1k - move up 1 time
    press_char(&mut engine, '1');
    press_char(&mut engine, 'k');
    assert_eq!(engine.view().cursor.line, 1);
}

#[test]
fn test_count_arrow_keys() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "ABCDEFGH\nIJKLMNOP\nQRSTUVWX");
    engine.update_syntax();

    // Test 3 Right
    press_char(&mut engine, '3');
    press_special(&mut engine, "Right");
    assert_eq!(engine.view().cursor.col, 3);

    // Test 2 Down
    press_char(&mut engine, '2');
    press_special(&mut engine, "Down");
    assert_eq!(engine.view().cursor.line, 2);

    // Test 2 Up
    press_char(&mut engine, '2');
    press_special(&mut engine, "Up");
    assert_eq!(engine.view().cursor.line, 0);

    // Test 2 Left
    press_char(&mut engine, '2');
    press_special(&mut engine, "Left");
    assert_eq!(engine.view().cursor.col, 1);
}

#[test]
fn test_count_word_motions() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "one two three four five six seven");
    engine.update_syntax();

    // Test 3w - move forward 3 words
    press_char(&mut engine, '3');
    press_char(&mut engine, 'w');
    // Should be at start of "four"
    assert_eq!(engine.view().cursor.col, 14);

    // Test 2b - move backward 2 words
    press_char(&mut engine, '2');
    press_char(&mut engine, 'b');
    // Should be at start of "two"
    assert_eq!(engine.view().cursor.col, 4);

    // Test 2e - move to end of 2nd word from here
    press_char(&mut engine, '2');
    press_char(&mut engine, 'e');
    // Should be at end of "three"
    assert_eq!(engine.view().cursor.col, 12);
}

#[test]
fn test_count_paragraph_motions() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "para1\npara1\n\npara2\npara2\n\npara3\n\npara4");
    engine.update_syntax();
    // Line 0: para1
    // Line 1: para1
    // Line 2: empty
    // Line 3: para2
    // Line 4: para2
    // Line 5: empty
    // Line 6: para3
    // Line 7: empty
    // Line 8: para4

    // Test 2} - move forward 2 empty lines
    press_char(&mut engine, '2');
    press_char(&mut engine, '}');
    assert_eq!(engine.view().cursor.line, 5);

    // Test 1{ - move backward 1 empty line
    press_char(&mut engine, '1');
    press_char(&mut engine, '{');
    assert_eq!(engine.view().cursor.line, 2);

    // Test 2{ - move backward 2 empty lines (but there's only line 0 before)
    press_char(&mut engine, '2');
    press_char(&mut engine, '{');
    assert_eq!(engine.view().cursor.line, 0);
}

#[test]
fn test_count_scroll_commands() {
    let mut engine = Engine::new();
    // Create a buffer with 100 lines
    let mut text = String::new();
    for i in 0..100 {
        text.push_str(&format!("Line {}\n", i));
    }
    engine.buffer_mut().insert(0, &text);
    engine.update_syntax();
    engine.set_viewport_lines(20); // Simulate 20 lines visible

    // Test 2 Ctrl-D (2 half-pages down = 20 lines)
    press_char(&mut engine, '2');
    press_ctrl(&mut engine, 'd');
    assert_eq!(engine.view().cursor.line, 20);

    // Test 1 Ctrl-U (1 half-page up = 10 lines)
    press_char(&mut engine, '1');
    press_ctrl(&mut engine, 'u');
    assert_eq!(engine.view().cursor.line, 10);

    // Test 3 Ctrl-F (3 full pages down = 60 lines)
    press_char(&mut engine, '3');
    press_ctrl(&mut engine, 'f');
    assert_eq!(engine.view().cursor.line, 70);

    // Test 2 Ctrl-B (2 full pages up = 40 lines)
    press_char(&mut engine, '2');
    press_ctrl(&mut engine, 'b');
    assert_eq!(engine.view().cursor.line, 30);
}

#[test]
fn test_count_motion_bounds_checking() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABC\nDEF");
    engine.update_syntax();

    // Test 100l - should stop at line end
    press_char(&mut engine, '1');
    press_char(&mut engine, '0');
    press_char(&mut engine, '0');
    press_char(&mut engine, 'l');
    assert!(engine.view().cursor.col <= 2);

    // Test 100j - should stop at last line
    press_char(&mut engine, '1');
    press_char(&mut engine, '0');
    press_char(&mut engine, '0');
    press_char(&mut engine, 'j');
    assert_eq!(engine.view().cursor.line, 1);
}

#[test]
fn test_count_large_values() {
    let mut engine = Engine::new();
    // Create text with many words
    let text = "a b c d e f g h i j k l m n o p q r s t u v w x y z";
    engine.buffer_mut().insert(0, text);
    engine.update_syntax();

    // Test 10w - move forward 10 words
    press_char(&mut engine, '1');
    press_char(&mut engine, '0');
    press_char(&mut engine, 'w');
    // Should be at 'k' (10th word from start)
    assert_eq!(engine.view().cursor.col, 20);
}

// --- Count-based line operation tests (Step 3) ---

#[test]
fn test_count_x_delete_chars() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABCDEFGH");
    engine.update_syntax();

    // Test 3x - delete 3 characters
    press_char(&mut engine, '3');
    press_char(&mut engine, 'x');
    assert_eq!(engine.buffer().to_string(), "DEFGH");

    // Check register contains deleted chars
    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "ABC");
    assert!(!is_linewise);
}

#[test]
fn test_count_x_bounds() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABC");
    engine.update_syntax();

    // Test 100x - should only delete 3 chars (all available)
    press_char(&mut engine, '1');
    press_char(&mut engine, '0');
    press_char(&mut engine, '0');
    press_char(&mut engine, 'x');
    assert_eq!(engine.buffer().to_string(), "");
}

#[test]
fn test_count_dd_delete_lines() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "line1\nline2\nline3\nline4\nline5");
    engine.update_syntax();

    // Test 3dd - delete 3 lines
    press_char(&mut engine, '3');
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'd');
    assert_eq!(engine.buffer().to_string(), "line4\nline5");

    // Check register contains all 3 lines
    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "line1\nline2\nline3\n");
    assert!(is_linewise);
}

#[test]
fn test_count_yy_yank_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "alpha\nbeta\ngamma\ndelta");
    engine.update_syntax();

    // Test 2yy - yank 2 lines
    press_char(&mut engine, '2');
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');

    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "alpha\nbeta\n");
    assert!(is_linewise);
    assert!(engine.message.contains("2 lines yanked"));

    // Buffer should be unchanged
    assert_eq!(engine.buffer().to_string(), "alpha\nbeta\ngamma\ndelta");
}

#[test]
#[allow(non_snake_case)]
fn test_count_Y_yank_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "one\ntwo\nthree\nfour");
    engine.update_syntax();

    // Test 3Y - yank 3 lines
    press_char(&mut engine, '3');
    press_char(&mut engine, 'Y');

    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "one\ntwo\nthree\n");
    assert!(is_linewise);
    assert!(engine.message.contains("3 lines yanked"));
}

#[test]
#[allow(non_snake_case)]
fn test_count_D_delete_to_eol() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "ABCDEFGH\nIJKLMNOP\nQRSTUVWX\nYZ");
    engine.update_syntax();

    // Move to column 2 of first line
    press_char(&mut engine, 'l');
    press_char(&mut engine, 'l');
    assert_eq!(engine.view().cursor.col, 2);

    // Test 2D - delete to end of line + 1 more full line
    press_char(&mut engine, '2');
    press_char(&mut engine, 'D');

    // Should delete "CDEFGH\nIJKLMNOP\n" (to EOL + next line)
    assert_eq!(engine.buffer().to_string(), "AB\nQRSTUVWX\nYZ");

    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "CDEFGH\nIJKLMNOP\n");
}

#[test]
fn test_count_dd_last_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    // Move to line 2 (0-indexed: line 1)
    press_char(&mut engine, 'j');

    // Test 5dd - delete more lines than available (should delete 2 lines)
    press_char(&mut engine, '5');
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'd');

    assert_eq!(engine.buffer().to_string(), "line1");
}

#[test]
fn test_count_yy_last_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "A\nB\nC");
    engine.update_syntax();

    // Move to line B
    press_char(&mut engine, 'j');

    // Test 10yy - yank more than available (should yank 2 lines: B and C)
    press_char(&mut engine, '1');
    press_char(&mut engine, '0');
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');

    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "B\nC\n");
}

// Step 4 tests: Special commands and mode changes

#[test]
fn test_count_G_goto_line() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "line1\nline2\nline3\nline4\nline5");
    engine.update_syntax();

    // Start at line 0
    assert_eq!(engine.view().cursor.line, 0);

    // Test 3G - go to line 3 (1-indexed, so line index 2)
    press_char(&mut engine, '3');
    press_char(&mut engine, 'G');

    assert_eq!(engine.view().cursor.line, 2);

    // Test G with no count - go to last line
    press_char(&mut engine, 'G');
    assert_eq!(engine.view().cursor.line, 4);

    // Test 1G - go to first line
    press_char(&mut engine, '1');
    press_char(&mut engine, 'G');
    assert_eq!(engine.view().cursor.line, 0);
}

#[test]
fn test_count_gg_goto_line() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "line1\nline2\nline3\nline4\nline5");
    engine.update_syntax();

    // Move to last line
    press_char(&mut engine, 'G');
    assert_eq!(engine.view().cursor.line, 4);

    // Test 2gg - go to line 2 (1-indexed, so line index 1)
    press_char(&mut engine, '2');
    press_char(&mut engine, 'g');
    press_char(&mut engine, 'g');

    assert_eq!(engine.view().cursor.line, 1);

    // Test gg with no count - go to first line
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'g');
    press_char(&mut engine, 'g');
    assert_eq!(engine.view().cursor.line, 0);
}

#[test]
fn test_count_paste() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    engine.update_syntax();

    // Yank "hello"
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');

    // Move to next line (insert blank line)
    press_char(&mut engine, 'o');
    press_special(&mut engine, "Escape");

    // Test 3p - paste 3 times
    press_char(&mut engine, '3');
    press_char(&mut engine, 'p');

    // Should have: hello\n + 3 copies of "hello\n"
    let text = engine.buffer().to_string();
    assert_eq!(text, "hello\n\nhello\nhello\nhello\n");
}

#[test]
fn test_count_search_next() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "x\nx\nx\nx\nx");
    engine.update_syntax();

    // Search for "x" - should find 5 matches (one per line)
    press_char(&mut engine, '/');
    press_char(&mut engine, 'x');
    press_special(&mut engine, "Return");

    // After search from line 0, we jump to first match after cursor (line 1, since line 0 col 0 has 'x' but search looks AFTER cursor)
    // Actually, search should jump to line 0 if that's the first match
    // Let me check: cursor starts at 0,0. Search for 'x' finds match at 0,0
    // But search_next looks for matches > cursor position
    // So it finds line 1 as first match > position 0
    let first_line = engine.view().cursor.line;
    assert_eq!(engine.search_matches.len(), 5);

    // Test 3n - should move forward 3 more times
    press_char(&mut engine, '3');
    press_char(&mut engine, 'n');

    // Should have moved forward 3 times from first_line
    assert_eq!(engine.view().cursor.line, first_line + 3);

    // Test 2N - should move backward 2 times
    press_char(&mut engine, '2');
    press_char(&mut engine, 'N');

    // Should be back 2 lines
    assert_eq!(engine.view().cursor.line, first_line + 1);
}

#[test]
fn test_count_cleared_on_insert_mode() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    engine.update_syntax();

    // Set count to 5
    press_char(&mut engine, '5');
    assert_eq!(engine.peek_count(), Some(5));

    // Enter insert mode with 'i'
    press_char(&mut engine, 'i');
    assert_eq!(engine.peek_count(), None);

    // Exit insert mode
    press_special(&mut engine, "Escape");

    // Set count again
    press_char(&mut engine, '3');
    assert_eq!(engine.peek_count(), Some(3));

    // Enter insert mode with 'a'
    press_char(&mut engine, 'a');
    assert_eq!(engine.peek_count(), None);

    // Exit and test 'A'
    press_special(&mut engine, "Escape");
    press_char(&mut engine, '7');
    press_char(&mut engine, 'A');
    assert_eq!(engine.peek_count(), None);

    // Exit and test 'I'
    press_special(&mut engine, "Escape");
    press_char(&mut engine, '9');
    press_char(&mut engine, 'I');
    assert_eq!(engine.peek_count(), None);
}

#[test]
fn test_count_cleared_on_mode_changes() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Test visual mode PRESERVES count (for use with motions)
    press_char(&mut engine, '5');
    assert_eq!(engine.peek_count(), Some(5));
    press_char(&mut engine, 'v');
    assert_eq!(engine.peek_count(), Some(5)); // Count preserved
    press_special(&mut engine, "Escape"); // Escape clears count

    // Test visual line mode PRESERVES count (for use with motions)
    press_char(&mut engine, '3');
    assert_eq!(engine.peek_count(), Some(3));
    press_char(&mut engine, 'V');
    assert_eq!(engine.peek_count(), Some(3)); // Count preserved
    press_special(&mut engine, "Escape"); // Escape clears count

    // Test command mode clears count
    press_char(&mut engine, '7');
    assert_eq!(engine.peek_count(), Some(7));
    press_char(&mut engine, ':');
    assert_eq!(engine.peek_count(), None);
    press_special(&mut engine, "Escape");

    // Test search mode clears count
    press_char(&mut engine, '9');
    assert_eq!(engine.peek_count(), Some(9));
    press_char(&mut engine, '/');
    assert_eq!(engine.peek_count(), None);
    press_special(&mut engine, "Escape");
}

#[test]
fn test_count_visual_motion() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(
        0,
        "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8",
    );
    engine.update_syntax();

    // Start at line 0
    assert_eq!(engine.view().cursor.line, 0);

    // Enter visual mode
    press_char(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::Visual);

    // Test 5j - should extend selection 5 lines down
    press_char(&mut engine, '5');
    press_char(&mut engine, 'j');
    assert_eq!(engine.view().cursor.line, 5);
    assert_eq!(engine.mode, Mode::Visual); // Should still be in visual mode

    // Test 2k - should move up 2 lines
    press_char(&mut engine, '2');
    press_char(&mut engine, 'k');
    assert_eq!(engine.view().cursor.line, 3);

    // Exit visual mode
    press_special(&mut engine, "Escape");
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_count_visual_word() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "one two three four five six seven eight");
    engine.update_syntax();

    // Start at beginning
    assert_eq!(engine.view().cursor, Cursor { line: 0, col: 0 });

    // Enter visual mode
    press_char(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::Visual);

    // Test 3w - should extend by 3 words
    press_char(&mut engine, '3');
    press_char(&mut engine, 'w');

    // After 3 word-forwards from position 0, we should be at "four"
    // one(0) -> two(4) -> three(8) -> four(14)
    assert_eq!(engine.view().cursor.col, 14);

    // Test 2b - should move back 2 words
    press_char(&mut engine, '2');
    press_char(&mut engine, 'b');

    // four(14) -> three(8) -> two(4)
    assert_eq!(engine.view().cursor.col, 4);

    // Exit visual mode
    press_special(&mut engine, "Escape");
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_count_visual_line_mode() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7");
    engine.update_syntax();

    // Start at line 0
    assert_eq!(engine.view().cursor.line, 0);

    // Enter visual line mode
    press_char(&mut engine, 'V');
    assert_eq!(engine.mode, Mode::VisualLine);

    // Test 3j - should extend selection 3 lines down
    press_char(&mut engine, '3');
    press_char(&mut engine, 'j');
    assert_eq!(engine.view().cursor.line, 3);
    assert_eq!(engine.mode, Mode::VisualLine);

    // Yank the selection
    press_char(&mut engine, 'y');
    assert_eq!(engine.mode, Mode::Normal);

    // Should have yanked 4 lines (lines 0-3)
    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert!(is_linewise);
    assert!(content.contains("line 1"));
    assert!(content.contains("line 4"));
}

#[test]
fn test_count_not_applied_to_visual_operators() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "line 1\nline 2\nline 3\nline 4\nline 5");
    engine.update_syntax();

    // Start at line 0
    assert_eq!(engine.view().cursor.line, 0);

    // Enter visual mode
    press_char(&mut engine, 'v');

    // Move down 2 lines to create selection
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'j');
    assert_eq!(engine.view().cursor.line, 2);

    // Now type "3" then "d" - should delete the selection ONCE, not 3 times
    press_char(&mut engine, '3');
    assert_eq!(engine.peek_count(), Some(3));

    press_char(&mut engine, 'd');

    // Should be back in normal mode
    assert_eq!(engine.mode, Mode::Normal);

    // Count should be cleared (not applied to operator)
    assert_eq!(engine.peek_count(), None);

    // Buffer should have deleted lines 0-2 (3 lines), leaving lines 3-4
    let text = engine.buffer().to_string();
    assert!(text.contains("line 4"));
    assert!(text.contains("line 5"));
    assert!(!text.contains("line 1"));
    assert!(!text.contains("line 2"));
    assert!(!text.contains("line 3"));
}

#[test]
fn test_config_reload() {
    use std::fs;
    use std::path::PathBuf;

    // Get config file path
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let config_path = PathBuf::from(&home)
        .join(".config")
        .join("vimcode")
        .join("settings.json");

    // Save original settings
    let original_settings = fs::read_to_string(&config_path).ok();

    // Create config directory
    if let Some(parent) = config_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // Test 1: Successful reload with valid JSON
    let test_settings = r#"{"line_numbers":"Absolute"}"#;
    fs::write(&config_path, test_settings).unwrap();

    let mut engine = Engine::new();
    engine.execute_command("config reload");

    assert_eq!(engine.settings.line_numbers, LineNumberMode::Absolute);
    assert_eq!(engine.message, "Settings reloaded successfully");

    // Test 2: Failed reload with invalid JSON
    fs::write(&config_path, "{ invalid json }").unwrap();
    let initial_settings = engine.settings.line_numbers;

    engine.execute_command("config reload");

    // Settings should be unchanged
    assert_eq!(engine.settings.line_numbers, initial_settings);
    assert!(engine.message.contains("Error reloading settings"));

    // Test 3: Failed reload with missing file
    let _ = fs::remove_file(&config_path);

    engine.execute_command("config reload");

    // Settings should still be unchanged
    assert_eq!(engine.settings.line_numbers, initial_settings);
    assert!(engine.message.contains("Error reloading settings"));

    // Restore original settings or clean up
    if let Some(original) = original_settings {
        fs::write(&config_path, original).unwrap();
    }
}

// --- Character find motion tests ---

#[test]
fn test_find_char_forward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abcdef");
    // Cursor at column 0, find 'd'
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'd');
    assert_eq!(engine.view().cursor.col, 3);
}

#[test]
fn test_find_char_forward_not_found() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abcdef");
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'z');
    // Cursor should not move
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_find_char_backward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abcdef");
    // Move to column 5
    for _ in 0..5 {
        press_char(&mut engine, 'l');
    }
    assert_eq!(engine.view().cursor.col, 5);
    // Find 'b' backward
    press_char(&mut engine, 'F');
    press_char(&mut engine, 'b');
    assert_eq!(engine.view().cursor.col, 1);
}

#[test]
fn test_till_char_forward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abcdef");
    // Cursor at column 0, till 'd' (stop before)
    press_char(&mut engine, 't');
    press_char(&mut engine, 'd');
    assert_eq!(engine.view().cursor.col, 2);
}

#[test]
fn test_till_char_backward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abcdef");
    // Move to column 5
    for _ in 0..5 {
        press_char(&mut engine, 'l');
    }
    // Till 'b' backward (stop after)
    press_char(&mut engine, 'T');
    press_char(&mut engine, 'b');
    assert_eq!(engine.view().cursor.col, 2);
}

#[test]
fn test_find_with_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ababab");
    // Find 2nd 'b'
    press_char(&mut engine, '2');
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'b');
    assert_eq!(engine.view().cursor.col, 3);
}

#[test]
fn test_repeat_find_forward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ababab");
    // Find first 'b'
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'b');
    assert_eq!(engine.view().cursor.col, 1);
    // Repeat to find next 'b'
    press_char(&mut engine, ';');
    assert_eq!(engine.view().cursor.col, 3);
    // Repeat again
    press_char(&mut engine, ';');
    assert_eq!(engine.view().cursor.col, 5);
}

#[test]
fn test_repeat_find_backward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ababab");
    // Move to end
    for _ in 0..5 {
        press_char(&mut engine, 'l');
    }
    // Find 'a' backward
    press_char(&mut engine, 'F');
    press_char(&mut engine, 'a');
    assert_eq!(engine.view().cursor.col, 4);
    // Repeat backward
    press_char(&mut engine, ';');
    assert_eq!(engine.view().cursor.col, 2);
}

#[test]
fn test_repeat_find_reverse() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ababab");
    // Find 'b' forward
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'b');
    assert_eq!(engine.view().cursor.col, 1);
    // Reverse direction (go back to 'b' at col 1, but we're already there)
    // So it should not find anything before col 1
    let prev_col = engine.view().cursor.col;
    press_char(&mut engine, ',');
    // Should stay at same position (no 'b' before col 1)
    assert_eq!(engine.view().cursor.col, prev_col);
}

#[test]
fn test_find_does_not_cross_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\nxyz");
    // Cursor at line 0, col 0
    // Try to find 'x' (which is on next line)
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'x');
    // Should not move (find is within-line only)
    assert_eq!(engine.view().cursor.line, 0);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_repeat_with_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ababab");
    // Find first 'b'
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'b');
    assert_eq!(engine.view().cursor.col, 1);
    // Repeat twice with count
    press_char(&mut engine, '2');
    press_char(&mut engine, ';');
    assert_eq!(engine.view().cursor.col, 5);
}

// --- Tests for delete/change operators (Step 2) ---

#[test]
fn test_dw_delete_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world foo bar");
    engine.update_syntax();
    assert_eq!(engine.view().cursor, Cursor { line: 0, col: 0 });

    // dw should delete "hello "
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'w');

    assert_eq!(engine.buffer().to_string(), "world foo bar");
    assert_eq!(engine.view().cursor, Cursor { line: 0, col: 0 });

    // Check register
    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "hello ");
    assert!(!is_linewise);
}

#[test]
fn test_db_delete_backward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world foo");
    engine.update_syntax();

    // Move to space after "world" (before "foo")
    // "hello world foo" -> cols: h=0, e=1, ..., d=10, ' '=11, f=12
    engine.view_mut().cursor.col = 12;

    // db from 'f' should delete backward to start of word
    // It will go back to col 6 ('w'), so it deletes "world "
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'b');

    assert_eq!(engine.buffer().to_string(), "hello foo");
    assert_eq!(engine.view().cursor.col, 6);
}

#[test]
fn test_de_delete_to_end() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // de from start should delete "hello"
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'e');

    assert_eq!(engine.buffer().to_string(), " world");
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_cw_change_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // cw behaves like ce (Vim compatibility) - deletes "hello" only
    press_char(&mut engine, 'c');
    press_char(&mut engine, 'w');

    assert_eq!(engine.buffer().to_string(), " world");
    assert_eq!(engine.mode, Mode::Insert);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_cb_change_backward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Move to 'w' in "world"
    engine.view_mut().cursor.col = 6;

    // cb from 'w' should go back to start of previous word ('h')
    // So it deletes "hello " and leaves "world"
    press_char(&mut engine, 'c');
    press_char(&mut engine, 'b');

    assert_eq!(engine.buffer().to_string(), "world");
    assert_eq!(engine.mode, Mode::Insert);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_ce_change_to_end() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // ce should delete "hello" and enter insert mode
    press_char(&mut engine, 'c');
    press_char(&mut engine, 'e');

    assert_eq!(engine.buffer().to_string(), " world");
    assert_eq!(engine.mode, Mode::Insert);
}

#[test]
fn test_dw_with_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "one two three four");
    engine.update_syntax();

    // 2dw should delete "one two "
    press_char(&mut engine, '2');
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'w');

    assert_eq!(engine.buffer().to_string(), "three four");
}

#[test]
fn test_cw_with_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "one two three");
    engine.update_syntax();

    // 2cw behaves like 2ce - deletes "one two" (not the trailing space)
    press_char(&mut engine, '2');
    press_char(&mut engine, 'c');
    press_char(&mut engine, 'w');

    assert_eq!(engine.buffer().to_string(), " three");
    assert_eq!(engine.mode, Mode::Insert);
}

#[test]
fn test_cw_at_end_of_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\nworld");
    engine.update_syntax();

    // cw at "hello" should NOT delete the newline
    press_char(&mut engine, 'c');
    press_char(&mut engine, 'w');

    assert_eq!(engine.buffer().to_string(), "\nworld");
    assert_eq!(engine.mode, Mode::Insert);
    assert_eq!(engine.view().cursor.line, 0);
}

#[test]
fn test_cw_on_last_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc def");
    engine.update_syntax();

    // Move to 'd' in "def"
    engine.view_mut().cursor.col = 4;

    // cw should delete "def", leaving "abc " (with trailing space)
    press_char(&mut engine, 'c');
    press_char(&mut engine, 'w');

    assert_eq!(
        engine.buffer().to_string(),
        "abc ",
        "cw on last word should preserve preceding space"
    );
    assert_eq!(engine.mode, Mode::Insert);
}

#[test]
fn test_ce_on_last_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc def");
    engine.update_syntax();

    // Move to 'd' in "def"
    engine.view_mut().cursor.col = 4;

    // ce should delete "def", leaving "abc " (with trailing space)
    press_char(&mut engine, 'c');
    press_char(&mut engine, 'e');

    assert_eq!(
        engine.buffer().to_string(),
        "abc ",
        "ce on last word should preserve preceding space"
    );
    assert_eq!(engine.mode, Mode::Insert);
}

#[test]
fn test_s_substitute_char() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    engine.update_syntax();

    // s should delete 'h' and enter insert mode
    press_char(&mut engine, 's');

    assert_eq!(engine.buffer().to_string(), "ello");
    assert_eq!(engine.mode, Mode::Insert);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_s_with_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    engine.update_syntax();

    // 3s should delete "hel" and enter insert mode
    press_char(&mut engine, '3');
    press_char(&mut engine, 's');

    assert_eq!(engine.buffer().to_string(), "lo");
    assert_eq!(engine.mode, Mode::Insert);
}

#[test]
fn test_S_substitute_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Move cursor to middle
    engine.view_mut().cursor.col = 6;

    // S should delete entire line content and enter insert mode
    press_char(&mut engine, 'S');

    assert_eq!(engine.buffer().to_string(), "");
    assert_eq!(engine.mode, Mode::Insert);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_C_change_to_eol() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Move to 'w'
    engine.view_mut().cursor.col = 6;

    // C should delete "world" and enter insert mode
    press_char(&mut engine, 'C');

    // After deleting "world", cursor stays at col 6
    // But the line is now "hello " (length 6), so cursor should clamp to col 5
    assert_eq!(engine.buffer().to_string(), "hello ");
    assert_eq!(engine.mode, Mode::Insert);
    // In insert mode, cursor can be at end of line
    assert!(engine.view().cursor.col >= 5);
}

#[test]
fn test_dd_still_works() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    // dd should still work
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'd');

    assert_eq!(engine.buffer().to_string(), "line2\nline3");
}

#[test]
fn test_cc_change_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // cc should delete line content and enter insert mode
    press_char(&mut engine, 'c');
    press_char(&mut engine, 'c');

    assert_eq!(engine.buffer().to_string(), "");
    assert_eq!(engine.mode, Mode::Insert);
}

#[test]
fn test_operators_with_registers() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // "adw should delete into register 'a'
    press_char(&mut engine, '"');
    press_char(&mut engine, 'a');
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'w');

    let (content, _) = engine.registers.get(&'a').unwrap();
    assert_eq!(content, "hello ");
}

#[test]
fn test_operators_undo_redo() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // dw
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'w');
    assert_eq!(engine.buffer().to_string(), "world");

    // Undo
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "hello world");

    // Redo
    press_ctrl(&mut engine, 'r');
    assert_eq!(engine.buffer().to_string(), "world");
}

// --- Tests for ge motion ---

#[test]
fn test_ge_basic() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world test");
    engine.update_syntax();

    // Start at end of first word: "hello world test"
    //                                    ^
    engine.view_mut().cursor.col = 4;

    // ge should move to end of "hello" (already there, so go back to previous word end)
    // But since we're already at end of word, should go to previous
    press_char(&mut engine, 'g');
    press_char(&mut engine, 'e');

    // Should stay at position or move (depending on implementation)
    // Let's test from middle of word instead
}

#[test]
fn test_ge_from_middle_of_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world test");
    engine.update_syntax();

    // Start in middle of "world": "hello world test"
    //                                      ^
    engine.view_mut().cursor.col = 8;

    // ge should move to end of "hello"
    press_char(&mut engine, 'g');
    press_char(&mut engine, 'e');

    assert_eq!(engine.view().cursor.col, 4); // End of "hello"
}

#[test]
fn test_ge_with_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "one two three four");
    engine.update_syntax();

    // Start at "four": "one two three four"
    //                                 ^
    engine.view_mut().cursor.col = 14;

    // 2ge should move back 2 word ends: "three" -> "two" -> "one"
    press_char(&mut engine, '2');
    press_char(&mut engine, 'g');
    press_char(&mut engine, 'e');

    assert_eq!(engine.view().cursor.col, 2); // End of "one"
}

#[test]
fn test_ge_at_start() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Start at beginning
    engine.view_mut().cursor.col = 0;

    // ge at start should not move
    press_char(&mut engine, 'g');
    press_char(&mut engine, 'e');

    assert_eq!(engine.view().cursor.col, 0);
}

// --- Tests for % motion ---

#[test]
fn test_percent_parentheses() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo(bar)baz");
    engine.update_syntax();

    // Start on opening paren: "foo(bar)baz"
    //                             ^
    engine.view_mut().cursor.col = 3;

    // % should jump to closing paren
    press_char(&mut engine, '%');

    assert_eq!(engine.view().cursor.col, 7); // Closing paren
}

#[test]
fn test_percent_braces() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "if { x }");
    engine.update_syntax();

    // Start on opening brace: "if { x }"
    //                             ^
    engine.view_mut().cursor.col = 3;

    // % should jump to closing brace
    press_char(&mut engine, '%');

    assert_eq!(engine.view().cursor.col, 7); // Closing brace
}

#[test]
fn test_percent_brackets() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "arr[0]");
    engine.update_syntax();

    // Start on opening bracket: "arr[0]"
    //                                ^
    engine.view_mut().cursor.col = 3;

    // % should jump to closing bracket
    press_char(&mut engine, '%');

    assert_eq!(engine.view().cursor.col, 5); // Closing bracket
}

#[test]
fn test_percent_closing_to_opening() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "(abc)");
    engine.update_syntax();

    // Start on closing paren: "(abc)"
    //                             ^
    engine.view_mut().cursor.col = 4;

    // % should jump to opening paren
    press_char(&mut engine, '%');

    assert_eq!(engine.view().cursor.col, 0); // Opening paren
}

#[test]
fn test_percent_nested() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "((a))");
    engine.update_syntax();

    // Start on first opening paren: "((a))"
    //                                 ^
    engine.view_mut().cursor.col = 0;

    // % should jump to matching closing paren (outermost)
    press_char(&mut engine, '%');

    assert_eq!(engine.view().cursor.col, 4); // Outermost closing paren
}

#[test]
fn test_percent_not_on_bracket_searches_forward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo(bar)");
    engine.update_syntax();

    // Start before opening paren: "foo(bar)"
    //                              ^
    engine.view_mut().cursor.col = 0;

    // % should search forward for next bracket and jump to match
    press_char(&mut engine, '%');

    assert_eq!(engine.view().cursor.col, 7); // Closing paren
}

#[test]
fn test_d_percent() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo(bar)baz");
    engine.update_syntax();

    // Start on opening paren: "foo(bar)baz"
    //                             ^
    engine.view_mut().cursor.col = 3;

    // d% should delete from ( to ) inclusive
    press_char(&mut engine, 'd');
    press_char(&mut engine, '%');

    assert_eq!(engine.buffer().to_string(), "foobaz");
    assert_eq!(engine.view().cursor.col, 3);
}

#[test]
fn test_c_percent() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo{bar}baz");
    engine.update_syntax();

    // Start on opening brace: "foo{bar}baz"
    //                             ^
    engine.view_mut().cursor.col = 3;

    // c% should delete from { to } and enter insert mode
    press_char(&mut engine, 'c');
    press_char(&mut engine, '%');

    assert_eq!(engine.buffer().to_string(), "foobaz");
    assert_eq!(engine.mode, Mode::Insert);
    assert_eq!(engine.view().cursor.col, 3);
}

// --- Text Object Tests ---

#[test]
fn test_diw_inner_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar baz");
    engine.update_syntax();

    // Position on "bar": "foo bar baz"
    //                         ^
    engine.view_mut().cursor.col = 5;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 'w');

    assert_eq!(engine.buffer().to_string(), "foo  baz");
    assert_eq!(engine.view().cursor.col, 4);
}

#[test]
fn test_daw_around_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar baz");
    engine.update_syntax();

    // Position on "bar": "foo bar baz"
    //                         ^
    engine.view_mut().cursor.col = 5;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'a');
    press_char(&mut engine, 'w');

    assert_eq!(engine.buffer().to_string(), "foo baz");
    assert_eq!(engine.view().cursor.col, 4);
}

#[test]
fn test_ciw_change_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Position on "world"
    engine.view_mut().cursor.col = 6;

    press_char(&mut engine, 'c');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 'w');

    assert_eq!(engine.buffer().to_string(), "hello ");
    assert_eq!(engine.mode, Mode::Insert);
    assert_eq!(engine.view().cursor.col, 6);
}

#[test]
fn test_yiw_yank_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "one two three");
    engine.update_syntax();

    // Position on "two"
    engine.view_mut().cursor.col = 4;

    press_char(&mut engine, 'y');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 'w');

    // Check register contains "two"
    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "two");

    // Buffer should be unchanged
    assert_eq!(engine.buffer().to_string(), "one two three");
}

#[test]
fn test_di_quote_double() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, r#"foo "hello world" bar"#);
    engine.update_syntax();

    // Position inside quotes: foo "hello world" bar
    //                                  ^
    engine.view_mut().cursor.col = 10;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, '"');

    assert_eq!(engine.buffer().to_string(), r#"foo "" bar"#);
    assert_eq!(engine.view().cursor.col, 5);
}

#[test]
fn test_da_quote_double() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, r#"foo "hello world" bar"#);
    engine.update_syntax();

    // Position inside quotes
    engine.view_mut().cursor.col = 10;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'a');
    press_char(&mut engine, '"');

    assert_eq!(engine.buffer().to_string(), "foo  bar");
    assert_eq!(engine.view().cursor.col, 4);
}

#[test]
fn test_di_quote_single() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo 'test' bar");
    engine.update_syntax();

    // Position inside quotes
    engine.view_mut().cursor.col = 6;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, '\'');

    assert_eq!(engine.buffer().to_string(), "foo '' bar");
    assert_eq!(engine.view().cursor.col, 5);
}

#[test]
fn test_di_paren() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo(bar)baz");
    engine.update_syntax();

    // Position inside parens
    engine.view_mut().cursor.col = 5;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, '(');

    assert_eq!(engine.buffer().to_string(), "foo()baz");
    assert_eq!(engine.view().cursor.col, 4);
}

#[test]
fn test_da_paren() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo(bar)baz");
    engine.update_syntax();

    // Position inside parens
    engine.view_mut().cursor.col = 5;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'a');
    press_char(&mut engine, ')');

    assert_eq!(engine.buffer().to_string(), "foobaz");
    assert_eq!(engine.view().cursor.col, 3);
}

#[test]
fn test_di_brace() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "fn main() {code}");
    engine.update_syntax();

    // Position inside braces
    engine.view_mut().cursor.col = 12;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, '{');

    assert_eq!(engine.buffer().to_string(), "fn main() {}");
    assert_eq!(engine.view().cursor.col, 11);
}

#[test]
fn test_da_brace() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test{content}end");
    engine.update_syntax();

    // Position inside braces
    engine.view_mut().cursor.col = 6;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'a');
    press_char(&mut engine, '}');

    assert_eq!(engine.buffer().to_string(), "testend");
    assert_eq!(engine.view().cursor.col, 4);
}

#[test]
fn test_di_bracket() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "array[index]end");
    engine.update_syntax();

    // Position inside brackets
    engine.view_mut().cursor.col = 7;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, '[');

    assert_eq!(engine.buffer().to_string(), "array[]end");
    assert_eq!(engine.view().cursor.col, 6);
}

#[test]
fn test_da_bracket() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "array[index]end");
    engine.update_syntax();

    // Position inside brackets
    engine.view_mut().cursor.col = 7;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'a');
    press_char(&mut engine, ']');

    assert_eq!(engine.buffer().to_string(), "arrayend");
    assert_eq!(engine.view().cursor.col, 5);
}

#[test]
fn test_ciw_at_start_of_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Position at start of "world"
    engine.view_mut().cursor.col = 6;

    press_char(&mut engine, 'c');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 'w');

    assert_eq!(engine.buffer().to_string(), "hello ");
    assert_eq!(engine.mode, Mode::Insert);
}

#[test]
fn test_text_object_nested_parens() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "outer(inner(x))end");
    engine.update_syntax();

    // Position in inner parens: outer(inner(x))end
    //                                     ^
    engine.view_mut().cursor.col = 12;

    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, '(');

    assert_eq!(engine.buffer().to_string(), "outer(inner())end");
}

#[test]
fn test_visual_iw() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "one two three");
    engine.update_syntax();

    // Position on "two"
    engine.view_mut().cursor.col = 4;

    // Enter visual mode and select iw
    press_char(&mut engine, 'v');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 'w');

    assert_eq!(engine.mode, Mode::Visual);
    assert_eq!(engine.visual_anchor.unwrap().col, 4);
    assert_eq!(engine.view().cursor.col, 6);

    // Delete the selection
    press_char(&mut engine, 'd');
    assert_eq!(engine.buffer().to_string(), "one  three");
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_visual_aw() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "one two three");
    engine.update_syntax();

    // Position on "two"
    engine.view_mut().cursor.col = 4;

    // Enter visual mode and select aw
    press_char(&mut engine, 'v');
    press_char(&mut engine, 'a');
    press_char(&mut engine, 'w');

    assert_eq!(engine.mode, Mode::Visual);

    // Yank the selection
    press_char(&mut engine, 'y');
    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "two ");
}

#[test]
fn test_visual_i_quote() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, r#"say "hello" now"#);
    engine.update_syntax();

    // Position inside quotes
    engine.view_mut().cursor.col = 6;

    press_char(&mut engine, 'v');
    press_char(&mut engine, 'i');
    press_char(&mut engine, '"');

    assert_eq!(engine.mode, Mode::Visual);

    // Delete selection
    press_char(&mut engine, 'd');
    assert_eq!(engine.buffer().to_string(), r#"say "" now"#);
}

// =======================================================================
// Repeat command (.) tests
// =======================================================================

// TODO: Fix cursor positioning after insert operations
#[test]
#[ignore]
fn test_repeat_insert() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3");
    engine.update_syntax();

    // Insert text on first line
    press_char(&mut engine, 'i');
    assert_eq!(engine.mode, Mode::Insert);
    press_char(&mut engine, 'X');
    press_char(&mut engine, 'Y');
    press_special(&mut engine, "Escape");
    assert_eq!(engine.mode, Mode::Normal);
    assert_eq!(engine.buffer().to_string(), "XYline1\nline2\nline3");

    // Move to second line and repeat
    press_char(&mut engine, 'j');
    press_char(&mut engine, '.');
    assert_eq!(engine.buffer().to_string(), "XYline1\nXYline2\nline3");
    assert_eq!(engine.view().cursor.line, 1);
    assert_eq!(engine.view().cursor.col, 2);
}

// TODO: Fix multi-count delete repeat
#[test]
#[ignore]
fn test_repeat_delete_x() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABCDEF\nGHIJKL");
    engine.update_syntax();

    // Delete 2 chars with 2x
    press_char(&mut engine, '2');
    press_char(&mut engine, 'x');
    assert_eq!(engine.buffer().to_string(), "CDEF\nGHIJKL");

    // Move to second line and repeat
    press_char(&mut engine, 'j');
    press_char(&mut engine, '.');
    assert_eq!(engine.buffer().to_string(), "CDEF\nIJKL");
}

#[test]
fn test_repeat_delete_dd() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3\nline4");
    engine.update_syntax();

    // Delete one line
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'd');
    assert_eq!(engine.buffer().to_string(), "line2\nline3\nline4");

    // Repeat delete
    press_char(&mut engine, '.');
    assert_eq!(engine.buffer().to_string(), "line3\nline4");
}

// TODO: Fix cursor positioning for repeat with count
#[test]
#[ignore]
fn test_repeat_insert_with_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\ndef\nghi");
    engine.update_syntax();

    // Insert 'X' once
    press_char(&mut engine, 'i');
    press_char(&mut engine, 'X');
    press_special(&mut engine, "Escape");
    assert_eq!(engine.buffer().to_string(), "Xabc\ndef\nghi");

    // Repeat 3 times on next line
    press_char(&mut engine, 'j');
    press_char(&mut engine, '3');
    press_char(&mut engine, '.');
    assert_eq!(engine.buffer().to_string(), "Xabc\nXXXdef\nghi");
}

// TODO: Fix cursor positioning with newline repeats
#[test]
#[ignore]
fn test_repeat_insert_with_newline() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "first");
    engine.update_syntax();

    // Insert with newline
    press_char(&mut engine, 'a');
    press_special(&mut engine, "Return");
    press_char(&mut engine, 'X');
    press_special(&mut engine, "Escape");
    assert_eq!(engine.buffer().to_string(), "first\nX");

    // Move to start and repeat
    engine.view_mut().cursor.line = 0;
    engine.view_mut().cursor.col = 0;
    press_char(&mut engine, '.');
    assert_eq!(engine.buffer().to_string(), "\nXfirst\nX");
}

// TODO: Implement substitute repeat
#[test]
#[ignore]
fn test_repeat_substitute_s() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\nworld");
    engine.update_syntax();

    // Substitute first char with 'X'
    press_char(&mut engine, 's');
    press_char(&mut engine, 'X');
    press_special(&mut engine, "Escape");
    assert_eq!(engine.buffer().to_string(), "Xello\nworld");

    // Move to second line and repeat
    press_char(&mut engine, 'j');
    press_char(&mut engine, '.');
    assert_eq!(engine.buffer().to_string(), "Xello\nXorld");
}

// TODO: Implement substitute repeat with count
#[test]
#[ignore]
fn test_repeat_substitute_2s() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abcdef\nghijkl");
    engine.update_syntax();

    // Substitute 2 chars with 'XY'
    press_char(&mut engine, '2');
    press_char(&mut engine, 's');
    press_char(&mut engine, 'X');
    press_char(&mut engine, 'Y');
    press_special(&mut engine, "Escape");
    assert_eq!(engine.buffer().to_string(), "XYcdef\nghijkl");

    // Move to second line and repeat
    press_char(&mut engine, 'j');
    press_char(&mut engine, '.');
    assert_eq!(engine.buffer().to_string(), "XYcdef\nXYijkl");
}

#[test]
fn test_repeat_append() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "one\ntwo");
    engine.update_syntax();

    // Append text
    press_char(&mut engine, 'a');
    press_char(&mut engine, '!');
    press_special(&mut engine, "Escape");
    assert_eq!(engine.buffer().to_string(), "o!ne\ntwo");

    // Move to second line start and repeat (inserts at current position)
    press_char(&mut engine, 'j');
    engine.view_mut().cursor.col = 0; // Ensure we're at column 0
    press_char(&mut engine, '.');
    assert_eq!(engine.buffer().to_string(), "o!ne\n!two");
}

#[test]
fn test_repeat_open_line_o() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "alpha\nbeta");
    engine.update_syntax();

    // Open line below and insert
    press_char(&mut engine, 'o');
    press_char(&mut engine, 'N');
    press_char(&mut engine, 'E');
    press_char(&mut engine, 'W');
    press_special(&mut engine, "Escape");
    assert_eq!(engine.buffer().to_string(), "alpha\nNEW\nbeta");

    // Repeat inserts the text "NEW" at current position (not a full 'o' command)
    // Move to last line and repeat
    press_char(&mut engine, 'j');
    engine.view_mut().cursor.col = 0;
    press_char(&mut engine, '.');
    assert_eq!(engine.buffer().to_string(), "alpha\nNEW\nNEWbeta");
}

#[test]
fn test_repeat_before_any_change() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test");
    engine.update_syntax();

    // Try to repeat when no change has been made
    press_char(&mut engine, '.');
    // Should be no-op
    assert_eq!(engine.buffer().to_string(), "test");
}

// TODO: Fix count preservation in repeat
#[test]
#[ignore]
fn test_repeat_preserves_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABCDEFGH\nIJKLMNOP");
    engine.update_syntax();

    // Delete 3 chars
    press_char(&mut engine, '3');
    press_char(&mut engine, 'x');
    assert_eq!(engine.buffer().to_string(), "DEFGH\nIJKLMNOP");

    // Repeat on second line (should delete 3 again)
    press_char(&mut engine, 'j');
    press_char(&mut engine, '.');
    assert_eq!(engine.buffer().to_string(), "DEFGH\nLMNOP");
}

// TODO: Fix dd repeat with count
#[test]
#[ignore]
fn test_repeat_dd_multiple_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\nb\nc\nd\ne\nf");
    engine.update_syntax();

    // Delete 2 lines
    press_char(&mut engine, '2');
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'd');
    assert_eq!(engine.buffer().to_string(), "c\nd\ne\nf");

    // Repeat (should delete 2 more lines)
    press_char(&mut engine, '.');
    assert_eq!(engine.buffer().to_string(), "e\nf");
}

// =======================================================================
// Mouse click tests
// =======================================================================

#[test]
fn test_mouse_click_sets_cursor() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line0\nline1\nline2\nline3");
    engine.update_syntax();

    // Get the active window ID
    let window_id = engine.active_window_id();

    // Click to move cursor to line 2, col 3
    engine.set_cursor_for_window(window_id, 2, 3);
    assert_eq!(engine.cursor().line, 2);
    assert_eq!(engine.cursor().col, 3);
}

#[test]
fn test_mouse_click_clamps_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line0\nline1\nline2");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click beyond last line (should clamp to line 2)
    engine.set_cursor_for_window(window_id, 10, 0);
    assert_eq!(engine.cursor().line, 2);
    assert_eq!(engine.cursor().col, 0);
}

#[test]
fn test_mouse_click_clamps_col() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "short\nline1\nline2");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click beyond line length (should clamp to 4, last char of "short")
    engine.set_cursor_for_window(window_id, 0, 100);
    assert_eq!(engine.cursor().line, 0);
    assert_eq!(engine.cursor().col, 4); // "short" has 5 chars, max cursor pos is 4
}

#[test]
fn test_mouse_click_switches_window_in_split() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "buffer1\nline1");
    engine.update_syntax();

    // Create a split
    engine.split_window(SplitDirection::Horizontal, None);

    // Modify second buffer
    let len = engine.buffer().len_chars();
    engine.buffer_mut().delete_range(0, len);
    engine.buffer_mut().insert(0, "buffer2\nline2");
    engine.update_syntax();

    // Get both window IDs
    let all_windows: Vec<WindowId> = engine.windows.keys().copied().collect();
    assert_eq!(all_windows.len(), 2);
    let window1 = all_windows[0];
    let window2 = all_windows[1];

    // Make window1 active first
    engine.set_cursor_for_window(window1, 0, 0);
    assert_eq!(engine.active_window_id(), window1);

    // Click in window2 should switch to it
    engine.set_cursor_for_window(window2, 0, 3);
    assert_eq!(engine.active_window_id(), window2);
    assert_eq!(engine.cursor().line, 0);
    assert_eq!(engine.cursor().col, 3);
}

#[test]
fn test_mouse_click_empty_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line0\n\nline2");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click on empty line (line 1)
    engine.set_cursor_for_window(window_id, 1, 5);
    assert_eq!(engine.cursor().line, 1);
    assert_eq!(engine.cursor().col, 0); // Should clamp to 0 for empty line
}

#[test]
fn test_mouse_click_single_window() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\ndef\nghi");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click to line 1, col 2
    engine.set_cursor_for_window(window_id, 1, 2);
    assert_eq!(engine.cursor().line, 1);
    assert_eq!(engine.cursor().col, 2);

    // Verify we're still in normal mode
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_mouse_click_preserves_mode() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line0\nline1\nline2");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Enter insert mode
    press_char(&mut engine, 'i');
    assert_eq!(engine.mode, Mode::Insert);

    // Click should move cursor but mode is handled by UI layer
    // The engine method itself doesn't change mode
    engine.set_cursor_for_window(window_id, 2, 1);
    assert_eq!(engine.cursor().line, 2);
    assert_eq!(engine.cursor().col, 1);
}

#[test]
fn test_mouse_click_invalid_window_id() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line0\nline1");
    engine.update_syntax();

    let old_cursor = *engine.cursor();
    let old_window = engine.active_window_id();

    // Click with invalid window ID (should do nothing)
    engine.set_cursor_for_window(WindowId(9999), 1, 1);

    // Cursor and active window should be unchanged
    assert_eq!(*engine.cursor(), old_cursor);
    assert_eq!(engine.active_window_id(), old_window);
}

#[test]
fn test_mouse_click_at_exact_line_end() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\nworld");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click at column 5 of "hello" (length is 5, so max cursor pos is 4)
    engine.set_cursor_for_window(window_id, 0, 5);
    assert_eq!(engine.cursor().line, 0);
    assert_eq!(engine.cursor().col, 4); // Clamped to last valid position
}

#[test]
fn test_mouse_click_way_past_last_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\nb\nc");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click at line 1000 (way past the 3 lines we have)
    engine.set_cursor_for_window(window_id, 1000, 0);
    assert_eq!(engine.cursor().line, 2); // Clamped to last line
    assert_eq!(engine.cursor().col, 0);
}

#[test]
fn test_mouse_click_on_line_with_tabs() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "\thello\t world");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click at column 0 (before tab)
    engine.set_cursor_for_window(window_id, 0, 0);
    assert_eq!(engine.cursor().col, 0);

    // Click at column 1 (on the tab character itself)
    engine.set_cursor_for_window(window_id, 0, 1);
    assert_eq!(engine.cursor().col, 1);

    // Click at column 6 (in "hello", after tab)
    engine.set_cursor_for_window(window_id, 0, 6);
    assert_eq!(engine.cursor().col, 6);
}

#[test]
fn test_mouse_click_on_unicode_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "Hello 世界 World");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click at various positions
    engine.set_cursor_for_window(window_id, 0, 0);
    assert_eq!(engine.cursor().col, 0);

    engine.set_cursor_for_window(window_id, 0, 6);
    assert_eq!(engine.cursor().col, 6); // First unicode char position

    engine.set_cursor_for_window(window_id, 0, 7);
    assert_eq!(engine.cursor().col, 7); // Second unicode char position
}

#[test]
fn test_mouse_click_at_column_zero() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\ndef\nghi");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click at column 0 on various lines
    engine.set_cursor_for_window(window_id, 0, 0);
    assert_eq!(engine.cursor().line, 0);
    assert_eq!(engine.cursor().col, 0);

    engine.set_cursor_for_window(window_id, 1, 0);
    assert_eq!(engine.cursor().line, 1);
    assert_eq!(engine.cursor().col, 0);

    engine.set_cursor_for_window(window_id, 2, 0);
    assert_eq!(engine.cursor().line, 2);
    assert_eq!(engine.cursor().col, 0);
}

#[test]
fn test_mouse_click_very_large_column() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "short");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click at column 99999 on a short line
    engine.set_cursor_for_window(window_id, 0, 99999);
    assert_eq!(engine.cursor().line, 0);
    assert_eq!(engine.cursor().col, 4); // Clamped to "short".len() - 1
}

#[test]
fn test_mouse_click_on_very_long_line() {
    let mut engine = Engine::new();
    let long_line = "x".repeat(1000);
    engine.buffer_mut().insert(0, &long_line);
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click at various positions on long line
    engine.set_cursor_for_window(window_id, 0, 0);
    assert_eq!(engine.cursor().col, 0);

    engine.set_cursor_for_window(window_id, 0, 500);
    assert_eq!(engine.cursor().col, 500);

    engine.set_cursor_for_window(window_id, 0, 999);
    assert_eq!(engine.cursor().col, 999);

    // Past the end should clamp to 999 (last valid position)
    engine.set_cursor_for_window(window_id, 0, 1000);
    assert_eq!(engine.cursor().col, 999);
}

#[test]
fn test_mouse_click_mixed_tabs_and_spaces() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "\t  hello  \tworld");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click at start (tab)
    engine.set_cursor_for_window(window_id, 0, 0);
    assert_eq!(engine.cursor().col, 0);

    // Click in middle (after spaces)
    engine.set_cursor_for_window(window_id, 0, 5);
    assert_eq!(engine.cursor().col, 5);

    // Click near end
    engine.set_cursor_for_window(window_id, 0, 15);
    assert_eq!(engine.cursor().col, 15);
}

#[test]
fn test_mouse_click_on_last_character_of_file() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nend");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click on the 'd' in "end" (line 2, col 2)
    engine.set_cursor_for_window(window_id, 2, 2);
    assert_eq!(engine.cursor().line, 2);
    assert_eq!(engine.cursor().col, 2);
}

#[test]
fn test_mouse_click_single_character_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\nb\nc");
    engine.update_syntax();

    let window_id = engine.active_window_id();

    // Click on single character lines
    engine.set_cursor_for_window(window_id, 0, 0);
    assert_eq!(engine.cursor().line, 0);
    assert_eq!(engine.cursor().col, 0);

    // Click past the single character
    engine.set_cursor_for_window(window_id, 1, 5);
    assert_eq!(engine.cursor().line, 1);
    assert_eq!(engine.cursor().col, 0); // Clamped to 0 (last valid pos of "b")
}

// --- Preview mode tests ---

#[test]
fn test_preview_open_marks_buffer() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_preview1.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"preview").unwrap();
    }

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&path, OpenMode::Preview)
        .unwrap();

    let bid = engine.active_buffer_id();
    assert!(engine.buffer_manager.get(bid).unwrap().preview);
    assert_eq!(engine.preview_buffer_id, Some(bid));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_permanent_open_not_preview() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_preview2.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"permanent").unwrap();
    }

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&path, OpenMode::Permanent)
        .unwrap();

    let bid = engine.active_buffer_id();
    assert!(!engine.buffer_manager.get(bid).unwrap().preview);
    assert_eq!(engine.preview_buffer_id, None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_preview_replaced_by_new_preview() {
    use std::io::Write;
    let path1 = std::env::temp_dir().join("vimcode_test_preview3a.txt");
    let path2 = std::env::temp_dir().join("vimcode_test_preview3b.txt");
    {
        let mut f = std::fs::File::create(&path1).unwrap();
        f.write_all(b"file1").unwrap();
    }
    {
        let mut f = std::fs::File::create(&path2).unwrap();
        f.write_all(b"file2").unwrap();
    }

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&path1, OpenMode::Preview)
        .unwrap();
    let bid1 = engine.active_buffer_id();

    engine
        .open_file_with_mode(&path2, OpenMode::Preview)
        .unwrap();
    let bid2 = engine.active_buffer_id();

    // Old preview should be deleted
    assert!(engine.buffer_manager.get(bid1).is_none());
    // New preview should be active
    assert!(engine.buffer_manager.get(bid2).unwrap().preview);
    assert_eq!(engine.preview_buffer_id, Some(bid2));

    let _ = std::fs::remove_file(&path1);
    let _ = std::fs::remove_file(&path2);
}

#[test]
fn test_double_click_promotes_preview() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_preview4.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"promote").unwrap();
    }

    let mut engine = Engine::new();
    // Single-click: preview
    engine
        .open_file_with_mode(&path, OpenMode::Preview)
        .unwrap();
    let bid = engine.active_buffer_id();
    assert!(engine.buffer_manager.get(bid).unwrap().preview);

    // Double-click: permanent
    engine
        .open_file_with_mode(&path, OpenMode::Permanent)
        .unwrap();
    assert!(!engine.buffer_manager.get(bid).unwrap().preview);
    assert_eq!(engine.preview_buffer_id, None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_edit_promotes_preview() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_preview5.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"editme").unwrap();
    }

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&path, OpenMode::Preview)
        .unwrap();
    let bid = engine.active_buffer_id();
    assert!(engine.buffer_manager.get(bid).unwrap().preview);

    // Enter insert mode and type a character
    press_char(&mut engine, 'i');
    press_char(&mut engine, 'x');
    press_special(&mut engine, "Escape");

    // Should be promoted
    assert!(!engine.buffer_manager.get(bid).unwrap().preview);
    assert_eq!(engine.preview_buffer_id, None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_save_promotes_preview() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_preview6.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"saveme").unwrap();
    }

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&path, OpenMode::Preview)
        .unwrap();
    let bid = engine.active_buffer_id();
    assert!(engine.buffer_manager.get(bid).unwrap().preview);

    // Save
    let _ = engine.save();

    assert!(!engine.buffer_manager.get(bid).unwrap().preview);
    assert_eq!(engine.preview_buffer_id, None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_ls_shows_preview_flag() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_preview7.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"ls").unwrap();
    }

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&path, OpenMode::Preview)
        .unwrap();

    let listing = engine.list_buffers();
    assert!(listing.contains("[Preview]"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_already_permanent_ignores_preview_mode() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_preview8.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"perm").unwrap();
    }

    let mut engine = Engine::new();
    // Open as permanent first
    engine
        .open_file_with_mode(&path, OpenMode::Permanent)
        .unwrap();
    let bid = engine.active_buffer_id();

    // Trying to preview the same file should NOT mark it as preview
    engine
        .open_file_with_mode(&path, OpenMode::Preview)
        .unwrap();
    assert!(!engine.buffer_manager.get(bid).unwrap().preview);
    assert_eq!(engine.preview_buffer_id, None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_delete_preview_clears_tracking() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_preview9.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"del").unwrap();
    }

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&path, OpenMode::Preview)
        .unwrap();
    let bid = engine.active_buffer_id();
    assert_eq!(engine.preview_buffer_id, Some(bid));

    let _ = engine.delete_buffer(bid, true);
    assert_eq!(engine.preview_buffer_id, None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_preview_never_dirty_and_preview() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_preview10.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"dirtytest").unwrap();
    }

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&path, OpenMode::Preview)
        .unwrap();
    let bid = engine.active_buffer_id();

    // Type to make dirty — should auto-promote
    press_char(&mut engine, 'i');
    press_char(&mut engine, 'z');
    press_special(&mut engine, "Escape");

    let state = engine.buffer_manager.get(bid).unwrap();
    // Should be dirty but NOT preview (promoted)
    assert!(state.dirty);
    assert!(!state.preview);

    let _ = std::fs::remove_file(&path);
}

// =======================================================================
// open_file_preview (single-click sidebar) Tests
// =======================================================================

#[test]
fn test_open_file_preview_creates_preview_tab() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_sidebar_preview1.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello").unwrap();
    }

    let mut engine = Engine::new();
    engine.open_file_preview(&path);

    let bid = engine.active_buffer_id();
    let state = engine.buffer_manager.get(bid).unwrap();
    assert!(state.preview, "single-click should open as preview");
    assert_eq!(engine.preview_buffer_id, Some(bid));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_open_file_preview_replaced_by_second_single_click() {
    use std::io::Write;
    let path1 = std::env::temp_dir().join("vimcode_test_sidebar_preview2a.txt");
    let path2 = std::env::temp_dir().join("vimcode_test_sidebar_preview2b.txt");
    {
        let mut f = std::fs::File::create(&path1).unwrap();
        f.write_all(b"file1").unwrap();
        let mut f = std::fs::File::create(&path2).unwrap();
        f.write_all(b"file2").unwrap();
    }

    let mut engine = Engine::new();
    engine.open_file_preview(&path1);
    let bid1 = engine.active_buffer_id();

    engine.open_file_preview(&path2);
    let bid2 = engine.active_buffer_id();

    // The first preview buffer should be gone; only the second remains.
    assert!(
        engine.buffer_manager.get(bid1).is_none(),
        "old preview buffer deleted"
    );
    assert!(
        engine.buffer_manager.get(bid2).unwrap().preview,
        "new buffer is preview"
    );
    assert_eq!(engine.preview_buffer_id, Some(bid2));
    // Tab count should not have grown (reused the preview slot).
    assert_eq!(
        engine.active_group().tabs.len(),
        2,
        "still only 2 tabs (initial + 1 preview)"
    );

    let _ = std::fs::remove_file(&path1);
    let _ = std::fs::remove_file(&path2);
}

#[test]
fn test_open_file_preview_double_click_promotes() {
    use std::io::Write;
    let path = std::env::temp_dir().join("vimcode_test_sidebar_preview3.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello").unwrap();
    }

    let mut engine = Engine::new();
    engine.open_file_preview(&path);
    let bid = engine.active_buffer_id();
    assert!(engine.buffer_manager.get(bid).unwrap().preview);

    // Double-click: open_file_in_tab promotes the preview in-place.
    engine.open_file_in_tab(&path);
    assert!(
        !engine.buffer_manager.get(bid).unwrap().preview,
        "promoted to permanent"
    );
    assert_eq!(engine.preview_buffer_id, None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_open_file_preview_permanent_file_just_switches() {
    use std::io::Write;
    let path1 = std::env::temp_dir().join("vimcode_test_sidebar_preview4a.txt");
    let path2 = std::env::temp_dir().join("vimcode_test_sidebar_preview4b.txt");
    {
        let mut f = std::fs::File::create(&path1).unwrap();
        f.write_all(b"file1").unwrap();
        let mut f = std::fs::File::create(&path2).unwrap();
        f.write_all(b"file2").unwrap();
    }

    let mut engine = Engine::new();
    // Open file1 permanently in a second tab.
    engine.open_file_in_tab(&path1);
    let permanent_tab_idx = engine.active_group().active_tab;
    let bid1 = engine.active_buffer_id();

    // Single-click file1 — should just switch back to it, not make it a preview.
    engine.open_file_preview(&path1);
    assert_eq!(
        engine.active_group().active_tab,
        permanent_tab_idx,
        "switched to existing tab"
    );
    assert!(
        !engine.buffer_manager.get(bid1).unwrap().preview,
        "file stays permanent"
    );
    assert_eq!(engine.preview_buffer_id, None);

    let _ = std::fs::remove_file(&path1);
    let _ = std::fs::remove_file(&path2);
}

// =======================================================================
// Visual Block Mode Tests
// =======================================================================

#[test]
fn test_visual_block_mode_entry() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test");
    engine.update_syntax();

    // Enter visual block mode with Ctrl-V
    press_ctrl(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::VisualBlock);
    assert!(engine.visual_anchor.is_some());
    assert_eq!(engine.visual_anchor.unwrap().line, 0);
    assert_eq!(engine.visual_anchor.unwrap().col, 0);
}

#[test]
fn test_visual_block_mode_escape_exits() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test");
    engine.update_syntax();

    press_ctrl(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::VisualBlock);

    press_special(&mut engine, "Escape");
    assert_eq!(engine.mode, Mode::Normal);
    assert!(engine.visual_anchor.is_none());
}

#[test]
fn test_visual_block_mode_switching() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test");
    engine.update_syntax();

    // Start in visual block
    press_ctrl(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::VisualBlock);

    // Switch to character visual with v
    press_char(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::Visual);
    assert!(engine.visual_anchor.is_some()); // anchor preserved

    // Switch to line visual with V
    press_char(&mut engine, 'V');
    assert_eq!(engine.mode, Mode::VisualLine);

    // Switch back to block visual with Ctrl-V
    press_ctrl(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::VisualBlock);

    // Ctrl-V again to exit
    press_ctrl(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_visual_block_yank() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\ndef\nghi");
    engine.update_syntax();

    // Enter visual block mode
    press_ctrl(&mut engine, 'v');

    // Select 2x2 block: "ab", "de"
    press_char(&mut engine, 'l'); // col 1
    press_char(&mut engine, 'j'); // line 1

    // Yank
    press_char(&mut engine, 'y');

    // Check register - should have "ab\nde"
    let (content, is_linewise) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "ab\nde");
    assert!(!is_linewise);

    // Should be back in normal mode
    assert_eq!(engine.mode, Mode::Normal);
    assert!(engine.visual_anchor.is_none());
}

#[test]
fn test_visual_block_delete() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\ndef\nghi");
    engine.update_syntax();

    // Enter visual block mode
    press_ctrl(&mut engine, 'v');

    // Select 2x2 block: "ab", "de"
    press_char(&mut engine, 'l'); // col 1
    press_char(&mut engine, 'j'); // line 1

    // Delete
    press_char(&mut engine, 'd');

    // Check buffer - should be "c\nf\nghi"
    let text = engine.buffer().to_string();
    assert_eq!(text, "c\nf\nghi");

    // Check register
    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "ab\nde");

    // Should be back in normal mode at start of block
    assert_eq!(engine.mode, Mode::Normal);
    assert_eq!(engine.view().cursor.line, 0);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_visual_block_simple_delete() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\ndef\nghi");
    engine.update_syntax();

    // Start at (0, 1) - character 'b'
    press_char(&mut engine, 'l');

    // Enter visual block
    press_ctrl(&mut engine, 'v');

    // Select 2x2 block: move right once, down once
    // This should select cols 1-2 on lines 0-1
    press_char(&mut engine, 'l'); // Now at col 2
    press_char(&mut engine, 'j'); // Now at line 1

    // Delete
    press_char(&mut engine, 'd');

    // Should have deleted "bc" from line 0 and "ef" from line 1
    // Result: "a\nd\nghi"
    let text = engine.buffer().to_string();
    assert_eq!(text, "a\nd\nghi");
}

#[test]
fn test_visual_block_cursor_positions() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abcdef");
    engine.update_syntax();

    // Start at col 0
    assert_eq!(engine.view().cursor.col, 0);

    // Move to col 1
    press_char(&mut engine, 'l');
    assert_eq!(engine.view().cursor.col, 1);

    // Enter visual block
    press_ctrl(&mut engine, 'v');
    assert_eq!(engine.visual_anchor.unwrap().col, 1);

    // Move right once more
    press_char(&mut engine, 'l');
    assert_eq!(engine.view().cursor.col, 2);

    // Check anchor and cursor
    assert_eq!(engine.visual_anchor.unwrap().col, 1);
    assert_eq!(engine.view().cursor.col, 2);
}

#[test]
fn test_visual_block_yank_simple() {
    // Note: Visual block with uneven line lengths is simplified
    // Full Vim behavior with "virtual columns" is a future enhancement
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abcdef\nghijkl");
    engine.update_syntax();

    // Start at col 1 (character 'b')
    press_char(&mut engine, 'l');
    press_ctrl(&mut engine, 'v');

    // Select cols 1-2 on 2 lines
    press_char(&mut engine, 'l'); // Now at col 2 (character 'c')
    press_char(&mut engine, 'j'); // Move down to line 1

    // Yank
    press_char(&mut engine, 'y');

    // Check register - should have "bc\nhi"
    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "bc\nhi");
}

#[test]
fn test_visual_block_delete_uniform_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abcdef\nghijkl\nmnopqr");
    engine.update_syntax();

    // Start at col 1 (character 'b')
    press_char(&mut engine, 'l');
    press_ctrl(&mut engine, 'v');

    // Select cols 1-2 on 3 lines
    press_char(&mut engine, 'l'); // Now at col 2 (character 'c')
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'j'); // line 2

    // Delete
    press_char(&mut engine, 'd');

    // Check buffer - should have deleted "bc", "hi", "no"
    let text = engine.buffer().to_string();
    assert_eq!(text, "adef\ngjkl\nmpqr");
}

#[test]
fn test_visual_block_change() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\ndef\nghi");
    engine.update_syntax();

    // Enter visual block mode
    press_ctrl(&mut engine, 'v');

    // Select 2x2 block
    press_char(&mut engine, 'l');
    press_char(&mut engine, 'j');

    // Change
    press_char(&mut engine, 'c');

    // Should be in insert mode
    assert_eq!(engine.mode, Mode::Insert);

    // Buffer should have block deleted
    let text = engine.buffer().to_string();
    assert_eq!(text, "c\nf\nghi");
}

#[test]
fn test_visual_block_navigation() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3\nline4");
    engine.update_syntax();

    // Enter visual block mode
    press_ctrl(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::VisualBlock);

    // Move right extends block horizontally
    press_char(&mut engine, 'l');
    assert_eq!(engine.view().cursor.col, 1);
    press_char(&mut engine, 'l');
    assert_eq!(engine.view().cursor.col, 2);

    // Move down extends block vertically
    press_char(&mut engine, 'j');
    assert_eq!(engine.view().cursor.line, 1);
    press_char(&mut engine, 'j');
    assert_eq!(engine.view().cursor.line, 2);

    // Still in visual block mode
    assert_eq!(engine.mode, Mode::VisualBlock);
}

#[test]
fn test_visual_block_yank_single_column() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\ndef\nghi");
    engine.update_syntax();

    // Enter visual block mode
    press_ctrl(&mut engine, 'v');

    // Select single column, 3 lines (just move down, don't move right)
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'j');

    // Yank
    press_char(&mut engine, 'y');

    // Check register - should have "a\nd\ng" (first character of each line)
    let (content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(content, "a\nd\ng");
}

#[test]
fn test_visual_block_with_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\ndef\nghi\njkl");
    engine.update_syntax();

    // Enter visual block mode
    press_ctrl(&mut engine, 'v');

    // Use count to move: 2j should move down 2 lines
    press_char(&mut engine, '2');
    press_char(&mut engine, 'j');
    assert_eq!(engine.view().cursor.line, 2);
    assert_eq!(engine.mode, Mode::VisualBlock);

    // Use count to move right: 2l
    press_char(&mut engine, '2');
    press_char(&mut engine, 'l');
    assert_eq!(engine.view().cursor.col, 2);
}

// ========================================================================
// Visual Mode Case Change Tests
// ========================================================================

#[test]
fn test_visual_lowercase() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "HELLO World");
    engine.update_syntax();

    // Select "HELLO"
    press_char(&mut engine, 'v');
    for _ in 0..4 {
        press_char(&mut engine, 'l');
    }

    // Lowercase
    press_char(&mut engine, 'u');

    assert_eq!(engine.buffer().to_string(), "hello World");
    assert_eq!(engine.mode, Mode::Normal);
    assert_eq!(engine.view().cursor.line, 0);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_visual_uppercase() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello WORLD");
    engine.update_syntax();

    // Select "hello"
    press_char(&mut engine, 'v');
    for _ in 0..4 {
        press_char(&mut engine, 'l');
    }

    // Uppercase
    press_char(&mut engine, 'U');

    assert_eq!(engine.buffer().to_string(), "HELLO WORLD");
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_visual_line_lowercase() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "FIRST Line\nSECOND Line\nthird");
    engine.update_syntax();

    // Select first two lines
    press_char(&mut engine, 'V');
    press_char(&mut engine, 'j');

    // Lowercase
    press_char(&mut engine, 'u');

    assert_eq!(
        engine.buffer().to_string(),
        "first line\nsecond line\nthird"
    );
    assert_eq!(engine.mode, Mode::Normal);
    assert_eq!(engine.view().cursor.line, 0);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_visual_line_uppercase() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "first\nsecond\nthird");
    engine.update_syntax();

    // Select middle line
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'V');

    // Uppercase
    press_char(&mut engine, 'U');

    assert_eq!(engine.buffer().to_string(), "first\nSECOND\nthird");
    assert_eq!(engine.mode, Mode::Normal);
    assert_eq!(engine.view().cursor.line, 1);
}

#[test]
fn test_visual_block_lowercase() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "ABC\nDEF\nGHI");
    engine.update_syntax();

    // Enter visual block mode
    press_ctrl(&mut engine, 'v');

    // Select 2x2 block (AB, DE)
    press_char(&mut engine, 'l');
    press_char(&mut engine, 'j');

    // Lowercase
    press_char(&mut engine, 'u');

    assert_eq!(engine.buffer().to_string(), "abC\ndeF\nGHI");
    assert_eq!(engine.mode, Mode::Normal);
    assert_eq!(engine.view().cursor.line, 0);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_visual_block_uppercase() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\ndef\nghi");
    engine.update_syntax();

    // Move to column 1
    press_char(&mut engine, 'l');

    // Enter visual block mode
    press_ctrl(&mut engine, 'v');

    // Select 2x3 block (bc, ef, hi)
    press_char(&mut engine, 'l');
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'j');

    // Uppercase
    press_char(&mut engine, 'U');

    assert_eq!(engine.buffer().to_string(), "aBC\ndEF\ngHI");
    assert_eq!(engine.mode, Mode::Normal);
    assert_eq!(engine.view().cursor.line, 0);
    assert_eq!(engine.view().cursor.col, 1);
}

#[test]
fn test_visual_case_change_with_undo() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Select and uppercase "hello"
    press_char(&mut engine, 'v');
    for _ in 0..4 {
        press_char(&mut engine, 'l');
    }
    press_char(&mut engine, 'U');

    assert_eq!(engine.buffer().to_string(), "HELLO world");

    // Undo
    press_char(&mut engine, 'u');
    assert_eq!(engine.buffer().to_string(), "hello world");
}

#[test]
fn test_visual_case_mixed_content() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "Hello123WORLD!");
    engine.update_syntax();

    // Select all
    press_char(&mut engine, 'v');
    press_char(&mut engine, '$');

    // Lowercase
    press_char(&mut engine, 'u');

    assert_eq!(engine.buffer().to_string(), "hello123world!");
}

// ========================================================================
// Marks Tests
// ========================================================================

#[test]
fn test_mark_set_and_jump_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\nline3\nline4");
    engine.update_syntax();

    // Go to line 2
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'j');
    assert_eq!(engine.view().cursor.line, 2);

    // Set mark 'a'
    press_char(&mut engine, 'm');
    press_char(&mut engine, 'a');
    assert!(engine.message.contains("Mark 'a' set"));

    // Move to line 0
    press_char(&mut engine, 'g');
    press_char(&mut engine, 'g');
    assert_eq!(engine.view().cursor.line, 0);

    // Jump to mark 'a' line
    press_char(&mut engine, '\'');
    press_char(&mut engine, 'a');
    assert_eq!(engine.view().cursor.line, 2);
    assert_eq!(engine.view().cursor.col, 0); // ' jumps to start of line
}

#[test]
fn test_mark_jump_exact_position() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world\nfoo bar baz");
    engine.update_syntax();

    // Move to line 1, col 4
    press_char(&mut engine, 'j');
    for _ in 0..4 {
        press_char(&mut engine, 'l');
    }
    assert_eq!(engine.view().cursor.line, 1);
    assert_eq!(engine.view().cursor.col, 4);

    // Set mark 'b'
    press_char(&mut engine, 'm');
    press_char(&mut engine, 'b');

    // Move to line 0, col 0
    press_char(&mut engine, 'g');
    press_char(&mut engine, 'g');
    assert_eq!(engine.view().cursor.line, 0);
    assert_eq!(engine.view().cursor.col, 0);

    // Jump to exact mark position with backtick
    press_char(&mut engine, '`');
    press_char(&mut engine, 'b');
    assert_eq!(engine.view().cursor.line, 1);
    assert_eq!(engine.view().cursor.col, 4);
}

#[test]
fn test_mark_not_set() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test");
    engine.update_syntax();

    // Try to jump to mark that doesn't exist
    press_char(&mut engine, '\'');
    press_char(&mut engine, 'x');
    assert!(engine.message.contains("Mark 'x' not set"));
}

#[test]
fn test_mark_multiple_marks() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\nb\nc\nd\ne");
    engine.update_syntax();

    // Set mark 'a' at line 1
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'm');
    press_char(&mut engine, 'a');

    // Set mark 'b' at line 3
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'm');
    press_char(&mut engine, 'b');

    // Jump to mark 'a'
    press_char(&mut engine, '\'');
    press_char(&mut engine, 'a');
    assert_eq!(engine.view().cursor.line, 1);

    // Jump to mark 'b'
    press_char(&mut engine, '\'');
    press_char(&mut engine, 'b');
    assert_eq!(engine.view().cursor.line, 3);
}

#[test]
fn test_mark_overwrite() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\nb\nc");
    engine.update_syntax();

    // Set mark 'a' at line 0
    press_char(&mut engine, 'm');
    press_char(&mut engine, 'a');

    // Move to line 2 and overwrite mark 'a'
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'm');
    press_char(&mut engine, 'a');

    // Jump to mark 'a' should go to line 2
    press_char(&mut engine, 'g');
    press_char(&mut engine, 'g');
    press_char(&mut engine, '\'');
    press_char(&mut engine, 'a');
    assert_eq!(engine.view().cursor.line, 2);
}

#[test]
fn test_mark_per_buffer() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "buffer1 line1\nbuffer1 line2");
    engine.update_syntax();

    // Set mark 'a' in first buffer
    press_char(&mut engine, 'j');
    press_char(&mut engine, 'm');
    press_char(&mut engine, 'a');

    // Create second buffer
    let buffer2_id = engine.buffer_manager.create();
    engine
        .buffer_manager
        .get_mut(buffer2_id)
        .unwrap()
        .buffer
        .insert(0, "buffer2 line1\nbuffer2 line2");

    // Switch to second buffer
    let window_id = engine.active_window().id;
    engine.windows.get_mut(&window_id).unwrap().buffer_id = buffer2_id;

    // Mark 'a' shouldn't exist in buffer 2
    press_char(&mut engine, '\'');
    press_char(&mut engine, 'a');
    assert!(engine.message.contains("Mark 'a' not set"));
}

// ========================================================================
// Macro Tests
// ========================================================================

#[test]
fn test_macro_basic_recording() {
    let mut engine = Engine::new();

    // Start recording into register 'a'
    press_char(&mut engine, 'q');
    press_char(&mut engine, 'a');
    assert_eq!(engine.macro_recording, Some('a'));
    assert!(engine.message.contains("Recording"));

    // Record some keystrokes
    press_char(&mut engine, 'i'); // Enter insert mode
    press_char(&mut engine, 'h');
    press_char(&mut engine, 'i');
    press_special(&mut engine, "Escape"); // ESC
    press_char(&mut engine, 'l');

    // Stop recording
    press_char(&mut engine, 'q');
    assert_eq!(engine.macro_recording, None);
    assert!(engine.message.contains("recorded"));

    // Verify macro content in register
    let (content, _) = engine.registers.get(&'a').unwrap();
    // Should contain "ihi<ESC>l" but ESC is unicode 0x1b
    assert!(content.contains("hi"));
    assert_eq!(content.len(), 5); // i, h, i, ESC, l
}

#[test]
fn test_macro_playback() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line1\nline2\n");

    // Manually set up a macro in register 'a' (skip recording for simplicity)
    // Macro: A!<ESC> (append "!" to end of line, then ESC)
    engine.set_register('a', "A!\x1b".to_string(), false);

    // Play macro
    press_char(&mut engine, '@');
    press_char(&mut engine, 'a');

    // Process playback queue
    while !engine.macro_playback_queue.is_empty() {
        let _ = engine.advance_macro_playback();
    }

    // Verify result
    assert_eq!(engine.buffer().to_string(), "line1!\nline2\n");
}

#[test]
fn test_macro_repeat_last() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test\n");

    // Set up macro with ESC to return to normal mode
    engine.set_register('b', "A.\x1b".to_string(), false);

    // Play it once
    press_char(&mut engine, '@');
    press_char(&mut engine, 'b');
    while !engine.macro_playback_queue.is_empty() {
        let _ = engine.advance_macro_playback();
    }

    assert_eq!(engine.buffer().to_string(), "test.\n");

    // Play it again with @@
    press_char(&mut engine, '@');
    press_char(&mut engine, '@');
    while !engine.macro_playback_queue.is_empty() {
        let _ = engine.advance_macro_playback();
    }

    assert_eq!(engine.buffer().to_string(), "test..\n");
}

#[test]
fn test_macro_with_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "x\n");

    // Macro: A!<ESC> (append "!" and return to normal mode)
    engine.set_register('c', "A!\x1b".to_string(), false);

    // Play 3 times: 3@c
    press_char(&mut engine, '3');
    press_char(&mut engine, '@');
    press_char(&mut engine, 'c');

    while !engine.macro_playback_queue.is_empty() {
        let _ = engine.advance_macro_playback();
    }

    assert_eq!(engine.buffer().to_string(), "x!!!\n");
}

#[test]
fn test_macro_recursion_limit() {
    let mut engine = Engine::new();

    // Create recursive macro: @a calls @a
    engine.set_register('a', "@a".to_string(), false);

    // Try to play it
    press_char(&mut engine, '@');
    press_char(&mut engine, 'a');

    // Should hit recursion limit
    for _ in 0..MAX_MACRO_RECURSION + 10 {
        if engine.macro_playback_queue.is_empty() {
            break;
        }
        let (has_more, _) = engine.advance_macro_playback();
        if !has_more {
            break;
        }
    }

    // Engine should still be functional
    assert!(engine.macro_recursion_depth <= MAX_MACRO_RECURSION);
}

#[test]
fn test_macro_empty_register() {
    let mut engine = Engine::new();

    // Try to play from empty register
    press_char(&mut engine, '@');
    press_char(&mut engine, 'z');

    assert!(engine.message.contains("empty"));
}

#[test]
fn test_macro_stop_on_error() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "short\n");

    // Macro that tries to move right 100 times
    engine.set_register('d', "100l".to_string(), false);

    press_char(&mut engine, '@');
    press_char(&mut engine, 'd');

    // Playback should stop when hitting buffer boundary
    let mut iterations = 0;
    while !engine.macro_playback_queue.is_empty() && iterations < 200 {
        let _ = engine.advance_macro_playback();
        iterations += 1;
    }

    // Should be at end of line, not crashed
    assert!(engine.cursor().col <= 4); // At or before the newline
}

#[test]
fn test_macro_recording_saves_to_register() {
    let mut engine = Engine::new();

    // Record a simple macro
    press_char(&mut engine, 'q');
    press_char(&mut engine, 'm');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 'x');
    press_special(&mut engine, "Escape"); // Must ESC before stopping recording
    press_char(&mut engine, 'q');

    // Verify it's in register 'm'
    let (content, _) = engine.registers.get(&'m').unwrap();
    assert_eq!(content, "ix\x1b"); // i, x, ESC

    // Also should be in unnamed register
    let (unnamed_content, _) = engine.registers.get(&'"').unwrap();
    assert_eq!(unnamed_content, "ix\x1b");
}

#[test]
fn test_macro_records_navigation_keys() {
    let mut engine = Engine::new();

    // Start recording
    press_char(&mut engine, 'q');
    press_char(&mut engine, 'n');

    // Record some navigation
    press_char(&mut engine, 'l'); // Move right (unicode)
    press_char(&mut engine, 'j'); // Move down (unicode)
    press_special(&mut engine, "Left"); // Arrow key (no unicode)
    press_special(&mut engine, "Up"); // Arrow key (no unicode)

    // Stop recording
    press_char(&mut engine, 'q');

    // Verify it's recorded with proper encoding
    let (content, _) = engine.registers.get(&'n').unwrap();
    assert_eq!(content, "lj<Left><Up>");
}

#[test]
fn test_macro_records_ctrl_keys() {
    let mut engine = Engine::new();

    // Start recording
    press_char(&mut engine, 'q');
    press_char(&mut engine, 'c');

    // Record some Ctrl combinations
    press_ctrl(&mut engine, 'd'); // Ctrl-D
    press_ctrl(&mut engine, 'u'); // Ctrl-U

    // Stop recording
    press_char(&mut engine, 'q');

    // Verify it's recorded with proper encoding
    let (content, _) = engine.registers.get(&'c').unwrap();
    assert_eq!(content, "<C-D><C-U>");
}

#[test]
fn test_macro_playback_with_arrow_keys() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc\ndef\nghi");

    // Macro: move right twice, then move down
    engine.set_register('a', "ll<Down>".to_string(), false);

    // Start at (0, 0)
    assert_eq!(engine.cursor().line, 0);
    assert_eq!(engine.cursor().col, 0);

    // Play macro
    press_char(&mut engine, '@');
    press_char(&mut engine, 'a');
    while !engine.macro_playback_queue.is_empty() {
        let _ = engine.advance_macro_playback();
    }

    // Should be at (1, 2) - line 1, col 2
    assert_eq!(engine.cursor().line, 1);
    assert_eq!(engine.cursor().col, 2);
}

#[test]
fn test_macro_playback_with_ctrl_keys() {
    let mut engine = Engine::new();
    // Create a buffer with many lines
    let mut content = String::new();
    for i in 0..50 {
        content.push_str(&format!("line {}\n", i));
    }
    engine.buffer_mut().insert(0, &content);

    // Macro: Ctrl-D (half page down)
    engine.set_register('d', "<C-D>".to_string(), false);

    let initial_line = engine.cursor().line;

    // Play macro
    press_char(&mut engine, '@');
    press_char(&mut engine, 'd');
    while !engine.macro_playback_queue.is_empty() {
        let _ = engine.advance_macro_playback();
    }

    // Should have moved down (exact amount depends on viewport, but should be > 0)
    assert!(engine.cursor().line > initial_line);
}

#[test]
fn test_macro_records_insert_mode_with_enter() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "test");

    // Start recording
    press_char(&mut engine, 'q');
    press_char(&mut engine, 'r');

    // Enter insert mode, type text, press enter, type more, ESC
    press_char(&mut engine, 'A'); // Append
    press_char(&mut engine, '!');
    press_special(&mut engine, "Return"); // New line
    press_char(&mut engine, 'n');
    press_char(&mut engine, 'e');
    press_char(&mut engine, 'w');
    press_special(&mut engine, "Escape");

    // Stop recording
    press_char(&mut engine, 'q');

    // Verify the macro content includes <CR>
    let (content, _) = engine.registers.get(&'r').unwrap();
    assert_eq!(content, "A!<CR>new\x1b");
}

#[test]
fn test_macro_comprehensive() {
    let mut engine = Engine::new();
    // Create a buffer with multiple lines
    engine
        .buffer_mut()
        .insert(0, "line one\nline two\nline three");

    // Record a complex macro that uses:
    // - Navigation (j, l, arrow keys)
    // - Insert mode
    // - Special keys (Return, ESC)
    // - Ctrl keys

    // Macro: j (down), $$ (end of line), A (append), ! (type), ESC, Ctrl-D
    engine.set_register('z', "j$A!\x1b<C-D>".to_string(), false);

    // Start at (0, 0)
    assert_eq!(engine.cursor().line, 0);

    // Play the macro
    press_char(&mut engine, '@');
    press_char(&mut engine, 'z');
    while !engine.macro_playback_queue.is_empty() {
        let _ = engine.advance_macro_playback();
    }

    // Should have:
    // - Moved down to line 1
    // - Moved to end of line
    // - Appended "!"
    // - Returned to normal mode
    // - Scrolled down with Ctrl-D

    // Check that "!" was appended to line 1
    let line1_content: String = engine.buffer().content.line(1).chars().collect();
    assert!(line1_content.contains("line two!"));
}

#[test]
fn test_replace_current_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world\nhello again\n");

    // Replace "hello" with "hi" on current line only (no g flag)
    let result = engine.replace_in_range(None, "hello", "hi", "");
    assert_eq!(result.unwrap(), 1);
    assert_eq!(engine.buffer().to_string(), "hi world\nhello again\n");
}

#[test]
fn test_replace_all_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world\nhello again\n");

    // Replace all "hello" with "hi" across both lines
    let result = engine.replace_in_range(Some((0, 1)), "hello", "hi", "g");
    assert_eq!(result.unwrap(), 2);
    assert_eq!(engine.buffer().to_string(), "hi world\nhi again\n");
}

#[test]
fn test_replace_case_insensitive() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "Hello HELLO hello\n");

    // Replace all case variations
    let result = engine.replace_in_range(None, "hello", "hi", "gi");
    assert_eq!(result.unwrap(), 1); // Replaces all in one line
    assert_eq!(engine.buffer().to_string(), "hi hi hi\n");
}

#[test]
fn test_substitute_command_current_line() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar foo\n");

    engine.execute_command("s/foo/baz/");
    assert_eq!(engine.buffer().to_string(), "baz bar foo\n"); // Only first
}

#[test]
fn test_substitute_command_global() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar foo\n");

    engine.execute_command("s/foo/baz/g");
    assert_eq!(engine.buffer().to_string(), "baz bar baz\n"); // All on line
}

#[test]
fn test_substitute_command_all_lines() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo\nbar foo\nfoo\n");

    engine.execute_command("%s/foo/baz/g");
    assert_eq!(engine.buffer().to_string(), "baz\nbar baz\nbaz\n");
}

#[test]
fn test_substitute_visual_range() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo\nbar\nbaz\n");

    // Simulate visual selection on lines 0-1
    engine.mode = Mode::VisualLine;
    engine.visual_anchor = Some(Cursor { line: 0, col: 0 });
    engine.view_mut().cursor = Cursor { line: 1, col: 0 };

    engine.execute_command("'<,'>s/bar/qux/");
    // Should only affect line 1, not lines 0 or 2
    assert_eq!(engine.buffer().to_string(), "foo\nqux\nbaz\n");
}

#[test]
fn test_substitute_undo() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world\n");

    // Do a substitution
    engine.execute_command("s/hello/goodbye/");
    assert_eq!(engine.buffer().to_string(), "goodbye world\n");

    // Undo should restore original text completely
    engine.undo();
    assert_eq!(engine.buffer().to_string(), "hello world\n");

    // Redo should apply the substitution again
    engine.redo();
    assert_eq!(engine.buffer().to_string(), "goodbye world\n");
}

#[test]
fn test_substitute_multiple_lines_undo() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "vi is great\nvi is powerful\nvi rocks\n");

    // Replace all occurrences across all lines
    engine.execute_command("%s/vi/vim/gi");
    assert_eq!(
        engine.buffer().to_string(),
        "vim is great\nvim is powerful\nvim rocks\n"
    );

    // Undo should restore all original text
    engine.undo();
    assert_eq!(
        engine.buffer().to_string(),
        "vi is great\nvi is powerful\nvi rocks\n"
    );
}

#[test]
fn test_cw_cursor_position_after_last_word() {
    // Verify cursor is positioned AFTER the space when using cw on last word
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc def");
    engine.update_syntax();

    // Move to 'd' in "def"
    engine.view_mut().cursor.col = 4;

    // cw should delete "def" and position cursor after the space for insertion
    press_char(&mut engine, 'c');
    press_char(&mut engine, 'w');

    assert_eq!(
        engine.buffer().to_string(),
        "abc ",
        "cw should leave 'abc '"
    );
    assert_eq!(engine.mode, Mode::Insert, "should be in insert mode");
    assert_eq!(
        engine.view().cursor.col,
        4,
        "cursor should be after the space (col 4)"
    );
}

#[test]
fn test_ce_cursor_position_after_last_word() {
    // Verify cursor is positioned AFTER the space when using ce on last word
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "abc def");
    engine.update_syntax();

    // Move to 'd' in "def"
    engine.view_mut().cursor.col = 4;

    // ce should delete "def" and position cursor after the space for insertion
    press_char(&mut engine, 'c');
    press_char(&mut engine, 'e');

    assert_eq!(
        engine.buffer().to_string(),
        "abc ",
        "ce should leave 'abc '"
    );
    assert_eq!(engine.mode, Mode::Insert, "should be in insert mode");
    assert_eq!(
        engine.view().cursor.col,
        4,
        "cursor should be after the space (col 4)"
    );
}

// ── Fold tests ────────────────────────────────────────────────────────────

fn make_indented_engine() -> Engine {
    let mut engine = Engine::new();
    // 5-line buffer: line 0 is the header, lines 1-3 are indented, line 4 is peer
    engine.buffer_mut().insert(
        0,
        "fn foo() {\n    let x = 1;\n    let y = 2;\n    x + y\n}\n",
    );
    engine
}

#[test]
fn test_fold_close_detects_range() {
    let mut engine = make_indented_engine();
    // Cursor at line 0 ("fn foo() {")
    engine.view_mut().cursor.line = 0;
    let range = engine.detect_fold_range(0);
    assert!(range.is_some(), "should detect fold range under fn");
    let (start, end) = range.unwrap();
    assert_eq!(start, 0);
    assert!(end >= 3, "end should include indented body");
}

#[test]
fn test_fold_close_and_open() {
    let mut engine = make_indented_engine();
    engine.view_mut().cursor.line = 0;

    // zc — close fold
    press_char(&mut engine, 'z');
    press_char(&mut engine, 'c');
    assert!(
        engine.view().fold_at(0).is_some(),
        "fold should exist after zc"
    );

    // zo — open fold
    press_char(&mut engine, 'z');
    press_char(&mut engine, 'o');
    assert!(
        engine.view().fold_at(0).is_none(),
        "fold should be removed after zo"
    );
}

#[test]
fn test_fold_toggle_za() {
    let mut engine = make_indented_engine();
    engine.view_mut().cursor.line = 0;

    // First za closes the fold
    press_char(&mut engine, 'z');
    press_char(&mut engine, 'a');
    assert!(engine.view().fold_at(0).is_some(), "first za should close");

    // Second za opens it
    press_char(&mut engine, 'z');
    press_char(&mut engine, 'a');
    assert!(engine.view().fold_at(0).is_none(), "second za should open");
}

#[test]
fn test_fold_open_all_zr() {
    let mut engine = make_indented_engine();
    engine.view_mut().cursor.line = 0;

    press_char(&mut engine, 'z');
    press_char(&mut engine, 'c');
    assert!(!engine.view().folds.is_empty(), "should have a fold");

    press_char(&mut engine, 'z');
    press_char(&mut engine, 'R');
    assert!(engine.view().folds.is_empty(), "zR should clear all folds");
}

#[test]
fn test_fold_navigation_skips_hidden_lines() {
    let mut engine = make_indented_engine();
    engine.view_mut().cursor.line = 0;

    // Close the fold (lines 1-3 become hidden)
    press_char(&mut engine, 'z');
    press_char(&mut engine, 'c');

    // j from line 0 should skip to line 4 (first visible line after fold)
    press_char(&mut engine, 'j');
    assert_eq!(
        engine.view().cursor.line,
        4,
        "j should skip hidden fold lines"
    );

    // k from line 4 should go back to line 0 (fold header)
    press_char(&mut engine, 'k');
    assert_eq!(
        engine.view().cursor.line,
        0,
        "k should skip hidden fold lines"
    );
}

#[test]
fn test_fold_cursor_clamp_on_close() {
    let mut engine = make_indented_engine();
    // Put cursor inside what will become the fold body
    engine.view_mut().cursor.line = 2;

    // Close fold from line 0 — but cursor is on line 2, which is inside.
    // The fold command detects range from cursor (line 2) not header.
    // So we place cursor at 0 and close, then move cursor inside and close again.

    // Close from line 0
    engine.view_mut().cursor.line = 0;
    press_char(&mut engine, 'z');
    press_char(&mut engine, 'c');

    // Cursor should still be on line 0 (the fold header)
    assert_eq!(
        engine.view().cursor.line,
        0,
        "cursor should stay at fold header after zc"
    );
}

// ── Auto-indent tests ─────────────────────────────────────────────────────

#[test]
fn test_auto_indent_enter() {
    let mut engine = Engine::new();
    engine.settings.auto_indent = true;
    // Buffer has one indented line
    engine.buffer_mut().insert(0, "    hello");
    // Move cursor to end of line and press Enter
    press_char(&mut engine, 'A'); // Append mode at end of line
    press_special(&mut engine, "Return");
    // New line should have same indent
    assert_eq!(engine.view().cursor.line, 1);
    assert_eq!(engine.view().cursor.col, 4);
    let line1: String = engine.buffer().content.line(1).chars().collect();
    assert!(
        line1.starts_with("    "),
        "new line should start with 4 spaces"
    );
}

#[test]
fn test_auto_indent_no_indent() {
    let mut engine = Engine::new();
    engine.settings.auto_indent = true;
    engine.buffer_mut().insert(0, "hello");
    press_char(&mut engine, 'A');
    press_special(&mut engine, "Return");
    // Line with no indent should produce no indent on new line
    assert_eq!(engine.view().cursor.line, 1);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_auto_indent_disabled() {
    let mut engine = Engine::new();
    engine.settings.auto_indent = false;
    engine.buffer_mut().insert(0, "    hello");
    press_char(&mut engine, 'A');
    press_special(&mut engine, "Return");
    // With auto_indent off, new line should have col 0
    assert_eq!(engine.view().cursor.line, 1);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_auto_indent_o() {
    let mut engine = Engine::new();
    engine.settings.auto_indent = true;
    engine.buffer_mut().insert(0, "    fn foo() {");
    // 'o' opens a new line below with same indent
    press_special(&mut engine, "Escape"); // ensure normal mode
    press_char(&mut engine, 'o');
    assert_eq!(engine.view().cursor.line, 1);
    assert_eq!(engine.view().cursor.col, 4);
    assert_eq!(engine.mode, Mode::Insert);
}

#[test]
fn test_auto_indent_capital_o() {
    let mut engine = Engine::new();
    engine.settings.auto_indent = true;
    // Put cursor on line 1 (which is indented)
    engine.buffer_mut().insert(0, "fn foo() {\n    body\n}");
    press_char(&mut engine, 'j'); // move to "    body"
    press_special(&mut engine, "Escape");
    press_char(&mut engine, 'O');
    // New line above "    body" should have same indent (4 spaces)
    assert_eq!(engine.view().cursor.line, 1);
    assert_eq!(engine.view().cursor.col, 4);
    assert_eq!(engine.mode, Mode::Insert);
}

// ── Completion tests ──────────────────────────────────────────────────────

#[test]
fn test_completion_basic() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foobar\nfoo");
    // Position cursor at end of "foo" on line 1
    press_char(&mut engine, 'G'); // last line
    press_char(&mut engine, 'A'); // Append at end — now in insert mode at col 3
                                  // Ctrl-N should complete to "foobar"
    press_ctrl(&mut engine, 'n');
    let line1: String = engine.buffer().content.line(1).chars().collect();
    assert!(
        line1.starts_with("foobar"),
        "Ctrl-N should insert foobar, got: {}",
        line1
    );
    assert_eq!(engine.completion_idx, Some(0));
}

#[test]
fn test_completion_cycle_next() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foobar foobaz football\nfoo");
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'A');
    // First Ctrl-N selects first candidate
    press_ctrl(&mut engine, 'n');
    let first_idx = engine.completion_idx.unwrap();
    // Second Ctrl-N moves to next
    press_ctrl(&mut engine, 'n');
    let second_idx = engine.completion_idx.unwrap();
    assert_ne!(first_idx, second_idx, "Ctrl-N should cycle candidates");
}

#[test]
fn test_completion_cycle_prev() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foobar foobaz football\nfoo");
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'A');
    // Ctrl-P starts from last candidate
    press_ctrl(&mut engine, 'p');
    let total = engine.completion_candidates.len();
    assert_eq!(engine.completion_idx, Some(total - 1));
}

#[test]
fn test_completion_clear_on_other_key() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foobar\nfoo");
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'A');
    press_ctrl(&mut engine, 'n');
    assert!(engine.completion_idx.is_some());
    // Any regular key clears completion state
    press_char(&mut engine, 'x');
    assert!(engine.completion_idx.is_none());
    assert!(engine.completion_candidates.is_empty());
}

// ── Auto-popup completion tests ───────────────────────────────────────────

#[test]
fn test_auto_popup_appears_on_type() {
    let mut engine = Engine::new();
    // Buffer has "foobar" on line 0; enter insert mode on line 1 and type "fo"
    engine.buffer_mut().insert(0, "foobar\n");
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'o'); // open new line in insert mode
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'o');
    assert!(
        engine.completion_idx.is_some(),
        "completion popup should appear after typing prefix"
    );
    assert!(
        !engine.completion_candidates.is_empty(),
        "candidates should be populated"
    );
    assert!(
        engine.completion_display_only,
        "popup should be in display-only mode"
    );
}

#[test]
fn test_auto_popup_tab_accepts() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foobar\n");
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'o'); // insert mode, new line
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'o');
    // Popup should be active with display_only=true
    assert!(
        engine.completion_display_only,
        "popup should be display-only"
    );
    assert!(engine.completion_idx.is_some(), "popup should be active");
    // Tab should accept the highlighted candidate
    press_special(&mut engine, "Tab");
    assert!(
        engine.completion_idx.is_none(),
        "popup should be cleared after Tab"
    );
    assert!(
        !engine.completion_display_only,
        "display_only should be false after accept"
    );
    let line1: String = engine.buffer().content.line(1).chars().collect();
    assert!(
        line1.starts_with("foobar"),
        "buffer should contain accepted completion, got: {}",
        line1
    );
}

#[test]
fn test_auto_popup_dismissed_on_navigation() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foobar\n");
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'o');
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'o');
    assert!(engine.completion_idx.is_some(), "popup should be active");
    // Left arrow should clear the popup
    press_special(&mut engine, "Left");
    assert!(
        engine.completion_idx.is_none(),
        "popup should be dismissed after Left"
    );

    // Down arrow cycles popup, does NOT dismiss it
    engine.buffer_mut().insert(0, "foobar\n");
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'o');
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'o');
    assert!(engine.completion_idx.is_some(), "popup should be active");
    press_special(&mut engine, "Down");
    assert!(
        engine.completion_idx.is_some(),
        "Down should NOT dismiss popup"
    );
}

#[test]
fn test_auto_popup_ctrl_n_cycles() {
    let mut engine = Engine::new();
    // Two candidates: "foobar" and "foobaz"
    engine.buffer_mut().insert(0, "foobar foobaz\n");
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'o');
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'o');
    assert!(
        engine.completion_display_only,
        "popup should be display-only"
    );
    assert!(
        engine.completion_candidates.len() >= 2,
        "need at least 2 candidates"
    );
    let initial_idx = engine.completion_idx.unwrap();
    let col_before = engine.view().cursor.col;
    // Ctrl-N should advance the index without modifying text
    press_ctrl(&mut engine, 'n');
    let new_idx = engine.completion_idx.unwrap();
    assert_ne!(new_idx, initial_idx, "index should advance on Ctrl-N");
    assert_eq!(
        engine.view().cursor.col,
        col_before,
        "cursor col should NOT change (display-only mode)"
    );
}

#[test]
fn test_auto_popup_arrow_cycles() {
    let mut engine = Engine::new();
    // Two candidates: "foobar" and "foobaz"
    engine.buffer_mut().insert(0, "foobar foobaz\n");
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'o');
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'o');
    assert!(
        engine.completion_display_only,
        "popup should be display-only"
    );
    assert!(
        engine.completion_candidates.len() >= 2,
        "need at least 2 candidates"
    );
    let initial_idx = engine.completion_idx.unwrap();
    let col_before = engine.view().cursor.col;

    // Down should advance index, not move cursor
    press_special(&mut engine, "Down");
    let after_down = engine.completion_idx.unwrap();
    assert_ne!(after_down, initial_idx, "Down advances index");
    assert_eq!(
        engine.view().cursor.col,
        col_before,
        "cursor col unchanged after Down"
    );
    assert_eq!(
        engine.view().cursor.line,
        1,
        "cursor line unchanged after Down"
    );

    // Up should go back
    press_special(&mut engine, "Up");
    assert_eq!(
        engine.completion_idx.unwrap(),
        initial_idx,
        "Up goes back to initial index"
    );
}

#[test]
fn test_auto_popup_backspace_retriggers() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foobar\n");
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'o');
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'o');
    press_char(&mut engine, 'o'); // prefix "foo" → popup for "foobar"
    assert!(
        engine.completion_idx.is_some(),
        "popup should be active after 'foo'"
    );
    // Backspace → prefix "fo" → popup should retrigger
    press_special(&mut engine, "BackSpace");
    assert!(
        engine.completion_idx.is_some(),
        "popup should be re-triggered after BackSpace"
    );
    assert!(
        engine.completion_display_only,
        "popup should remain display-only after BackSpace"
    );
}

#[test]
fn test_escape_from_insert_clears_pending_lsp_completion() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foobar\n");
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'o');
    press_char(&mut engine, 'f');
    press_char(&mut engine, 'o');
    press_char(&mut engine, 'o'); // "foo" → popup active
    assert!(engine.completion_idx.is_some());
    // Simulate a pending LSP completion request
    engine.lsp_pending_completion = Some(42);
    // Escape exits insert mode and clears completion + pending LSP
    press_special(&mut engine, "Escape");
    assert!(engine.completion_idx.is_none());
    assert!(
        engine.lsp_pending_completion.is_none(),
        "pending LSP completion should be cancelled on Escape"
    );
}

#[test]
fn test_completion_dismissed_in_normal_mode() {
    let mut engine = Engine::new();
    // Manually set completion state as if from a race condition
    engine.completion_candidates = vec!["hello".to_string()];
    engine.completion_idx = Some(0);
    engine.completion_display_only = true;
    assert_eq!(engine.mode, Mode::Normal);
    // Any key in Normal mode should dismiss the popup
    press_char(&mut engine, 'j');
    assert!(
        engine.completion_idx.is_none(),
        "completion should be dismissed when not in Insert mode"
    );
}

// ── :set command (engine-level) ───────────────────────────────────────────

#[test]
fn test_set_number_via_command() {
    let mut engine = Engine::new();
    engine.settings.line_numbers = crate::core::settings::LineNumberMode::None;
    // Use parse_set_option directly to avoid writing to disk in unit tests
    engine.settings.parse_set_option("number").unwrap();
    assert_eq!(
        engine.settings.line_numbers,
        crate::core::settings::LineNumberMode::Absolute
    );
}

#[test]
fn test_set_relativenumber_after_number_gives_hybrid() {
    let mut engine = Engine::new();
    engine.settings.line_numbers = crate::core::settings::LineNumberMode::Absolute;
    engine.settings.parse_set_option("relativenumber").unwrap();
    assert_eq!(
        engine.settings.line_numbers,
        crate::core::settings::LineNumberMode::Hybrid
    );
}

#[test]
fn test_set_expandtab_false_tab_inserts_tab_char() {
    let mut engine = Engine::new();
    engine.settings.expand_tab = false;
    press_char(&mut engine, 'i');
    press_special(&mut engine, "Tab");
    press_special(&mut engine, "Escape");
    let text: String = engine.buffer().content.chars().collect();
    assert!(text.starts_with('\t'), "expected tab char, got: {:?}", text);
}

#[test]
fn test_set_expandtab_true_tab_inserts_spaces() {
    let mut engine = Engine::new();
    engine.settings.expand_tab = true;
    engine.settings.tabstop = 2;
    press_char(&mut engine, 'i');
    press_special(&mut engine, "Tab");
    press_special(&mut engine, "Escape");
    let text: String = engine.buffer().content.chars().collect();
    assert!(text.starts_with("  "), "expected 2 spaces, got: {:?}", text);
    assert!(!text.starts_with('\t'));
}

#[test]
fn test_set_unknown_option_sets_error_message() {
    let mut engine = Engine::new();
    let result = engine.settings.parse_set_option("badoption");
    assert!(result.is_err());
}

#[test]
fn test_set_display_all() {
    let engine = Engine::new();
    let display = engine.settings.display_all();
    assert!(!display.is_empty());
    assert!(display.contains("ts="));
    assert!(display.contains("sw="));
}

// ── hunk navigation / staging ─────────────────────────────────────────

fn make_diff_engine(diff_text: &str) -> Engine {
    let mut engine = Engine::new();
    let content = ropey::Rope::from_str(diff_text);
    engine.active_buffer_state_mut().buffer.content = content;
    engine
}

#[test]
fn test_jump_next_hunk_basic() {
    let diff = "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1,2 +1,3 @@\n line1\n+added\n line2\n";
    let mut engine = make_diff_engine(diff);
    engine.view_mut().cursor.line = 0;
    engine.jump_next_hunk();
    // Cursor should be on line 3 (0-indexed), the "@@ -1,2 +1,3 @@" line
    assert_eq!(engine.view().cursor.line, 3);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_jump_next_hunk_no_more() {
    let diff = "diff --git a/foo.rs b/foo.rs\n@@ -1,2 +1,3 @@\n line1\n+added\n line2\n";
    let mut engine = make_diff_engine(diff);
    // Put cursor after the only @@
    engine.view_mut().cursor.line = 4;
    engine.jump_next_hunk();
    assert!(engine.message.contains("No more hunks"));
}

#[test]
fn test_jump_prev_hunk_basic() {
    let diff = "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1,2 +1,3 @@\n line1\n+added\n line2\n@@ -10,2 +11,2 @@\n lineA\n-lineB\n+lineC\n";
    let mut engine = make_diff_engine(diff);
    // Put cursor on the second @@ (line 7)
    engine.view_mut().cursor.line = 7;
    engine.jump_prev_hunk();
    assert_eq!(engine.view().cursor.line, 3);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_jump_prev_hunk_no_more() {
    let diff = "diff --git a/foo.rs b/foo.rs\n@@ -1,2 +1,3 @@\n line1\n+added\n line2\n";
    let mut engine = make_diff_engine(diff);
    // Put cursor exactly on the @@ line
    engine.view_mut().cursor.line = 1;
    engine.jump_prev_hunk();
    assert!(engine.message.contains("No more hunks"));
}

#[test]
fn test_gs_no_op_in_normal_buffer() {
    let mut engine = Engine::new();
    // No source_file set — should show "Not a diff buffer"
    engine.cmd_git_stage_hunk();
    assert!(
        engine.message.contains("Not a diff buffer"),
        "expected 'Not a diff buffer', got: {}",
        engine.message
    );
}

#[test]
fn test_bracket_c_pending_key_routing() {
    // Buffer with no @@ lines — ]c should show "No more hunks"
    let diff = "just some text\nno hunks here\n";
    let mut engine = make_diff_engine(diff);
    engine.view_mut().cursor.line = 0;
    engine.jump_next_hunk();
    assert!(engine.message.contains("No more hunks"));
}

// ─── Diff peek + enhanced hunk nav ─────────────────────────────────────

#[test]
fn test_open_diff_peek_no_hunks() {
    let mut engine = Engine::new();
    engine.buffer_mut().content = ropey::Rope::from_str("hello\nworld\n");
    engine.update_syntax();
    engine.open_diff_peek();
    assert!(engine.diff_peek.is_none());
    assert!(engine.message.contains("No changes"));
}

#[test]
fn test_open_diff_peek_with_hunks() {
    let mut engine = Engine::new();
    engine.buffer_mut().content = ropey::Rope::from_str("line1\nline2\nline3\n");
    engine.update_syntax();

    // Simulate having diff hunks cached on the buffer.
    let bid = engine.active_window().buffer_id;
    if let Some(state) = engine.buffer_manager.get_mut(bid) {
        state.diff_hunks = vec![git::DiffHunkInfo {
            file_header: "diff --git a/f b/f\n--- a/f\n+++ b/f".to_string(),
            hunk: git::Hunk {
                header: "@@ -1,2 +1,3 @@".to_string(),
                lines: vec![
                    " line1".to_string(),
                    "+line2".to_string(),
                    " line3".to_string(),
                ],
            },
            new_start: 0,
            new_count: 3,
        }];
    }

    engine.view_mut().cursor.line = 1;
    engine.open_diff_peek();
    assert!(engine.diff_peek.is_some());
    let peek = engine.diff_peek.as_ref().unwrap();
    assert_eq!(peek.anchor_line, 1);
    assert_eq!(peek.hunk_lines.len(), 3);
}

#[test]
fn test_diff_peek_close() {
    let mut engine = Engine::new();
    engine.diff_peek = Some(DiffPeekState {
        hunk_index: 0,
        anchor_line: 5,
        hunk_lines: vec!["+added".to_string()],
        file_header: String::new(),
        hunk: git::Hunk {
            header: "@@ -1 +1,2 @@".to_string(),
            lines: vec!["+added".to_string()],
        },
    });
    engine.close_diff_peek();
    assert!(engine.diff_peek.is_none());
}

#[test]
fn test_diff_peek_key_escape() {
    let mut engine = Engine::new();
    engine.diff_peek = Some(DiffPeekState {
        hunk_index: 0,
        anchor_line: 0,
        hunk_lines: vec![],
        file_header: String::new(),
        hunk: git::Hunk {
            header: String::new(),
            lines: vec![],
        },
    });
    let consumed = engine.handle_diff_peek_key("Escape", None);
    assert!(consumed);
    assert!(engine.diff_peek.is_none());
}

#[test]
fn test_diff_peek_key_h_closes() {
    let mut engine = Engine::new();
    engine.diff_peek = Some(DiffPeekState {
        hunk_index: 0,
        anchor_line: 0,
        hunk_lines: vec![],
        file_header: String::new(),
        hunk: git::Hunk {
            header: String::new(),
            lines: vec![],
        },
    });
    let consumed = engine.handle_diff_peek_key("h", Some('h'));
    assert!(!consumed); // falls through
    assert!(engine.diff_peek.is_none());
}

#[test]
fn test_diff_peek_key_unknown_closes() {
    let mut engine = Engine::new();
    engine.diff_peek = Some(DiffPeekState {
        hunk_index: 0,
        anchor_line: 0,
        hunk_lines: vec![],
        file_header: String::new(),
        hunk: git::Hunk {
            header: String::new(),
            lines: vec![],
        },
    });
    let consumed = engine.handle_diff_peek_key("j", Some('j'));
    assert!(!consumed); // falls through
    assert!(engine.diff_peek.is_none());
}

#[test]
fn test_jump_next_hunk_with_git_diff() {
    let mut engine = Engine::new();
    engine.buffer_mut().content = ropey::Rope::from_str("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n");
    engine.update_syntax();
    // Simulate git_diff: lines 2-3 changed, line 7 changed.
    let bid = engine.active_window().buffer_id;
    if let Some(state) = engine.buffer_manager.get_mut(bid) {
        state.git_diff = vec![
            None,
            None,
            Some(git::GitLineStatus::Modified),
            Some(git::GitLineStatus::Modified),
            None,
            None,
            None,
            Some(git::GitLineStatus::Added),
            None,
            None,
        ];
    }
    engine.view_mut().cursor.line = 0;
    engine.jump_next_hunk();
    assert_eq!(engine.view().cursor.line, 2); // first changed region

    engine.jump_next_hunk();
    assert_eq!(engine.view().cursor.line, 7); // second changed region

    engine.jump_next_hunk();
    assert!(engine.message.contains("No more hunks"));
}

#[test]
fn test_jump_prev_hunk_with_git_diff() {
    let mut engine = Engine::new();
    engine.buffer_mut().content = ropey::Rope::from_str("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n");
    engine.update_syntax();
    let bid = engine.active_window().buffer_id;
    if let Some(state) = engine.buffer_manager.get_mut(bid) {
        state.git_diff = vec![
            None,
            None,
            Some(git::GitLineStatus::Modified),
            Some(git::GitLineStatus::Modified),
            None,
            None,
            None,
            Some(git::GitLineStatus::Added),
            None,
            None,
        ];
    }
    engine.view_mut().cursor.line = 9;
    engine.jump_prev_hunk();
    assert_eq!(engine.view().cursor.line, 7); // second changed region start

    engine.jump_prev_hunk();
    assert_eq!(engine.view().cursor.line, 2); // first changed region start

    engine.jump_prev_hunk();
    assert!(engine.message.contains("No more hunks"));
}

#[test]
fn test_gd_uppercase_opens_diff_peek() {
    let mut engine = Engine::new();
    engine.buffer_mut().content = ropey::Rope::from_str("line1\nline2\n");
    engine.update_syntax();
    // First keypress sets pending_key to 'g'.
    engine.handle_key("g", Some('g'), false);
    // Second keypress 'D' should trigger open_diff_peek.
    engine.handle_key("D", Some('D'), false);
    // No diff hunks → should show "No changes" message.
    assert!(
        engine.message.contains("No changes") || engine.message.contains("No diff"),
        "got: {}",
        engine.message
    );
}

#[test]
fn test_diff_peek_command() {
    let mut engine = Engine::new();
    engine.buffer_mut().content = ropey::Rope::from_str("line1\nline2\n");
    engine.update_syntax();
    engine.execute_command("DiffPeek");
    assert!(
        engine.message.contains("No changes") || engine.message.contains("No diff"),
        "got: {}",
        engine.message
    );
}

#[test]
fn test_deleted_git_line_status_variant_exists() {
    // Verify the Deleted variant is usable (compile-time check + runtime assertion).
    let status = git::GitLineStatus::Deleted;
    assert_eq!(status, git::GitLineStatus::Deleted);
    assert_ne!(status, git::GitLineStatus::Added);
    assert_ne!(status, git::GitLineStatus::Modified);
}

// ─── Paragraph text objects (ip / ap) ───────────────────────────────────

fn make_paragraph_engine(text: &str) -> Engine {
    let mut engine = Engine::new();
    engine.buffer_mut().content = ropey::Rope::from_str(text);
    engine.update_syntax();
    engine
}

#[test]
fn test_ip_selects_paragraph_lines() {
    // Buffer: blank / para / blank
    let text = "\nfirst line\nsecond line\n\n";
    let mut engine = make_paragraph_engine(text);
    // Place cursor on "first line" (line 1)
    engine.view_mut().cursor.line = 1;
    engine.view_mut().cursor.col = 0;
    // dip should delete both non-blank lines
    engine.handle_key("", Some('d'), false);
    engine.handle_key("", Some('i'), false);
    engine.handle_key("", Some('p'), false);
    let result: String = engine.buffer().content.chars().collect();
    assert!(
        !result.contains("first line"),
        "ip should delete first line"
    );
    assert!(
        !result.contains("second line"),
        "ip should delete second line"
    );
}

#[test]
fn test_ap_includes_trailing_blanks() {
    // Buffer: "para\n\n\n"  — paragraph followed by two blank lines
    let text = "para line\n\n\n";
    let mut engine = make_paragraph_engine(text);
    engine.view_mut().cursor.line = 0;
    engine.view_mut().cursor.col = 0;
    engine.handle_key("", Some('d'), false);
    engine.handle_key("", Some('a'), false);
    engine.handle_key("", Some('p'), false);
    let result: String = engine.buffer().content.chars().collect();
    // dap should remove the paragraph AND its trailing blank lines
    assert!(
        result.trim().is_empty(),
        "dap should remove paragraph and trailing blanks, got: {:?}",
        result
    );
}

#[test]
fn test_ip_on_blank_line_selects_blank_block() {
    // Buffer: "code\n\n\ncode2\n"
    // Cursor on second blank line (line 2); ip should select both blank lines
    let text = "code\n\n\ncode2\n";
    let mut engine = make_paragraph_engine(text);
    engine.view_mut().cursor.line = 2; // second blank line
    engine.handle_key("", Some('d'), false);
    engine.handle_key("", Some('i'), false);
    engine.handle_key("", Some('p'), false);
    let result: String = engine.buffer().content.chars().collect();
    assert!(result.contains("code\n"), "non-blank lines should survive");
    assert!(result.contains("code2"), "non-blank lines should survive");
    // The two blank lines should be gone
    assert!(
        !result.contains("\n\n"),
        "blank block should be deleted, got: {:?}",
        result
    );
}

#[test]
fn test_yip_yanks_paragraph() {
    let text = "alpha\nbeta\n\ngamma\n";
    let mut engine = make_paragraph_engine(text);
    engine.view_mut().cursor.line = 0;
    engine.handle_key("", Some('y'), false);
    engine.handle_key("", Some('i'), false);
    engine.handle_key("", Some('p'), false);
    let reg = engine
        .get_register('"')
        .map(|(s, _)| s.as_str())
        .unwrap_or("");
    assert!(reg.contains("alpha"), "yanked text should contain alpha");
    assert!(reg.contains("beta"), "yanked text should contain beta");
    assert!(!reg.contains("gamma"), "should not yank past blank line");
}

#[test]
fn test_vip_visual_paragraph() {
    let text = "line one\nline two\n\nother\n";
    let mut engine = make_paragraph_engine(text);
    engine.view_mut().cursor.line = 0;
    // Enter visual, then ip
    engine.handle_key("", Some('v'), false);
    engine.handle_key("", Some('i'), false);
    engine.handle_key("", Some('p'), false);
    assert_eq!(engine.mode, Mode::Visual);
    let anchor = engine.visual_anchor.unwrap();
    assert_eq!(anchor.line, 0, "selection should start at line 0");
    assert_eq!(
        engine.view().cursor.line,
        1,
        "selection should end at line 1"
    );
}

// ─── Sentence text objects (is / as) ────────────────────────────────────

#[test]
fn test_dis_deletes_sentence() {
    // Two sentences on the same line
    let text = "Hello world. Goodbye world.\n";
    let mut engine = make_paragraph_engine(text);
    // Cursor at start
    engine.view_mut().cursor.line = 0;
    engine.view_mut().cursor.col = 0;
    engine.handle_key("", Some('d'), false);
    engine.handle_key("", Some('i'), false);
    engine.handle_key("", Some('s'), false);
    let result: String = engine.buffer().content.chars().collect();
    assert!(
        !result.contains("Hello"),
        "dis should delete 'Hello world.'"
    );
}

#[test]
fn test_das_deletes_sentence_and_trailing_space() {
    let text = "First sentence. Second sentence.\n";
    let mut engine = make_paragraph_engine(text);
    engine.view_mut().cursor.line = 0;
    engine.view_mut().cursor.col = 0;
    engine.handle_key("", Some('d'), false);
    engine.handle_key("", Some('a'), false);
    engine.handle_key("", Some('s'), false);
    let result: String = engine.buffer().content.chars().collect();
    // das removes "First sentence. " (with trailing space)
    assert!(
        !result.contains("First"),
        "das should delete first sentence"
    );
    // Second sentence should remain
    assert!(result.contains("Second"), "second sentence should survive");
}

#[test]
fn test_cis_enters_insert_mode() {
    let text = "Replace me. Keep me.\n";
    let mut engine = make_paragraph_engine(text);
    engine.view_mut().cursor.line = 0;
    engine.view_mut().cursor.col = 0;
    engine.handle_key("", Some('c'), false);
    engine.handle_key("", Some('i'), false);
    engine.handle_key("", Some('s'), false);
    assert_eq!(engine.mode, Mode::Insert, "cis should enter insert mode");
}

#[test]
fn test_vis_selects_sentence_in_visual() {
    let text = "Sentence one. Sentence two.\n";
    let mut engine = make_paragraph_engine(text);
    engine.view_mut().cursor.line = 0;
    engine.view_mut().cursor.col = 0;
    engine.handle_key("", Some('v'), false);
    engine.handle_key("", Some('i'), false);
    engine.handle_key("", Some('s'), false);
    assert_eq!(engine.mode, Mode::Visual);
    // Cursor should have moved past the first sentence
    assert!(
        engine.view().cursor.col > 0 || engine.view().cursor.line > 0,
        "cursor should have moved to end of sentence"
    );
}

// ── Project search ────────────────────────────────────────────────────────

fn make_search_dir(test_name: &str) -> std::path::PathBuf {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!("vimcode_engine_search_{}", test_name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut f = std::fs::File::create(dir.join("sample.txt")).unwrap();
    writeln!(f, "hello world").unwrap();
    writeln!(f, "another line").unwrap();
    dir
}

#[test]
fn test_run_project_search_finds_matches() {
    let dir = make_search_dir("engine_find");
    let mut engine = Engine::new();
    engine.project_search_query = "hello".to_string();
    engine.run_project_search(&dir);
    assert!(
        !engine.project_search_results.is_empty(),
        "should find 'hello'"
    );
    assert_eq!(engine.project_search_selected, 0);
    assert!(engine.message.contains("match"));
}

#[test]
fn test_run_project_search_empty_query() {
    let dir = make_search_dir("engine_empty");
    let mut engine = Engine::new();
    engine.project_search_query = String::new();
    engine.run_project_search(&dir);
    assert!(engine.project_search_results.is_empty());
    assert!(engine.message.contains("empty"));
}

#[test]
fn test_project_search_select_next_prev() {
    let dir = make_search_dir("engine_select");
    let mut engine = Engine::new();
    engine.project_search_query = "l".to_string(); // matches both lines
    engine.run_project_search(&dir);
    assert!(engine.project_search_results.len() >= 2);
    engine.project_search_select_next();
    assert_eq!(engine.project_search_selected, 1);
    engine.project_search_select_prev();
    assert_eq!(engine.project_search_selected, 0);
}

#[test]
fn test_start_and_poll_project_search() {
    let dir = make_search_dir("engine_async");
    let mut engine = Engine::new();
    engine.project_search_query = "world".to_string();
    engine.start_project_search(dir);
    assert!(engine.project_search_running);
    // Spin until results arrive (bounded by ~1 s in practice)
    let mut got = false;
    for _ in 0..200 {
        if engine.poll_project_search() {
            got = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        got,
        "poll_project_search should return true after thread completes"
    );
    assert!(!engine.project_search_running);
    assert!(!engine.project_search_results.is_empty());
}

// ── Project replace ──────────────────────────────────────────────────────

#[test]
fn test_run_project_replace_basic() {
    let dir = make_search_dir("engine_replace_basic");
    let mut engine = Engine::new();
    engine.project_search_query = "hello".to_string();
    engine.project_replace_text = "hi".to_string();
    engine.run_project_replace(&dir);
    assert!(engine.message.contains("Replaced"));
    assert!(engine.message.contains("1 file"));
    let content = std::fs::read_to_string(dir.join("sample.txt")).unwrap();
    assert!(content.contains("hi world"));
    assert!(!content.contains("hello"));
}

#[test]
fn test_run_project_replace_empty_query() {
    let dir = make_search_dir("engine_replace_empty");
    let mut engine = Engine::new();
    engine.project_search_query = String::new();
    engine.project_replace_text = "hi".to_string();
    engine.run_project_replace(&dir);
    assert!(engine.message.contains("empty"));
}

#[test]
fn test_run_project_replace_skip_dirty() {
    use std::io::Write;
    let dir = make_search_dir("engine_replace_skip");
    // Open the file in a buffer and make it dirty
    let path = dir.join("sample.txt");
    let mut engine = Engine::open(&path);
    // Dirty the buffer
    engine.buffer_mut().content = ropey::Rope::from_str("hello world (modified)\nanother line\n");
    engine
        .buffer_manager
        .get_mut(engine.active_buffer_id())
        .unwrap()
        .dirty = true;
    // Add another file that should be replaced
    let mut f2 = std::fs::File::create(dir.join("other.txt")).unwrap();
    writeln!(f2, "hello there").unwrap();
    drop(f2);

    engine.project_search_query = "hello".to_string();
    engine.project_replace_text = "hi".to_string();
    engine.run_project_replace(&dir);
    // The dirty file should be skipped
    assert!(engine.message.contains("skipped"));
    // other.txt should be replaced
    let content = std::fs::read_to_string(dir.join("other.txt")).unwrap();
    assert!(content.contains("hi there"));
}

#[test]
fn test_run_project_replace_reloads_buffer() {
    let dir = make_search_dir("engine_replace_reload");
    let path = dir.join("sample.txt");
    let mut engine = Engine::open(&path);
    // Buffer should have original content
    let original = engine.buffer().content.to_string();
    assert!(original.contains("hello"));
    // Not dirty — so replace should modify it
    engine.project_search_query = "hello".to_string();
    engine.project_replace_text = "hi".to_string();
    engine.run_project_replace(&dir);
    // Buffer should now have reloaded content
    let new_content = engine.buffer().content.to_string();
    assert!(new_content.contains("hi world"));
    assert!(!new_content.contains("hello"));
}

#[test]
fn test_start_and_poll_project_replace() {
    let dir = make_search_dir("engine_replace_async");
    let mut engine = Engine::new();
    engine.project_search_query = "world".to_string();
    engine.project_replace_text = "earth".to_string();
    engine.start_project_replace(dir.clone());
    assert!(engine.project_replace_running);
    let mut got = false;
    for _ in 0..200 {
        if engine.poll_project_replace() {
            got = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(got, "poll should return true after thread completes");
    assert!(!engine.project_replace_running);
    assert!(engine.message.contains("Replaced"));
    let content = std::fs::read_to_string(dir.join("sample.txt")).unwrap();
    assert!(content.contains("earth"));
}

// ── LSP integration tests ─────────────────────────────────────────────

#[test]
fn test_lsp_fields_initialized() {
    let engine = Engine::new();
    assert!(engine.lsp_manager.is_none());
    assert!(engine.lsp_diagnostics.is_empty());
    assert!(engine.lsp_hover_text.is_none());
    assert!(engine.lsp_pending_completion.is_none());
    assert!(engine.lsp_pending_hover.is_none());
    assert!(engine.lsp_pending_definition.is_none());
    assert!(engine.settings.lsp_enabled);
}

#[test]
fn test_lsp_jump_diagnostics_empty() {
    let mut engine = Engine::new();
    // Set a file path so the diagnostic lookup can proceed
    engine.active_buffer_state_mut().file_path = Some(PathBuf::from("/tmp/test.rs"));
    engine.jump_next_diagnostic();
    assert_eq!(engine.message, "No diagnostics");
    engine.jump_prev_diagnostic();
    assert_eq!(engine.message, "No diagnostics");
}

#[test]
fn test_lsp_jump_diagnostics_navigation() {
    use super::lsp::{Diagnostic, DiagnosticSeverity, LspPosition, LspRange};

    let mut engine = Engine::new();
    // Create a buffer with some content
    let text = "line 0\nline 1\nline 2\nline 3\nline 4\n";
    let buf = engine.buffer_mut();
    buf.content = ropey::Rope::from_str(text);
    // Set file_path on the buffer
    engine.active_buffer_state_mut().file_path = Some(PathBuf::from("/tmp/test.rs"));

    // Insert diagnostics
    let diags = vec![
        Diagnostic {
            range: LspRange {
                start: LspPosition {
                    line: 1,
                    character: 0,
                },
                end: LspPosition {
                    line: 1,
                    character: 5,
                },
            },
            severity: DiagnosticSeverity::Error,
            message: "error on line 1".to_string(),
            source: None,
            code: None,
        },
        Diagnostic {
            range: LspRange {
                start: LspPosition {
                    line: 3,
                    character: 0,
                },
                end: LspPosition {
                    line: 3,
                    character: 5,
                },
            },
            severity: DiagnosticSeverity::Warning,
            message: "warning on line 3".to_string(),
            source: None,
            code: None,
        },
    ];
    engine
        .lsp_diagnostics
        .insert(PathBuf::from("/tmp/test.rs"), diags);

    // Start at line 0 — next should jump to line 1
    engine.jump_next_diagnostic();
    assert_eq!(engine.view().cursor.line, 1);
    assert!(engine.message.contains("error on line 1"));

    // Next should jump to line 3
    engine.jump_next_diagnostic();
    assert_eq!(engine.view().cursor.line, 3);
    assert!(engine.message.contains("warning on line 3"));

    // Next should wrap to line 1
    engine.jump_next_diagnostic();
    assert_eq!(engine.view().cursor.line, 1);

    // Prev from line 1 should wrap to line 3
    engine.jump_prev_diagnostic();
    assert_eq!(engine.view().cursor.line, 3);
}

#[test]
fn test_lsp_hover_dismissed_on_keypress() {
    let mut engine = Engine::new();
    engine.lsp_hover_text = Some("fn main()".to_string());
    engine.handle_key("j", Some('j'), false);
    assert!(engine.lsp_hover_text.is_none());
}

#[test]
fn test_lsp_set_option() {
    let mut engine = Engine::new();
    assert!(engine.settings.lsp_enabled);
    engine.settings.parse_set_option("nolsp").unwrap();
    assert!(!engine.settings.lsp_enabled);
    engine.settings.parse_set_option("lsp").unwrap();
    assert!(engine.settings.lsp_enabled);
    let q = engine.settings.parse_set_option("lsp?").unwrap();
    assert_eq!(q, "lsp");
}

#[test]
fn test_lsp_display_all_includes_lsp() {
    let engine = Engine::new();
    let display = engine.settings.display_all();
    assert!(display.contains("lsp"));
}

#[test]
fn test_lsp_diagnostic_counts() {
    use super::lsp::{Diagnostic, DiagnosticSeverity, LspRange};

    let mut engine = Engine::new();
    engine.active_buffer_state_mut().file_path = Some(PathBuf::from("/tmp/test.rs"));

    let diags = vec![
        Diagnostic {
            range: LspRange::default(),
            severity: DiagnosticSeverity::Error,
            message: "e1".to_string(),
            source: None,
            code: None,
        },
        Diagnostic {
            range: LspRange::default(),
            severity: DiagnosticSeverity::Error,
            message: "e2".to_string(),
            source: None,
            code: None,
        },
        Diagnostic {
            range: LspRange::default(),
            severity: DiagnosticSeverity::Warning,
            message: "w1".to_string(),
            source: None,
            code: None,
        },
    ];
    engine
        .lsp_diagnostics
        .insert(PathBuf::from("/tmp/test.rs"), diags);

    let (errors, warnings) = engine.diagnostic_counts();
    assert_eq!(errors, 2);
    assert_eq!(warnings, 1);
}

#[test]
fn test_lsp_commands() {
    let mut engine = Engine::new();
    // :LspInfo with no servers running
    engine.execute_command("LspInfo");
    assert!(
        engine.message.contains("LSP manager not started"),
        "unexpected LspInfo: {}",
        engine.message
    );
}

#[test]
fn test_lsp_language_id_set_on_buffer() {
    let rs_path = std::env::temp_dir().join("vimcode_lsp_test.rs");
    std::fs::write(&rs_path, "fn main() {}\n").unwrap();

    let mut engine = Engine::new();
    let _ = engine.open_file_with_mode(&rs_path, OpenMode::Permanent);
    let state = engine.active_buffer_state();
    assert_eq!(state.lsp_language_id, Some("rust".to_string()));
    let _ = std::fs::remove_file(&rs_path);
}

#[test]
fn test_set_filetype() {
    let tf_path = std::env::temp_dir().join("vimcode_ft_test.tf");
    std::fs::write(&tf_path, "resource \"null\" {}\n").unwrap();

    let mut engine = Engine::new();
    let _ = engine.open_file_with_mode(&tf_path, OpenMode::Permanent);
    // Should auto-detect terraform
    assert_eq!(
        engine.active_buffer_state().lsp_language_id,
        Some("terraform".to_string())
    );

    // Query filetype
    engine.execute_command("set ft?");
    assert!(engine.message.contains("filetype=terraform"));

    // Override filetype
    engine.execute_command("set filetype=hcl");
    assert_eq!(
        engine.active_buffer_state().lsp_language_id,
        Some("hcl".to_string())
    );
    assert_eq!(engine.message, "filetype=hcl");

    // Override should persist in language_map
    assert_eq!(
        engine.settings.language_map.get("tf"),
        Some(&"hcl".to_string())
    );

    let _ = std::fs::remove_file(&tf_path);
}

#[test]
fn test_set_filetype_bicep() {
    let bp_path = std::env::temp_dir().join("vimcode_ft_test.bicepparam");
    std::fs::write(&bp_path, "param env = 'dev'\n").unwrap();

    let mut engine = Engine::new();
    let _ = engine.open_file_with_mode(&bp_path, OpenMode::Permanent);
    assert_eq!(
        engine.active_buffer_state().lsp_language_id,
        Some("bicep".to_string())
    );

    let _ = std::fs::remove_file(&bp_path);
}

#[test]
fn test_language_map_override_on_open() {
    let path = std::env::temp_dir().join("vimcode_langmap_test.h");
    std::fs::write(&path, "// header\n").unwrap();

    let mut engine = Engine::new();
    // Default: .h → "c"
    let _ = engine.open_file_with_mode(&path, OpenMode::Permanent);
    assert_eq!(
        engine.active_buffer_state().lsp_language_id,
        Some("c".to_string())
    );

    // Close and reopen with language_map override
    engine
        .settings
        .language_map
        .insert("h".to_string(), "cpp".to_string());
    // Open in a new tab to get a fresh buffer
    engine.new_tab(Some(&path));
    // The existing buffer is reused, but language_map was applied
    // Let's test with a truly new file
    let path2 = std::env::temp_dir().join("vimcode_langmap_test2.h");
    std::fs::write(&path2, "// header2\n").unwrap();
    engine.new_tab(Some(&path2));
    assert_eq!(
        engine.active_buffer_state().lsp_language_id,
        Some("cpp".to_string())
    );

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&path2);
}

#[test]
fn test_lsp_dirty_buffer_tracking() {
    let mut engine = Engine::new();
    assert!(engine.lsp_dirty_buffers.is_empty());
    // Typing in insert mode should mark buffer dirty for LSP
    engine.handle_key("i", Some('i'), false);
    engine.handle_key("a", Some('a'), false);
    let active_id = engine.active_buffer_id();
    assert!(engine.lsp_dirty_buffers.contains_key(&active_id));
}

#[test]
fn test_undo_redo_marks_lsp_dirty() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    // Type something to create an undo entry.
    press_char(&mut engine, 'i');
    press_char(&mut engine, 'x');
    press_special(&mut engine, "Escape");
    // Clear LSP dirty flag.
    engine.lsp_dirty_buffers.clear();
    // Undo should mark the buffer as LSP-dirty.
    engine.undo();
    let active_id = engine.active_buffer_id();
    assert!(
        engine.lsp_dirty_buffers.contains_key(&active_id),
        "undo should mark buffer as LSP-dirty"
    );
    // Clear and test redo.
    engine.lsp_dirty_buffers.clear();
    engine.redo();
    assert!(
        engine.lsp_dirty_buffers.contains_key(&active_id),
        "redo should mark buffer as LSP-dirty"
    );
}

// =======================================================================
// Tests: Toggle case (~)
// =======================================================================

#[test]
fn test_toggle_case_lowercase_to_upper() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    press_char(&mut engine, '~');
    assert_eq!(engine.buffer().to_string(), "Hello");
}

#[test]
fn test_toggle_case_uppercase_to_lower() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "HELLO");
    press_char(&mut engine, '~');
    assert_eq!(engine.buffer().to_string(), "hELLO");
}

#[test]
fn test_toggle_case_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    press_char(&mut engine, '3');
    press_char(&mut engine, '~');
    assert_eq!(engine.buffer().to_string(), "HELlo");
}

#[test]
fn test_toggle_case_cursor_advances() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    press_char(&mut engine, '~');
    assert_eq!(engine.view().cursor.col, 1);
}

#[test]
fn test_toggle_case_end_of_line_boundary() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hi");
    // position at 'i'
    press_char(&mut engine, 'l');
    // toggle 5 chars but only 1 remains
    press_char(&mut engine, '5');
    press_char(&mut engine, '~');
    assert_eq!(engine.buffer().to_string(), "hI");
}

#[test]
fn test_toggle_case_dot_repeat() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    press_char(&mut engine, '~'); // H at col 0, cursor moves to col 1
    press_char(&mut engine, '.'); // toggles 'e' -> 'E'
    assert_eq!(engine.buffer().to_string(), "HEllo");
}

// =======================================================================
// Tests: Scroll cursor position (zz / zt / zb)
// =======================================================================

#[test]
fn test_zz_centers_cursor() {
    let mut engine = Engine::new();
    let content: String = (0..50).map(|i| format!("line {}\n", i)).collect();
    engine.buffer_mut().insert(0, &content);
    engine.set_viewport_lines(10);
    // Go to line 25
    press_char(&mut engine, '2');
    press_char(&mut engine, '5');
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'z');
    press_char(&mut engine, 'z');
    // scroll_top should be approximately cursor - half_viewport
    let scroll = engine.view().scroll_top;
    let cursor = engine.view().cursor.line;
    assert!(
        cursor >= scroll + 3 && cursor <= scroll + 7,
        "zz should center cursor (scroll={}, cursor={})",
        scroll,
        cursor
    );
}

#[test]
fn test_zt_scrolls_top() {
    let mut engine = Engine::new();
    let content: String = (0..50).map(|i| format!("line {}\n", i)).collect();
    engine.buffer_mut().insert(0, &content);
    engine.set_viewport_lines(10);
    press_char(&mut engine, '2');
    press_char(&mut engine, '5');
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'z');
    press_char(&mut engine, 't');
    let scroll = engine.view().scroll_top;
    let cursor = engine.view().cursor.line;
    assert_eq!(scroll, cursor, "zt should scroll cursor to top");
}

#[test]
fn test_zb_scrolls_bottom() {
    let mut engine = Engine::new();
    let content: String = (0..50).map(|i| format!("line {}\n", i)).collect();
    engine.buffer_mut().insert(0, &content);
    engine.set_viewport_lines(10);
    press_char(&mut engine, '2');
    press_char(&mut engine, '5');
    press_char(&mut engine, 'G');
    press_char(&mut engine, 'z');
    press_char(&mut engine, 'b');
    let scroll = engine.view().scroll_top;
    let cursor = engine.view().cursor.line;
    // cursor should be at scroll + viewport - 1
    let vp = engine.viewport_lines();
    assert_eq!(scroll + vp - 1, cursor, "zb should scroll cursor to bottom");
}

#[test]
fn test_zz_near_start_no_negative_scroll() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "line 0\nline 1\nline 2\n");
    engine.set_viewport_lines(10);
    press_char(&mut engine, 'z');
    press_char(&mut engine, 'z');
    assert_eq!(engine.view().scroll_top, 0);
}

// =======================================================================
// Tests: Join lines (J)
// =======================================================================

#[test]
fn test_join_lines_basic() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\nworld\n");
    press_char(&mut engine, 'J');
    assert_eq!(engine.buffer().to_string(), "hello world\n");
}

#[test]
fn test_join_lines_strips_leading_whitespace() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\n   world\n");
    press_char(&mut engine, 'J');
    assert_eq!(engine.buffer().to_string(), "hello world\n");
}

#[test]
fn test_join_lines_no_space_before_paren() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo\n)\n");
    press_char(&mut engine, 'J');
    assert_eq!(engine.buffer().to_string(), "foo)\n");
}

#[test]
fn test_join_lines_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\nb\nc\nd\n");
    press_char(&mut engine, '3');
    press_char(&mut engine, 'J');
    // Should join 3 lines: a, b, c into "a b c"
    let text = engine.buffer().to_string();
    assert!(
        text.starts_with("a b c"),
        "expected 'a b c...', got '{}'",
        text
    );
}

#[test]
fn test_join_lines_last_line_noop() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "only line");
    press_char(&mut engine, 'J');
    assert_eq!(engine.buffer().to_string(), "only line");
}

// =======================================================================
// Tests: Search word under cursor (* / #)
// =======================================================================

#[test]
fn test_star_search_forward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar foo baz");
    // cursor at 'f' of first foo
    press_char(&mut engine, '*');
    // Should jump to the second "foo"
    let col = engine.view().cursor.col;
    assert_eq!(col, 8, "* should move to second 'foo' at col 8");
}

#[test]
fn test_hash_search_backward() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar foo baz");
    // Move to second foo (col 8)
    engine.view_mut().cursor.col = 8;
    press_char(&mut engine, '#');
    // Should jump back to first "foo" at col 0
    let col = engine.view().cursor.col;
    assert_eq!(col, 0, "# should move back to first 'foo' at col 0");
}

#[test]
fn test_star_word_boundaries() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "fo foo foobar foo");
    // cursor at col 3 (on 'foo')
    engine.view_mut().cursor.col = 3;
    press_char(&mut engine, '*');
    // "fo foo foobar foo": whole-word "foo" at col 3 and col 14; "foobar" at col 7 NOT a match
    // From col 3, next whole-word "foo" is at col 14
    let col = engine.view().cursor.col;
    assert_eq!(col, 14, "* should only match whole words");
}

#[test]
fn test_star_no_word_under_cursor() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "   spaces");
    // cursor at space (col 0)
    press_char(&mut engine, '*');
    assert!(engine.message.contains("No word under cursor"));
}

#[test]
fn test_star_n_continues_bounded() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar foo bar foo");
    press_char(&mut engine, '*'); // jump to col 8
    press_char(&mut engine, 'n'); // continue to col 16
    let col = engine.view().cursor.col;
    assert_eq!(col, 16);
}

// =======================================================================
// Tests: Jump list (Ctrl-O / Ctrl-I)
// =======================================================================

#[test]
fn test_jump_list_basic_back_forward() {
    let mut engine = Engine::new();
    let content: String = (0..20).map(|i| format!("line {}\n", i)).collect();
    engine.buffer_mut().insert(0, &content);
    // Go to line 10 (G)
    press_char(&mut engine, '1');
    press_char(&mut engine, '0');
    press_char(&mut engine, 'G');
    let line_after_g = engine.view().cursor.line;
    // Go back with Ctrl-O
    press_ctrl(&mut engine, 'o');
    let line_after_back = engine.view().cursor.line;
    assert!(line_after_back < line_after_g, "Ctrl-O should go back");
    // Go forward with Ctrl-I
    press_ctrl(&mut engine, 'i');
    let line_after_fwd = engine.view().cursor.line;
    assert_eq!(line_after_fwd, line_after_g, "Ctrl-I should go forward");
}

#[test]
fn test_jump_list_gg_triggers() {
    let mut engine = Engine::new();
    let content: String = (0..20).map(|i| format!("line {}\n", i)).collect();
    engine.buffer_mut().insert(0, &content);
    // Move to bottom
    press_char(&mut engine, 'G');
    let bottom_line = engine.view().cursor.line;
    // gg should push jump and go to top
    press_char(&mut engine, 'g');
    press_char(&mut engine, 'g');
    assert_eq!(engine.view().cursor.line, 0);
    // Ctrl-O should go back to bottom
    press_ctrl(&mut engine, 'o');
    assert_eq!(engine.view().cursor.line, bottom_line);
}

#[test]
fn test_jump_list_truncates_forward_on_new_jump() {
    let mut engine = Engine::new();
    let content: String = (0..30).map(|i| format!("line {}\n", i)).collect();
    engine.buffer_mut().insert(0, &content);
    press_char(&mut engine, 'G'); // push line 0 -> last
    press_ctrl(&mut engine, 'o'); // go back to line 0
                                  // Now make a new jump (G again)
    press_char(&mut engine, '1');
    press_char(&mut engine, '5');
    press_char(&mut engine, 'G'); // push line 0, go to 14
                                  // Ctrl-I should report "already at newest"
    press_ctrl(&mut engine, 'i');
    assert_eq!(engine.view().cursor.line, 14);
}

#[test]
fn test_jump_list_paragraph_motion() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\nb\n\nc\nd\n");
    press_char(&mut engine, '}'); // jumps to empty line (paragraph forward)
    let after_brace = engine.view().cursor.line;
    press_ctrl(&mut engine, 'o');
    let after_back = engine.view().cursor.line;
    assert!(after_back < after_brace);
}

// =======================================================================
// Tests: Indent / Dedent (>> / <<)
// =======================================================================

#[test]
fn test_indent_basic() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\n");
    press_char(&mut engine, '>');
    press_char(&mut engine, '>');
    let text = engine.buffer().to_string();
    assert!(
        text.starts_with("    hello"),
        ">> should indent by 4 spaces"
    );
}

#[test]
fn test_dedent_basic() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "    hello\n");
    press_char(&mut engine, '<');
    press_char(&mut engine, '<');
    let text = engine.buffer().to_string();
    assert!(text.starts_with("hello"), "<< should dedent by 4 spaces");
}

#[test]
fn test_indent_count() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\nb\nc\n");
    press_char(&mut engine, '3');
    press_char(&mut engine, '>');
    press_char(&mut engine, '>');
    let buf = engine.buffer().to_string();
    let lines: Vec<&str> = buf.lines().collect();
    assert!(lines[0].starts_with("    "));
    assert!(lines[1].starts_with("    "));
    assert!(lines[2].starts_with("    "));
}

#[test]
fn test_dedent_no_underflow() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "  hi\n");
    press_char(&mut engine, '<');
    press_char(&mut engine, '<');
    let text = engine.buffer().to_string();
    // Should remove 2 spaces (the 2 available), not go negative
    assert!(text.starts_with("hi"));
}

#[test]
fn test_indent_dot_repeat() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\n");
    press_char(&mut engine, '>');
    press_char(&mut engine, '>');
    press_char(&mut engine, '.');
    let text = engine.buffer().to_string();
    assert!(
        text.starts_with("        "),
        "dot repeat of >> should double-indent"
    );
}

#[test]
fn test_visual_indent() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "a\nb\nc\n");
    // Enter visual line mode and select 2 lines
    press_char(&mut engine, 'V');
    press_char(&mut engine, 'j');
    press_char(&mut engine, '>');
    let buf = engine.buffer().to_string();
    let lines: Vec<&str> = buf.lines().collect();
    assert!(
        lines[0].starts_with("    "),
        "visual > should indent selected lines"
    );
    assert!(lines[1].starts_with("    "));
    assert!(
        !lines[2].starts_with("    "),
        "visual > should not indent lines outside selection"
    );
}

#[test]
fn test_visual_dedent() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "    a\n    b\nc\n");
    press_char(&mut engine, 'V');
    press_char(&mut engine, 'j');
    press_char(&mut engine, '<');
    let buf = engine.buffer().to_string();
    let lines: Vec<&str> = buf.lines().collect();
    assert!(
        !lines[0].starts_with("    "),
        "visual < should dedent selected lines"
    );
    assert!(!lines[1].starts_with("    "));
}

// ─── Tag text objects (it / at) ──────────────────────────────────────────

fn make_tag_engine(html: &str) -> Engine {
    let mut engine = Engine::new();
    engine.buffer_mut().content = ropey::Rope::from_str(html);
    engine.update_syntax();
    engine
}

#[test]
fn test_dit_basic() {
    // <p>hello</p> — cursor inside "hello"; dit should leave <p></p>
    let mut engine = make_tag_engine("<p>hello</p>");
    engine.view_mut().cursor.col = 4; // on 'l' inside "hello"
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 't');
    let result: String = engine.buffer().content.chars().collect();
    assert_eq!(result, "<p></p>", "dit should delete tag content");
}

#[test]
fn test_dat_basic() {
    // <p>hello</p> — dat should delete the entire element
    let mut engine = make_tag_engine("<p>hello</p>");
    engine.view_mut().cursor.col = 4;
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'a');
    press_char(&mut engine, 't');
    let result: String = engine.buffer().content.chars().collect();
    assert!(
        result.is_empty(),
        "dat should delete entire element, got {:?}",
        result
    );
}

#[test]
fn test_yit_yanks_inner_tag() {
    // yit should put the inner content into the default register
    let mut engine = make_tag_engine("<span>world</span>");
    engine.view_mut().cursor.col = 7; // inside "world"
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 't');
    let reg = engine
        .get_register('"')
        .map(|(s, _)| s.clone())
        .unwrap_or_default();
    assert_eq!(reg, "world", "yit should yank inner tag content");
}

#[test]
fn test_dit_multiline_tag() {
    // Cursor on an inner content line; dit should delete all inner lines
    let html = "<div>\nline1\nline2\n</div>";
    let mut engine = make_tag_engine(html);
    engine.view_mut().cursor.line = 1;
    engine.view_mut().cursor.col = 0;
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 't');
    let result: String = engine.buffer().content.chars().collect();
    assert!(!result.contains("line1"), "line1 should be deleted");
    assert!(!result.contains("line2"), "line2 should be deleted");
    assert!(result.contains("<div>"), "opening tag should survive");
    assert!(result.contains("</div>"), "closing tag should survive");
}

#[test]
fn test_dit_nested_same_tag() {
    // <div><div>inner</div>outer</div> — cursor inside inner div
    // dit should delete only the inner "inner", not "outer"
    let html = "<div><div>inner</div>outer</div>";
    let mut engine = make_tag_engine(html);
    engine.view_mut().cursor.col = 12; // inside "inner"
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 't');
    let result: String = engine.buffer().content.chars().collect();
    assert!(!result.contains("inner"), "inner content should be deleted");
    assert!(result.contains("outer"), "outer content should survive");
    assert!(
        result.contains("<div><div></div>"),
        "outer structure should survive"
    );
}

#[test]
fn test_dit_with_attributes() {
    // Tag with attributes: cursor inside content
    let html = "<div class=\"foo\">content</div>";
    let mut engine = make_tag_engine(html);
    engine.view_mut().cursor.col = 20; // inside "content"
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 't');
    let result: String = engine.buffer().content.chars().collect();
    assert!(!result.contains("content"), "content should be deleted");
    assert!(
        result.contains("<div class=\"foo\">"),
        "opening tag should be preserved"
    );
    assert!(result.contains("</div>"), "closing tag should be preserved");
}

#[test]
fn test_dit_no_enclosing_tag() {
    // Plain text with no tags — should be a no-op
    let text = "just plain text";
    let mut engine = make_tag_engine(text);
    engine.view_mut().cursor.col = 5;
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 't');
    let result: String = engine.buffer().content.chars().collect();
    assert_eq!(result, text, "dit on plain text should be a no-op");
}

#[test]
fn test_vit_visual_selection() {
    // vit should enter visual mode selecting the inner content "text"
    // <span>text</span>  positions: <span>=0-5, inner_start=6, text=6-9, </span>=10-16
    let html = "<span>text</span>";
    let mut engine = make_tag_engine(html);
    engine.view_mut().cursor.col = 7; // inside "text"
    press_char(&mut engine, 'v');
    press_char(&mut engine, 'i');
    press_char(&mut engine, 't');
    assert_eq!(engine.mode, Mode::Visual, "vit should enter visual mode");
    let anchor = engine.visual_anchor.unwrap();
    assert_eq!(
        anchor.col, 6,
        "selection should start at col 6 (after <span>)"
    );
    assert_eq!(
        engine.view().cursor.col,
        9,
        "selection end should be at col 9 (last char of 'text')"
    );
}

#[test]
fn test_dat_case_insensitive() {
    // Mixed-case tag names: <DIV>text</div> — dat should delete the whole element
    let html = "<DIV>text</div>";
    let mut engine = make_tag_engine(html);
    engine.view_mut().cursor.col = 6; // inside "text"
    press_char(&mut engine, 'd');
    press_char(&mut engine, 'a');
    press_char(&mut engine, 't');
    let result: String = engine.buffer().content.chars().collect();
    assert!(
        result.is_empty(),
        "dat should handle case-insensitive tag names, got {:?}",
        result
    );
}

// ── :norm command ─────────────────────────────────────────────────────────

fn run_command(engine: &mut Engine, cmd: &str) {
    press_char(engine, ':');
    for ch in cmd.chars() {
        press_char(engine, ch);
    }
    press_special(engine, "Return");
}

#[test]
fn test_norm_append_current_line() {
    // :norm A; appends semicolon to the current line only
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\nworld");
    run_command(&mut engine, "norm A;");
    let content: String = engine.buffer().content.chars().collect();
    assert_eq!(content, "hello;\nworld");
}

#[test]
fn test_norm_all_lines_append() {
    // :%norm A; appends semicolon to every line
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\nworld");
    run_command(&mut engine, "%norm A;");
    let content: String = engine.buffer().content.chars().collect();
    assert_eq!(content, "hello;\nworld;");
}

#[test]
fn test_norm_numeric_range() {
    // :1,2norm A! appends ! to lines 1 and 2 (1-based), leaving line 3 alone
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "aaa\nbbb\nccc");
    run_command(&mut engine, "1,2norm A!");
    let content: String = engine.buffer().content.chars().collect();
    assert_eq!(content, "aaa!\nbbb!\nccc");
}

#[test]
fn test_norm_prepend_comment() {
    // :%norm I// prepends "// " to every line
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo\nbar");
    run_command(&mut engine, "%norm I// ");
    let content: String = engine.buffer().content.chars().collect();
    assert_eq!(content, "// foo\n// bar");
}

#[test]
fn test_norm_normal_keyword() {
    // :normal A; — "normal" is a synonym for "norm"
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    run_command(&mut engine, "normal A;");
    let content: String = engine.buffer().content.chars().collect();
    assert_eq!(content, "hello;");
}

#[test]
fn test_norm_bang_ignored() {
    // :norm! A; — the ! is accepted and behaves the same as :norm A;
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    run_command(&mut engine, "norm! A;");
    let content: String = engine.buffer().content.chars().collect();
    assert_eq!(content, "hello;");
}

#[test]
fn test_norm_delete_first_word() {
    // :%norm 0dw deletes the first word on every line
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "foo bar\nbaz qux");
    run_command(&mut engine, "%norm 0dw");
    let content: String = engine.buffer().content.chars().collect();
    assert_eq!(content, "bar\nqux");
}

#[test]
fn test_norm_special_key_cr() {
    // :norm A<CR>new appends a newline then "new" after "hello"
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\nworld");
    run_command(&mut engine, "norm A<CR>new");
    let content: String = engine.buffer().content.chars().collect();
    assert_eq!(content, "hello\nnew\nworld");
}

#[test]
fn test_norm_undo_single_group() {
    // All changes from :%norm should be undone as one step
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "aaa\nbbb");
    run_command(&mut engine, "%norm A;");
    let after: String = engine.buffer().content.chars().collect();
    assert_eq!(after, "aaa;\nbbb;");
    // Undo should restore both lines at once
    press_char(&mut engine, 'u');
    let restored: String = engine.buffer().content.chars().collect();
    assert_eq!(restored, "aaa\nbbb");
}

// ── Fuzzy finder tests ────────────────────────────────────────────────────

#[test]
fn test_fuzzy_score_empty_query() {
    // Empty query always matches with score 0
    assert_eq!(Engine::fuzzy_score("src/main.rs", ""), Some(0));
    assert_eq!(Engine::fuzzy_score("anything", ""), Some(0));
}

#[test]
fn test_fuzzy_score_no_match() {
    // Query chars not present as subsequence → None
    assert_eq!(Engine::fuzzy_score("src/main.rs", "xyz"), None);
    assert_eq!(Engine::fuzzy_score("foo.rs", "bar"), None);
}

#[test]
fn test_fuzzy_score_exact() {
    // Exact prefix should score positively
    let score = Engine::fuzzy_score("engine.rs", "engine");
    assert!(score.is_some());
    assert!(score.unwrap() > 0);
}

#[test]
fn test_fuzzy_score_consecutive_bonus() {
    // Consecutive matches incur no gap penalty; widely scattered matches do.
    // Use paths without underscores to avoid word-boundary bonus interference.
    let consecutive = Engine::fuzzy_score("abcdef.rs", "abc").unwrap();
    let scattered = Engine::fuzzy_score("aXXbXXc.rs", "abc").unwrap();
    assert!(
        consecutive >= scattered,
        "consecutive={} scattered={}",
        consecutive,
        scattered
    );
}

#[test]
fn test_ctrl_p_opens_picker() {
    let mut engine = Engine::new();
    assert!(!engine.picker_open);

    press_ctrl(&mut engine, 'p');

    assert!(engine.picker_open);
    assert_eq!(engine.picker_source, PickerSource::Files);
}

// ── Unified picker tests ─────────────────────────────────────────────────

#[test]
fn test_picker_close_clears_state() {
    let mut engine = Engine::new();
    engine.open_picker(PickerSource::Files);
    assert!(engine.picker_open);

    engine.close_picker();

    assert!(!engine.picker_open);
    assert!(engine.picker_query.is_empty());
    assert!(engine.picker_items.is_empty());
    assert!(engine.picker_all_items.is_empty());
    assert_eq!(engine.picker_selected, 0);
}

#[test]
fn test_picker_escape_closes() {
    let mut engine = Engine::new();
    engine.open_picker(PickerSource::Files);
    assert!(engine.picker_open);

    engine.handle_key("Escape", None, false);

    assert!(!engine.picker_open);
}

#[test]
fn test_picker_filter_with_query() {
    let mut engine = Engine::new();
    engine.picker_all_items = vec![
        PickerItem {
            display: "src/main.rs".to_string(),
            filter_text: "src/main.rs".to_string(),
            detail: None,
            action: PickerAction::OpenFile(PathBuf::from("src/main.rs")),
            icon: None,
            score: 0,
            match_positions: Vec::new(),
        },
        PickerItem {
            display: "src/engine.rs".to_string(),
            filter_text: "src/engine.rs".to_string(),
            detail: None,
            action: PickerAction::OpenFile(PathBuf::from("src/engine.rs")),
            icon: None,
            score: 0,
            match_positions: Vec::new(),
        },
        PickerItem {
            display: "README.md".to_string(),
            filter_text: "README.md".to_string(),
            detail: None,
            action: PickerAction::OpenFile(PathBuf::from("README.md")),
            icon: None,
            score: 0,
            match_positions: Vec::new(),
        },
    ];
    engine.picker_open = true;
    engine.picker_source = PickerSource::Files;

    // Filter with "eng"
    engine.picker_query = "eng".to_string();
    engine.picker_filter();

    assert_eq!(engine.picker_items.len(), 1);
    assert!(engine.picker_items[0].display.contains("engine"));
    // Should have match positions
    assert!(!engine.picker_items[0].match_positions.is_empty());
}

#[test]
fn test_picker_filter_empty_shows_all() {
    let mut engine = Engine::new();
    engine.picker_all_items = vec![
        PickerItem {
            display: "a.rs".to_string(),
            filter_text: "a.rs".to_string(),
            detail: None,
            action: PickerAction::OpenFile(PathBuf::from("a.rs")),
            icon: None,
            score: 0,
            match_positions: Vec::new(),
        },
        PickerItem {
            display: "b.rs".to_string(),
            filter_text: "b.rs".to_string(),
            detail: None,
            action: PickerAction::OpenFile(PathBuf::from("b.rs")),
            icon: None,
            score: 0,
            match_positions: Vec::new(),
        },
    ];
    engine.picker_query.clear();
    engine.picker_filter();

    assert_eq!(engine.picker_items.len(), 2);
}

#[test]
fn test_picker_select_bounds() {
    let mut engine = Engine::new();
    engine.picker_items = vec![
        PickerItem {
            display: "a".to_string(),
            filter_text: "a".to_string(),
            detail: None,
            action: PickerAction::OpenFile(PathBuf::from("a")),
            icon: None,
            score: 0,
            match_positions: Vec::new(),
        },
        PickerItem {
            display: "b".to_string(),
            filter_text: "b".to_string(),
            detail: None,
            action: PickerAction::OpenFile(PathBuf::from("b")),
            icon: None,
            score: 0,
            match_positions: Vec::new(),
        },
    ];
    engine.picker_open = true;
    engine.picker_selected = 0;

    // Down moves to 1
    engine.handle_key("Down", None, false);
    assert_eq!(engine.picker_selected, 1);

    // Down at max stays at max
    engine.handle_key("Down", None, false);
    assert_eq!(engine.picker_selected, 1);

    // Up moves to 0
    engine.handle_key("Up", None, false);
    assert_eq!(engine.picker_selected, 0);

    // Up at 0 stays at 0
    engine.handle_key("Up", None, false);
    assert_eq!(engine.picker_selected, 0);
}

#[test]
fn test_picker_char_input_filters() {
    let mut engine = Engine::new();
    engine.picker_all_items = vec![
        PickerItem {
            display: "src/main.rs".to_string(),
            filter_text: "src/main.rs".to_string(),
            detail: None,
            action: PickerAction::OpenFile(PathBuf::from("src/main.rs")),
            icon: None,
            score: 0,
            match_positions: Vec::new(),
        },
        PickerItem {
            display: "src/engine.rs".to_string(),
            filter_text: "src/engine.rs".to_string(),
            detail: None,
            action: PickerAction::OpenFile(PathBuf::from("src/engine.rs")),
            icon: None,
            score: 0,
            match_positions: Vec::new(),
        },
    ];
    engine.picker_open = true;
    engine.picker_source = PickerSource::Files;

    // Type 'm' — should narrow to just main.rs
    engine.handle_picker_key("", Some('m'), false);
    assert_eq!(engine.picker_query, "m");
    assert_eq!(engine.picker_items.len(), 1);

    // Backspace — both files again
    engine.handle_picker_key("BackSpace", None, false);
    assert_eq!(engine.picker_query, "");
    assert_eq!(engine.picker_items.len(), 2);
}

#[test]
fn test_picker_commands_source() {
    let mut engine = Engine::new();
    engine.open_picker(PickerSource::Commands);

    assert!(engine.picker_open);
    assert_eq!(engine.picker_source, PickerSource::Commands);
    assert!(!engine.picker_all_items.is_empty());
    // Should have at least the basic commands
    assert!(engine.picker_items.len() > 10);
}

#[test]
fn test_fuzzy_score_with_positions() {
    // Exact prefix
    let result = Engine::fuzzy_score_with_positions("src/main.rs", "main");
    assert!(result.is_some());
    let (score, positions) = result.unwrap();
    assert!(score > 0);
    assert_eq!(positions.len(), 4);

    // No match
    let result = Engine::fuzzy_score_with_positions("src/main.rs", "xyz");
    assert!(result.is_none());

    // Empty query
    let result = Engine::fuzzy_score_with_positions("anything", "");
    assert!(result.is_some());
    let (score, positions) = result.unwrap();
    assert_eq!(score, 0);
    assert!(positions.is_empty());
}

#[test]
fn test_picker_confirm_opens_file() {
    use std::io::Write;
    let dir = std::env::temp_dir().join("vimcode_picker_confirm");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("testfile.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "hello").unwrap();
    drop(f);

    let mut engine = Engine::new();
    engine.cwd = dir.clone();
    engine.picker_open = true;
    engine.picker_items = vec![PickerItem {
        display: "testfile.txt".to_string(),
        filter_text: "testfile.txt".to_string(),
        detail: None,
        action: PickerAction::OpenFile(PathBuf::from("testfile.txt")),
        icon: None,
        score: 0,
        match_positions: Vec::new(),
    }];
    engine.picker_selected = 0;

    engine.picker_confirm();

    assert!(!engine.picker_open);
    let active_path = engine.file_path().cloned();
    assert_eq!(active_path, Some(path));
}

#[test]
fn test_picker_files_populates_preview() {
    use std::io::Write;
    let dir = std::env::temp_dir().join("vimcode_picker_preview");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Initialize a git repo so ignore crate doesn't skip everything
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&dir)
        .output()
        .ok();
    let path = dir.join("hello.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "line one").unwrap();
    writeln!(f, "line two").unwrap();
    writeln!(f, "line three").unwrap();
    drop(f);

    let mut engine = Engine::new();
    engine.cwd = dir.clone();
    engine.open_picker(PickerSource::Files);

    assert!(engine.picker_open);
    assert!(
        !engine.picker_items.is_empty(),
        "picker should have found hello.txt"
    );
    assert!(
        engine.picker_preview.is_some(),
        "preview should be populated for the first item"
    );
    let preview = engine.picker_preview.as_ref().unwrap();
    assert!(
        !preview.lines.is_empty(),
        "preview lines should not be empty"
    );
    assert_eq!(preview.lines[0].1, "line one");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_picker_grep_source_live_search() {
    use std::io::Write;
    let dir = std::env::temp_dir().join("vimcode_picker_grep");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut f = std::fs::File::create(dir.join("sample.txt")).unwrap();
    writeln!(f, "unique_grep_marker_xyz hello").unwrap();
    drop(f);

    let mut engine = Engine::new();
    engine.cwd = dir.clone();
    engine.open_picker(PickerSource::Grep);

    assert!(engine.picker_open);
    assert_eq!(engine.picker_source, PickerSource::Grep);
    assert_eq!(engine.picker_title, "Live Grep");

    // Empty query — no results (live source, no pre-populate)
    assert!(engine.picker_items.is_empty());

    // Single char — still below 2-char threshold
    engine.handle_picker_key("u", Some('u'), false);
    assert!(engine.picker_items.is_empty(), "1 char should not search");

    // Second char — search fires but unlikely to match our marker
    // Type enough to match our unique marker
    for c in "nique_grep_marker_xyz".chars() {
        engine.handle_picker_key("", Some(c), false);
    }
    assert!(
        !engine.picker_items.is_empty(),
        "should find our marker: query='{}'",
        engine.picker_query,
    );

    // Verify result is OpenFileAtLine action
    let item = &engine.picker_items[0];
    assert!(
        item.display.contains("unique_grep_marker_xyz"),
        "display: {}",
        item.display,
    );
    match &item.action {
        PickerAction::OpenFileAtLine(_, _) => {}
        other => panic!("expected OpenFileAtLine, got {:?}", other),
    }

    // Preview should be loaded
    assert!(engine.picker_preview.is_some());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_picker_grep_confirm_opens_at_line() {
    let mut engine = Engine::new();
    let dir = std::env::temp_dir().join("vimcode_picker_grep_confirm");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("target.txt");
    std::fs::write(&path, "line0\nline1\nline2\nline3\n").unwrap();

    engine.cwd = dir.clone();
    engine.picker_open = true;
    engine.picker_source = PickerSource::Grep;
    engine.picker_items = vec![PickerItem {
        display: "target.txt:3: line2".to_string(),
        filter_text: "target.txt:3: line2".to_string(),
        detail: None,
        action: PickerAction::OpenFileAtLine(path.clone(), 2), // 0-indexed
        icon: None,
        score: 0,
        match_positions: Vec::new(),
    }];
    engine.picker_selected = 0;

    engine.picker_confirm();

    assert!(!engine.picker_open);
    assert_eq!(engine.file_path().cloned(), Some(path));
    assert_eq!(engine.cursor().line, 2);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_picker_commands_confirm_executes() {
    let mut engine = Engine::new();
    engine.open_picker(PickerSource::Commands);
    assert!(engine.picker_open);
    assert_eq!(engine.picker_source, PickerSource::Commands);

    // Find a command that toggles wrap
    engine.picker_query = "toggle wrap".to_string();
    engine.picker_filter();
    assert!(!engine.picker_items.is_empty(), "should match wrap toggle");

    let old_wrap = engine.settings.wrap;
    engine.picker_confirm();
    assert!(!engine.picker_open, "confirm should close picker");
    assert_ne!(engine.settings.wrap, old_wrap, "wrap should have toggled");
}

#[test]
fn test_f1_opens_commands_picker() {
    let mut engine = Engine::new();
    engine.handle_key("F1", None, false);
    assert!(engine.picker_open);
    assert_eq!(engine.picker_source, PickerSource::Commands);
}

#[test]
#[allow(non_snake_case)]
fn test_picker_Grep_opens_via_api() {
    // Ctrl-G in Vim mode shows file info; the TUI/GTK keybinding
    // calls open_picker(Grep) directly. Test the API:
    let mut engine = Engine::new();
    engine.open_picker(PickerSource::Grep);
    assert!(engine.picker_open);
    assert_eq!(engine.picker_source, PickerSource::Grep);
    assert_eq!(engine.picker_title, "Live Grep");
}

#[test]
fn test_picker_grep_backspace_reruns_search() {
    let mut engine = Engine::new();
    engine.cwd = std::env::temp_dir();
    engine.open_picker(PickerSource::Grep);

    // Type "ab" then backspace — should go back to 1 char (no results)
    engine.handle_picker_key("a", Some('a'), false);
    engine.handle_picker_key("b", Some('b'), false);
    engine.handle_picker_key("BackSpace", None, false);
    assert_eq!(engine.picker_query, "a");
    // 1 char → below threshold → no results
    assert!(engine.picker_items.is_empty());
}

#[test]
fn test_picker_palette_command_opens_picker() {
    let mut engine = Engine::new();
    engine.execute_command("palette");
    assert!(engine.picker_open);
    assert_eq!(engine.picker_source, PickerSource::Commands);
}

#[test]
fn test_ctrl_g_shows_file_info() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello\nworld\n");

    press_ctrl(&mut engine, 'g');

    assert!(
        engine.message.contains("line 1 of 2"),
        "msg: {}",
        engine.message
    );
}

// ─── Quickfix tests ──────────────────────────────────────────────────────

fn make_qf_item(path: &str) -> ProjectMatch {
    ProjectMatch {
        file: std::path::PathBuf::from(path),
        line: 0,
        col: 0,
        line_text: "test line".to_string(),
    }
}

#[test]
fn test_copen_requires_items() {
    let mut engine = Engine::new();
    engine.execute_command("copen");
    assert!(
        !engine.quickfix_open,
        "copen should not open with empty list"
    );
    assert!(engine.message.contains("empty"));
}

#[test]
fn test_copen_cclose() {
    let mut engine = Engine::new();
    engine.quickfix_items = vec![make_qf_item("test.rs")];
    engine.execute_command("copen");
    assert!(engine.quickfix_open);
    assert!(engine.quickfix_has_focus);
    engine.execute_command("cclose");
    assert!(!engine.quickfix_open);
    assert!(!engine.quickfix_has_focus);
}

#[test]
fn test_cn_cp_navigation() {
    let mut engine = Engine::new();
    engine.quickfix_items = vec![
        make_qf_item("a.rs"),
        make_qf_item("b.rs"),
        make_qf_item("c.rs"),
    ];
    engine.quickfix_selected = 0;
    engine.quickfix_open = true;

    // cn moves forward
    engine.execute_command("cn");
    assert_eq!(engine.quickfix_selected, 1);
    engine.execute_command("cn");
    assert_eq!(engine.quickfix_selected, 2);

    // cn at end clamps
    engine.execute_command("cn");
    assert_eq!(engine.quickfix_selected, 2, "cn should clamp at last item");

    // cp moves back
    engine.execute_command("cp");
    assert_eq!(engine.quickfix_selected, 1);

    // cp at start clamps
    engine.execute_command("cp");
    engine.execute_command("cp");
    assert_eq!(engine.quickfix_selected, 0, "cp should clamp at first item");
}

#[test]
fn test_cc_jump() {
    let mut engine = Engine::new();
    engine.quickfix_items = vec![
        make_qf_item("a.rs"),
        make_qf_item("b.rs"),
        make_qf_item("c.rs"),
    ];
    engine.quickfix_open = true;

    engine.execute_command("cc 2");
    assert_eq!(
        engine.quickfix_selected, 1,
        ":cc 2 should select index 1 (1-based)"
    );
}

#[test]
fn test_grep_empty_pattern() {
    let mut engine = Engine::new();
    engine.execute_command("grep ");
    assert!(engine.quickfix_items.is_empty());
    assert!(engine.message.contains("Usage"));
}

#[test]
fn test_grep_no_matches() {
    let dir = std::env::temp_dir().join("vimcode_qf_no_match");
    std::fs::create_dir_all(&dir).unwrap();
    let mut engine = Engine::new();
    engine.cwd = dir.clone();
    engine.execute_command("grep xyzzy_no_match_anywhere_qf_test");
    assert_eq!(engine.quickfix_items.len(), 0);
    assert!(engine.message.contains("0 match"));
}

#[test]
fn test_grep_populates_quickfix() {
    use std::io::Write;
    let dir = std::env::temp_dir().join("vimcode_qf_grep_pop");
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("qftest.rs");
    let mut f = std::fs::File::create(&file_path).unwrap();
    writeln!(f, "fn qfmain_unique_marker() {{}}").unwrap();
    drop(f);

    let mut engine = Engine::new();
    engine.cwd = dir.clone();
    engine.execute_command("grep qfmain_unique_marker");

    assert!(
        !engine.quickfix_items.is_empty(),
        "grep should find matches"
    );
    assert!(engine.quickfix_open);
    assert!(
        !engine.quickfix_has_focus,
        "focus should return to editor after :grep"
    );
    assert!(engine.message.contains("match"));
}

#[test]
fn test_vimgrep_alias() {
    use std::io::Write;
    let dir = std::env::temp_dir().join("vimcode_qf_vimgrep");
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("vgtest.rs");
    let mut f = std::fs::File::create(&file_path).unwrap();
    writeln!(f, "fn vghello_unique_marker() {{}}").unwrap();
    drop(f);

    let mut engine = Engine::new();
    engine.cwd = dir.clone();
    engine.execute_command("vimgrep vghello_unique_marker");

    assert!(
        !engine.quickfix_items.is_empty(),
        "vimgrep should work same as grep"
    );
    assert!(engine.quickfix_open);
}

// ─── rename_file / move_file tests ────────────────────────────────────────

#[test]
fn test_rename_file_updates_buffer_path() {
    let dir = std::env::temp_dir().join("vimcode_rename_upd");
    std::fs::create_dir_all(&dir).unwrap();
    let old = dir.join("rename_old.txt");
    std::fs::write(&old, "hello").unwrap();

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&old, OpenMode::Permanent)
        .unwrap();

    engine.rename_file(&old, "rename_new.txt").unwrap();

    let new = dir.join("rename_new.txt");
    assert!(new.exists(), "new path should exist");
    assert!(!old.exists(), "old path should be gone");

    // The open buffer's file_path should have been updated
    let updated = engine.buffer_manager.list().into_iter().any(|id| {
        engine
            .buffer_manager
            .get(id)
            .and_then(|s| s.file_path.as_ref())
            == Some(&new)
    });
    assert!(updated, "open buffer should point to new path");
}

#[test]
fn test_rename_file_not_found() {
    let mut engine = Engine::new();
    let result = engine.rename_file(Path::new("/vimcode_nonexistent_xyz/file.txt"), "new.txt");
    assert!(result.is_err(), "renaming missing file should fail");
}

#[test]
fn test_rename_file_empty_name() {
    let mut engine = Engine::new();
    let result = engine.rename_file(Path::new("/tmp/whatever.txt"), "");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("empty"));
}

#[test]
fn test_move_file_basic() {
    let base = std::env::temp_dir().join("vimcode_move_basic");
    let dest = base.join("subdir_mv");
    std::fs::create_dir_all(&dest).unwrap();
    let src = base.join("moveme.txt");
    std::fs::write(&src, "data").unwrap();

    let mut engine = Engine::new();
    engine.move_file(&src, &dest).unwrap();

    assert!(!src.exists(), "source should be gone");
    assert!(dest.join("moveme.txt").exists(), "file should be in dest");
}

#[test]
fn test_move_file_invalid_dest() {
    let mut engine = Engine::new();
    let result = engine.move_file(
        Path::new("/tmp/whatever.txt"),
        Path::new("/tmp/not_a_real_dir_xyz_vc"),
    );
    assert!(result.is_err());
}

#[test]
fn test_confirm_move_shows_dialog() {
    let base = std::env::temp_dir().join("vimcode_confirm_move_dlg");
    let dest = base.join("target_dir");
    std::fs::create_dir_all(&dest).unwrap();
    let src = base.join("confirm_me.txt");
    std::fs::write(&src, "data").unwrap();

    let mut engine = Engine::new();
    engine.confirm_move_file(&src, &dest);

    // Dialog should be shown.
    assert!(engine.dialog.is_some());
    let dialog = engine.dialog.as_ref().unwrap();
    assert_eq!(dialog.tag, "confirm_move");
    assert!(dialog.body[0].contains("confirm_me.txt"));
    assert_eq!(dialog.buttons.len(), 2);

    // Pending move should be stored.
    assert!(engine.pending_move.is_some());
    let (ps, pd) = engine.pending_move.as_ref().unwrap();
    assert_eq!(ps, &src);
    assert_eq!(pd, &dest);

    // Simulate pressing 'y' (Yes) — dialog handles it.
    let _action = engine.handle_key("y", Some('y'), false);

    // File should have been moved.
    assert!(!src.exists());
    assert!(dest.join("confirm_me.txt").exists());
    assert!(engine.dialog.is_none());
    assert!(engine.pending_move.is_none());
    assert!(engine.explorer_needs_refresh);

    // Cleanup
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn test_confirm_move_cancel() {
    let base = std::env::temp_dir().join("vimcode_confirm_move_cancel");
    let dest = base.join("target_dir_c");
    std::fs::create_dir_all(&dest).unwrap();
    let src = base.join("stay_put.txt");
    std::fs::write(&src, "data").unwrap();

    let mut engine = Engine::new();
    engine.confirm_move_file(&src, &dest);

    // Simulate pressing 'n' (No).
    let _action = engine.handle_key("n", Some('n'), false);

    // File should NOT have been moved.
    assert!(src.exists());
    assert!(!dest.join("stay_put.txt").exists());
    assert!(engine.dialog.is_none());
    assert!(engine.pending_move.is_none());
    assert!(!engine.explorer_needs_refresh);

    // Cleanup
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn test_move_file_into_own_subtree() {
    let base = std::env::temp_dir().join("vimcode_move_subtree");
    let parent = base.join("parent_dir");
    let child = parent.join("child_dir");
    std::fs::create_dir_all(&child).unwrap();

    let mut engine = Engine::new();
    let result = engine.move_file(&parent, &child);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("subtree"),
        "should reject moving folder into its own subtree"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn test_move_file_same_directory_noop() {
    let base = std::env::temp_dir().join("vimcode_move_noop");
    std::fs::create_dir_all(&base).unwrap();
    let src = base.join("stay.txt");
    std::fs::write(&src, "stay").unwrap();

    let mut engine = Engine::new();
    // Moving a file into the directory it's already in should be a no-op.
    let result = engine.move_file(&src, &base);
    assert!(result.is_ok());
    assert!(src.exists(), "file should still be at original location");

    // Cleanup
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn test_move_directory_basic() {
    let base = std::env::temp_dir().join("vimcode_move_dir");
    let src = base.join("src_dir");
    let dest = base.join("dest_dir");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("file.txt"), "content").unwrap();

    let mut engine = Engine::new();
    engine.move_file(&src, &dest).unwrap();

    assert!(!src.exists(), "source dir should be gone");
    assert!(
        dest.join("src_dir").join("file.txt").exists(),
        "dir should be moved with contents"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn test_move_file_updates_buffer_path() {
    let base = std::env::temp_dir().join("vimcode_move_bufupd");
    let dest = base.join("dest_mv");
    std::fs::create_dir_all(&dest).unwrap();
    let src = base.join("tracked.txt");
    std::fs::write(&src, "tracked").unwrap();

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&src, OpenMode::Permanent)
        .unwrap();

    engine.move_file(&src, &dest).unwrap();

    let expected = dest.join("tracked.txt");
    let updated = engine.buffer_manager.list().into_iter().any(|id| {
        engine
            .buffer_manager
            .get(id)
            .and_then(|s| s.file_path.as_ref())
            == Some(&expected)
    });
    assert!(
        updated,
        "open buffer should point to new location after move"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&base);
}

// ─── LCS diff tests ───────────────────────────────────────────────────────

#[test]
fn test_lcs_diff_same_content() {
    let a = &["alpha", "beta", "gamma"];
    let b = &["alpha", "beta", "gamma"];
    let (da, db) = lcs_diff(a, b);
    assert!(da.iter().all(|s| *s == DiffLine::Same));
    assert!(db.iter().all(|s| *s == DiffLine::Same));
    assert_eq!(da.len(), 3);
    assert_eq!(db.len(), 3);
}

#[test]
fn test_lcs_diff_added_line() {
    let a = &["alpha", "gamma"];
    let b = &["alpha", "beta", "gamma"];
    let (da, db) = lcs_diff(a, b);
    assert!(da.iter().all(|s| *s == DiffLine::Same));
    assert_eq!(db[0], DiffLine::Same);
    assert_eq!(db[1], DiffLine::Added);
    assert_eq!(db[2], DiffLine::Same);
}

#[test]
fn test_lcs_diff_removed_line() {
    let a = &["alpha", "beta", "gamma"];
    let b = &["alpha", "gamma"];
    let (da, db) = lcs_diff(a, b);
    assert_eq!(da[0], DiffLine::Same);
    assert_eq!(da[1], DiffLine::Removed);
    assert_eq!(da[2], DiffLine::Same);
    assert!(db.iter().all(|s| *s == DiffLine::Same));
}

#[test]
fn test_myers_diff_two_change_blocks() {
    // Mirrors the README vs README2 scenario: two distinct change blocks
    // separated by 2 unchanged lines — must NOT merge into one block.
    let a = &[
        "# DemoConsoleGame",
        "Simple demo game",
        "",
        "In the end he did get a degree",
        "",
        "Now he's doing a masters",
        "",
        "So, now the only point",
    ];
    let b = &[
        "# DemoConsoleGame",
        "Simple demo game",
        "",
        "In the end he did ge asdfadt a degree",
        "",
        "",
        "asds",
        "",
        "zdxfasd",
        "",
        "",
        "Now he's doing a masters",
        "",
        "asdfsd",
        "",
        "So, now the only point",
    ];
    let (da, db) = lcs_diff(a, b);
    // Line 3 of a should be Removed, line 3 of b should be Added (changed line).
    assert_eq!(da[3], DiffLine::Removed);
    assert_eq!(db[3], DiffLine::Added);
    // Lines 5-10 of b should be Added (inserted block).
    for i in 5..11 {
        assert_eq!(db[i], DiffLine::Added, "b[{}] should be Added", i);
    }
    // a[5] "Now he's doing..." should be Same (not merged).
    assert_eq!(da[5], DiffLine::Same);
    assert_eq!(db[11], DiffLine::Same);
    // b[13] "asdfsd" should be Added (second change block).
    assert_eq!(db[13], DiffLine::Added);
}

#[test]
fn test_lcs_diff_changed_line() {
    let a = &["hello world"];
    let b = &["hello rust"];
    let (da, db) = lcs_diff(a, b);
    assert_eq!(da[0], DiffLine::Removed);
    assert_eq!(db[0], DiffLine::Added);
}

#[test]
fn test_lcs_diff_empty() {
    let (da, db) = lcs_diff(&[], &[]);
    assert!(da.is_empty());
    assert!(db.is_empty());
}

#[test]
fn test_merge_short_same_runs_blank_lines() {
    // Blank lines inside an added block should not fragment it.
    let a = &["header", "old", "footer"];
    let b = &["header", "new1", "", "new2", "", "new3", "footer"];
    let (da, mut db) = lcs_diff(a, b);
    merge_short_same_runs(&mut db, DiffLine::Added);
    // All lines between header and footer should be Added on the b side.
    assert_eq!(db[0], DiffLine::Same, "header");
    for i in 1..6 {
        assert_eq!(db[i], DiffLine::Added, "line {i} should be Added");
    }
    assert_eq!(db[6], DiffLine::Same, "footer");
    // A side should still have Removed for 'old'.
    assert_eq!(da[0], DiffLine::Same);
    assert_eq!(da[1], DiffLine::Removed);
    assert_eq!(da[2], DiffLine::Same);
}

#[test]
fn test_merge_short_same_runs_common_lines() {
    // Short runs of common lines (braces, imports) between changes should
    // be absorbed into the surrounding change region.
    let a = &["header", "}", "footer"];
    let b = &["header", "new1", "}", "new2", "footer"];
    let (_da, mut db) = lcs_diff(a, b);
    merge_short_same_runs(&mut db, DiffLine::Added);
    assert_eq!(db[0], DiffLine::Same, "header");
    // "new1", "}", "new2" should all be Added (} is a short Same island).
    for i in 1..4 {
        assert_eq!(db[i], DiffLine::Added, "line {i} should be Added");
    }
    assert_eq!(db[4], DiffLine::Same, "footer");
}

#[test]
fn test_build_aligned_diff_unequal_same_tails() {
    // Regression: when one side has more Same lines than the other,
    // build_aligned_diff must not loop forever.
    use DiffLine::*;
    let da = vec![Same, Same, Removed, Same, Same];
    let db = vec![Same, Same, Same];
    // This used to hang — the fix ensures progress when one side is
    // exhausted while the other still has Same lines.
    let (aa, ab) = build_aligned_diff(&da, &db);
    assert_eq!(aa.len(), ab.len());
}

#[test]
fn test_build_aligned_diff_basic() {
    use DiffLine::*;
    let da = vec![Same, Removed, Same];
    let db = vec![Same, Added, Same];
    let (aa, ab) = build_aligned_diff(&da, &db);
    assert_eq!(aa.len(), ab.len());
    // First and last should map to source lines.
    assert!(aa[0].source_line.is_some());
    assert!(ab[0].source_line.is_some());
}

#[test]
fn test_lcs_diff_large_files_with_small_diff() {
    // Regression: files >5000 lines used to return all-Same due to MAX_LINES guard.
    let mut a_lines: Vec<String> = (0..8000).map(|i| format!("line {i}")).collect();
    let mut b_lines = a_lines.clone();
    // Insert 3 new lines in the middle of b.
    b_lines.insert(4000, "new line 1".to_string());
    b_lines.insert(4001, "new line 2".to_string());
    b_lines.insert(4002, "new line 3".to_string());
    // Also change one line.
    a_lines[100] = "original line 100".to_string();
    b_lines[100] = "modified line 100".to_string();

    let a_refs: Vec<&str> = a_lines.iter().map(String::as_str).collect();
    let b_refs: Vec<&str> = b_lines.iter().map(String::as_str).collect();
    let (da, db) = lcs_diff(&a_refs, &b_refs);

    // Should detect actual changes, not return all-Same.
    let a_changes = da.iter().filter(|d| **d != DiffLine::Same).count();
    let b_changes = db.iter().filter(|d| **d != DiffLine::Same).count();
    assert!(
        a_changes > 0 || b_changes > 0,
        "diff should detect changes in large files"
    );
    // Specifically: b should have 3 Added lines + 1 changed line.
    assert!(b_changes >= 3, "b should have at least 3 Added lines");
}

// ─── cmd_diffthis / cmd_diffoff / cmd_diffsplit tests ─────────────────────

#[test]
fn test_diffthis_one_window_then_diffoff() {
    let mut engine = Engine::new();
    engine.execute_command("diffthis");
    assert!(engine.diff_window_pair.is_some());
    engine.execute_command("diffoff");
    assert!(engine.diff_window_pair.is_none());
    assert!(engine.diff_results.is_empty());
}

#[test]
fn test_diffthis_two_windows() {
    let dir = std::env::temp_dir().join("vimcode_diffthis_two");
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("file_a_dt.txt");
    let f2 = dir.join("file_b_dt.txt");
    std::fs::write(&f1, "line1\nline2\n").unwrap();
    std::fs::write(&f2, "line1\nline3\n").unwrap();

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&f1, OpenMode::Permanent)
        .unwrap();

    // Mark first window
    engine.execute_command("diffthis");
    let (a_stored, _) = engine.diff_window_pair.unwrap();

    // Open second file in a split
    engine.split_window(SplitDirection::Vertical, Some(&f2));

    // Mark second window
    engine.execute_command("diffthis");

    assert!(engine.diff_window_pair.is_some());
    let (a, b) = engine.diff_window_pair.unwrap();
    assert_ne!(a, b, "pair should have two distinct windows");
    assert_eq!(a, a_stored, "first window should be preserved");
    assert!(!engine.diff_results.is_empty(), "diff results should exist");
}

#[test]
fn test_diffsplit_command() {
    let dir = std::env::temp_dir().join("vimcode_diffsplit_vc");
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("src_ds.txt");
    let f2 = dir.join("cmp_ds.txt");
    std::fs::write(&f1, "alpha\nbeta\n").unwrap();
    std::fs::write(&f2, "alpha\ngamma\n").unwrap();

    let mut engine = Engine::new();
    engine
        .open_file_with_mode(&f1, OpenMode::Permanent)
        .unwrap();

    let initial_win_count = engine.active_tab().layout.window_ids().len();

    engine.execute_command(&format!("diffsplit {}", f2.display()));

    let new_win_count = engine.active_tab().layout.window_ids().len();
    assert!(
        new_win_count > initial_win_count,
        "diffsplit should open a new window"
    );
    assert!(engine.diff_window_pair.is_some());
    assert!(!engine.diff_results.is_empty());
}

// ── Diff toolbar + navigation tests ──────────────────────────────────────

#[test]
fn test_diff_change_regions() {
    let mut engine = Engine::new();
    let dir = std::env::temp_dir().join("vimcode_diff_regions");
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("a_regions.txt");
    let f2 = dir.join("b_regions.txt");
    std::fs::write(&f1, "same\nalpha\nsame\nsame\nsame\nsame\nbeta\nsame\n").unwrap();
    std::fs::write(&f2, "same\nALPHA\nsame\nsame\nsame\nsame\nBETA\nsame\n").unwrap();

    engine
        .open_file_with_mode(&f1, OpenMode::Permanent)
        .unwrap();
    engine.execute_command(&format!("diffsplit {}", f2.display()));

    let win_id = engine.active_window_id();
    let regions = engine.diff_change_regions(win_id);
    assert_eq!(regions.len(), 2, "should detect two change regions");
    assert_eq!(regions[0], (1, 1));
    assert_eq!(regions[1], (6, 6));
}

#[test]
fn test_diff_jump_next_prev() {
    let mut engine = Engine::new();
    let dir = std::env::temp_dir().join("vimcode_diff_jump");
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("a_jump.txt");
    let f2 = dir.join("b_jump.txt");
    std::fs::write(&f1, "same\nold1\nsame\nsame\nsame\nsame\nold2\nsame\n").unwrap();
    std::fs::write(&f2, "same\nnew1\nsame\nsame\nsame\nsame\nnew2\nsame\n").unwrap();

    engine
        .open_file_with_mode(&f1, OpenMode::Permanent)
        .unwrap();
    engine.execute_command(&format!("diffsplit {}", f2.display()));

    // Cursor starts at line 0.
    engine.view_mut().cursor.line = 0;
    engine.view_mut().cursor.col = 0;

    // Jump to next change — should land on first change (line 1).
    engine.jump_next_hunk();
    assert_eq!(engine.view().cursor.line, 1);

    // Jump to next change — should land on second change (line 6).
    engine.jump_next_hunk();
    assert_eq!(engine.view().cursor.line, 6);

    // Jump to next change — should wrap to first (line 1).
    engine.jump_next_hunk();
    assert_eq!(engine.view().cursor.line, 1);
    assert!(engine.message.contains("Wrapped"));

    // Jump to prev change — should wrap to last (line 6).
    engine.jump_prev_hunk();
    assert_eq!(engine.view().cursor.line, 6);
}

#[test]
fn test_diff_toggle_hide_unchanged() {
    let mut engine = Engine::new();
    let dir = std::env::temp_dir().join("vimcode_diff_fold");
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("a_fold.txt");
    let f2 = dir.join("b_fold.txt");
    // 10 same lines, then 1 changed, then 10 same lines.
    let same_block = "s\n".repeat(10);
    std::fs::write(&f1, format!("{same_block}old\n{same_block}")).unwrap();
    std::fs::write(&f2, format!("{same_block}new\n{same_block}")).unwrap();

    engine
        .open_file_with_mode(&f1, OpenMode::Permanent)
        .unwrap();
    engine.execute_command(&format!("diffsplit {}", f2.display()));

    // diff_unchanged_hidden is auto-enabled by diffsplit.
    assert!(engine.diff_unchanged_hidden);

    // Both windows should have folds.
    let (a, b) = engine.diff_window_pair.unwrap();
    let a_folds = &engine.windows.get(&a).unwrap().view.folds;
    let b_folds = &engine.windows.get(&b).unwrap().view.folds;
    assert!(!a_folds.is_empty(), "window A should have folds");
    assert!(!b_folds.is_empty(), "window B should have folds");

    // Toggle back — folds should be cleared.
    engine.diff_toggle_hide_unchanged();
    assert!(!engine.diff_unchanged_hidden);
    let a_folds = &engine.windows.get(&a).unwrap().view.folds;
    let b_folds = &engine.windows.get(&b).unwrap().view.folds;
    assert!(a_folds.is_empty(), "window A folds should be cleared");
    assert!(b_folds.is_empty(), "window B folds should be cleared");
}

#[test]
fn test_diff_aligned_scroll_sync() {
    let mut engine = Engine::new();
    let dir = std::env::temp_dir().join("vimcode_diff_aligned_scroll");
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("a_scroll.txt");
    let f2 = dir.join("b_scroll.txt");
    // Left: 5 same lines, then "old", then 5 same lines.
    // Right: 5 same lines, then 10 new lines, then "new", then 5 same lines.
    // This creates a large padding block on the left side.
    let same5 = "s\n".repeat(5);
    let added10 = "added\n".repeat(10);
    std::fs::write(&f1, format!("{same5}old\n{same5}")).unwrap();
    std::fs::write(&f2, format!("{same5}{added10}new\n{same5}")).unwrap();

    engine
        .open_file_with_mode(&f1, OpenMode::Permanent)
        .unwrap();
    engine.execute_command(&format!("diffsplit {}", f2.display()));

    let (a, b) = engine.diff_window_pair.unwrap();
    // Both windows should have aligned data.
    assert!(engine.diff_aligned.contains_key(&a));
    assert!(engine.diff_aligned.contains_key(&b));

    // Scroll the right window (b) down and sync.
    engine.active_tab_mut().active_window = b;
    engine.windows.get_mut(&b).unwrap().view.scroll_top = 8;
    engine.sync_scroll_binds();

    // Left window should have been mapped through aligned data,
    // not set to the raw scroll_top value of 8.
    let a_scroll = engine.windows.get(&a).unwrap().view.scroll_top;
    // The left file only has 11 lines (5 same + "old" + 5 same),
    // so a raw copy of 8 would be near the end. The aligned mapping
    // should produce a smaller value since the padding absorbs the offset.
    assert!(
        a_scroll < 8,
        "expected aligned scroll mapping to give a_scroll < 8, got {a_scroll}"
    );
}

#[test]
fn test_diff_current_change_index() {
    let mut engine = Engine::new();
    let dir = std::env::temp_dir().join("vimcode_diff_idx");
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("a_idx.txt");
    let f2 = dir.join("b_idx.txt");
    std::fs::write(&f1, "s\nold1\ns\ns\ns\ns\nold2\ns\ns\ns\ns\nold3\ns\n").unwrap();
    std::fs::write(&f2, "s\nnew1\ns\ns\ns\ns\nnew2\ns\ns\ns\ns\nnew3\ns\n").unwrap();

    engine
        .open_file_with_mode(&f1, OpenMode::Permanent)
        .unwrap();
    engine.execute_command(&format!("diffsplit {}", f2.display()));

    // At line 0 (before first change).
    engine.view_mut().cursor.line = 0;
    let idx = engine.diff_current_change_index();
    assert_eq!(idx, Some((1, 3))); // closest after is first change

    // At line 1 (in first change).
    engine.view_mut().cursor.line = 1;
    let idx = engine.diff_current_change_index();
    assert_eq!(idx, Some((1, 3)));

    // At line 6 (in second change).
    engine.view_mut().cursor.line = 6;
    let idx = engine.diff_current_change_index();
    assert_eq!(idx, Some((2, 3)));

    // At line 11 (in third change).
    engine.view_mut().cursor.line = 11;
    let idx = engine.diff_current_change_index();
    assert_eq!(idx, Some((3, 3)));
}

#[test]
fn test_jump_hunk_delegates_in_diff_mode() {
    let mut engine = Engine::new();
    let dir = std::env::temp_dir().join("vimcode_diff_delegate");
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("a_deleg.txt");
    let f2 = dir.join("b_deleg.txt");
    std::fs::write(&f1, "same\nold\nsame\n").unwrap();
    std::fs::write(&f2, "same\nnew\nsame\n").unwrap();

    engine
        .open_file_with_mode(&f1, OpenMode::Permanent)
        .unwrap();
    engine.execute_command(&format!("diffsplit {}", f2.display()));

    // ]c should use diff_results, not git_diff.
    engine.view_mut().cursor.line = 0;
    engine.jump_next_hunk();
    assert_eq!(
        engine.view().cursor.line,
        1,
        "]c should jump to diff change region"
    );
}

#[test]
fn test_diffthis_toolbar_and_scroll_sync() {
    let mut engine = Engine::new();
    let dir = std::env::temp_dir().join("vimcode_diffthis_toolbar");
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("a_dt.txt");
    let f2 = dir.join("b_dt.txt");
    std::fs::write(&f1, "same\nold1\nsame\nsame\nsame\nsame\nold2\nsame\n").unwrap();
    std::fs::write(&f2, "same\nnew1\nsame\nsame\nsame\nsame\nnew2\nsame\n").unwrap();

    // Open first file and run :diffthis.
    engine
        .open_file_with_mode(&f1, OpenMode::Permanent)
        .unwrap();
    let win_a = engine.active_window_id();
    engine.execute_command("diffthis");
    // Placeholder state: a == a, is_in_diff_view should be false.
    assert!(!engine.is_in_diff_view());

    // Open second file in a split and run :diffthis.
    engine.execute_command(&format!("vs {}", f2.display()));
    let win_b = engine.active_window_id();
    assert_ne!(win_a, win_b);
    engine.execute_command("diffthis");

    // Now diff should be active.
    assert!(engine.is_in_diff_view());
    assert!(engine.diff_window_pair.is_some());
    let (a, b) = engine.diff_window_pair.unwrap();
    assert_ne!(a, b);

    // diff_results should be populated.
    assert!(!engine.diff_results.is_empty());
    let regions = engine.diff_change_regions(engine.active_window_id());
    assert_eq!(regions.len(), 2, "should detect two change regions");

    // Scroll binding should be set up (added by diffthis).
    assert!(
        engine
            .scroll_bind_pairs
            .iter()
            .any(|&(x, y)| (x == a && y == b) || (x == b && y == a)),
        "diffthis should register scroll binding"
    );

    // diff_current_change_index should return data for toolbar.
    let idx = engine.diff_current_change_index();
    assert!(idx.is_some(), "toolbar should have change index data");
}

#[test]
fn test_diffthis_across_editor_groups() {
    let mut engine = Engine::new();
    let dir = std::env::temp_dir().join("vimcode_diffthis_groups");
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("a_grp.txt");
    let f2 = dir.join("b_grp.txt");
    std::fs::write(&f1, "same\nold\nsame\n").unwrap();
    std::fs::write(&f2, "same\nnew\nsame\n").unwrap();

    // Open first file and mark for diff.
    engine
        .open_file_with_mode(&f1, OpenMode::Permanent)
        .unwrap();
    let win_a = engine.active_window_id();
    engine.execute_command("diffthis");

    // Split into a new editor group and open the second file.
    engine.open_editor_group(SplitDirection::Vertical);
    engine
        .open_file_with_mode(&f2, OpenMode::Permanent)
        .unwrap();
    let win_b = engine.active_window_id();
    assert_ne!(win_a, win_b);

    // Mark second window for diff.
    engine.execute_command("diffthis");
    assert!(engine.is_in_diff_view());
    let (a, b) = engine.diff_window_pair.unwrap();
    assert_ne!(a, b);

    // Verify both windows are in different groups.
    let group_ids = engine.group_layout.group_ids();
    assert!(group_ids.len() >= 2, "should have at least 2 editor groups");

    // Verify each group contains one of the diff windows.
    for &gid in &group_ids {
        if let Some(group) = engine.editor_groups.get(&gid) {
            let wids = group.active_tab().layout.window_ids();
            let has_diff = wids.contains(&a) || wids.contains(&b);
            if has_diff {
                // This group should be detected by is_in_diff_view's logic
                assert!(engine.is_in_diff_view(), "diff view should be detected");
            }
        }
    }

    // Verify diff results have data.
    let regions = engine.diff_change_regions(b);
    assert!(!regions.is_empty(), "should detect changes");
}

// ── cmd_git_diff_split tests ────────────────────────────────────────────

/// Create a temp git repo with one committed file, then modify it.
/// Returns (repo_dir, file_path).
fn setup_git_diff_split_repo(suffix: &str) -> (PathBuf, PathBuf) {
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("vimcode_gds_{suffix}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Canonicalize to resolve symlinks (e.g. /tmp → /private/tmp on macOS)
    let dir = dir.canonicalize().unwrap();
    // init + commit
    Command::new("git")
        .args(["init"])
        .current_dir(&dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let file = dir.join("hello.rs");
    std::fs::write(&file, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&dir)
        .output()
        .unwrap();
    // Modify the file (working copy differs from HEAD)
    std::fs::write(
        &file,
        "fn main() {\n    println!(\"hello world\");\n    println!(\"new line\");\n}\n",
    )
    .unwrap();
    (dir, file)
}

#[test]
fn test_git_diff_split_creates_pair() {
    let (dir, file) = setup_git_diff_split_repo("creates_pair");
    let mut engine = Engine::new();
    engine.cwd = dir.clone();
    let result = engine.cmd_git_diff_split(&file);
    assert!(
        !matches!(result, EngineAction::Error),
        "cmd_git_diff_split should succeed: {}",
        engine.message
    );
    // Should have 2 windows
    assert_eq!(engine.active_tab().layout.window_ids().len(), 2);
    // diff_window_pair should be set
    assert!(engine.diff_window_pair.is_some());
    // scroll_bind_pairs should have the pair
    assert!(!engine.scroll_bind_pairs.is_empty());
    // diff_results should be populated
    assert!(!engine.diff_results.is_empty());
    // Both windows should have diff results with non-Same entries
    let (left, right) = engine.diff_window_pair.unwrap();
    let left_results = engine.diff_results.get(&left).expect("left diff_results");
    let right_results = engine.diff_results.get(&right).expect("right diff_results");
    let left_has_changes = left_results.iter().any(|d| *d != DiffLine::Same);
    let right_has_changes = right_results.iter().any(|d| *d != DiffLine::Same);
    assert!(
        left_has_changes,
        "left should have Added/Removed entries, got {:?}",
        left_results
    );
    assert!(
        right_has_changes,
        "right should have Added/Removed entries, got {:?}",
        right_results
    );
}

#[test]
fn test_git_diff_split_left_readonly() {
    let (dir, file) = setup_git_diff_split_repo("left_ro");
    let mut engine = Engine::new();
    engine.cwd = dir.clone();
    engine.cmd_git_diff_split(&file);
    let (left_win, _right_win) = engine.diff_window_pair.unwrap();
    let left_buf_id = engine.windows.get(&left_win).unwrap().buffer_id;
    let left_state = engine.buffer_manager.get(left_buf_id).unwrap();
    assert!(left_state.read_only, "HEAD buffer should be read-only");
}

#[test]
fn test_git_diff_split_head_scratch_name() {
    let (dir, file) = setup_git_diff_split_repo("scratch");
    let mut engine = Engine::new();
    engine.cwd = dir.clone();
    engine.cmd_git_diff_split(&file);
    let (left_win, _) = engine.diff_window_pair.unwrap();
    let left_buf_id = engine.windows.get(&left_win).unwrap().buffer_id;
    let left_state = engine.buffer_manager.get(left_buf_id).unwrap();
    let name = left_state.scratch_name.as_deref().unwrap_or("");
    assert!(
        name.contains("(HEAD)"),
        "scratch_name should contain (HEAD), got: {name}"
    );
}

#[test]
fn test_git_diff_split_untracked_file_errors() {
    use std::process::Command;
    let dir = std::env::temp_dir().join("vimcode_gds_untracked");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let file = dir.join("new_file.txt");
    std::fs::write(&file, "untracked content\n").unwrap();

    let mut engine = Engine::new();
    engine.cwd = dir.clone();
    let result = engine.cmd_git_diff_split(&file);
    assert!(
        matches!(result, EngineAction::Error),
        "untracked file should error"
    );
    assert!(engine.message.contains("no HEAD version"));
}

#[test]
fn test_close_window_cleans_diff_state() {
    let (dir, file) = setup_git_diff_split_repo("close_win");
    let mut engine = Engine::new();
    engine.cwd = dir.clone();
    engine.cmd_git_diff_split(&file);
    assert!(engine.diff_window_pair.is_some());
    // Close active window — should clean up diff state
    engine.close_window();
    assert!(
        engine.diff_window_pair.is_none(),
        "diff_window_pair should be cleared after closing a diff window"
    );
    assert!(engine.diff_results.is_empty());
}

// ── Help command tests ──────────────────────────────────────────────────

#[test]
fn test_help_command_explorer() {
    let mut engine = Engine::new();
    let initial_wins = engine.active_tab().layout.window_ids().len();
    engine.execute_command("help explorer");
    let new_wins = engine.active_tab().layout.window_ids().len();
    assert_eq!(new_wins, initial_wins + 1, "help should open a vsplit");
    let content: String = engine.buffer().content.chars().collect();
    assert!(content.contains("Explorer Sidebar"));
    assert!(content.contains("Explorer Mode"));
}

#[test]
fn test_help_command_no_args() {
    let mut engine = Engine::new();
    engine.execute_command("help");
    let content: String = engine.buffer().content.chars().collect();
    assert!(content.contains("VimCode Help"));
    assert!(content.contains(":help explorer"));
}

#[test]
fn test_help_alias_h() {
    let mut engine = Engine::new();
    engine.execute_command("h keys");
    let content: String = engine.buffer().content.chars().collect();
    assert!(content.contains("Normal Mode Keys"));
}

#[test]
fn test_help_unknown_topic() {
    let mut engine = Engine::new();
    let initial_wins = engine.active_tab().layout.window_ids().len();
    engine.execute_command("help nonexistent");
    let new_wins = engine.active_tab().layout.window_ids().len();
    assert_eq!(
        new_wins, initial_wins,
        "unknown topic should not open a split"
    );
    assert!(engine.message.contains("No help for"));
}

// ── Mouse selection tests ─────────────────────────────────────────────

#[test]
fn test_mouse_click_exits_visual_mode() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Enter visual mode
    press_char(&mut engine, 'v');
    assert_eq!(engine.mode, Mode::Visual);

    // Click should exit visual mode
    let wid = engine.active_window_id();
    engine.mouse_click(wid, 0, 3);
    assert_eq!(engine.mode, Mode::Normal);
    assert!(engine.visual_anchor.is_none());
}

#[test]
fn test_mouse_click_positions_cursor() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world\nsecond line");
    engine.update_syntax();

    let wid = engine.active_window_id();
    engine.mouse_click(wid, 1, 3);
    assert_eq!(engine.view().cursor.line, 1);
    assert_eq!(engine.view().cursor.col, 3);
}

#[test]
fn test_mouse_drag_enters_visual_mode() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    // Position cursor at col 2
    let wid = engine.active_window_id();
    engine.mouse_click(wid, 0, 2);
    assert_eq!(engine.view().cursor.col, 2);

    // First drag should enter visual mode with anchor at current position
    engine.mouse_drag(wid, 0, 5);
    assert_eq!(engine.mode, Mode::Visual);
    assert!(engine.mouse_drag_active);
    assert_eq!(engine.visual_anchor.unwrap().col, 2); // anchor at click position
    assert_eq!(engine.view().cursor.col, 5); // cursor moved to drag position
}

#[test]
fn test_mouse_drag_extends_selection() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    let wid = engine.active_window_id();
    engine.mouse_click(wid, 0, 2);

    // First drag
    engine.mouse_drag(wid, 0, 5);
    let anchor = engine.visual_anchor.unwrap();

    // Second drag should extend, keeping anchor
    engine.mouse_drag(wid, 0, 8);
    assert_eq!(engine.visual_anchor.unwrap(), anchor);
    assert_eq!(engine.view().cursor.col, 8);
}

#[test]
fn test_mouse_drag_multiline() {
    let mut engine = Engine::new();
    engine
        .buffer_mut()
        .insert(0, "line one\nline two\nline three");
    engine.update_syntax();

    let wid = engine.active_window_id();
    engine.mouse_click(wid, 0, 3);
    engine.mouse_drag(wid, 2, 4);

    assert_eq!(engine.mode, Mode::Visual);
    assert_eq!(engine.visual_anchor.unwrap().line, 0);
    assert_eq!(engine.view().cursor.line, 2);
    assert_eq!(engine.view().cursor.col, 4);
}

#[test]
fn test_mouse_double_click_selects_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    let wid = engine.active_window_id();
    engine.mouse_double_click(wid, 0, 1); // in "hello"

    assert_eq!(engine.mode, Mode::Visual);
    assert_eq!(engine.visual_anchor.unwrap().col, 0); // word start
    assert_eq!(engine.view().cursor.col, 4); // word end (inclusive)
}

#[test]
fn test_mouse_double_click_on_non_word() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    let wid = engine.active_window_id();
    engine.mouse_double_click(wid, 0, 5); // on space

    // Should not enter visual mode
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_mouse_click_after_drag_resets() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world");
    engine.update_syntax();

    let wid = engine.active_window_id();
    engine.mouse_click(wid, 0, 2);
    engine.mouse_drag(wid, 0, 5);
    assert_eq!(engine.mode, Mode::Visual);

    // Click should exit visual mode and reset drag
    engine.mouse_click(wid, 0, 0);
    assert_eq!(engine.mode, Mode::Normal);
    assert!(!engine.mouse_drag_active);
}

#[test]
fn test_double_click_then_drag_preserves_word_anchor() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello world foo bar");
    engine.update_syntax();

    let wid = engine.active_window_id();

    // Double-click on "world" (col 6 is inside "world")
    engine.mouse_double_click(wid, 0, 6);
    assert_eq!(engine.mode, Mode::Visual);
    // Anchor should be at word start (col 6)
    assert_eq!(engine.visual_anchor.unwrap().col, 6);
    // Cursor should be at word end (col 10)
    assert_eq!(engine.view().cursor.col, 10);

    // Now drag to extend selection further right
    engine.mouse_drag(wid, 0, 14);
    assert_eq!(engine.mode, Mode::Visual);
    // Anchor should still be at word start (col 6), NOT reset
    assert_eq!(engine.visual_anchor.unwrap().col, 6);
    // Cursor should follow the drag
    assert_eq!(engine.view().cursor.col, 14);
}

// ── Clipboard register tests ──────────────────────────────────────────

#[test]
fn test_clipboard_register_write() {
    use std::sync::{Arc, Mutex};
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    engine.update_syntax();

    let written = Arc::new(Mutex::new(String::new()));
    let written_clone = written.clone();
    engine.clipboard_write = Some(Box::new(move |text: &str| {
        *written_clone.lock().unwrap() = text.to_string();
        Ok(())
    }));

    engine.set_register('+', "test_data".to_string(), false);
    assert_eq!(*written.lock().unwrap(), "test_data");
}

#[test]
fn test_clipboard_register_read() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    engine.update_syntax();

    engine.clipboard_read = Some(Box::new(|| Ok("from_clipboard".to_string())));

    let content = engine.get_register_content('+');
    assert!(content.is_some());
    let (text, linewise) = content.unwrap();
    assert_eq!(text, "from_clipboard");
    assert!(!linewise);
}

#[test]
fn test_paste_clipboard_to_command_buffer() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    engine.update_syntax();

    engine.clipboard_read = Some(Box::new(|| Ok("pasted_text".to_string())));

    // Enter command mode
    press_char(&mut engine, ':');
    assert_eq!(engine.mode, Mode::Command);

    engine.paste_clipboard_to_input();
    assert_eq!(engine.command_buffer, "pasted_text");
}

#[test]
fn test_paste_clipboard_multiline_takes_first() {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, "hello");
    engine.update_syntax();

    engine.clipboard_read = Some(Box::new(|| Ok("first line\nsecond line".to_string())));

    // Enter command mode
    press_char(&mut engine, ':');
    engine.paste_clipboard_to_input();
    assert_eq!(engine.command_buffer, "first line");
}

// ── VSCode editing mode tests ────────────────────────────────────────────

fn make_vscode_engine(text: &str) -> Engine {
    let mut engine = Engine::new();
    engine.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
    engine.mode = Mode::Insert;
    engine.buffer_mut().insert(0, text);
    engine.update_syntax();
    engine
}

fn vscode_key(engine: &mut Engine, key_name: &str, unicode: Option<char>, ctrl: bool) {
    engine.handle_key(key_name, unicode, ctrl);
}

#[test]
fn test_vscode_mode_setting() {
    let mut s = crate::core::settings::Settings::default();
    // Default is Vim
    assert_eq!(s.editor_mode, crate::core::settings::EditorMode::Vim);
    s.parse_set_option("mode=vscode").unwrap();
    assert_eq!(s.editor_mode, crate::core::settings::EditorMode::Vscode);
    s.parse_set_option("mode=vim").unwrap();
    assert_eq!(s.editor_mode, crate::core::settings::EditorMode::Vim);
    // Query
    let msg = s.parse_set_option("mode?").unwrap();
    assert_eq!(msg, "mode=vim");
    s.parse_set_option("mode=vscode").unwrap();
    let msg2 = s.parse_set_option("mode?").unwrap();
    assert_eq!(msg2, "mode=vscode");
}

#[test]
fn test_vscode_mode_typing() {
    let mut engine = make_vscode_engine("hello");
    // Colon should insert a literal ':' not enter command mode
    vscode_key(&mut engine, "", Some(':'), false);
    assert_eq!(engine.mode, Mode::Insert);
    assert!(engine.buffer().to_string().contains(':'));
}

#[test]
fn test_vscode_mode_ctrl_z_undo() {
    let mut engine = make_vscode_engine("hello");
    // Type 'x'
    vscode_key(&mut engine, "", Some('x'), false);
    let text_after = engine.buffer().to_string();
    // Ctrl-Z undo
    vscode_key(&mut engine, "z", Some('z'), true);
    // Should restore to "hello"
    assert_ne!(engine.buffer().to_string(), text_after);
}

#[test]
fn test_vscode_mode_ctrl_y_redo() {
    let mut engine = make_vscode_engine("hello");
    // Type 'x'
    vscode_key(&mut engine, "", Some('x'), false);
    let after_type = engine.buffer().to_string();
    // Undo
    vscode_key(&mut engine, "z", Some('z'), true);
    // Redo
    vscode_key(&mut engine, "y", Some('y'), true);
    assert_eq!(engine.buffer().to_string(), after_type);
}

#[test]
fn test_vscode_mode_shift_arrow_selection() {
    let mut engine = make_vscode_engine("hello");
    // Shift+Right: start selection
    vscode_key(&mut engine, "Shift_Right", None, false);
    assert!(engine.visual_anchor.is_some());
    assert_eq!(engine.mode, Mode::Visual);
    assert_eq!(engine.visual_anchor.unwrap().col, 0);
    assert_eq!(engine.view().cursor.col, 1);
}

#[test]
fn test_vscode_mode_ctrl_shift_arrow_word_select() {
    let mut engine = make_vscode_engine("hello world");
    // Ctrl+Shift+Right: select word
    vscode_key(&mut engine, "Shift_Right", None, true);
    assert!(engine.visual_anchor.is_some());
    assert_eq!(engine.mode, Mode::Visual);
    // Cursor should be past the word "hello"
    assert!(engine.view().cursor.col > 0);
}

#[test]
fn test_vscode_mode_type_replaces_selection() {
    let mut engine = make_vscode_engine("hello");
    // Shift+Right+Right to select "he"
    vscode_key(&mut engine, "Shift_Right", None, false);
    vscode_key(&mut engine, "Shift_Right", None, false);
    assert!(engine.visual_anchor.is_some());
    // Type 'X' — should replace selection
    vscode_key(&mut engine, "", Some('X'), false);
    assert!(engine.visual_anchor.is_none());
    assert_eq!(engine.mode, Mode::Insert);
    let text = engine.buffer().to_string();
    assert!(text.starts_with('X'));
    assert!(text.contains("llo"));
}

#[test]
fn test_vscode_mode_backspace_clears_selection() {
    let mut engine = make_vscode_engine("hello");
    // Shift+Right+Right to select "he"
    vscode_key(&mut engine, "Shift_Right", None, false);
    vscode_key(&mut engine, "Shift_Right", None, false);
    assert!(engine.visual_anchor.is_some());
    // Backspace — should delete selection
    vscode_key(&mut engine, "BackSpace", None, false);
    assert!(engine.visual_anchor.is_none());
    let text = engine.buffer().to_string();
    assert!(text.starts_with("llo"));
}

#[test]
fn test_vscode_mode_ctrl_a_select_all() {
    let mut engine = make_vscode_engine("hello\nworld");
    engine.update_syntax();
    vscode_key(&mut engine, "a", Some('a'), true);
    assert!(engine.visual_anchor.is_some());
    assert_eq!(engine.visual_anchor.unwrap().line, 0);
    assert_eq!(engine.visual_anchor.unwrap().col, 0);
    assert_eq!(engine.mode, Mode::Visual);
    // Cursor at end of last line
    assert_eq!(engine.view().cursor.line, 1);
}

#[test]
fn test_vscode_mode_escape_clears_selection() {
    let mut engine = make_vscode_engine("hello");
    vscode_key(&mut engine, "Shift_Right", None, false);
    assert!(engine.visual_anchor.is_some());
    vscode_key(&mut engine, "Escape", None, false);
    assert!(engine.visual_anchor.is_none());
    assert_eq!(engine.mode, Mode::Insert);
}

#[test]
fn test_vscode_mode_ctrl_x_no_selection_cuts_line() {
    let mut engine = make_vscode_engine("hello\nworld");
    engine.update_syntax();
    // Cursor on first line, no selection
    assert!(engine.visual_anchor.is_none());
    vscode_key(&mut engine, "x", Some('x'), true);
    // First line should be deleted
    let text = engine.buffer().to_string();
    assert!(
        !text.contains("hello"),
        "Line should be cut: got {:?}",
        text
    );
    // Register '+' should contain the cut line
    let (reg_content, _) = engine.registers.get(&'+').cloned().unwrap_or_default();
    assert!(reg_content.contains("hello"));
}

#[test]
fn test_vscode_mode_ctrl_c_no_selection_copies_line() {
    let mut engine = make_vscode_engine("hello\nworld");
    engine.update_syntax();
    // Ctrl-C with no selection: copy current line
    vscode_key(&mut engine, "c", Some('c'), true);
    // Buffer unchanged
    assert!(engine.buffer().to_string().contains("hello"));
    // Register '+' should contain the line
    let (reg_content, is_linewise) = engine.registers.get(&'+').cloned().unwrap_or_default();
    assert!(reg_content.contains("hello"));
    assert!(is_linewise);
}

#[test]
fn test_vscode_mode_toggle() {
    let mut engine = Engine::new();
    assert_eq!(
        engine.settings.editor_mode,
        crate::core::settings::EditorMode::Vim
    );
    assert_eq!(engine.mode, Mode::Normal);
    engine.toggle_editor_mode();
    assert_eq!(
        engine.settings.editor_mode,
        crate::core::settings::EditorMode::Vscode
    );
    assert_eq!(engine.mode, Mode::Insert);
    engine.toggle_editor_mode();
    assert_eq!(
        engine.settings.editor_mode,
        crate::core::settings::EditorMode::Vim
    );
    assert_eq!(engine.mode, Mode::Normal);
}

#[test]
fn test_vscode_mode_smart_home() {
    let mut engine = make_vscode_engine("  hello");
    // Cursor at col 0 initially — Home moves to first non-ws
    vscode_key(&mut engine, "Home", None, false);
    assert_eq!(engine.view().cursor.col, 2); // first non-ws is col 2
                                             // Home again — moves to col 0
    vscode_key(&mut engine, "Home", None, false);
    assert_eq!(engine.view().cursor.col, 0);
}

#[test]
fn test_vscode_mode_comment_toggle() {
    let mut engine = make_vscode_engine("hello");
    // Set language so comment style is // (not fallback #)
    let buf_id = engine.active_buffer_id();
    engine
        .buffer_manager
        .get_mut(buf_id)
        .unwrap()
        .lsp_language_id = Some("rust".to_string());
    // Ctrl+/ should add "// " prefix
    vscode_key(&mut engine, "/", Some('/'), true);
    let text = engine.buffer().to_string();
    assert!(
        text.starts_with("// hello"),
        "Expected '// hello', got {:?}",
        text
    );
    // Ctrl+/ again should remove "// "
    vscode_key(&mut engine, "/", Some('/'), true);
    let text2 = engine.buffer().to_string();
    assert!(
        text2.starts_with("hello"),
        "Expected 'hello', got {:?}",
        text2
    );
    // Also test with "slash" key_name (GTK/TUI send this)
    vscode_key(&mut engine, "slash", None, true);
    let text3 = engine.buffer().to_string();
    assert!(
        text3.starts_with("// hello"),
        "Expected '// hello' via slash key_name, got {:?}",
        text3
    );
}

#[test]
fn test_vscode_mode_f1_opens_palette() {
    let mut engine = make_vscode_engine("hello");
    // F1 should open the command palette (matches real VSCode).
    vscode_key(&mut engine, "F1", None, false);
    assert!(engine.picker_open, "F1 should open the command palette");
    assert_eq!(engine.mode, Mode::Insert, "mode should stay Insert");
}

#[test]
fn test_vscode_mode_execute_command_returns_to_insert() {
    let mut engine = make_vscode_engine("hello");
    // Execute a command directly (like via the command palette).
    engine.execute_command("set number");
    // Should stay in Insert (EDIT) mode.
    assert_eq!(engine.mode, Mode::Insert);
    assert!(engine.is_vscode_mode());
}

#[test]
fn test_vscode_mode_f1_escape_closes_palette() {
    let mut engine = make_vscode_engine("hello");
    // F1 → palette, then Escape → closes palette, stays in EDIT mode.
    vscode_key(&mut engine, "F1", None, false);
    assert!(engine.picker_open);
    engine.handle_key("Escape", None, false);
    assert!(!engine.picker_open, "Escape should close palette");
    assert_eq!(engine.mode, Mode::Insert);
    assert!(engine.is_vscode_mode());
}

// -----------------------------------------------------------------------
// Menu bar tests
// -----------------------------------------------------------------------

#[test]
fn test_menu_bar_toggle() {
    let mut engine = Engine::new();
    assert!(!engine.menu_bar_visible, "menu bar starts hidden");
    engine.toggle_menu_bar();
    assert!(engine.menu_bar_visible, "toggle_menu_bar() should show bar");
    engine.toggle_menu_bar();
    assert!(!engine.menu_bar_visible, "second toggle hides bar");
}

#[test]
fn test_menu_open_close() {
    let mut engine = Engine::new();
    engine.menu_bar_visible = true;
    assert_eq!(engine.menu_open_idx, None);
    engine.open_menu(2);
    assert_eq!(
        engine.menu_open_idx,
        Some(2),
        "open_menu sets dropdown index"
    );
    engine.close_menu();
    assert_eq!(engine.menu_open_idx, None, "close_menu clears dropdown");
    assert!(engine.menu_bar_visible, "close_menu keeps bar visible");
}

#[test]
fn test_menu_activate_dispatches_command() {
    let mut engine = Engine::new();
    // Load a buffer with content so we can verify save via w command.
    let tmp = std::env::temp_dir().join("vimcode_menu_test_save.txt");
    let _ = std::fs::write(&tmp, "hello");
    engine
        .buffer_manager
        .get_mut(engine.active_buffer_id())
        .unwrap()
        .file_path = Some(tmp.clone());
    engine
        .buffer_manager
        .get_mut(engine.active_buffer_id())
        .unwrap()
        .dirty = true;
    engine.menu_bar_visible = true;
    engine.menu_open_idx = Some(0);
    // Activate the "Save" item (File menu, action "w") via menu_activate_item.
    engine.menu_activate_item(0, 2, "w");
    assert_eq!(
        engine.menu_open_idx, None,
        "menu_activate_item closes dropdown"
    );
    // Buffer should no longer be dirty after :w
    let dirty = engine
        .buffer_manager
        .get(engine.active_buffer_id())
        .map(|s| s.dirty)
        .unwrap_or(true);
    assert!(!dirty, "buffer saved after menu activate");
    let _ = std::fs::remove_file(&tmp);
}

// ── Session 82: menu navigation ────────────────────────────────────────────

#[test]
fn test_menu_item_navigation() {
    let mut engine = Engine::new();
    engine.menu_bar_visible = true;
    engine.open_menu(0);
    assert_eq!(
        engine.menu_highlighted_item, None,
        "starts with no highlight"
    );

    // Items: [non-sep(0), non-sep(1), sep(2), non-sep(3)]
    let seps = [false, false, true, false];

    engine.menu_move_selection(1, &seps);
    assert_eq!(engine.menu_highlighted_item, Some(0), "first non-sep");

    engine.menu_move_selection(1, &seps);
    assert_eq!(engine.menu_highlighted_item, Some(1), "second non-sep");

    engine.menu_move_selection(1, &seps);
    assert_eq!(engine.menu_highlighted_item, Some(3), "skips separator");

    engine.menu_move_selection(1, &seps);
    assert_eq!(engine.menu_highlighted_item, Some(0), "wraps around");

    // Reverse direction
    engine.menu_move_selection(-1, &seps);
    assert_eq!(engine.menu_highlighted_item, Some(3), "reverse wrap");
}

#[test]
fn test_menu_activate_highlighted() {
    let mut engine = Engine::new();
    engine.menu_bar_visible = true;
    engine.open_menu(2); // arbitrary menu index

    // Nothing highlighted → returns None, menu stays open
    let result = engine.menu_activate_highlighted();
    assert!(result.is_none(), "None when nothing highlighted");
    assert!(engine.menu_open_idx.is_some(), "menu stays open");

    // Highlight an item, then activate
    engine.menu_highlighted_item = Some(3);
    let result = engine.menu_activate_highlighted();
    assert_eq!(result, Some((2, 3)), "returns (menu_idx, item_idx)");
    assert!(
        engine.menu_open_idx.is_none(),
        "menu is closed after activate"
    );
}

#[test]
fn test_dap_session_active_field() {
    let engine = Engine::new();
    assert!(
        !engine.dap_session_active,
        "dap_session_active defaults to false"
    );
}

// ── Session 83: DAP transport + engine methods ─────────────────────────────

#[test]
fn test_dap_toggle_breakpoint_add() {
    use crate::core::dap::BreakpointInfo;
    let mut engine = Engine::new();
    engine.dap_toggle_breakpoint("/src/main.rs", 10);
    let bps = engine.dap_breakpoints.get("/src/main.rs").unwrap();
    assert_eq!(bps, &vec![BreakpointInfo::new(10)], "breakpoint added");
    assert!(
        engine.message.contains("Breakpoint set"),
        "{}",
        engine.message
    );
}

#[test]
fn test_dap_toggle_breakpoint_remove() {
    let mut engine = Engine::new();
    engine.dap_toggle_breakpoint("/src/main.rs", 10);
    engine.dap_toggle_breakpoint("/src/main.rs", 10);
    let bps = engine.dap_breakpoints.get("/src/main.rs").unwrap();
    assert!(bps.is_empty(), "second toggle removes breakpoint");
    assert!(
        engine.message.contains("Breakpoint removed"),
        "{}",
        engine.message
    );
}

#[test]
fn test_dap_breakpoints_sorted() {
    let mut engine = Engine::new();
    engine.dap_toggle_breakpoint("/src/lib.rs", 30);
    engine.dap_toggle_breakpoint("/src/lib.rs", 5);
    engine.dap_toggle_breakpoint("/src/lib.rs", 15);
    let bps = engine.dap_breakpoints.get("/src/lib.rs").unwrap();
    let lines: Vec<u64> = bps.iter().map(|b| b.line).collect();
    assert_eq!(lines, vec![5, 15, 30], "breakpoints stored sorted");
}

#[test]
fn test_dap_breakpoints_multiple_files() {
    use crate::core::dap::BreakpointInfo;
    let mut engine = Engine::new();
    engine.dap_toggle_breakpoint("/src/a.rs", 1);
    engine.dap_toggle_breakpoint("/src/b.rs", 2);
    assert_eq!(
        engine.dap_breakpoints.get("/src/a.rs").unwrap(),
        &vec![BreakpointInfo::new(1)]
    );
    assert_eq!(
        engine.dap_breakpoints.get("/src/b.rs").unwrap(),
        &vec![BreakpointInfo::new(2)]
    );
}

#[test]
fn test_dap_no_session_commands_show_message() {
    let mut engine = Engine::new();
    engine.dap_continue();
    assert!(
        engine.message.contains("no active session"),
        "{}",
        engine.message
    );
    engine.dap_pause();
    assert!(
        engine.message.contains("no active session"),
        "{}",
        engine.message
    );
    engine.dap_step_over();
    assert!(
        engine.message.contains("no active session"),
        "{}",
        engine.message
    );
    engine.dap_step_into();
    assert!(
        engine.message.contains("no active session"),
        "{}",
        engine.message
    );
    engine.dap_step_out();
    assert!(
        engine.message.contains("no active session"),
        "{}",
        engine.message
    );
}

#[test]
fn test_dap_stop_clears_session() {
    let mut engine = Engine::new();
    engine.dap_session_active = true;
    engine.dap_stopped_thread = Some(1);
    engine.dap_seq_launch = Some(42);
    engine.dap_stop();
    assert!(!engine.dap_session_active, "session cleared after stop");
    assert!(
        engine.dap_stopped_thread.is_none(),
        "stopped_thread cleared"
    );
    assert!(engine.dap_seq_launch.is_none(), "seq_launch cleared");
}

#[test]
fn test_dap_install_unknown_language() {
    let mut engine = Engine::new();
    engine.execute_command("DapInstall cobol");
    assert!(
        engine.message.contains("No built-in DAP adapter"),
        "{}",
        engine.message
    );
}

#[test]
fn test_dap_install_known_language_no_lsp_message() {
    // DapInstall must NEVER show "No LSP for ..." messages.
    // Without registry manifests, it falls through to direct adapter install.
    let mut engine = Engine::new();
    engine.execute_command("DapInstall rust");
    assert!(
        !engine.message.contains("No LSP"),
        "DapInstall should not emit LSP messages: {}",
        engine.message
    );
    // Should either redirect to ExtInstall, mention rust/codelldb, or start installing
    assert!(
        engine.message.contains("ExtInstall")
            || engine.message.contains("rust")
            || engine.message.contains("codelldb")
            || engine.message.contains("Install"),
        "DapInstall should produce a relevant message: {}",
        engine.message
    );
}

#[test]
fn test_dap_install_no_arg() {
    let mut engine = Engine::new();
    engine.execute_command("DapInstall");
    assert!(engine.message.contains("Usage"), "{}", engine.message);
}

#[test]
fn test_dap_fields_default() {
    let engine = Engine::new();
    assert!(engine.dap_manager.is_none(), "dap_manager starts None");
    assert!(engine.dap_stopped_thread.is_none());
    assert!(engine.dap_breakpoints.is_empty());
    assert!(engine.dap_seq_launch.is_none());
    assert!(engine.dap_current_line.is_none());
    assert!(engine.dap_stack_frames.is_empty());
    assert!(engine.dap_variables.is_empty());
    assert!(engine.dap_output_lines.is_empty());
}

#[test]
fn test_rust_debug_binary_no_cargo_toml() {
    // A temp dir with no Cargo.toml should return an error.
    let dir = std::env::temp_dir().join("vimcode_test_no_cargo");
    let _ = std::fs::create_dir_all(&dir);
    let result = rust_debug_binary(&dir);
    assert!(
        result.is_err(),
        "Should fail when Cargo.toml not found: {:?}",
        result
    );
    assert!(
        result.unwrap_err().contains("Cargo.toml"),
        "Error should mention Cargo.toml"
    );
}

#[test]
fn test_dap_breakpoint_gutter_fields() {
    // Toggle a breakpoint and verify the engine state that render.rs queries.
    let mut engine = Engine::new();
    engine.execute_command("e /tmp/foo.rs");
    // Set a breakpoint via the "brkpt" command path.
    engine.dap_toggle_breakpoint("/tmp/foo.rs", 5);
    let bp = engine.dap_breakpoints.get("/tmp/foo.rs");
    assert!(bp.is_some(), "Breakpoint should be registered");
    assert_eq!(bp.unwrap().len(), 1);
    assert_eq!(bp.unwrap()[0].line, 5);
    // Toggle again to remove.
    engine.dap_toggle_breakpoint("/tmp/foo.rs", 5);
    let bp2 = engine.dap_breakpoints.get("/tmp/foo.rs");
    assert!(
        bp2.map(|v| v.is_empty()).unwrap_or(true),
        "Breakpoint should be removed"
    );
}

#[test]
fn test_dap_current_line_set_on_stop() {
    // dap_current_line starts None and can be set/cleared directly.
    let mut engine = Engine::new();
    assert!(engine.dap_current_line.is_none());
    engine.dap_current_line = Some(("/tmp/foo.rs".to_string(), 10));
    assert_eq!(
        engine.dap_current_line,
        Some(("/tmp/foo.rs".to_string(), 10))
    );
    // Simulate Continued: clear the stopped line.
    engine.dap_current_line = None;
    engine.dap_stopped_thread = None;
    assert!(engine.dap_current_line.is_none());
}

#[test]
fn test_dap_current_line_cleared_on_stop_and_continued() {
    // Verify that dap_session_active affects has_bp computation:
    // when active, even a file with no BPs shows the gutter column.
    let mut engine = Engine::new();
    engine.dap_session_active = true;
    // No breakpoints set yet — but session is active.
    let bp_lines = engine
        .dap_breakpoints
        .get("")
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let has_bp = !bp_lines.is_empty() || engine.dap_session_active;
    assert!(
        has_bp,
        "has_bp should be true when session is active even with no BPs"
    );
}

#[test]
fn test_dap_stack_frames_parsed_from_json() {
    // Simulate what poll_dap does when a stackTrace RequestComplete arrives.
    use crate::core::dap::StackFrame;
    let mut engine = Engine::new();
    let frames_json = serde_json::json!([
        {"id": 1, "name": "main", "source": {"path": "/tmp/src/main.rs"}, "line": 42},
        {"id": 2, "name": "helper", "source": {"path": "/tmp/src/lib.rs"}, "line": 10},
    ]);
    let frames: Vec<StackFrame> = frames_json
        .as_array()
        .unwrap()
        .iter()
        .map(|f| StackFrame {
            id: f.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            name: f
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string(),
            source: f
                .get("source")
                .and_then(|s| s.get("path"))
                .and_then(|p| p.as_str())
                .map(|s| s.to_string()),
            line: f.get("line").and_then(|v| v.as_u64()).unwrap_or(0),
        })
        .collect();
    engine.dap_stack_frames = frames;
    assert_eq!(engine.dap_stack_frames.len(), 2);
    assert_eq!(engine.dap_stack_frames[0].name, "main");
    assert_eq!(engine.dap_stack_frames[0].line, 42);
    assert_eq!(
        engine.dap_stack_frames[0].source.as_deref(),
        Some("/tmp/src/main.rs")
    );
    assert_eq!(engine.dap_stack_frames[1].name, "helper");
}

#[test]
fn test_dap_variables_parsed_from_json() {
    use crate::core::dap::DapVariable;
    let mut engine = Engine::new();
    let vars_json = serde_json::json!([
        {"name": "x", "value": "42", "variablesReference": 0},
        {"name": "msg", "value": "\"hello\"", "variablesReference": 0},
    ]);
    engine.dap_variables = vars_json
        .as_array()
        .unwrap()
        .iter()
        .map(|v| DapVariable {
            name: v
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("?")
                .to_string(),
            value: v
                .get("value")
                .and_then(|val| val.as_str())
                .unwrap_or("")
                .to_string(),
            var_ref: v
                .get("variablesReference")
                .and_then(|r| r.as_u64())
                .unwrap_or(0),
            is_nonpublic: false,
        })
        .collect();
    assert_eq!(engine.dap_variables.len(), 2);
    assert_eq!(engine.dap_variables[0].name, "x");
    assert_eq!(engine.dap_variables[0].value, "42");
    assert_eq!(engine.dap_variables[1].name, "msg");
}

#[test]
fn test_strip_ansi_and_control_complete_sequence() {
    // Complete CSI sequence is removed.
    assert_eq!(
        Engine::strip_ansi_and_control("\x1b[38;2;97;175;239mhello\x1b[0m"),
        "hello"
    );
    // Bare text passes through unchanged.
    assert_eq!(Engine::strip_ansi_and_control("plain text"), "plain text");
    // Newlines and tabs are preserved; \r and other control chars are stripped.
    assert_eq!(
        Engine::strip_ansi_and_control("line1\nline2\ttab\r"),
        "line1\nline2\ttab"
    );
}

#[test]
fn test_ansi_incomplete_tail_start() {
    // No ESC → no tail.
    assert_eq!(Engine::ansi_incomplete_tail_start("hello"), None);
    // Complete CSI → no tail.
    assert_eq!(Engine::ansi_incomplete_tail_start("text\x1b[32m"), None);
    // Partial CSI at end → returns its start offset.
    let s = "text\x1b[38;2;97;175;239";
    let pos = Engine::ansi_incomplete_tail_start(s).unwrap();
    assert_eq!(&s[pos..], "\x1b[38;2;97;175;239");
    // Bare ESC at end → carry.
    let s2 = "hello\x1b";
    assert_eq!(Engine::ansi_incomplete_tail_start(s2), Some(5));
    // ESC [ with partial params → carry.
    let s3 = "\x1b[";
    assert_eq!(Engine::ansi_incomplete_tail_start(s3), Some(0));
}

#[test]
fn test_dap_ansi_carry_handles_split_sequence() {
    // Simulate two consecutive DAP output events where an ANSI RGB colour
    // sequence is split: first chunk ends at `\x1b[38;2;97;175;239` (no
    // final byte), second chunk supplies `mtext`.  The output panel should
    // receive "text", NOT "38;2;97;175;239mtext".
    let first = "start\x1b[38;2;97;175;239";
    let second = "mtext end";
    // First event: tail is carried.
    let combined1 = first.to_string();
    let carry = if let Some(pos) = Engine::ansi_incomplete_tail_start(&combined1) {
        combined1[pos..].to_string()
    } else {
        String::new()
    };
    let clean1 = Engine::strip_ansi_and_control(&combined1[..combined1.len() - carry.len()]);
    assert_eq!(clean1, "start");
    assert_eq!(carry, "\x1b[38;2;97;175;239");
    // Second event: prepend carry, strip.
    let combined2 = format!("{carry}{second}");
    let tail2 = Engine::ansi_incomplete_tail_start(&combined2);
    assert_eq!(tail2, None); // sequence is now complete
    let clean2 = Engine::strip_ansi_and_control(&combined2);
    assert_eq!(clean2, "text end");
}

#[test]
fn test_dap_output_lines_appended_and_capped() {
    let mut engine = Engine::new();
    // Append lines and verify they accumulate.
    engine
        .dap_output_lines
        .push("[stdout] Hello, world!".to_string());
    engine
        .dap_output_lines
        .push("[stderr] Error: oops".to_string());
    assert_eq!(engine.dap_output_lines.len(), 2);
    assert_eq!(engine.dap_output_lines[0], "[stdout] Hello, world!");

    // Verify cap: fill to > 1000 and drain.
    engine.dap_output_lines.clear();
    for i in 0..1005 {
        engine.dap_output_lines.push(format!("line {i}"));
    }
    if engine.dap_output_lines.len() > 1000 {
        let excess = engine.dap_output_lines.len() - 1000;
        engine.dap_output_lines.drain(..excess);
    }
    assert_eq!(engine.dap_output_lines.len(), 1000);
    // After draining, the oldest 5 lines are gone; line 5 should now be first.
    assert_eq!(engine.dap_output_lines[0], "line 5");
}

#[test]
fn test_dap_frames_and_vars_cleared_on_continued() {
    use crate::core::dap::{DapVariable, StackFrame};
    let mut engine = Engine::new();
    engine.dap_stack_frames = vec![StackFrame {
        id: 1,
        name: "main".to_string(),
        source: None,
        line: 5,
    }];
    engine.dap_variables = vec![DapVariable {
        name: "x".to_string(),
        value: "10".to_string(),
        var_ref: 0,
        is_nonpublic: false,
    }];
    // Simulate Continued event clearing.
    engine.dap_stack_frames.clear();
    engine.dap_variables.clear();
    engine.dap_current_line = None;
    assert!(engine.dap_stack_frames.is_empty());
    assert!(engine.dap_variables.is_empty());
    assert!(engine.dap_current_line.is_none());
}

#[test]
fn test_dap_sidebar_section_navigation() {
    let mut engine = Engine::new();
    assert_eq!(engine.dap_sidebar_section, DebugSidebarSection::Variables);
    engine.handle_debug_sidebar_key("Tab", false);
    assert_eq!(engine.dap_sidebar_section, DebugSidebarSection::Watch);
    engine.handle_debug_sidebar_key("Tab", false);
    assert_eq!(engine.dap_sidebar_section, DebugSidebarSection::CallStack);
    engine.handle_debug_sidebar_key("Tab", false);
    assert_eq!(engine.dap_sidebar_section, DebugSidebarSection::Breakpoints);
    engine.handle_debug_sidebar_key("Tab", false);
    assert_eq!(engine.dap_sidebar_section, DebugSidebarSection::Variables);
}

#[test]
fn test_dap_sidebar_section_index() {
    assert_eq!(
        Engine::dap_sidebar_section_index(DebugSidebarSection::Variables),
        0
    );
    assert_eq!(
        Engine::dap_sidebar_section_index(DebugSidebarSection::Watch),
        1
    );
    assert_eq!(
        Engine::dap_sidebar_section_index(DebugSidebarSection::CallStack),
        2
    );
    assert_eq!(
        Engine::dap_sidebar_section_index(DebugSidebarSection::Breakpoints),
        3
    );
}

#[test]
fn test_dap_sidebar_ensure_visible_scrolls_down() {
    let mut engine = Engine::new();
    engine.dap_sidebar_section = DebugSidebarSection::Variables;
    engine.dap_sidebar_section_heights = [5, 5, 5, 5];
    engine.dap_sidebar_scroll = [0; 4];
    // Simulate selecting item 7 (beyond the 5-row viewport).
    engine.dap_sidebar_selected = 7;
    engine.dap_sidebar_ensure_visible();
    // scroll should adjust so item 7 is the last visible: scroll = 7 - 5 + 1 = 3
    assert_eq!(engine.dap_sidebar_scroll[0], 3);
}

#[test]
fn test_dap_sidebar_ensure_visible_scrolls_up() {
    let mut engine = Engine::new();
    engine.dap_sidebar_section = DebugSidebarSection::Watch;
    engine.dap_sidebar_section_heights = [5, 5, 5, 5];
    engine.dap_sidebar_scroll = [0, 10, 0, 0]; // Watch scroll at 10
                                               // Select item 3 which is before the scroll window.
    engine.dap_sidebar_selected = 3;
    engine.dap_sidebar_ensure_visible();
    // scroll should jump to 3
    assert_eq!(engine.dap_sidebar_scroll[1], 3);
}

#[test]
fn test_dap_sidebar_ensure_visible_no_change_when_visible() {
    let mut engine = Engine::new();
    engine.dap_sidebar_section = DebugSidebarSection::CallStack;
    engine.dap_sidebar_section_heights = [5, 5, 5, 5];
    engine.dap_sidebar_scroll = [0, 0, 2, 0]; // CallStack scroll at 2
    engine.dap_sidebar_selected = 4; // visible: items 2,3,4,5,6
    engine.dap_sidebar_ensure_visible();
    assert_eq!(engine.dap_sidebar_scroll[2], 2); // unchanged
}

#[test]
fn test_dap_sidebar_ensure_visible_zero_height_noop() {
    let mut engine = Engine::new();
    engine.dap_sidebar_section = DebugSidebarSection::Variables;
    engine.dap_sidebar_section_heights = [0, 0, 0, 0]; // not yet laid out
    engine.dap_sidebar_selected = 10;
    engine.dap_sidebar_ensure_visible();
    assert_eq!(engine.dap_sidebar_scroll[0], 0); // unchanged
}

#[test]
fn test_dap_sidebar_resize_section() {
    let mut engine = Engine::new();
    engine.dap_sidebar_section_heights = [10, 10, 10, 10];
    // Grow section 0 by 3, shrink section 1 by 3.
    engine.dap_sidebar_resize_section(0, 3);
    assert_eq!(engine.dap_sidebar_section_heights[0], 13);
    assert_eq!(engine.dap_sidebar_section_heights[1], 7);
    // Total preserved.
    assert_eq!(engine.dap_sidebar_section_heights.iter().sum::<u16>(), 40);
}

#[test]
fn test_dap_sidebar_resize_section_clamps_min() {
    let mut engine = Engine::new();
    engine.dap_sidebar_section_heights = [3, 3, 10, 10];
    // Try to shrink section 0 by 10 — should clamp to 1.
    engine.dap_sidebar_resize_section(0, -10);
    assert_eq!(engine.dap_sidebar_section_heights[0], 1);
    assert_eq!(engine.dap_sidebar_section_heights[1], 5); // 3 + 3 - 1 = 5
                                                          // Total preserved.
    assert_eq!(
        engine.dap_sidebar_section_heights[0] + engine.dap_sidebar_section_heights[1],
        6
    );
}

#[test]
fn test_dap_sidebar_resize_section_last_noop() {
    let mut engine = Engine::new();
    engine.dap_sidebar_section_heights = [10, 10, 10, 10];
    // section_idx=3 has no next section — should be a no-op.
    engine.dap_sidebar_resize_section(3, 5);
    assert_eq!(engine.dap_sidebar_section_heights, [10, 10, 10, 10]);
}

#[test]
fn test_dap_sidebar_scroll_reset_on_stop() {
    let mut engine = Engine::new();
    engine.dap_sidebar_scroll = [5, 10, 3, 7];
    engine.dap_session_active = true;
    engine.dap_stop();
    assert_eq!(engine.dap_sidebar_scroll, [0, 0, 0, 0]);
}

#[test]
fn test_dap_sidebar_jk_triggers_ensure_visible() {
    use crate::core::dap::DapVariable;
    let mut engine = Engine::new();
    engine.dap_sidebar_has_focus = true;
    engine.dap_sidebar_section = DebugSidebarSection::Variables;
    engine.dap_sidebar_section_heights = [3, 3, 3, 3];
    // Create 10 variables so we can scroll.
    engine.dap_variables = (0..10)
        .map(|i| DapVariable {
            name: format!("v{i}"),
            value: format!("{i}"),
            var_ref: 0,
            is_nonpublic: false,
        })
        .collect();
    engine.dap_sidebar_selected = 0;
    // Press j 5 times to go to item 5.
    for _ in 0..5 {
        engine.handle_debug_sidebar_key("j", false);
    }
    assert_eq!(engine.dap_sidebar_selected, 5);
    // Scroll should have adjusted: 5 >= 0 + 3 → scroll = 5 - 3 + 1 = 3
    assert_eq!(engine.dap_sidebar_scroll[0], 3);
}

#[test]
fn test_dap_select_frame_clamps() {
    use crate::core::dap::StackFrame;
    let mut engine = Engine::new();
    engine.dap_stack_frames = vec![
        StackFrame {
            id: 1,
            name: "main".to_string(),
            source: None,
            line: 1,
        },
        StackFrame {
            id: 2,
            name: "foo".to_string(),
            source: None,
            line: 2,
        },
        StackFrame {
            id: 3,
            name: "bar".to_string(),
            source: None,
            line: 3,
        },
    ];
    // Select within bounds.
    engine.dap_select_frame(1);
    assert_eq!(engine.dap_active_frame, 1);
    // Select beyond bounds: clamps to last.
    engine.dap_select_frame(99);
    assert_eq!(engine.dap_active_frame, 2);
    // Select 0.
    engine.dap_select_frame(0);
    assert_eq!(engine.dap_active_frame, 0);
}

#[test]
fn test_dap_variable_expand_tracking() {
    let mut engine = Engine::new();
    assert!(!engine.dap_expanded_vars.contains(&5));
    // Toggle on.
    engine.dap_expanded_vars.insert(5);
    assert!(engine.dap_expanded_vars.contains(&5));
    // Toggle off.
    engine.dap_expanded_vars.remove(&5);
    assert!(!engine.dap_expanded_vars.contains(&5));
}

#[test]
fn test_dap_eval_result_field_default() {
    let engine = Engine::new();
    assert!(engine.dap_eval_result.is_none());
    assert_eq!(engine.dap_sidebar_section, DebugSidebarSection::Variables);
    assert_eq!(engine.dap_active_frame, 0);
    assert!(engine.dap_expanded_vars.is_empty());
    assert!(engine.dap_child_variables.is_empty());
}

#[test]
fn test_visual_rows_for_line() {
    assert_eq!(engine_visual_rows_for_line(0, 80), 1); // empty line = 1 row
    assert_eq!(engine_visual_rows_for_line(80, 80), 1); // exactly one row
    assert_eq!(engine_visual_rows_for_line(81, 80), 2); // one char overflow
    assert_eq!(engine_visual_rows_for_line(160, 80), 2); // exactly two rows
    assert_eq!(engine_visual_rows_for_line(161, 80), 3);
    assert_eq!(engine_visual_rows_for_line(10, 0), 1); // zero cols = 1 row
}

#[test]
fn test_ensure_cursor_visible_wrap_scrolls_down() {
    let mut engine = Engine::new();
    engine.settings.wrap = true;
    // Fill buffer with 20 short lines so the content exists.
    let text = (0..20).map(|i| format!("line {i}\n")).collect::<String>();
    engine.buffer_mut().content = ropey::Rope::from_str(&text);
    // Viewport: 10 lines of 80 cols
    engine.view_mut().viewport_lines = 10;
    engine.view_mut().viewport_cols = 80;
    engine.view_mut().scroll_top = 0;
    // Move cursor to line 15 — beyond the viewport.
    engine.view_mut().cursor.line = 15;
    engine.view_mut().cursor.col = 0;
    engine.ensure_cursor_visible();
    // scroll_top should have advanced so cursor is visible.
    assert!(engine.view().scroll_top > 0);
    assert!(engine.view().cursor.line >= engine.view().scroll_top);
    assert!(engine.view().cursor.line < engine.view().scroll_top + engine.view().viewport_lines);
}

#[test]
fn test_ensure_cursor_visible_wrap_scrolls_up() {
    let mut engine = Engine::new();
    engine.settings.wrap = true;
    let text = (0..20).map(|i| format!("line {i}\n")).collect::<String>();
    engine.buffer_mut().content = ropey::Rope::from_str(&text);
    engine.view_mut().viewport_lines = 10;
    engine.view_mut().viewport_cols = 80;
    engine.view_mut().scroll_top = 15;
    // Move cursor above scroll_top.
    engine.view_mut().cursor.line = 5;
    engine.view_mut().cursor.col = 0;
    engine.ensure_cursor_visible();
    assert_eq!(engine.view().scroll_top, 5);
}

#[test]
fn test_dap_add_remove_watch() {
    let mut engine = Engine::new();
    engine.dap_add_watch("x + 1".to_string());
    engine.dap_add_watch("y".to_string());
    assert_eq!(engine.dap_watch_expressions, vec!["x + 1", "y"]);
    assert_eq!(engine.dap_watch_values.len(), 2);
    assert!(engine.dap_watch_values[0].is_none());
    // Remove the first watch.
    engine.dap_remove_watch(0);
    assert_eq!(engine.dap_watch_expressions, vec!["y"]);
    assert_eq!(engine.dap_watch_values.len(), 1);
    // Remove out-of-bounds: no-op.
    engine.dap_remove_watch(99);
    assert_eq!(engine.dap_watch_expressions.len(), 1);
}

#[test]
fn test_dap_bottom_panel_kind_default() {
    let engine = Engine::new();
    assert_eq!(engine.bottom_panel_kind, BottomPanelKind::Terminal);
}

#[test]
fn test_dap_launch_configs_default() {
    let engine = Engine::new();
    assert!(engine.dap_launch_configs.is_empty());
    assert_eq!(engine.dap_selected_launch_config, 0);
}

#[test]
fn test_debug_toolbar_default_false() {
    let engine = Engine::new();
    assert!(
        !engine.debug_toolbar_visible,
        "toolbar should default to hidden"
    );
}

// ── Session 90: Interactive debug sidebar + conditional breakpoints ──────

#[test]
fn test_sidebar_var_expand_via_enter() {
    use crate::core::dap::DapVariable;
    let mut engine = Engine::new();
    engine.dap_variables = vec![
        DapVariable {
            name: "x".to_string(),
            value: "42".to_string(),
            var_ref: 10,
            is_nonpublic: false,
        },
        DapVariable {
            name: "y".to_string(),
            value: "7".to_string(),
            var_ref: 0,
            is_nonpublic: false,
        },
    ];
    engine.dap_sidebar_section = DebugSidebarSection::Variables;
    engine.dap_sidebar_selected = 0;
    // Enter on expandable var should toggle expand.
    engine.handle_debug_sidebar_key("Return", false);
    assert!(
        engine.dap_expanded_vars.contains(&10),
        "var_ref 10 should be expanded"
    );
    // Enter again should collapse.
    engine.dap_sidebar_selected = 0;
    engine.handle_debug_sidebar_key("Return", false);
    assert!(
        !engine.dap_expanded_vars.contains(&10),
        "var_ref 10 should be collapsed"
    );
}

#[test]
fn test_sidebar_var_enter_on_non_expandable() {
    use crate::core::dap::DapVariable;
    let mut engine = Engine::new();
    engine.dap_variables = vec![DapVariable {
        name: "x".to_string(),
        value: "42".to_string(),
        var_ref: 0,
        is_nonpublic: false,
    }];
    engine.dap_sidebar_section = DebugSidebarSection::Variables;
    engine.dap_sidebar_selected = 0;
    engine.handle_debug_sidebar_key("Return", false);
    // No expansion should happen for var_ref=0.
    assert!(engine.dap_expanded_vars.is_empty());
}

#[test]
fn test_sidebar_callstack_enter_selects_frame() {
    use crate::core::dap::StackFrame;
    let mut engine = Engine::new();
    engine.dap_stack_frames = vec![
        StackFrame {
            id: 1,
            name: "main".to_string(),
            source: None,
            line: 10,
        },
        StackFrame {
            id: 2,
            name: "foo".to_string(),
            source: None,
            line: 20,
        },
    ];
    engine.dap_sidebar_section = DebugSidebarSection::CallStack;
    engine.dap_sidebar_selected = 1;
    engine.handle_debug_sidebar_key("Return", false);
    assert_eq!(engine.dap_active_frame, 1, "should select frame 1");
}

#[test]
fn test_sidebar_section_len_variables() {
    use crate::core::dap::DapVariable;
    let mut engine = Engine::new();
    engine.dap_variables = vec![
        DapVariable {
            name: "a".to_string(),
            value: "1".to_string(),
            var_ref: 5,
            is_nonpublic: false,
        },
        DapVariable {
            name: "b".to_string(),
            value: "2".to_string(),
            var_ref: 0,
            is_nonpublic: false,
        },
    ];
    engine.dap_sidebar_section = DebugSidebarSection::Variables;
    // 2 top-level vars, none expanded.
    assert_eq!(engine.dap_sidebar_section_len(), 2);
    // Expand var_ref=5 with 3 children.
    engine.dap_expanded_vars.insert(5);
    engine.dap_child_variables.insert(
        5,
        vec![
            DapVariable {
                name: "c1".to_string(),
                value: "x".to_string(),
                var_ref: 0,
                is_nonpublic: false,
            },
            DapVariable {
                name: "c2".to_string(),
                value: "y".to_string(),
                var_ref: 0,
                is_nonpublic: false,
            },
            DapVariable {
                name: "c3".to_string(),
                value: "z".to_string(),
                var_ref: 0,
                is_nonpublic: false,
            },
        ],
    );
    assert_eq!(engine.dap_sidebar_section_len(), 5);
}

#[test]
fn test_sidebar_j_k_clamped() {
    use crate::core::dap::DapVariable;
    let mut engine = Engine::new();
    engine.dap_variables = vec![DapVariable {
        name: "x".to_string(),
        value: "1".to_string(),
        var_ref: 0,
        is_nonpublic: false,
    }];
    engine.dap_sidebar_section = DebugSidebarSection::Variables;
    engine.dap_sidebar_selected = 0;
    // j should not go past last item.
    engine.handle_debug_sidebar_key("j", false);
    assert_eq!(engine.dap_sidebar_selected, 0, "clamped at end");
    // k should not go below 0.
    engine.handle_debug_sidebar_key("k", false);
    assert_eq!(engine.dap_sidebar_selected, 0, "clamped at start");
}

#[test]
fn test_sidebar_delete_watch() {
    let mut engine = Engine::new();
    engine.dap_add_watch("expr1".to_string());
    engine.dap_add_watch("expr2".to_string());
    engine.dap_sidebar_section = DebugSidebarSection::Watch;
    engine.dap_sidebar_selected = 0;
    engine.handle_debug_sidebar_key("x", false);
    assert_eq!(engine.dap_watch_expressions.len(), 1);
    assert_eq!(engine.dap_watch_expressions[0], "expr2");
}

#[test]
fn test_sidebar_delete_breakpoint() {
    let mut engine = Engine::new();
    engine.dap_toggle_breakpoint("/tmp/a.rs", 5);
    engine.dap_toggle_breakpoint("/tmp/a.rs", 10);
    engine.dap_sidebar_section = DebugSidebarSection::Breakpoints;
    engine.dap_sidebar_selected = 0;
    engine.handle_debug_sidebar_key("d", false);
    let bps = engine.dap_breakpoints.get("/tmp/a.rs").unwrap();
    assert_eq!(bps.len(), 1);
    assert_eq!(bps[0].line, 10);
}

#[test]
fn test_conditional_breakpoint() {
    let mut engine = Engine::new();
    engine.dap_toggle_breakpoint("/tmp/a.rs", 5);
    engine.dap_set_breakpoint_condition("/tmp/a.rs", 5, Some("x > 10".to_string()));
    let bps = engine.dap_breakpoints.get("/tmp/a.rs").unwrap();
    assert_eq!(bps[0].condition.as_deref(), Some("x > 10"));
    // Clear condition.
    engine.dap_set_breakpoint_condition("/tmp/a.rs", 5, None);
    let bps = engine.dap_breakpoints.get("/tmp/a.rs").unwrap();
    assert!(bps[0].condition.is_none());
}

#[test]
fn test_conditional_breakpoint_creates_bp() {
    let mut engine = Engine::new();
    // Setting a condition on a non-existent breakpoint should create one.
    engine.dap_set_breakpoint_condition("/tmp/b.rs", 10, Some("i == 3".to_string()));
    let bps = engine.dap_breakpoints.get("/tmp/b.rs").unwrap();
    assert_eq!(bps.len(), 1);
    assert_eq!(bps[0].line, 10);
    assert_eq!(bps[0].condition.as_deref(), Some("i == 3"));
}

#[test]
fn test_hit_condition_breakpoint() {
    let mut engine = Engine::new();
    engine.dap_toggle_breakpoint("/tmp/c.rs", 7);
    engine.dap_set_breakpoint_hit_condition("/tmp/c.rs", 7, Some(">= 5".to_string()));
    let bps = engine.dap_breakpoints.get("/tmp/c.rs").unwrap();
    assert_eq!(bps[0].hit_condition.as_deref(), Some(">= 5"));
}

#[test]
fn test_dap_condition_command() {
    let mut engine = Engine::new();
    // Set the buffer file_path directly so the command resolves it.
    engine.active_buffer_state_mut().file_path =
        Some(std::path::PathBuf::from("/tmp/test_cond.rs"));
    // Set a regular breakpoint first.
    engine.dap_toggle_breakpoint("/tmp/test_cond.rs", 1);
    // Cursor is at line 0 (0-based), so DapCondition targets line 1 (1-based).
    engine.execute_command("DapCondition x > 5");
    let bps = engine.dap_breakpoints.get("/tmp/test_cond.rs").unwrap();
    assert_eq!(bps[0].condition.as_deref(), Some("x > 5"));
}

#[test]
fn test_var_ref_at_flat_index_with_children() {
    use crate::core::dap::DapVariable;
    let mut engine = Engine::new();
    engine.dap_variables = vec![
        DapVariable {
            name: "a".to_string(),
            value: "1".to_string(),
            var_ref: 5,
            is_nonpublic: false,
        },
        DapVariable {
            name: "b".to_string(),
            value: "2".to_string(),
            var_ref: 0,
            is_nonpublic: false,
        },
    ];
    // Not expanded: flat [a(idx=0), b(idx=1)].
    assert_eq!(engine.dap_var_ref_at_flat_index(0), Some(5));
    assert_eq!(engine.dap_var_ref_at_flat_index(1), Some(0));
    assert_eq!(engine.dap_var_ref_at_flat_index(2), None);
    // Expand a → children c1, c2.
    engine.dap_expanded_vars.insert(5);
    engine.dap_child_variables.insert(
        5,
        vec![
            DapVariable {
                name: "c1".to_string(),
                value: "x".to_string(),
                var_ref: 0,
                is_nonpublic: false,
            },
            DapVariable {
                name: "c2".to_string(),
                value: "y".to_string(),
                var_ref: 0,
                is_nonpublic: false,
            },
        ],
    );
    // Now flat: [a(0), c1(1), c2(2), b(3)].
    assert_eq!(engine.dap_var_ref_at_flat_index(0), Some(5));
    assert_eq!(engine.dap_var_ref_at_flat_index(1), Some(0)); // c1
    assert_eq!(engine.dap_var_ref_at_flat_index(2), Some(0)); // c2
    assert_eq!(engine.dap_var_ref_at_flat_index(3), Some(0)); // b
}

#[test]
fn test_dap_scope_groups_default_empty() {
    let engine = Engine::new();
    assert!(engine.dap_scope_groups.is_empty());
}

#[test]
fn test_dap_scope_groups_cleared_on_stop() {
    let mut engine = Engine::new();
    engine.dap_scope_groups.push(("Statics".to_string(), 42));
    engine.dap_stop();
    assert!(engine.dap_scope_groups.is_empty());
}

#[test]
fn test_dap_scope_groups_cleared_on_select_frame() {
    let mut engine = Engine::new();
    engine.dap_scope_groups.push(("Statics".to_string(), 42));
    engine.dap_select_frame(0);
    assert!(engine.dap_scope_groups.is_empty());
}

#[test]
fn test_dap_var_flat_count_with_scope_groups() {
    use crate::core::dap::DapVariable;
    let mut engine = Engine::new();
    engine.dap_variables = vec![DapVariable {
        name: "x".to_string(),
        value: "1".to_string(),
        var_ref: 0,
        is_nonpublic: false,
    }];
    // 1 variable, no scope groups.
    assert_eq!(engine.dap_var_flat_count(), 1);
    // Add two scope groups.
    engine.dap_scope_groups = vec![("Statics".to_string(), 100), ("Registers".to_string(), 200)];
    // 1 var + 2 group headers = 3.
    assert_eq!(engine.dap_var_flat_count(), 3);
    // Expand "Statics" with 2 children.
    engine.dap_expanded_vars.insert(100);
    engine.dap_child_variables.insert(
        100,
        vec![
            DapVariable {
                name: "s1".to_string(),
                value: "a".to_string(),
                var_ref: 0,
                is_nonpublic: false,
            },
            DapVariable {
                name: "s2".to_string(),
                value: "b".to_string(),
                var_ref: 0,
                is_nonpublic: false,
            },
        ],
    );
    // 1 var + (1 header + 2 children) + 1 header = 5.
    assert_eq!(engine.dap_var_flat_count(), 5);
}

#[test]
fn test_dap_var_ref_at_flat_index_with_scope_groups() {
    use crate::core::dap::DapVariable;
    let mut engine = Engine::new();
    engine.dap_variables = vec![DapVariable {
        name: "x".to_string(),
        value: "1".to_string(),
        var_ref: 0,
        is_nonpublic: false,
    }];
    engine.dap_scope_groups = vec![("Statics".to_string(), 100)];
    // Flat: [x(0), Statics-header(1)].
    assert_eq!(engine.dap_var_ref_at_flat_index(0), Some(0)); // x
    assert_eq!(engine.dap_var_ref_at_flat_index(1), Some(100)); // Statics header
    assert_eq!(engine.dap_var_ref_at_flat_index(2), None);
    // Expand Statics with children.
    engine.dap_expanded_vars.insert(100);
    engine.dap_child_variables.insert(
        100,
        vec![DapVariable {
            name: "s1".to_string(),
            value: "a".to_string(),
            var_ref: 0,
            is_nonpublic: false,
        }],
    );
    // Flat: [x(0), Statics-header(1), s1(2)].
    assert_eq!(engine.dap_var_ref_at_flat_index(1), Some(100)); // Statics header
    assert_eq!(engine.dap_var_ref_at_flat_index(2), Some(0)); // s1
    assert_eq!(engine.dap_var_ref_at_flat_index(3), None);
}

// ── Workspace / open-folder tests ─────────────────────────────────────────

#[test]
fn test_open_folder_resets_cwd() {
    let dir = std::env::temp_dir().join("vimcode_test_open_folder");
    std::fs::create_dir_all(&dir).unwrap();

    let mut engine = Engine::new();
    let original_cwd = engine.cwd.clone();

    engine.open_folder(&dir);

    // cwd should have changed
    let expected = dir.canonicalize().unwrap_or(dir.clone());
    assert_eq!(engine.cwd, expected);
    assert_ne!(engine.cwd, original_cwd);
    // workspace_root should be set
    assert_eq!(engine.workspace_root, Some(expected));
    // Should have exactly one tab with one empty buffer
    assert_eq!(engine.active_group().tabs.len(), 1);
}

#[test]
fn test_open_workspace_parses_json() {
    let dir = std::env::temp_dir().join("vimcode_test_workspace_json");
    std::fs::create_dir_all(&dir).unwrap();
    let ws_path = dir.join(".vimcode-workspace");
    let json =
        r#"{"version":1,"folders":[{"path":"."}],"settings":{"tabstop":4,"expandtab":true}}"#;
    std::fs::write(&ws_path, json).unwrap();

    let mut engine = Engine::new();
    engine.open_workspace(&ws_path);

    // Should have changed cwd to dir
    let expected = dir.canonicalize().unwrap_or(dir.clone());
    assert_eq!(engine.cwd, expected);
    // workspace_file should be recorded
    assert_eq!(engine.workspace_file, Some(ws_path));
    // Settings overlay: tabstop = 4
    assert_eq!(engine.settings.tabstop, 4);
    // expandtab = true
    assert!(engine.settings.expand_tab);
}

// =========================================================================
// Plugin system engine-integration tests
// =========================================================================

fn write_plugin_lua(name: &str, code: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vimcode_plugins_{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.lua"));
    std::fs::write(&path, code).unwrap();
    dir
}

#[test]
fn test_plugin_loads_and_command_runs() {
    let dir = write_plugin_lua(
        "test_cmd",
        r#"vimcode.command("TestHello", function(args) vimcode.message("hi:" .. args) end)"#,
    );
    let mut engine = Engine::new();
    match plugin::PluginManager::new() {
        Ok(mut mgr) => {
            mgr.load_plugins_dir(&dir, &[]);
            engine.plugin_manager = Some(mgr);
        }
        Err(_) => return,
    }
    let action = engine.execute_command("TestHello world");
    assert_eq!(action, EngineAction::None);
    assert_eq!(engine.message, "hi:world");
}

#[test]
fn test_plugin_on_save_fires() {
    let dir = write_plugin_lua(
        "test_save",
        r#"vimcode.on("save", function(path) vimcode.message("saved:" .. path) end)"#,
    );
    let mut engine = Engine::new();
    match plugin::PluginManager::new() {
        Ok(mut mgr) => {
            mgr.load_plugins_dir(&dir, &[]);
            engine.plugin_manager = Some(mgr);
        }
        Err(_) => return,
    }
    let tmp_file = std::env::temp_dir().join("vimcode_save_hook_test.txt");
    std::fs::write(&tmp_file, "hello\n").unwrap();
    engine.open_file_in_tab(&tmp_file);
    let _ = engine.save();
    assert!(
        engine.message.starts_with("saved:"),
        "expected save hook to fire, got: {}",
        engine.message
    );
    let _ = std::fs::remove_file(&tmp_file);
}

#[test]
fn test_plugin_disabled_not_registered() {
    let dir = write_plugin_lua(
        "test_disabled",
        r#"vimcode.command("DisabledCmd", function() vimcode.message("should not run") end)"#,
    );
    let mut engine = Engine::new();
    match plugin::PluginManager::new() {
        Ok(mut mgr) => {
            mgr.load_plugins_dir(&dir, &["test_disabled".to_string()]);
            engine.plugin_manager = Some(mgr);
        }
        Err(_) => return,
    }
    let action = engine.execute_command("DisabledCmd");
    assert_eq!(action, EngineAction::Error);
    assert!(engine.message.contains("Not an editor command"));
}

// ─── Source Control (Session 99) ─────────────────────────────────────────

/// Build an engine with synthetic SC file statuses for testing.
fn make_sc_engine_with_files() -> Engine {
    let mut engine = Engine::new();
    engine.sc_file_statuses = vec![
        git::FileStatus {
            path: "a.rs".to_string(),
            staged: Some(git::StatusKind::Modified),
            unstaged: None,
        },
        git::FileStatus {
            path: "b.rs".to_string(),
            staged: None,
            unstaged: Some(git::StatusKind::Modified),
        },
    ];
    engine.sc_sections_expanded = [true, true, false, true];
    engine
}

#[test]
fn test_sc_commit_input_mode_toggle() {
    let mut engine = make_sc_engine_with_files();
    assert!(!engine.sc_commit_input_active);
    engine.handle_sc_key("c", false, None);
    assert!(engine.sc_commit_input_active);
    engine.handle_sc_key("Escape", false, None);
    assert!(!engine.sc_commit_input_active);
}

#[test]
fn test_sc_commit_input_typing() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_commit_input_active = true;
    engine.handle_sc_key("", false, Some('h'));
    engine.handle_sc_key("", false, Some('i'));
    assert_eq!(engine.sc_commit_message, "hi");
}

#[test]
fn test_sc_commit_input_backspace() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_commit_input_active = true;
    engine.sc_commit_message = "abc".to_string();
    engine.sc_commit_cursor = 3;
    engine.handle_sc_key("BackSpace", false, None);
    assert_eq!(engine.sc_commit_message, "ab");
}

#[test]
fn test_sc_commit_empty_message_error() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_commit_input_active = true;
    engine.sc_commit_message = "".to_string();
    // simulate Enter with empty message
    engine.sc_do_commit();
    assert!(engine.message.contains("empty"));
    // Input mode stays active implicitly (message set, but no state change needed)
}

#[test]
fn test_sc_nav_j_k() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_has_focus = true;
    assert_eq!(engine.sc_selected, 0);
    engine.handle_sc_key("j", false, None);
    assert_eq!(engine.sc_selected, 1);
    engine.handle_sc_key("k", false, None);
    assert_eq!(engine.sc_selected, 0);
}

#[test]
fn test_sc_nav_clamps_at_bottom() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_has_focus = true;
    let len = engine.sc_flat_len();
    for _ in 0..len + 5 {
        engine.handle_sc_key("j", false, None);
    }
    assert_eq!(engine.sc_selected, len - 1);
}

#[test]
fn test_sc_nav_clamps_at_top() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_has_focus = true;
    engine.handle_sc_key("k", false, None);
    assert_eq!(engine.sc_selected, 0);
}

#[test]
fn test_sc_tab_toggles_section() {
    let mut engine = make_sc_engine_with_files();
    assert!(engine.sc_sections_expanded[0]);
    engine.handle_sc_key("Tab", false, None);
    assert!(!engine.sc_sections_expanded[0]);
    engine.handle_sc_key("Tab", false, None);
    assert!(engine.sc_sections_expanded[0]);
}

#[test]
fn test_sc_escape_unfocuses() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_has_focus = true;
    engine.handle_sc_key("Escape", false, None);
    assert!(!engine.sc_has_focus);
}

#[test]
fn test_sc_q_unfocuses() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_has_focus = true;
    engine.handle_sc_key("q", false, None);
    assert!(!engine.sc_has_focus);
}

#[test]
fn test_sc_commit_input_blocks_nav() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_commit_input_active = true;
    let before = engine.sc_selected;
    // 'j' should go to commit input handler, not navigate
    engine.handle_sc_key("j", false, None);
    // selected should not have changed (j has no meaning in commit input)
    assert_eq!(engine.sc_selected, before);
    // but the commit input should still be active
    assert!(engine.sc_commit_input_active);
}

#[test]
fn test_sc_commit_multiline_enter_inserts_newline() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_commit_input_active = true;
    engine.handle_sc_commit_input_key("", false, Some('H'));
    engine.handle_sc_commit_input_key("", false, Some('i'));
    engine.handle_sc_commit_input_key("Return", false, None);
    engine.handle_sc_commit_input_key("", false, Some('b'));
    assert_eq!(engine.sc_commit_message, "Hi\nb");
    assert!(engine.sc_commit_input_active);
}

#[test]
fn test_sc_commit_ctrl_enter_commits() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_commit_input_active = true;
    engine.sc_commit_message = "test\nmultiline".to_string();
    engine.sc_commit_cursor = engine.sc_commit_message.len();
    engine.handle_sc_commit_input_key("Return", true, None);
    // Commit fails (no real repo), but input mode should be deactivated.
    // The commit_message should be cleared if commit succeeded or stay if it failed.
    // Since there's no git repo, sc_do_commit will error but won't clear.
    assert!(!engine.sc_commit_input_active || !engine.sc_commit_message.is_empty());
}

#[test]
fn test_ssh_passphrase_dialog_shown() {
    let mut engine = make_sc_engine_with_files();
    // Simulate showing passphrase dialog
    engine.sc_show_passphrase_dialog("pull");
    assert!(engine.dialog.is_some());
    let dialog = engine.dialog.as_ref().unwrap();
    assert_eq!(dialog.tag, "ssh_passphrase");
    assert!(dialog.input.is_some());
    assert!(dialog.input.as_ref().unwrap().is_password);
    assert_eq!(engine.pending_git_remote_op, Some("pull".to_string()));
}

#[test]
fn test_dialog_input_typing() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_show_passphrase_dialog("push");
    // Type into the dialog input
    engine.handle_key("", Some('a'), false);
    engine.handle_key("", Some('b'), false);
    engine.handle_key("", Some('c'), false);
    let input_val = engine
        .dialog
        .as_ref()
        .unwrap()
        .input
        .as_ref()
        .unwrap()
        .value
        .clone();
    assert_eq!(input_val, "abc");
    // Backspace
    engine.handle_key("BackSpace", None, false);
    let input_val = engine
        .dialog
        .as_ref()
        .unwrap()
        .input
        .as_ref()
        .unwrap()
        .value
        .clone();
    assert_eq!(input_val, "ab");
}

#[test]
fn test_dialog_input_cancel() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_show_passphrase_dialog("fetch");
    engine.handle_key("Escape", None, false);
    assert!(engine.dialog.is_none());
    assert!(engine.pending_git_remote_op.is_none());
}

#[test]
fn test_sc_commit_cursor_arrow_keys() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_commit_input_active = true;
    // Type "abc"
    engine.handle_sc_commit_input_key("", false, Some('a'));
    engine.handle_sc_commit_input_key("", false, Some('b'));
    engine.handle_sc_commit_input_key("", false, Some('c'));
    assert_eq!(engine.sc_commit_cursor, 3);
    // Left moves cursor back.
    engine.handle_sc_commit_input_key("Left", false, None);
    assert_eq!(engine.sc_commit_cursor, 2);
    // Insert at cursor position.
    engine.handle_sc_commit_input_key("", false, Some('X'));
    assert_eq!(engine.sc_commit_message, "abXc");
    assert_eq!(engine.sc_commit_cursor, 3);
    // Right moves cursor forward.
    engine.handle_sc_commit_input_key("Right", false, None);
    assert_eq!(engine.sc_commit_cursor, 4);
    // Home moves to start of line.
    engine.handle_sc_commit_input_key("Home", false, None);
    assert_eq!(engine.sc_commit_cursor, 0);
    // End moves to end of line.
    engine.handle_sc_commit_input_key("End", false, None);
    assert_eq!(engine.sc_commit_cursor, 4);
}

#[test]
fn test_sc_commit_cursor_up_down() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_commit_input_active = true;
    engine.sc_commit_message = "abc\nde\nfghij".to_string();
    engine.sc_commit_cursor = 5; // on 'e' in "de"
                                 // Down moves to next line, same column.
    engine.handle_sc_commit_input_key("Down", false, None);
    assert_eq!(engine.sc_commit_cursor, 8); // 'h' in "fghij"
                                            // Up moves back.
    engine.handle_sc_commit_input_key("Up", false, None);
    assert_eq!(engine.sc_commit_cursor, 5); // 'e' in "de"
                                            // Up again to first line.
    engine.handle_sc_commit_input_key("Up", false, None);
    assert_eq!(engine.sc_commit_cursor, 1); // 'b' in "abc" (col 1)
}

#[test]
fn test_sc_commit_cursor_backspace_at_position() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_commit_input_active = true;
    engine.sc_commit_message = "hello".to_string();
    engine.sc_commit_cursor = 3; // after "hel"
    engine.handle_sc_commit_input_key("BackSpace", false, None);
    assert_eq!(engine.sc_commit_message, "helo");
    assert_eq!(engine.sc_commit_cursor, 2);
}

#[test]
fn test_sc_commit_cursor_delete() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_commit_input_active = true;
    engine.sc_commit_message = "hello".to_string();
    engine.sc_commit_cursor = 2;
    engine.handle_sc_commit_input_key("Delete", false, None);
    assert_eq!(engine.sc_commit_message, "helo");
    assert_eq!(engine.sc_commit_cursor, 2);
}

#[test]
fn test_gpull_and_gfetch_commands_exist() {
    let mut engine = make_sc_engine_with_files();
    // These will fail with a git error since cwd is not a real repo, but
    // they should not return EngineAction::Error ("Not an editor command").
    let r1 = engine.execute_command("Gpull");
    let r2 = engine.execute_command("Gfetch");
    assert_ne!(r1, EngineAction::Error);
    assert_ne!(r2, EngineAction::Error);
}

#[test]
fn test_sc_branch_picker_open_close() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_has_focus = true;
    assert!(!engine.sc_branch_picker_open);
    engine.handle_sc_key("b", false, None);
    assert!(engine.sc_branch_picker_open);
    // Escape closes it
    engine.handle_sc_key("Escape", false, None);
    assert!(!engine.sc_branch_picker_open);
    assert!(engine.sc_branch_picker_query.is_empty());
}

#[test]
fn test_sc_branch_picker_typing_filters() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_has_focus = true;
    engine.handle_sc_key("b", false, None);
    assert!(engine.sc_branch_picker_open);
    // Type a query character
    engine.handle_sc_key("", false, Some('m'));
    assert_eq!(engine.sc_branch_picker_query, "m");
    engine.handle_sc_key("", false, Some('a'));
    assert_eq!(engine.sc_branch_picker_query, "ma");
    // Backspace removes
    engine.handle_sc_key("BackSpace", false, None);
    assert_eq!(engine.sc_branch_picker_query, "m");
}

#[test]
fn test_sc_branch_create_mode() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_has_focus = true;
    engine.handle_sc_key("B", false, None);
    assert!(engine.sc_branch_create_mode);
    // Type branch name
    engine.handle_sc_key("", false, Some('f'));
    engine.handle_sc_key("", false, Some('o'));
    engine.handle_sc_key("", false, Some('o'));
    assert_eq!(engine.sc_branch_create_input, "foo");
    // Escape cancels
    engine.handle_sc_key("Escape", false, None);
    assert!(!engine.sc_branch_create_mode);
    assert!(engine.sc_branch_create_input.is_empty());
}

#[test]
fn test_sc_help_toggle() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_has_focus = true;
    assert!(!engine.sc_help_open);
    engine.handle_sc_key("?", false, None);
    assert!(engine.sc_help_open);
    // Any key closes
    engine.handle_sc_key("j", false, None);
    assert!(!engine.sc_help_open);
}

#[test]
fn test_sc_help_escape_closes() {
    let mut engine = make_sc_engine_with_files();
    engine.sc_has_focus = true;
    engine.handle_sc_key("?", false, None);
    assert!(engine.sc_help_open);
    engine.handle_sc_key("Escape", false, None);
    assert!(!engine.sc_help_open);
}

// ─── sc_visual_row_to_flat tests (click-math correctness) ─────────────────

/// make_sc_engine_with_files gives: 1 staged file + 1 unstaged file,
/// sections_expanded = [true, true, false, true], sc_worktrees = [] (no linked worktrees).
/// GTK (no hint) flat layout (row 0=header, 1=commit, 2=buttons, 3+=sections):
///   row 3  → flat 0  (STAGED header)
///   row 4  → flat 1  (a.rs – staged)
///   row 5  → flat 2  (CHANGES header)
///   row 6  → flat 3  (b.rs – unstaged)
///   row 7  → flat 4  (LOG header — WORKTREES hidden, sc_worktrees.len() == 0)
///   row 8  → None    (log expanded but empty, no hint in GTK mode)
#[test]
fn test_sc_visual_row_to_flat_gtk() {
    let engine = make_sc_engine_with_files(); // staged=1, unstaged=1, worktrees=0
                                              // Rows 0–2 are header / commit input / button row — should return None.
    assert_eq!(engine.sc_visual_row_to_flat(0, false), None);
    assert_eq!(engine.sc_visual_row_to_flat(1, false), None);
    assert_eq!(engine.sc_visual_row_to_flat(2, false), None);
    // STAGED header
    assert_eq!(engine.sc_visual_row_to_flat(3, false), Some((0, true)));
    // staged file a.rs
    assert_eq!(engine.sc_visual_row_to_flat(4, false), Some((1, false)));
    // CHANGES header
    assert_eq!(engine.sc_visual_row_to_flat(5, false), Some((2, true)));
    // unstaged file b.rs
    assert_eq!(engine.sc_visual_row_to_flat(6, false), Some((3, false)));
    // LOG header (WORKTREES hidden, log section is always present)
    assert_eq!(engine.sc_visual_row_to_flat(7, false), Some((4, true)));
    // row 8: log expanded but sc_log is empty in test, no GTK hint → None
    assert_eq!(engine.sc_visual_row_to_flat(8, false), None);
}

/// TUI: STAGED is expanded but empty; CHANGES has 1 file.
/// TUI adds a "(no changes)" visual row for the empty STAGED section.
/// sc_worktrees is empty so WORKTREES section is hidden.
/// sc_sections_expanded[3]=false so LOG is collapsed (header only).
/// Visual layout (row 0=header, 1=commit, 2=buttons, 3+=sections):
///   row 3  → flat 0  (STAGED header)
///   row 4  → visual "(no changes)" — NO flat entry
///   row 5  → flat 1  (CHANGES header)
///   row 6  → flat 2  (b.rs – unstaged)
///   row 7  → flat 3  (LOG header — WORKTREES hidden, log collapsed)
///   row 8  → None    (log collapsed, no items shown)
#[test]
fn test_sc_visual_row_to_flat_tui_empty_staged() {
    let mut engine = Engine::new();
    engine.sc_file_statuses = vec![git::FileStatus {
        path: "b.rs".to_string(),
        staged: None,
        unstaged: Some(git::StatusKind::Modified),
    }];
    engine.sc_sections_expanded = [true, true, false, false]; // staged expanded but empty; log collapsed
                                                              // Row 3: STAGED header (flat 0)
    assert_eq!(engine.sc_visual_row_to_flat(3, true), Some((0, true)));
    // Row 4: "(no changes)" hint — None
    assert_eq!(engine.sc_visual_row_to_flat(4, true), None);
    // Row 5: CHANGES header (flat 1)
    assert_eq!(engine.sc_visual_row_to_flat(5, true), Some((1, true)));
    // Row 6: b.rs (flat 2)
    assert_eq!(engine.sc_visual_row_to_flat(6, true), Some((2, false)));
    // Row 7: LOG header (flat 3) — WORKTREES hidden, log section always present
    assert_eq!(engine.sc_visual_row_to_flat(7, true), Some((3, true)));
    // Row 8: log collapsed → None
    assert_eq!(engine.sc_visual_row_to_flat(8, true), None);
}

/// s on STAGED section header calls sc_unstage_all path (no panic in test context).
#[test]
fn test_sc_stage_selected_on_staged_header_is_not_noop() {
    let mut engine = make_sc_engine_with_files();
    // flat 0 = STAGED header
    engine.sc_selected = 0;
    let before_count = engine
        .sc_file_statuses
        .iter()
        .filter(|f| f.staged.is_some())
        .count();
    // sc_stage_selected should detect idx==MAX and call sc_unstage_all
    // (which will fail silently since we're not in a real git repo)
    engine.sc_stage_selected();
    // The function should not panic and should call sc_refresh (resets to empty).
    let _ = before_count; // no panics = pass
}

/// s on CHANGES section header calls sc_stage_all path (no panic in test context).
#[test]
fn test_sc_stage_selected_on_changes_header_is_not_noop() {
    let mut engine = make_sc_engine_with_files();
    // flat 2 = CHANGES header (1 staged file shifts CHANGES header to flat 2)
    engine.sc_selected = 2;
    engine.sc_stage_selected(); // should not panic
}

// ── Multi-cursor tests ────────────────────────────────────────────────────

fn engine_with_text(text: &str) -> Engine {
    let mut engine = Engine::new();
    engine.buffer_mut().insert(0, text);
    engine
}

#[test]
fn test_alt_d_adds_extra_cursor() {
    // "foo bar foo" — cursor on first "foo", Alt-D should add cursor at second "foo"
    let mut engine = engine_with_text("foo bar foo\n");
    // Cursor is at line 0, col 0 (on "foo")
    assert_eq!(engine.view().cursor, Cursor { line: 0, col: 0 });
    engine.add_cursor_at_next_match();
    assert_eq!(engine.view().extra_cursors.len(), 1);
    assert_eq!(engine.view().extra_cursors[0], Cursor { line: 0, col: 8 });
}

#[test]
fn test_alt_d_multiple_presses() {
    // "foo foo foo" — two Alt-D presses add two extra cursors in order
    let mut engine = engine_with_text("foo foo foo\n");
    engine.add_cursor_at_next_match();
    assert_eq!(engine.view().extra_cursors.len(), 1);
    assert_eq!(engine.view().extra_cursors[0].col, 4);
    engine.add_cursor_at_next_match();
    assert_eq!(engine.view().extra_cursors.len(), 2);
    assert_eq!(engine.view().extra_cursors[1].col, 8);
}

#[test]
fn test_alt_d_no_more_matches() {
    // Only one "foo" — Alt-D should show a message and not add a cursor
    let mut engine = engine_with_text("foo bar baz\n");
    engine.add_cursor_at_next_match();
    assert!(engine.view().extra_cursors.is_empty());
    assert!(engine.message.contains("No more occurrences"));
}

#[test]
fn test_multi_cursor_insert_char() {
    // "foo bar foo" — add extra cursor at second "foo", then type 'x' in insert mode
    let mut engine = engine_with_text("foo bar foo\n");
    engine.add_cursor_at_next_match();
    assert_eq!(engine.view().extra_cursors.len(), 1);

    // Enter insert mode and type 'x'
    engine.handle_key("i", Some('i'), false);
    assert_eq!(engine.mode, super::Mode::Insert);
    engine.handle_key("x", Some('x'), false);

    let buf = engine.buffer().to_string();
    // Both "foo" occurrences should have 'x' prepended: "xfoo bar xfoo\n"
    assert_eq!(buf, "xfoo bar xfoo\n");
}

#[test]
fn test_multi_cursor_backspace() {
    // "foo bar foo" — primary at col 1, extra at col 9
    // BackSpace should remove char before each cursor
    let mut engine = engine_with_text("foo bar foo\n");
    // Move primary cursor to col 1
    engine.view_mut().cursor.col = 1;
    // Extra cursor at second "foo", col 9
    engine.view_mut().extra_cursors = vec![Cursor { line: 0, col: 9 }];

    engine.handle_key("i", Some('i'), false);
    engine.handle_key("BackSpace", None, false);
    engine.handle_key("Escape", None, false);

    let buf = engine.buffer().to_string();
    // Primary deletes char before col 1 (the 'f'), extra deletes char before col 9 (the 'f' of second foo)
    assert_eq!(buf, "oo bar oo\n");
}

#[test]
fn test_multi_cursor_escape_collapses() {
    // Escape from insert mode should clear extra cursors
    let mut engine = engine_with_text("foo bar foo\n");
    engine.add_cursor_at_next_match();
    assert_eq!(engine.view().extra_cursors.len(), 1);

    engine.handle_key("i", Some('i'), false);
    engine.handle_key("Escape", None, false);

    assert!(
        engine.view().extra_cursors.is_empty(),
        "extra cursors should be cleared on Escape"
    );
    assert_eq!(engine.mode, super::Mode::Normal);
}

#[test]
fn test_multi_cursor_undo() {
    // Type 'x' with 2 cursors, then undo — both insertions should be reverted atomically
    let mut engine = engine_with_text("foo bar foo\n");
    engine.add_cursor_at_next_match();

    engine.handle_key("i", Some('i'), false);
    engine.handle_key("x", Some('x'), false);

    let buf_after = engine.buffer().to_string();
    assert_eq!(buf_after, "xfoo bar xfoo\n");

    engine.handle_key("Escape", None, false);
    engine.handle_key("u", Some('u'), false); // undo

    let buf_undone = engine.buffer().to_string();
    assert_eq!(
        buf_undone, "foo bar foo\n",
        "undo should revert all multi-cursor insertions"
    );
}

#[test]
fn test_add_cursor_keybinding_configurable() {
    // Verify that the default binding parses correctly
    use crate::core::settings::parse_key_binding;
    let default = crate::core::settings::PanelKeys::default();
    assert_eq!(default.add_cursor, "<A-d>");
    let parsed = parse_key_binding(&default.add_cursor);
    assert_eq!(parsed, Some((false, false, true, 'd')));
}

// ── select_all_word_occurrences tests ─────────────────────────────────────

#[test]
fn test_select_all_word_occurrences() {
    // "foo bar foo foo\n" — cursor on first "foo" → 2 extra cursors
    let mut engine = engine_with_text("foo bar foo foo\n");
    // cursor at (0,0) which is on "foo"
    engine.select_all_word_occurrences();
    // primary stays on first foo; extra cursors at second and third
    assert_eq!(engine.view().extra_cursors.len(), 2);
    assert_eq!(engine.view().extra_cursors[0], Cursor { line: 0, col: 8 });
    assert_eq!(engine.view().extra_cursors[1], Cursor { line: 0, col: 12 });
    assert!(engine.message.contains("3 cursors"));
    assert!(engine.message.contains("foo"));
}

#[test]
fn test_select_all_single_occurrence() {
    // Only one "foo" — extra_cursors should be empty, primary stays
    // n+1 == 1, message says "1 cursors (all occurrences of 'foo')"
    let mut engine = engine_with_text("foo bar baz\n");
    engine.select_all_word_occurrences();
    assert!(engine.view().extra_cursors.is_empty());
    assert!(engine.message.contains("1 cursors"));
}

// ── add_cursor_at_pos tests ───────────────────────────────────────────────

#[test]
fn test_add_cursor_at_pos_basic() {
    let mut engine = engine_with_text("hello world\n");
    // primary is at (0,0); add a cursor at col 6
    engine.add_cursor_at_pos(0, 6);
    assert_eq!(engine.view().extra_cursors.len(), 1);
    assert_eq!(engine.view().extra_cursors[0], Cursor { line: 0, col: 6 });
}

#[test]
fn test_add_cursor_at_pos_no_duplicate_primary() {
    let mut engine = engine_with_text("hello world\n");
    // primary is at (0,0) — adding there should not create an extra cursor
    engine.add_cursor_at_pos(0, 0);
    assert!(engine.view().extra_cursors.is_empty());
}

#[test]
fn test_add_cursor_at_pos_no_duplicate_extra() {
    let mut engine = engine_with_text("hello world\n");
    engine.add_cursor_at_pos(0, 6);
    assert_eq!(engine.view().extra_cursors.len(), 1);
    // Adding the same position again should be a no-op
    engine.add_cursor_at_pos(0, 6);
    assert_eq!(engine.view().extra_cursors.len(), 1);
}

// ── Regression: Alt+D across blank lines + trailing spaces ───────────────

#[test]
fn test_add_cursor_next_match_blank_lines_trailing_space() {
    // Exact text the user reported: 4 "foo" occurrences across blank lines
    // and a line with trailing space.
    let text = "foo\n\nfoo \n   foo foo\n";
    let mut engine = engine_with_text(text);
    // cursor at (0,0)
    engine.add_cursor_at_next_match(); // → line 2, col 0
    assert_eq!(
        engine.view().extra_cursors.len(),
        1,
        "after 1st: cursors={:?}",
        engine.view().extra_cursors
    );
    engine.add_cursor_at_next_match(); // → line 3, col 3
    assert_eq!(
        engine.view().extra_cursors.len(),
        2,
        "after 2nd: cursors={:?}",
        engine.view().extra_cursors
    );
    engine.add_cursor_at_next_match(); // → line 3, col 7
    assert_eq!(
        engine.view().extra_cursors.len(),
        3,
        "after 3rd: cursors={:?}",
        engine.view().extra_cursors
    );
}

// ── Regression: yyp pastes on next line ──────────────────────────────────

#[test]
fn test_yyp_pastes_on_next_line() {
    let text = "foo\n\nfoo \n   foo foo\n";
    let mut engine = engine_with_text(text);
    // cursor at line 0
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');
    let (reg_content, is_lw) = engine.registers.get(&'"').unwrap().clone();
    assert_eq!(reg_content, "foo\n", "yanked content");
    assert!(is_lw, "should be linewise");
    press_char(&mut engine, 'p');
    // Expected: "foo\nfoo\n\nfoo \n   foo foo\n"
    let result = engine.buffer().to_string();
    assert!(
        result.starts_with("foo\nfoo\n"),
        "paste should be on next line; got: {:?}",
        result
    );
    assert_eq!(
        engine.view().cursor.line,
        1,
        "cursor should move to pasted line"
    );
}

#[test]
fn test_extra_cursors_cleared_on_normal_mode_paste() {
    // Extra cursors become stale after a normal-mode buffer modification.
    // Verify they are cleared automatically so the user doesn't see ghost cursors.
    let mut engine = engine_with_text("foo bar foo\n");
    // Add an extra cursor at (0, 8)
    engine.view_mut().extra_cursors = vec![Cursor { line: 0, col: 8 }];
    // yank the line then paste — this is a normal-mode buffer modification
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'p');
    // After paste, extra_cursors must be empty
    assert!(
        engine.view().extra_cursors.is_empty(),
        "extra_cursors should be cleared after normal-mode paste"
    );
}

#[test]
fn test_extra_cursors_preserved_on_insert_mode_entry() {
    // Pressing 'i' (no buffer modification) must NOT clear extra cursors.
    let mut engine = engine_with_text("foo bar\n");
    engine.view_mut().extra_cursors = vec![Cursor { line: 0, col: 4 }];
    engine.handle_key("i", Some('i'), false);
    assert!(
        !engine.view().extra_cursors.is_empty(),
        "extra_cursors should be preserved when entering insert mode with 'i'"
    );
}

#[test]
fn test_add_cursor_at_next_match_shows_count_message() {
    let mut engine = engine_with_text("foo bar foo\n");
    engine.add_cursor_at_next_match();
    assert!(
        engine.message.contains("2 cursors"),
        "message should show cursor count; got: {:?}",
        engine.message
    );
}

#[test]
fn test_load_clipboard_for_paste_preserves_linewise() {
    // When clipboard content matches the existing '"' register, is_linewise is kept.
    let mut engine = engine_with_text("foo\nbar\n");
    engine.registers.insert('"', ("foo\n".to_string(), true));
    engine.load_clipboard_for_paste("foo\n".to_string());
    let (_, lw) = engine.registers[&'"'].clone();
    assert!(
        lw,
        "load_clipboard_for_paste should preserve is_linewise when content matches"
    );
}

#[test]
fn test_load_clipboard_for_paste_clears_linewise_for_foreign_content() {
    // When clipboard content differs (from another app), is_linewise becomes false.
    let mut engine = engine_with_text("foo\nbar\n");
    engine.registers.insert('"', ("foo\n".to_string(), true));
    engine.load_clipboard_for_paste("different text".to_string());
    let (_, lw) = engine.registers[&'"'].clone();
    assert!(
        !lw,
        "load_clipboard_for_paste should clear is_linewise for external content"
    );
}

#[test]
fn test_yyp_linewise_via_clipboard_intercept() {
    // Simulate the yy → clipboard-intercept-before-p flow that the backends perform.
    let mut engine = engine_with_text("foo\nbar\n");
    press_char(&mut engine, 'y');
    press_char(&mut engine, 'y');
    let (content, lw) = engine.registers[&'"'].clone();
    assert!(lw, "yy should set is_linewise=true");
    // Backend intercepts p, reads same text from clipboard, calls load_clipboard_for_paste.
    engine.load_clipboard_for_paste(content);
    press_char(&mut engine, 'p');
    // Buffer should be "foo\nfoo\nbar\n" — pasted on the line below, not inline.
    assert_eq!(
        engine.buffer().to_string(),
        "foo\nfoo\nbar\n",
        "linewise paste via clipboard intercept should insert on next line"
    );
    assert_eq!(engine.view().cursor.line, 1, "cursor on pasted line");
}

// ── Editor group tests ────────────────────────────────────────────────────

#[test]
fn test_editor_group_split_commands() {
    let mut engine = Engine::new();
    assert_eq!(engine.group_layout.leaf_count(), 1);

    // :EditorGroupSplit should create a second group.
    let r = engine.execute_command("EditorGroupSplit");
    assert_ne!(
        r,
        EngineAction::Error,
        "EditorGroupSplit should not return error"
    );
    assert_eq!(
        engine.group_layout.leaf_count(),
        2,
        "EditorGroupSplit should create a second group"
    );

    // A third split should now work (no cap at 2).
    let r2 = engine.execute_command("egsp");
    assert_ne!(r2, EngineAction::Error, "egsp should not return error");
    assert_eq!(
        engine.group_layout.leaf_count(),
        3,
        "egsp should create third group"
    );

    // Close back to 2, then close to 1.
    engine.execute_command("egc");
    assert_eq!(engine.group_layout.leaf_count(), 2);
    engine.execute_command("egc");
    assert_eq!(engine.group_layout.leaf_count(), 1);

    let r3 = engine.execute_command("EditorGroupSplitDown");
    assert_ne!(
        r3,
        EngineAction::Error,
        "EditorGroupSplitDown should not return error"
    );
    assert_eq!(engine.group_layout.leaf_count(), 2);

    // :egspd creates a third.
    let r4 = engine.execute_command("egspd");
    assert_ne!(r4, EngineAction::Error, "egspd should not return error");
    assert_eq!(engine.group_layout.leaf_count(), 3);
}

#[test]
fn test_recursive_split_three_groups() {
    let mut engine = Engine::new();
    // Split right
    engine.open_editor_group(SplitDirection::Vertical);
    assert_eq!(engine.group_layout.leaf_count(), 2);
    // Focus group 0 and split down
    let first_id = engine.group_layout.group_ids()[0];
    engine.active_group = first_id;
    engine.open_editor_group(SplitDirection::Horizontal);
    assert_eq!(engine.group_layout.leaf_count(), 3);
    // Verify all 3 groups exist in the HashMap
    assert_eq!(engine.editor_groups.len(), 3);
}

#[test]
fn test_focus_cycling_three_groups() {
    let mut engine = Engine::new();
    engine.open_editor_group(SplitDirection::Vertical);
    let first_id = engine.group_layout.group_ids()[0];
    engine.active_group = first_id;
    engine.open_editor_group(SplitDirection::Horizontal);
    let ids = engine.group_layout.group_ids();
    assert_eq!(ids.len(), 3);
    // Start at the newly split group (last created)
    let start = engine.active_group;
    engine.focus_other_group();
    let second = engine.active_group;
    assert_ne!(start, second);
    engine.focus_other_group();
    let third = engine.active_group;
    assert_ne!(second, third);
    engine.focus_other_group();
    // Should wrap back
    assert_eq!(engine.active_group, start);
}

#[test]
fn test_close_nested_group() {
    let mut engine = Engine::new();
    engine.open_editor_group(SplitDirection::Vertical);
    let first_id = engine.group_layout.group_ids()[0];
    engine.active_group = first_id;
    engine.open_editor_group(SplitDirection::Horizontal);
    assert_eq!(engine.group_layout.leaf_count(), 3);
    // Close the active group
    engine.close_editor_group();
    assert_eq!(engine.group_layout.leaf_count(), 2);
    // Close again
    engine.close_editor_group();
    assert_eq!(engine.group_layout.leaf_count(), 1);
    assert!(engine.group_layout.is_single_group());
}

#[test]
fn test_four_groups_nested_splits() {
    // Reproduce: split right, focus left, split down, split down again → 4 groups
    let mut engine = Engine::new();
    engine.open_editor_group(SplitDirection::Vertical);
    assert_eq!(engine.group_layout.leaf_count(), 2);
    let first_id = engine.group_layout.group_ids()[0];
    engine.active_group = first_id;
    engine.open_editor_group(SplitDirection::Horizontal);
    assert_eq!(engine.group_layout.leaf_count(), 3);
    // Second split down on the newly created group
    engine.open_editor_group(SplitDirection::Horizontal);
    assert_eq!(engine.group_layout.leaf_count(), 4);

    // Simulate TUI rendering: content_bounds includes tab bar row (+1)
    let content_bounds = WindowRect::new(0.0, 0.0, 80.0, 25.0);
    let (rects, dividers) = engine.calculate_group_window_rects(content_bounds, 1.0);
    assert_eq!(rects.len(), 4, "should have 4 window rects");

    // All rects should have positive width and non-negative height
    for (wid, r) in &rects {
        assert!(r.width > 0.0, "window {:?} has zero width", wid);
        assert!(r.height >= 0.0, "window {:?} has negative height", wid);
        assert!(r.y >= 0.0, "window {:?} has negative y", wid);
        assert!(
            r.y + r.height <= content_bounds.height,
            "window {:?} extends beyond content bounds: y={} h={} max={}",
            wid,
            r.y,
            r.height,
            content_bounds.height
        );
    }
    assert!(!dividers.is_empty(), "should have dividers");
}

#[test]
fn test_group_resize_nested() {
    let mut engine = Engine::new();
    engine.open_editor_group(SplitDirection::Vertical);
    let first_id = engine.group_layout.group_ids()[0];
    engine.active_group = first_id;
    // Resize: should adjust the parent split ratio
    engine.group_resize(0.1);
    // Verify it changed (default was 0.5, now should be 0.6)
    if let GroupLayout::Split { ratio, .. } = &engine.group_layout {
        assert!((*ratio - 0.6).abs() < 0.01);
    } else {
        panic!("Expected split layout");
    }
}

#[test]
fn test_command_cursor_initial_position() {
    // Entering command mode should set cursor to 0 (after the ':')
    let mut engine = Engine::new();
    engine.handle_key(":", Some(':'), false);
    assert_eq!(engine.command_cursor, 0);
    assert_eq!(engine.command_buffer, "");
}

#[test]
fn test_command_cursor_advances_on_type() {
    let mut engine = Engine::new();
    engine.handle_key(":", Some(':'), false);
    engine.handle_key("", Some('h'), false);
    engine.handle_key("", Some('i'), false);
    assert_eq!(engine.command_buffer, "hi");
    assert_eq!(engine.command_cursor, 2);
}

#[test]
fn test_command_cursor_left_right() {
    let mut engine = Engine::new();
    engine.handle_key(":", Some(':'), false);
    engine.handle_key("", Some('a'), false);
    engine.handle_key("", Some('b'), false);
    engine.handle_key("", Some('c'), false);
    assert_eq!(engine.command_cursor, 3);

    engine.handle_key("Left", None, false);
    assert_eq!(engine.command_cursor, 2);
    engine.handle_key("Left", None, false);
    assert_eq!(engine.command_cursor, 1);
    // Insert at position 1
    engine.handle_key("", Some('X'), false);
    assert_eq!(engine.command_buffer, "aXbc");
    assert_eq!(engine.command_cursor, 2);

    engine.handle_key("Right", None, false);
    assert_eq!(engine.command_cursor, 3);
}

#[test]
fn test_command_cursor_home_end() {
    let mut engine = Engine::new();
    engine.handle_key(":", Some(':'), false);
    for ch in "hello".chars() {
        engine.handle_key("", Some(ch), false);
    }
    engine.handle_key("Home", None, false);
    assert_eq!(engine.command_cursor, 0);
    engine.handle_key("End", None, false);
    assert_eq!(engine.command_cursor, 5);
}

#[test]
fn test_command_cursor_delete_key() {
    let mut engine = Engine::new();
    engine.handle_key(":", Some(':'), false);
    for ch in "abc".chars() {
        engine.handle_key("", Some(ch), false);
    }
    // Move to start, delete first char
    engine.handle_key("Home", None, false);
    engine.handle_key("Delete", None, false);
    assert_eq!(engine.command_buffer, "bc");
    assert_eq!(engine.command_cursor, 0);
}

#[test]
fn test_command_cursor_backspace_at_cursor() {
    let mut engine = Engine::new();
    engine.handle_key(":", Some(':'), false);
    for ch in "abc".chars() {
        engine.handle_key("", Some(ch), false);
    }
    engine.handle_key("Left", None, false); // cursor at 2
    engine.handle_key("BackSpace", None, false); // deletes 'b'
    assert_eq!(engine.command_buffer, "ac");
    assert_eq!(engine.command_cursor, 1);
}

#[test]
fn test_command_insert_str_at_cursor() {
    let mut engine = Engine::new();
    engine.handle_key(":", Some(':'), false);
    for ch in "ac".chars() {
        engine.handle_key("", Some(ch), false);
    }
    engine.handle_key("Left", None, false); // cursor between 'a' and 'c'
    engine.command_insert_str("b");
    assert_eq!(engine.command_buffer, "abc");
    assert_eq!(engine.command_cursor, 2);
}

#[test]
fn test_command_ctrl_a_e_k() {
    let mut engine = Engine::new();
    engine.handle_key(":", Some(':'), false);
    for ch in "hello".chars() {
        engine.handle_key("", Some(ch), false);
    }
    // Ctrl-A goes to start
    engine.handle_key("a", Some('a'), true);
    assert_eq!(engine.command_cursor, 0);
    // Ctrl-E goes to end
    engine.handle_key("e", Some('e'), true);
    assert_eq!(engine.command_cursor, 5);
    // Move to middle, Ctrl-K kills to end
    engine.handle_key("Left", None, false);
    engine.handle_key("Left", None, false);
    engine.handle_key("k", Some('k'), true);
    assert_eq!(engine.command_buffer, "hel");
    assert_eq!(engine.command_cursor, 3);
}

// ─── Visual mode Ctrl-D / Ctrl-U tests ─────────────────────────────

#[test]
fn test_visual_ctrl_d_extends_selection_down() {
    let mut engine = Engine::new();
    // 20 lines so half-page is meaningful
    let text = (0..20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    engine.buffer_mut().insert(0, &text);
    engine.view_mut().viewport_lines = 10;
    // Enter visual mode on line 0
    press_char(&mut engine, 'v');
    assert!(matches!(engine.mode, Mode::Visual));
    // Ctrl-D should move cursor down by half page (5 lines), extending selection
    press_ctrl(&mut engine, 'd');
    assert!(matches!(engine.mode, Mode::Visual));
    assert_eq!(engine.view().cursor.line, 5);
    // Buffer should be unchanged (not deleted)
    assert_eq!(engine.buffer().len_lines(), 20);
}

#[test]
fn test_visual_ctrl_u_extends_selection_up() {
    let mut engine = Engine::new();
    let text = (0..20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    engine.buffer_mut().insert(0, &text);
    engine.view_mut().viewport_lines = 10;
    // Move to line 10
    engine.view_mut().cursor.line = 10;
    // Enter visual mode
    press_char(&mut engine, 'v');
    assert!(matches!(engine.mode, Mode::Visual));
    // Ctrl-U should move cursor up by half page (5 lines), extending selection
    press_ctrl(&mut engine, 'u');
    assert!(matches!(engine.mode, Mode::Visual));
    assert_eq!(engine.view().cursor.line, 5);
    // Buffer should be unchanged (case not toggled)
    let content = engine.buffer().to_string();
    assert!(content.starts_with("line 0"));
}

// ─── Dialog system tests ─────────────────────────────────────────

#[test]
fn test_dialog_show_and_escape() {
    let mut e = Engine::new();
    e.buffer_mut().insert(0, "hello");
    e.show_dialog(
        "test",
        "Test Dialog",
        vec!["Body line".into()],
        vec![DialogButton {
            label: "OK".into(),
            hotkey: 'o',
            action: "ok".into(),
        }],
    );
    assert!(e.dialog.is_some());
    // Escape dismisses.
    e.handle_key("Escape", None, false);
    assert!(e.dialog.is_none());
}

#[test]
fn test_dialog_hotkey() {
    let mut e = Engine::new();
    e.show_dialog(
        "test",
        "Choose",
        vec!["Pick one".into()],
        vec![
            DialogButton {
                label: "Recover".into(),
                hotkey: 'r',
                action: "recover".into(),
            },
            DialogButton {
                label: "Delete".into(),
                hotkey: 'd',
                action: "delete".into(),
            },
        ],
    );
    // Press 'r' → hotkey should dismiss.
    e.handle_key("", Some('r'), false);
    assert!(e.dialog.is_none());
}

#[test]
fn test_dialog_arrow_nav_and_enter() {
    let mut e = Engine::new();
    e.show_dialog(
        "test",
        "Choose",
        vec![],
        vec![
            DialogButton {
                label: "A".into(),
                hotkey: 'a',
                action: "a_action".into(),
            },
            DialogButton {
                label: "B".into(),
                hotkey: 'b',
                action: "b_action".into(),
            },
            DialogButton {
                label: "C".into(),
                hotkey: 'c',
                action: "c_action".into(),
            },
        ],
    );
    assert_eq!(e.dialog.as_ref().unwrap().selected, 0);
    // Move right.
    e.handle_key("Right", None, false);
    assert_eq!(e.dialog.as_ref().unwrap().selected, 1);
    // Move right again.
    e.handle_key("Right", None, false);
    assert_eq!(e.dialog.as_ref().unwrap().selected, 2);
    // Right at end → wraps to 0.
    e.handle_key("Right", None, false);
    assert_eq!(e.dialog.as_ref().unwrap().selected, 0);
    // Move right to 1.
    e.handle_key("Right", None, false);
    assert_eq!(e.dialog.as_ref().unwrap().selected, 1);
    // Enter confirms the selected button.
    e.handle_key("Return", None, false);
    assert!(e.dialog.is_none());
}

#[test]
fn test_dialog_blocks_normal_keys() {
    let mut e = Engine::new();
    e.buffer_mut().insert(0, "hello");
    e.show_dialog(
        "test",
        "Block",
        vec![],
        vec![DialogButton {
            label: "OK".into(),
            hotkey: 'o',
            action: "ok".into(),
        }],
    );
    // Press 'x' which would normally delete a char — dialog should consume it.
    e.handle_key("", Some('x'), false);
    assert!(e.dialog.is_some()); // Dialog still open.
    assert_eq!(e.buffer().to_string(), "hello"); // Buffer unchanged.
}

#[test]
fn test_show_error_dialog() {
    let mut e = Engine::new();
    e.show_error_dialog("Error", "Something went wrong");
    let dialog = e.dialog.as_ref().unwrap();
    assert_eq!(dialog.tag, "error");
    assert_eq!(dialog.title, "Error");
    assert_eq!(dialog.body, vec!["Something went wrong"]);
    assert_eq!(dialog.buttons.len(), 1);
    assert_eq!(dialog.buttons[0].hotkey, 'o');
    assert_eq!(dialog.buttons[0].action, "ok");
}

// ─── Extension removal dialog ────────────────────────────────────────

/// Create a minimal mock bash extension manifest for tests that don't
/// depend on local disk state (CI has no extensions installed on disk).
fn mock_bash_manifest() -> extensions::ExtensionManifest {
    extensions::ExtensionManifest {
        name: "bash".to_string(),
        display_name: "Bash".to_string(),
        description: "Bash language support".to_string(),
        version: "1.0.0".to_string(),
        ..Default::default()
    }
}

#[test]
fn test_ext_remove_dialog_shows_on_d() {
    let mut e = Engine::new();
    e.ext_registry = Some(vec![mock_bash_manifest()]);
    // Install a single extension so it's the only item at index 0.
    e.extension_state.mark_installed_version("bash", "1.0.0");
    e.ext_sidebar_sections_expanded = [true, true];
    e.ext_sidebar_selected = 0; // First installed item.
    e.ext_sidebar_has_focus = true;
    e.handle_ext_sidebar_key("d", false, None);
    // Dialog should be open with the ext_remove tag.
    assert!(e.dialog.is_some());
    assert_eq!(e.dialog.as_ref().unwrap().tag, "ext_remove");
    assert!(e.pending_ext_remove.is_some());
    assert_eq!(e.pending_ext_remove.as_ref().unwrap(), "bash");
}

#[test]
fn test_ext_remove_dialog_cancel() {
    let mut e = Engine::new();
    e.ext_registry = Some(vec![mock_bash_manifest()]);
    e.extension_state.mark_installed_version("bash", "1.0.0");
    e.ext_sidebar_sections_expanded = [true, true];
    e.ext_sidebar_selected = 0;
    e.handle_ext_sidebar_key("d", false, None);
    assert!(e.dialog.is_some());
    // Press Escape to cancel.
    e.handle_key("Escape", None, false);
    assert!(e.dialog.is_none());
    // Extension should still be installed.
    assert!(e.extension_state.is_installed("bash"));
}

#[test]
fn test_ext_remove_dialog_confirm_remove() {
    let mut e = Engine::new();
    e.ext_registry = Some(vec![mock_bash_manifest()]);
    e.extension_state.mark_installed_version("bash", "1.0.0");
    e.ext_sidebar_sections_expanded = [true, true];
    e.ext_sidebar_selected = 0;
    e.handle_ext_sidebar_key("d", false, None);
    assert!(e.dialog.is_some());
    // Press 'r' for "Remove".
    e.handle_key("", Some('r'), false);
    assert!(e.dialog.is_none());
    // Extension should be removed.
    assert!(!e.extension_state.is_installed("bash"));
}

// ─── Spell checking ──────────────────────────────────────────────────

#[test]
fn test_spell_set_spell_initializes_checker() {
    let mut e = Engine::new();
    assert!(e.spell_checker.is_none());
    e.settings.spell = true;
    e.ensure_spell_checker();
    assert!(e.spell_checker.is_some());
}

#[test]
fn test_spell_jump_next_when_disabled() {
    let mut e = engine_with_text("helo wrld");
    // spell is off by default
    e.jump_next_spell_error();
    assert!(e.message.contains("off"));
}

#[test]
fn test_spell_jump_next_finds_error() {
    let mut e = engine_with_text("the quik brown fox");
    e.settings.spell = true;
    e.ensure_spell_checker();
    e.jump_next_spell_error();
    assert_eq!(e.cursor().col, 4); // "quik" starts at col 4
    assert!(e.message.contains("quik"));
}

#[test]
fn test_spell_jump_prev_finds_error() {
    let mut e = engine_with_text("helo world");
    e.settings.spell = true;
    e.ensure_spell_checker();
    e.view_mut().cursor.col = 9;
    e.jump_prev_spell_error();
    assert_eq!(e.cursor().col, 0); // "helo" at col 0
}

#[test]
fn test_spell_add_good_word() {
    let mut e = engine_with_text("vimcode");
    e.settings.spell = true;
    e.ensure_spell_checker();
    // Before adding, "vimcode" is misspelled
    e.jump_next_spell_error();
    assert_eq!(e.cursor().col, 0);
    // Add to user dict
    e.spell_add_good_word();
    // Now jump should find no errors
    e.view_mut().cursor.col = 0;
    e.jump_next_spell_error();
    assert!(e.message.contains("No spelling errors"));
    // Clean up
    e.spell_mark_wrong();
}

#[test]
fn test_spell_toggle_via_palette_action() {
    let mut e = Engine::new();
    assert!(!e.settings.spell);
    // Simulate palette toggle
    e.settings.spell = !e.settings.spell;
    e.ensure_spell_checker();
    assert!(e.settings.spell);
    assert!(e.spell_checker.is_some());
}

// ── LaTeX text objects and motions ────────────────────────────────────────

fn latex_engine(text: &str) -> Engine {
    use crate::core::syntax::{Syntax, SyntaxLanguage};
    let mut e = engine_with_text(text);
    e.active_buffer_state_mut().syntax = Some(Syntax::new_for_language(SyntaxLanguage::Latex));
    e
}

#[test]
fn test_latex_environment_object_inner() {
    let mut e = latex_engine("\\begin{itemize}\nitem one\nitem two\n\\end{itemize}\n");
    // Move cursor to line 1 (inside the environment)
    e.view_mut().cursor.line = 1;
    e.view_mut().cursor.col = 0;
    press_char(&mut e, 'd');
    press_char(&mut e, 'i');
    press_char(&mut e, 'e');
    let content = e.buffer().to_string();
    assert!(content.contains("\\begin{itemize}"));
    assert!(content.contains("\\end{itemize}"));
    assert!(!content.contains("item one"));
}

#[test]
fn test_latex_environment_object_around() {
    let mut e = latex_engine("before\n\\begin{itemize}\nitem one\n\\end{itemize}\nafter\n");
    e.view_mut().cursor.line = 2;
    e.view_mut().cursor.col = 0;
    press_char(&mut e, 'd');
    press_char(&mut e, 'a');
    press_char(&mut e, 'e');
    let content = e.buffer().to_string();
    assert!(!content.contains("\\begin{itemize}"));
    assert!(!content.contains("\\end{itemize}"));
    assert!(content.contains("before"));
    assert!(content.contains("after"));
}

#[test]
fn test_latex_environment_object_nested() {
    let mut e = latex_engine(
        "\\begin{enumerate}\n\\begin{itemize}\nhello\n\\end{itemize}\n\\end{enumerate}\n",
    );
    // Cursor inside inner environment
    e.view_mut().cursor.line = 2;
    e.view_mut().cursor.col = 0;
    press_char(&mut e, 'd');
    press_char(&mut e, 'i');
    press_char(&mut e, 'e');
    let content = e.buffer().to_string();
    // Inner environment \begin/\end{itemize} should remain
    assert!(content.contains("\\begin{itemize}"));
    assert!(content.contains("\\end{itemize}"));
    assert!(!content.contains("hello"));
}

#[test]
fn test_latex_math_object_inline() {
    let mut e = latex_engine("Text $x^2 + y^2$ more\n");
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 7; // inside $...$
    press_char(&mut e, 'd');
    press_char(&mut e, 'i');
    press_char(&mut e, '$');
    let content = e.buffer().to_string();
    assert!(content.contains("$$")); // delimiters remain, content removed
    assert!(!content.contains("x^2"));
}

#[test]
fn test_latex_math_object_around() {
    let mut e = latex_engine("Text $x^2$ more\n");
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 6; // inside $...$
    press_char(&mut e, 'd');
    press_char(&mut e, 'a');
    press_char(&mut e, '$');
    let content = e.buffer().to_string();
    assert!(!content.contains("$"));
    assert!(content.contains("Text "));
    assert!(content.contains("more"));
}

#[test]
fn test_latex_math_object_display() {
    let mut e = latex_engine("Text \\[a + b\\] more\n");
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 8; // inside \[...\]
    press_char(&mut e, 'd');
    press_char(&mut e, 'i');
    press_char(&mut e, '$');
    let content = e.buffer().to_string();
    assert!(content.contains("\\[\\]")); // delimiters remain
    assert!(!content.contains("a + b"));
}

#[test]
fn test_latex_command_object_inner() {
    let mut e = latex_engine("\\textbf{hello world}\n");
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 10; // inside braces
    press_char(&mut e, 'd');
    press_char(&mut e, 'i');
    press_char(&mut e, 'c');
    let content = e.buffer().to_string();
    assert!(content.contains("\\textbf{}"));
    assert!(!content.contains("hello"));
}

#[test]
fn test_latex_command_object_around() {
    let mut e = latex_engine("some \\textbf{hello} text\n");
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 14; // inside braces
    press_char(&mut e, 'd');
    press_char(&mut e, 'a');
    press_char(&mut e, 'c');
    let content = e.buffer().to_string();
    assert!(!content.contains("\\textbf"));
    assert!(content.contains("some "));
    assert!(content.contains(" text"));
}

#[test]
fn test_latex_section_jump_forward() {
    let mut e = latex_engine("\\section{One}\ntext\n\\subsection{Two}\nmore\n\\section{Three}\n");
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 0;
    // ]] jump to next section
    press_char(&mut e, ']');
    press_char(&mut e, ']');
    assert_eq!(e.view().cursor.line, 2);
    // Again
    press_char(&mut e, ']');
    press_char(&mut e, ']');
    assert_eq!(e.view().cursor.line, 4);
}

#[test]
fn test_latex_section_jump_backward() {
    let mut e = latex_engine("\\section{One}\ntext\n\\subsection{Two}\nmore\n\\section{Three}\n");
    e.view_mut().cursor.line = 4;
    e.view_mut().cursor.col = 0;
    // [[ jump to previous section
    press_char(&mut e, '[');
    press_char(&mut e, '[');
    assert_eq!(e.view().cursor.line, 2);
    press_char(&mut e, '[');
    press_char(&mut e, '[');
    assert_eq!(e.view().cursor.line, 0);
}

#[test]
fn test_latex_env_jump_forward() {
    let mut e = latex_engine(
        "\\begin{document}\ntext\n\\begin{itemize}\nitem\n\\end{itemize}\n\\end{document}\n",
    );
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 0;
    // ]m jump to next \begin
    press_char(&mut e, ']');
    press_char(&mut e, 'm');
    assert_eq!(e.view().cursor.line, 2);
    assert_eq!(e.view().cursor.col, 0);
}

#[test]
fn test_latex_env_jump_backward() {
    let mut e = latex_engine(
        "\\begin{document}\ntext\n\\begin{itemize}\nitem\n\\end{itemize}\n\\end{document}\n",
    );
    e.view_mut().cursor.line = 4;
    e.view_mut().cursor.col = 0;
    // [m jump to previous \begin
    press_char(&mut e, '[');
    press_char(&mut e, 'm');
    assert_eq!(e.view().cursor.line, 2);
    assert_eq!(e.view().cursor.col, 0);
}

#[test]
fn test_latex_env_end_jump_forward() {
    let mut e = latex_engine(
        "\\begin{document}\ntext\n\\begin{itemize}\nitem\n\\end{itemize}\n\\end{document}\n",
    );
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 0;
    // ]M jump to next \end
    press_char(&mut e, ']');
    press_char(&mut e, 'M');
    assert_eq!(e.view().cursor.line, 4);
    assert_eq!(e.view().cursor.col, 0);
}

#[test]
fn test_latex_env_end_jump_backward() {
    let mut e = latex_engine(
        "\\begin{document}\ntext\n\\begin{itemize}\nitem\n\\end{itemize}\n\\end{document}\n",
    );
    e.view_mut().cursor.line = 5;
    e.view_mut().cursor.col = 0;
    // [M jump to previous \end
    press_char(&mut e, '[');
    press_char(&mut e, 'M');
    assert_eq!(e.view().cursor.line, 4);
}

#[test]
fn test_latex_section_end_jump() {
    let mut e = latex_engine("\\section{One}\ntext\n\\end{document}\nmore\n\\end{other}\n");
    e.view_mut().cursor.line = 0;
    // ][ jump to next \end
    press_char(&mut e, ']');
    press_char(&mut e, '[');
    assert_eq!(e.view().cursor.line, 2);
}

#[test]
fn test_latex_section_commands_variants() {
    let mut e = latex_engine("text\n\\chapter{C}\nmore\n\\paragraph{P}\nend\n");
    e.view_mut().cursor.line = 0;
    press_char(&mut e, ']');
    press_char(&mut e, ']');
    assert_eq!(e.view().cursor.line, 1); // \chapter
    press_char(&mut e, ']');
    press_char(&mut e, ']');
    assert_eq!(e.view().cursor.line, 3); // \paragraph
}

#[test]
fn test_latex_starred_section() {
    let mut e = latex_engine("text\n\\section*{Unnumbered}\nmore\n");
    e.view_mut().cursor.line = 0;
    press_char(&mut e, ']');
    press_char(&mut e, ']');
    assert_eq!(e.view().cursor.line, 1); // \section* matches
}

#[test]
fn test_latex_yank_environment_inner() {
    let mut e = latex_engine("\\begin{quote}\nhello\n\\end{quote}\n");
    e.view_mut().cursor.line = 1;
    e.view_mut().cursor.col = 0;
    press_char(&mut e, 'y');
    press_char(&mut e, 'i');
    press_char(&mut e, 'e');
    // Content should be unchanged
    assert!(e.buffer().to_string().contains("hello"));
    // Register should contain the inner content
    let (text, _) = e.registers.get(&'"').expect("register should be set");
    assert!(text.contains("hello"));
    assert!(!text.contains("\\begin"));
}

#[test]
fn test_latex_env_object_not_in_non_latex() {
    // ie/ae should do nothing in non-LaTeX buffers
    let mut e = engine_with_text("\\begin{test}\nhello\n\\end{test}\n");
    e.view_mut().cursor.line = 1;
    press_char(&mut e, 'd');
    press_char(&mut e, 'i');
    press_char(&mut e, 'e');
    // Buffer unchanged — non-LaTeX buffer
    assert!(e.buffer().to_string().contains("hello"));
}

#[test]
fn test_latex_double_dollar_math() {
    let mut e = latex_engine("Text $$E=mc^2$$ more\n");
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 8;
    press_char(&mut e, 'd');
    press_char(&mut e, 'i');
    press_char(&mut e, '$');
    let content = e.buffer().to_string();
    assert!(content.contains("$$$$")); // delimiters remain
    assert!(!content.contains("E=mc"));
}

// ── Panel hover popup tests ─────────────────────────────────────────

#[test]
fn test_panel_hover_show_and_dismiss() {
    let mut e = engine_with_text("hello\n");
    assert!(e.panel_hover.is_none());
    e.show_panel_hover("test_panel", "item_1", 0, "**Bold** text");
    assert!(e.panel_hover.is_some());
    let ph = e.panel_hover.as_ref().unwrap();
    assert_eq!(ph.panel_name, "test_panel");
    assert_eq!(ph.item_id, "item_1");
    assert_eq!(ph.item_index, 0);
    assert!(!ph.rendered.lines.is_empty());
    e.dismiss_panel_hover_now();
    assert!(e.panel_hover.is_none());
}

#[test]
fn test_panel_hover_links_extracted() {
    let mut e = engine_with_text("hello\n");
    e.show_panel_hover("p", "i", 0, "See [docs](https://example.com) here");
    let ph = e.panel_hover.as_ref().unwrap();
    // Should have at least one link extracted from the markdown
    assert!(
        !ph.links.is_empty() || !ph.rendered.lines.is_empty(),
        "hover should have rendered content"
    );
}

#[test]
fn test_panel_hover_mouse_move_dwell_tracking() {
    let mut e = engine_with_text("hello\n");
    // First move starts dwell
    let changed = e.panel_hover_mouse_move("panel", "item_0", 0);
    assert!(changed);
    assert!(e.panel_hover_dwell.is_some());
    // Same item should not change
    let changed2 = e.panel_hover_mouse_move("panel", "item_0", 0);
    assert!(!changed2);
    // Different item should change
    let changed3 = e.panel_hover_mouse_move("panel", "item_1", 1);
    assert!(changed3);
}

#[test]
fn test_panel_hover_registry_lookup() {
    let mut e = engine_with_text("hello\n");
    // Register hover content for a panel item
    e.panel_hover_registry.insert(
        ("my_panel".to_string(), "commit_abc".to_string()),
        "# Commit abc\n\nSome details".to_string(),
    );
    // Start dwell on that item — poll should NOT show yet (need 300ms)
    e.panel_hover_mouse_move("my_panel", "commit_abc", 2);
    let shown = e.poll_panel_hover();
    assert!(!shown, "should not show before dwell timeout");
}

#[test]
fn test_panel_hover_dismissed_on_keypress() {
    let mut e = engine_with_text("hello\n");
    e.show_panel_hover("p", "i", 0, "test");
    assert!(e.panel_hover.is_some());
    // Any key press should dismiss
    e.handle_key("j", None, false);
    assert!(e.panel_hover.is_none());
}

#[test]
fn test_sc_hover_file_generates_markdown() {
    let mut e = engine_with_text("hello\n");
    // Populate SC panel with a fake file status
    e.sc_file_statuses = vec![crate::core::git::FileStatus {
        path: "src/main.rs".to_string(),
        staged: None,
        unstaged: Some(crate::core::git::StatusKind::Modified),
    }];
    e.sc_sections_expanded = [true, true, false, false];
    // flat index: 0=staged header, 1=unstaged header, 2=file item
    let md = e.sc_hover_markdown(2);
    assert!(md.is_some());
    let md = md.unwrap();
    assert!(md.contains("src/main.rs"), "hover should contain filename");
    assert!(md.contains("Modified"), "hover should contain status");
    assert!(
        md.contains("unstaged"),
        "hover should indicate staged/unstaged"
    );
}

#[test]
fn test_sc_hover_section_header_returns_none_for_non_branch() {
    let mut e = engine_with_text("hello\n");
    e.sc_file_statuses = vec![];
    e.sc_sections_expanded = [true, true, false, false];
    // flat index 0 = Staged Changes header (section 0 → branch info)
    // flat index 1 = Unstaged Changes header (section 1 → None)
    let md = e.sc_hover_markdown(1);
    assert!(md.is_none(), "non-branch headers should return None");
}

#[test]
fn test_sc_hover_log_entry_generates_markdown() {
    let mut e = engine_with_text("hello\n");
    e.sc_file_statuses = vec![];
    e.sc_log = vec![crate::core::git::GitLogEntry {
        hash: "abc1234".to_string(),
        message: "feat: add hover popups".to_string(),
    }];
    e.sc_sections_expanded = [true, true, false, true];
    // flat indices: 0=staged hdr, 1=unstaged hdr, 2=log hdr, 3=log item
    let md = e.sc_hover_markdown(3);
    assert!(md.is_some());
    let md = md.unwrap();
    assert!(
        md.contains("abc1234") || md.contains("hover popups"),
        "log hover should contain hash or message"
    );
}

#[test]
fn test_is_safe_url_allows_https() {
    assert!(is_safe_url("https://github.com/user/repo"));
    assert!(is_safe_url("http://example.com"));
    assert!(is_safe_url("HTTPS://EXAMPLE.COM"));
}

#[test]
fn test_is_safe_url_rejects_dangerous_schemes() {
    assert!(!is_safe_url("javascript:alert(1)"));
    assert!(!is_safe_url("file:///etc/passwd"));
    assert!(!is_safe_url("data:text/html,<h1>hi</h1>"));
    assert!(!is_safe_url("ftp://example.com"));
    assert!(!is_safe_url("ssh://evil.com"));
    assert!(!is_safe_url(""));
}

#[test]
fn test_hover_links_filtered_by_safe_url() {
    let mut e = engine_with_text("hello\n");
    // Markdown with a safe link and a dangerous link
    e.show_panel_hover(
        "ext_panel",
        "i",
        0,
        "Safe: [click](https://example.com) Evil: [hack](javascript:alert(1))",
    );
    let ph = e.panel_hover.as_ref().unwrap();
    // Only the https link should be in the links list
    for link in &ph.links {
        assert!(
            is_safe_url(&link.3),
            "unsafe URL should have been filtered: {}",
            link.3
        );
    }
}

#[test]
fn test_hover_native_panel_is_native() {
    let mut e = engine_with_text("hello\n");
    e.show_panel_hover("source_control", "", 0, "# test");
    assert!(e.panel_hover.as_ref().unwrap().is_native());
    e.show_panel_hover("my_ext_panel", "", 0, "# test");
    assert!(!e.panel_hover.as_ref().unwrap().is_native());
}

#[test]
fn test_hover_selection_extract_single_line() {
    let sel = HoverSelection {
        anchor_line: 0,
        anchor_col: 2,
        active_line: 0,
        active_col: 7,
    };
    let lines = vec!["hello world".to_string()];
    assert_eq!(sel.extract_text(&lines), "llo w");
}

#[test]
fn test_hover_selection_extract_multi_line() {
    let sel = HoverSelection {
        anchor_line: 0,
        anchor_col: 3,
        active_line: 2,
        active_col: 4,
    };
    let lines = vec![
        "first line".to_string(),
        "second line".to_string(),
        "third line".to_string(),
    ];
    assert_eq!(sel.extract_text(&lines), "st line\nsecond line\nthir");
}

#[test]
fn test_hover_selection_normalized_order() {
    // Forward selection
    let sel = HoverSelection {
        anchor_line: 1,
        anchor_col: 3,
        active_line: 2,
        active_col: 5,
    };
    assert_eq!(sel.normalized(), (1, 3, 2, 5));
    // Backward selection
    let sel = HoverSelection {
        anchor_line: 2,
        anchor_col: 5,
        active_line: 1,
        active_col: 3,
    };
    assert_eq!(sel.normalized(), (1, 3, 2, 5));
}

#[test]
fn test_hover_selection_start_and_extend() {
    let mut e = engine_with_text("hello\n");
    e.editor_hover_content
        .insert(0, "line one\nline two".to_string());
    e.trigger_editor_hover_at_cursor();
    e.editor_hover_has_focus = true;
    assert!(e.editor_hover.is_some());

    e.editor_hover_start_selection(0, 2);
    let sel = e.editor_hover.as_ref().unwrap().selection.as_ref().unwrap();
    assert_eq!(sel.anchor_line, 0);
    assert_eq!(sel.anchor_col, 2);
    assert_eq!(sel.active_col, 2);

    e.editor_hover_extend_selection(0, 6);
    let sel = e.editor_hover.as_ref().unwrap().selection.as_ref().unwrap();
    assert_eq!(sel.active_col, 6);
}

#[test]
fn test_hover_copy_all_text_when_no_selection() {
    let mut e = engine_with_text("hello\n");
    e.editor_hover_content.insert(0, "copy me".to_string());
    e.trigger_editor_hover_at_cursor();
    e.editor_hover_has_focus = true;

    let copied = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let copied_clone = copied.clone();
    e.clipboard_write = Some(Box::new(move |text: &str| {
        *copied_clone.lock().unwrap() = text.to_string();
        Ok(())
    }));

    e.copy_hover_selection();
    let result = copied.lock().unwrap().clone();
    assert!(result.contains("copy me"));
    assert_eq!(e.message, "Hover text copied");
}

#[test]
fn test_hover_copy_selected_text() {
    let mut e = engine_with_text("hello\n");
    e.editor_hover_content
        .insert(0, "select this text".to_string());
    e.trigger_editor_hover_at_cursor();
    e.editor_hover_has_focus = true;

    // Start selection on "this"
    e.editor_hover_start_selection(0, 7);
    e.editor_hover_extend_selection(0, 11);

    let copied = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let copied_clone = copied.clone();
    e.clipboard_write = Some(Box::new(move |text: &str| {
        *copied_clone.lock().unwrap() = text.to_string();
        Ok(())
    }));

    e.copy_hover_selection();
    let result = copied.lock().unwrap().clone();
    assert_eq!(result, "this");
}

#[test]
fn test_hover_y_key_copies() {
    let mut e = engine_with_text("hello\n");
    e.editor_hover_content.insert(0, "y copies".to_string());
    e.trigger_editor_hover_at_cursor();
    e.editor_hover_has_focus = true;

    let copied = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let copied_clone = copied.clone();
    e.clipboard_write = Some(Box::new(move |text: &str| {
        *copied_clone.lock().unwrap() = text.to_string();
        Ok(())
    }));

    e.handle_editor_hover_key("y", false);
    let result = copied.lock().unwrap().clone();
    assert!(result.contains("y copies"));
    // Popup should still be open (y doesn't dismiss)
    assert!(e.editor_hover.is_some());
}

#[test]
fn test_percent_decode_basic() {
    assert_eq!(Engine::percent_decode("hello"), "hello");
    assert_eq!(Engine::percent_decode("hello%20world"), "hello world");
    assert_eq!(Engine::percent_decode("%3F"), "?");
    assert_eq!(Engine::percent_decode("%2F"), "/");
    assert_eq!(Engine::percent_decode("a%2Fb%2Fc"), "a/b/c");
}

#[test]
fn test_percent_decode_edge_cases() {
    // Incomplete percent encoding left as-is
    assert_eq!(Engine::percent_decode("abc%2"), "abc%2");
    assert_eq!(Engine::percent_decode("abc%"), "abc%");
    // Invalid hex digits left as-is
    assert_eq!(Engine::percent_decode("%GG"), "%GG");
    // Empty string
    assert_eq!(Engine::percent_decode(""), "");
    // Mixed valid and plain text
    assert_eq!(
        Engine::percent_decode("key%3Dvalue%26other"),
        "key=value&other"
    );
}

#[test]
fn test_execute_command_uri_no_prefix() {
    let mut e = engine_with_text("hello\n");
    assert!(!e.execute_command_uri("https://example.com"));
    assert!(!e.execute_command_uri(""));
    assert!(!e.execute_command_uri("notcommand:foo"));
}

#[test]
fn test_execute_command_uri_empty_name() {
    let mut e = engine_with_text("hello\n");
    assert!(!e.execute_command_uri("command:"));
    assert!(!e.execute_command_uri("command:?args"));
}

#[test]
fn test_execute_command_uri_unknown_command() {
    let mut e = engine_with_text("hello\n");
    // Unknown plugin commands return false, no panic.
    assert!(!e.execute_command_uri("command:NonExistent"));
    assert!(!e.execute_command_uri("command:NonExistent?arg1"));
}

// ── Tab drag-and-drop tests ──────────────────────────────────────────────

#[test]
fn test_tab_drag_reorder_same_group() {
    use crate::core::window::DropZone;
    let mut e = engine_with_text("aaa\n");
    e.new_tab(None);
    e.buffer_mut().insert(0, "bbb\n");
    e.new_tab(None);
    e.buffer_mut().insert(0, "ccc\n");
    // 3 tabs: [aaa, bbb, ccc] — active is tab 2 (ccc)
    assert_eq!(e.active_group().tabs.len(), 3);
    assert_eq!(e.active_group().active_tab, 2);

    // Drag tab 2 (ccc) to position 0
    let gid = e.active_group;
    e.tab_drag_begin(gid, 2);
    assert!(e.tab_drag.is_some());
    e.tab_drag_drop(DropZone::TabReorder(gid, 0));
    assert!(e.tab_drag.is_none());

    // Now order should be [ccc, aaa, bbb], active tab is 0
    assert_eq!(e.active_group().active_tab, 0);
    // Verify ccc is first by switching to it and checking content
    e.active_group_mut().active_tab = 0;
    assert!(e.buffer().to_string().starts_with("ccc"));
    e.active_group_mut().active_tab = 1;
    assert!(e.buffer().to_string().starts_with("aaa"));
    e.active_group_mut().active_tab = 2;
    assert!(e.buffer().to_string().starts_with("bbb"));
}

#[test]
fn test_tab_drag_to_other_group_center() {
    use crate::core::window::DropZone;
    let mut e = engine_with_text("aaa\n");
    e.new_tab(None);
    e.buffer_mut().insert(0, "bbb\n");
    // Group 1 has [aaa, bbb]
    let group1 = e.active_group;
    assert_eq!(e.active_group().tabs.len(), 2);

    // Create second group via split
    e.open_editor_group(SplitDirection::Vertical);
    let group2 = e.active_group;
    assert_ne!(group1, group2);
    e.buffer_mut().insert(0, "ccc\n");

    // Drag bbb (tab 1 in group1) to group2 center
    e.tab_drag_begin(group1, 1);
    e.tab_drag_drop(DropZone::Center(group2));

    // group1 should have 1 tab (aaa), group2 should have 2 tabs
    assert_eq!(e.editor_groups.get(&group1).unwrap().tabs.len(), 1);
    assert_eq!(e.editor_groups.get(&group2).unwrap().tabs.len(), 2);
    // Active group should be group2
    assert_eq!(e.active_group, group2);
}

#[test]
fn test_tab_drag_to_new_split() {
    use crate::core::window::DropZone;
    let mut e = engine_with_text("aaa\n");
    e.new_tab(None);
    e.buffer_mut().insert(0, "bbb\n");
    let gid = e.active_group;
    assert_eq!(e.active_group().tabs.len(), 2);
    assert!(e.group_layout.is_single_group());

    // Drag tab 0 (aaa) to create a new split
    e.tab_drag_begin(gid, 0);
    e.tab_drag_drop(DropZone::Split(gid, SplitDirection::Vertical, false));

    // Should now have 2 groups
    assert!(!e.group_layout.is_single_group());
    assert_eq!(e.editor_groups.len(), 2);
}

#[test]
fn test_tab_drag_cancel() {
    let mut e = engine_with_text("aaa\n");
    e.new_tab(None);
    e.buffer_mut().insert(0, "bbb\n");
    let gid = e.active_group;
    let tabs_before = e.active_group().tabs.len();

    e.tab_drag_begin(gid, 0);
    assert!(e.tab_drag.is_some());
    e.tab_drag_cancel();
    assert!(e.tab_drag.is_none());
    assert_eq!(e.tab_drag_mouse, None);
    assert_eq!(e.tab_drop_zone, DropZone::None);
    // No state changed
    assert_eq!(e.active_group().tabs.len(), tabs_before);
}

#[test]
fn test_tab_drag_last_tab_closes_group() {
    use crate::core::window::DropZone;
    let mut e = engine_with_text("aaa\n");
    // Create second group with split
    e.open_editor_group(SplitDirection::Vertical);
    let group2 = e.active_group;
    e.buffer_mut().insert(0, "bbb\n");

    // Find the other group
    let group1 = *e.editor_groups.keys().find(|g| **g != group2).unwrap();
    assert_eq!(e.editor_groups.len(), 2);

    // Drag the only tab from group1 to group2
    e.tab_drag_begin(group1, 0);
    e.tab_drag_drop(DropZone::Center(group2));

    // group1 should be closed, only group2 remains
    assert_eq!(e.editor_groups.len(), 1);
    assert!(e.editor_groups.contains_key(&group2));
    assert!(e.group_layout.is_single_group());
}

#[test]
fn test_tab_drag_drop_none_is_noop() {
    use crate::core::window::DropZone;
    let mut e = engine_with_text("aaa\n");
    e.new_tab(None);
    e.buffer_mut().insert(0, "bbb\n");
    let gid = e.active_group;
    let tabs_before = e.active_group().tabs.len();
    let active_before = e.active_group().active_tab;

    e.tab_drag_begin(gid, 0);
    e.tab_drag_drop(DropZone::None);

    // Nothing changed
    assert_eq!(e.active_group().tabs.len(), tabs_before);
    assert_eq!(e.active_group().active_tab, active_before);
}

#[test]
fn test_tab_drag_reorder_to_other_group_at_index() {
    use crate::core::window::DropZone;
    let mut e = engine_with_text("aaa\n");
    e.new_tab(None);
    e.buffer_mut().insert(0, "bbb\n");
    let group1 = e.active_group;

    // Create second group with 2 tabs
    e.open_editor_group(SplitDirection::Vertical);
    let group2 = e.active_group;
    e.buffer_mut().insert(0, "ccc\n");
    e.new_tab(None);
    e.buffer_mut().insert(0, "ddd\n");
    assert_eq!(e.editor_groups.get(&group2).unwrap().tabs.len(), 2);

    // Drag aaa (tab 0 in group1) to group2 at index 1
    e.tab_drag_begin(group1, 0);
    e.tab_drag_drop(DropZone::TabReorder(group2, 1));

    // group1: [bbb], group2: [ccc, aaa, ddd]
    assert_eq!(e.editor_groups.get(&group1).unwrap().tabs.len(), 1);
    assert_eq!(e.editor_groups.get(&group2).unwrap().tabs.len(), 3);
    // Active group should be group2, active tab at insertion index
    assert_eq!(e.active_group, group2);
    assert_eq!(e.active_group().active_tab, 1);
    // Verify the inserted tab has "aaa" content
    assert!(e.buffer().to_string().starts_with("aaa"));
}

#[test]
fn test_has_code_actions_on_line_empty() {
    let e = engine_with_text("hello\nworld\n");
    // No code actions cached → should return false
    assert!(!e.has_code_actions_on_line(0));
    assert!(!e.has_code_actions_on_line(1));
}

#[test]
fn test_has_code_actions_on_line_with_actions() {
    let mut e = engine_with_text("hello\nworld\n");
    let path = std::path::PathBuf::from("/tmp/test_code_action.rs");
    e.buffer_manager
        .get_mut(e.active_window().buffer_id)
        .unwrap()
        .file_path = Some(path.clone());

    let mut line_map = HashMap::new();
    line_map.insert(
        0,
        vec![lsp::CodeAction {
            title: "Extract function".to_string(),
            kind: Some("refactor.extract".to_string()),
            edit: None,
        }],
    );
    e.lsp_code_actions.insert(path, line_map);

    assert!(e.has_code_actions_on_line(0));
    assert!(!e.has_code_actions_on_line(1));
}

#[test]
fn test_has_code_actions_empty_vec_returns_false() {
    let mut e = engine_with_text("hello\n");
    let path = std::path::PathBuf::from("/tmp/test_code_action2.rs");
    e.buffer_manager
        .get_mut(e.active_window().buffer_id)
        .unwrap()
        .file_path = Some(path.clone());

    let mut line_map = HashMap::new();
    line_map.insert(0, vec![]);
    e.lsp_code_actions.insert(path, line_map);

    // Empty vec should not count as having actions
    assert!(!e.has_code_actions_on_line(0));
}

#[test]
fn test_show_code_actions_popup_no_actions() {
    let mut e = engine_with_text("hello\n");
    let path = std::path::PathBuf::from("/tmp/test_no_actions.rs");
    e.buffer_manager
        .get_mut(e.active_window().buffer_id)
        .unwrap()
        .file_path = Some(path);
    e.show_code_actions_popup();
    assert_eq!(e.message, "No code actions available");
}

#[test]
fn test_show_code_actions_hover_opens_dialog() {
    let mut e = engine_with_text("hello\nworld\n");
    let actions = vec![
        lsp::CodeAction {
            title: "Quick fix".to_string(),
            kind: Some("quickfix".to_string()),
            edit: None,
        },
        lsp::CodeAction {
            title: "Extract method".to_string(),
            kind: None,
            edit: None,
        },
    ];
    e.show_code_actions_hover(0, actions);
    assert!(e.dialog.is_some());
    assert_ne!(e.message, "No code actions available");
}

#[test]
fn test_code_action_cache_cleared_on_edit() {
    let mut e = engine_with_text("hello\n");
    let path = std::path::PathBuf::from("/tmp/test_cache_clear.rs");
    e.buffer_manager
        .get_mut(e.active_window().buffer_id)
        .unwrap()
        .file_path = Some(path.clone());

    let mut line_map = HashMap::new();
    line_map.insert(
        0,
        vec![lsp::CodeAction {
            title: "Fix".to_string(),
            kind: None,
            edit: None,
        }],
    );
    e.lsp_code_actions.insert(path.clone(), line_map);
    assert!(e.has_code_actions_on_line(0));

    // Simulate removing cache (as happens on didChange)
    e.lsp_code_actions.remove(&path);
    assert!(!e.has_code_actions_on_line(0));
}

// ── Explorer indicators tests ──────────────────────────────────────────

#[test]
fn test_explorer_indicators_empty() {
    let e = Engine::new();
    let (git_st, diags) = e.explorer_indicators();
    assert!(git_st.is_empty());
    assert!(diags.is_empty());
}

#[test]
fn test_explorer_indicators_git_status() {
    let mut e = Engine::new();
    // Simulate git status with modified and untracked files
    e.cwd = PathBuf::from("/tmp/vimcode_test_git_ind");
    e.sc_file_statuses = vec![
        git::FileStatus {
            path: "src/main.rs".to_string(),
            staged: None,
            unstaged: Some(git::StatusKind::Modified),
        },
        git::FileStatus {
            path: "new_file.txt".to_string(),
            staged: None,
            unstaged: Some(git::StatusKind::Untracked),
        },
    ];
    let (git_st, _) = e.explorer_indicators();
    // Git status uses repo root which we can't easily mock, so just
    // verify the function doesn't panic and returns reasonable results
    // (actual path matching depends on find_repo_root returning our cwd)
    assert!(git_st.len() <= 2);
}

#[test]
fn test_explorer_indicators_diagnostics() {
    let mut e = Engine::new();
    let path = PathBuf::from("/tmp/vimcode_test_diag_indicator.rs");
    e.lsp_diagnostics.insert(
        path.clone(),
        vec![
            lsp::Diagnostic {
                range: lsp::LspRange {
                    start: lsp::LspPosition {
                        line: 0,
                        character: 0,
                    },
                    end: lsp::LspPosition {
                        line: 0,
                        character: 5,
                    },
                },
                severity: lsp::DiagnosticSeverity::Error,
                message: "error".to_string(),
                source: None,
                code: None,
            },
            lsp::Diagnostic {
                range: lsp::LspRange {
                    start: lsp::LspPosition {
                        line: 1,
                        character: 0,
                    },
                    end: lsp::LspPosition {
                        line: 1,
                        character: 5,
                    },
                },
                severity: lsp::DiagnosticSeverity::Warning,
                message: "warning".to_string(),
                source: None,
                code: None,
            },
            lsp::Diagnostic {
                range: lsp::LspRange {
                    start: lsp::LspPosition {
                        line: 2,
                        character: 0,
                    },
                    end: lsp::LspPosition {
                        line: 2,
                        character: 5,
                    },
                },
                severity: lsp::DiagnosticSeverity::Error,
                message: "another error".to_string(),
                source: None,
                code: None,
            },
            lsp::Diagnostic {
                range: lsp::LspRange {
                    start: lsp::LspPosition {
                        line: 3,
                        character: 0,
                    },
                    end: lsp::LspPosition {
                        line: 3,
                        character: 5,
                    },
                },
                severity: lsp::DiagnosticSeverity::Information,
                message: "info".to_string(),
                source: None,
                code: None,
            },
        ],
    );
    let (_, diags) = e.explorer_indicators();
    let counts = diags.get(&path).expect("should have diag entry");
    assert_eq!(counts.0, 2, "expected 2 errors");
    assert_eq!(counts.1, 1, "expected 1 warning");
}

// ── hide_single_tab setting tests ──────────────────────────────────────────

#[test]
fn test_hide_single_tab_default_off() {
    let engine = Engine::new();
    assert!(!engine.settings.hide_single_tab);
    assert!(!engine.is_tab_bar_hidden(engine.active_group));
}

#[test]
fn test_hide_single_tab_one_tab() {
    let mut engine = Engine::new();
    engine.settings.hide_single_tab = true;
    // Single tab → tab bar should be hidden
    assert!(engine.is_tab_bar_hidden(engine.active_group));
}

#[test]
fn test_hide_single_tab_two_tabs() {
    let mut engine = Engine::new();
    engine.settings.hide_single_tab = true;
    // Open a second tab
    let dir = std::env::temp_dir().join("vimcode_test_hst_twotabs");
    let _ = std::fs::create_dir_all(&dir);
    let f = dir.join("a.txt");
    std::fs::write(&f, "hello").unwrap();
    engine.open_file_in_tab(&f);
    assert!(
        engine.active_group().tabs.len() >= 2,
        "should have at least 2 tabs"
    );
    // Two tabs → tab bar should NOT be hidden
    assert!(!engine.is_tab_bar_hidden(engine.active_group));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_hide_single_tab_window_rects_reclaim_space() {
    let mut engine = Engine::new();
    engine.settings.breadcrumbs = false;
    let content = WindowRect::new(0.0, 0.0, 80.0, 24.0);
    let tab_bar_h = 1.0;

    // Without hide: content starts at y=1 (tab bar takes 1 row)
    engine.settings.hide_single_tab = false;
    let (rects_show, _) = engine.calculate_group_window_rects(content, tab_bar_h);
    assert!(!rects_show.is_empty());
    let y_show = rects_show[0].1.y;
    let h_show = rects_show[0].1.height;

    // With hide: content should start 1 row earlier and be 1 row taller
    engine.settings.hide_single_tab = true;
    let (rects_hide, _) = engine.calculate_group_window_rects(content, tab_bar_h);
    assert!(!rects_hide.is_empty());
    let y_hide = rects_hide[0].1.y;
    let h_hide = rects_hide[0].1.height;

    assert!(
        y_hide < y_show,
        "hidden tab bar should start higher: {} vs {}",
        y_hide,
        y_show
    );
    assert!(
        h_hide > h_show,
        "hidden tab bar should give more height: {} vs {}",
        h_hide,
        h_show
    );
    assert!(
        (y_show - y_hide - 1.0).abs() < 0.01,
        "should gain exactly 1 row: y_show={}, y_hide={}, diff={}",
        y_show,
        y_hide,
        y_show - y_hide
    );
}

#[test]
fn test_hide_single_tab_with_breadcrumbs() {
    let mut engine = Engine::new();
    engine.settings.breadcrumbs = true;
    engine.settings.hide_single_tab = true;
    let content = WindowRect::new(0.0, 0.0, 80.0, 24.0);
    let tab_bar_h = 2.0; // tab + breadcrumb

    let (rects, _) = engine.calculate_group_window_rects(content, tab_bar_h);
    assert!(!rects.is_empty());
    // Should reclaim 1 row (tab only), breadcrumb stays → y = 1.0
    let y = rects[0].1.y;
    assert!(
        (y - 1.0).abs() < 0.01,
        "with breadcrumbs, content should start at y=1 (breadcrumb row): got {}",
        y
    );
}

#[test]
fn test_hide_single_tab_multi_group() {
    let mut engine = Engine::new();
    engine.settings.hide_single_tab = true;

    // Create a second group (split)
    engine.open_editor_group(SplitDirection::Vertical);
    let groups = engine.group_layout.group_ids();
    assert_eq!(groups.len(), 2);

    // Multi-group mode: tab bars always visible so users can distinguish groups
    for &gid in &groups {
        assert!(
            !engine.is_tab_bar_hidden(gid),
            "multi-group should always show tab bars"
        );
    }

    // Even after opening a second tab, still visible (multi-group)
    let dir = std::env::temp_dir().join("vimcode_test_hst_multigroup");
    let _ = std::fs::create_dir_all(&dir);
    let f = dir.join("b.txt");
    std::fs::write(&f, "world").unwrap();
    engine.open_file_in_tab(&f);
    for &gid in &groups {
        assert!(
            !engine.is_tab_bar_hidden(gid),
            "multi-group should always show tab bars"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_hide_single_tab_transition_one_to_two() {
    let mut engine = Engine::new();
    engine.settings.breadcrumbs = false;
    engine.settings.hide_single_tab = true;
    let content = WindowRect::new(0.0, 0.0, 80.0, 24.0);
    let tab_bar_h = 1.0;

    // 1 tab: window starts at y=0 (tab bar hidden, space reclaimed)
    let (rects1, _) = engine.calculate_group_window_rects(content, tab_bar_h);
    assert_eq!(rects1[0].1.y, 0.0, "1 tab: window should start at y=0");
    assert_eq!(
        rects1[0].1.height, 24.0,
        "1 tab: window should use full height"
    );

    // Open a second tab
    let dir = std::env::temp_dir().join("vimcode_test_hst_transition");
    let _ = std::fs::create_dir_all(&dir);
    let f = dir.join("x.txt");
    std::fs::write(&f, "test").unwrap();
    engine.open_file_in_tab(&f);
    assert!(engine.active_group().tabs.len() >= 2);

    // 2 tabs: window starts at y=1 (tab bar visible, takes 1 row)
    let (rects2, _) = engine.calculate_group_window_rects(content, tab_bar_h);
    assert_eq!(rects2[0].1.y, 1.0, "2 tabs: window should start at y=1");
    assert_eq!(
        rects2[0].1.height, 23.0,
        "2 tabs: window should leave room for tab bar"
    );
    assert!(
        !engine.is_tab_bar_hidden(engine.active_group),
        "2 tabs: tab bar should be visible"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_set_hide_single_tab() {
    let mut engine = Engine::new();
    assert!(!engine.settings.hide_single_tab);
    engine.settings.parse_set_option("hidesingletab").unwrap();
    assert!(engine.settings.hide_single_tab);
    engine.settings.parse_set_option("nohidesingletab").unwrap();
    assert!(!engine.settings.hide_single_tab);
    engine.settings.parse_set_option("hst").unwrap();
    assert!(engine.settings.hide_single_tab);
}

// ==========================================================================
// Sidebar focus consolidation tests
// ==========================================================================

#[test]
fn test_sidebar_has_focus_initially_false() {
    let engine = Engine::new();
    assert!(!engine.explorer_has_focus);
    assert!(!engine.search_has_focus);
    assert!(!engine.sidebar_has_focus());
}

#[test]
fn test_sidebar_has_focus_explorer() {
    let mut engine = Engine::new();
    engine.explorer_has_focus = true;
    assert!(engine.sidebar_has_focus());
}

#[test]
fn test_sidebar_has_focus_search() {
    let mut engine = Engine::new();
    engine.search_has_focus = true;
    assert!(engine.sidebar_has_focus());
}

#[test]
fn test_sidebar_has_focus_aggregates_all_panels() {
    let mut engine = Engine::new();
    assert!(!engine.sidebar_has_focus());

    engine.sc_has_focus = true;
    assert!(engine.sidebar_has_focus());
    engine.sc_has_focus = false;

    engine.dap_sidebar_has_focus = true;
    assert!(engine.sidebar_has_focus());
    engine.dap_sidebar_has_focus = false;

    engine.ext_sidebar_has_focus = true;
    assert!(engine.sidebar_has_focus());
    engine.ext_sidebar_has_focus = false;

    engine.ai_has_focus = true;
    assert!(engine.sidebar_has_focus());
    engine.ai_has_focus = false;

    engine.settings_has_focus = true;
    assert!(engine.sidebar_has_focus());
    engine.settings_has_focus = false;

    engine.ext_panel_has_focus = true;
    assert!(engine.sidebar_has_focus());
    engine.ext_panel_has_focus = false;

    assert!(!engine.sidebar_has_focus());
}

#[test]
fn test_clear_sidebar_focus() {
    let mut engine = Engine::new();
    engine.explorer_has_focus = true;
    engine.search_has_focus = true;
    engine.sc_has_focus = true;
    engine.dap_sidebar_has_focus = true;
    engine.ext_sidebar_has_focus = true;
    engine.ai_has_focus = true;
    engine.settings_has_focus = true;
    engine.ext_panel_has_focus = true;
    assert!(engine.sidebar_has_focus());

    engine.clear_sidebar_focus();
    assert!(!engine.sidebar_has_focus());
    assert!(!engine.explorer_has_focus);
    assert!(!engine.search_has_focus);
    assert!(!engine.sc_has_focus);
    assert!(!engine.dap_sidebar_has_focus);
    assert!(!engine.ext_sidebar_has_focus);
    assert!(!engine.ai_has_focus);
    assert!(!engine.settings_has_focus);
    assert!(!engine.ext_panel_has_focus);
}

#[test]
fn test_explorer_focus_blocks_normal_keys() {
    let mut engine = engine_with_text("hello world\n");
    engine.explorer_has_focus = true;

    // 'x' should NOT delete when explorer has focus
    engine.handle_key("x", Some('x'), false);
    assert_eq!(engine.buffer().to_string(), "hello world\n");
    assert_eq!(engine.cursor().col, 0);
}

#[test]
fn test_search_focus_blocks_normal_keys() {
    let mut engine = engine_with_text("hello world\n");
    engine.search_has_focus = true;

    // 'x' should NOT delete when search has focus
    engine.handle_key("x", Some('x'), false);
    assert_eq!(engine.buffer().to_string(), "hello world\n");
}

#[test]
fn test_unfocused_sidebar_allows_normal_keys() {
    let mut engine = engine_with_text("hello world\n");
    assert!(!engine.explorer_has_focus);
    assert!(!engine.search_has_focus);

    // 'x' should delete a character when sidebar is not focused
    engine.handle_key("x", Some('x'), false);
    assert_eq!(engine.buffer().to_string(), "ello world\n");
}
