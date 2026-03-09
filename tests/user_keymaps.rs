mod common;
use common::*;

/// Helper: create an engine with user keymaps configured.
fn engine_with_keymaps(text: &str, keymaps: &[&str]) -> vimcode_core::Engine {
    let mut e = engine_with(text);
    e.settings.keymaps = keymaps.iter().map(|s| s.to_string()).collect();
    e.rebuild_user_keymaps();
    e
}

// ── Parsing ──────────────────────────────────────────────────────────────────

#[test]
fn single_key_keymap_fires_command() {
    let mut e = engine_with_keymaps("hello\nworld\n", &["n K :join"]);
    press(&mut e, 'K');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "hello world", "K should have joined lines");
}

#[test]
fn ctrl_key_keymap_fires_command() {
    let mut e = engine_with_keymaps("hello\nworld\n", &["n <C-j> :join"]);
    ctrl(&mut e, 'j');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "hello world", "<C-j> should have joined lines");
}

#[test]
fn two_key_sequence_keymap() {
    // Map "gc" in normal mode to :join (overriding the built-in gc commentary)
    let mut e = engine_with_keymaps("aaa\nbbb\n", &["n gc :join"]);
    press(&mut e, 'g');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "aaa bbb", "gc should have joined lines");
}

#[test]
fn three_key_sequence_keymap() {
    // Map "gcc" to :join
    let mut e = engine_with_keymaps("aaa\nbbb\n", &["n gcc :join"]);
    press(&mut e, 'g');
    press(&mut e, 'c');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "aaa bbb", "gcc should have joined lines");
}

#[test]
fn visual_mode_keymap() {
    let mut e = engine_with_keymaps("aaa\nbbb\nccc\n", &["v K :delete"]);
    // Enter visual line mode, select two lines
    press(&mut e, 'V');
    press(&mut e, 'j');
    // Press K (user keymap should fire :delete)
    press(&mut e, 'K');
    let lines = get_lines(&e);
    // :delete removes the current line — the keymap should fire
    assert!(
        lines.len() < 3,
        "K in visual should have deleted: {:?}",
        lines
    );
}

#[test]
fn keymap_with_count() {
    // Count is consumed by try_user_keymap and passed to the command
    // Use :echo which shows the argument in the message bar
    let mut e = engine_with_keymaps("hello\n", &["n K :echo {count}"]);
    press(&mut e, '3');
    press(&mut e, 'K');
    assert!(
        e.message.contains('3'),
        "count should be substituted: {}",
        e.message
    );
}

#[test]
fn keymap_with_count_placeholder() {
    let mut e = engine_with_keymaps("let x = 1;\nlet y = 2;\n", &["n gcc :Commentary {count}"]);
    // Load commentary plugin
    let dir = std::env::temp_dir().join("vc_keymap_commentary_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    use vimcode_core::core::extensions::BUNDLED;
    let commentary = BUNDLED.iter().find(|b| b.name == "commentary").unwrap();
    let script = commentary
        .scripts
        .iter()
        .find(|s| s.0 == "commentary.lua")
        .unwrap();
    std::fs::write(dir.join("commentary.lua"), script.1).unwrap();
    let buf_id = e.active_buffer_id();
    if let Some(state) = e.buffer_manager.get_mut(buf_id) {
        state.lsp_language_id = Some("rust".to_string());
    }
    match vimcode_core::core::plugin::PluginManager::new() {
        Ok(mut mgr) => {
            mgr.load_plugins_dir(&dir, &[]);
            e.plugin_manager = Some(mgr);
        }
        Err(_) => panic!("failed to create PluginManager"),
    }

    // gcc with count 2 should comment 2 lines
    press(&mut e, '2');
    press(&mut e, 'g');
    press(&mut e, 'c');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "// let x = 1;", "line 1 should be commented");
    assert_eq!(lines[1], "// let y = 2;", "line 2 should be commented");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_match_falls_through_to_builtin() {
    // Define a keymap for "gc" but press "gg" — should fall through to built-in gg (go to top)
    let mut e = engine_with_keymaps("aaa\nbbb\nccc\n", &["n gc :join"]);
    // Move to last line first
    press(&mut e, 'G');
    assert_eq!(e.cursor().line, 2, "G should go to last line");
    // Now gg should go to first line (not intercepted by keymap)
    press(&mut e, 'g');
    press(&mut e, 'g');
    assert_eq!(
        e.cursor().line,
        0,
        "gg should go to first line (fallthrough)"
    );
}

#[test]
fn multiple_keymaps_coexist() {
    let mut e = engine_with_keymaps("aaa\nbbb\nccc\n", &["n <C-j> :join", "n <C-k> :delete"]);
    // <C-j> should join
    ctrl(&mut e, 'j');
    assert_eq!(get_lines(&e)[0], "aaa bbb");
    // <C-k> should delete current line
    ctrl(&mut e, 'k');
    assert_eq!(get_lines(&e)[0], "ccc");
}

#[test]
fn invalid_keymap_definitions_ignored() {
    // These should be silently ignored (bad format)
    let e = engine_with_keymaps(
        "hello\n",
        &[
            "x K :join", // invalid mode
            "n K",       // missing action
            "n K join",  // action missing ':'
            "",          // empty
            "n",         // incomplete
        ],
    );
    assert!(e.user_keymaps.is_empty(), "all keymaps should be invalid");
}

#[test]
fn keymap_overrides_builtin_key() {
    // Override 'J' (normally join) to do delete instead
    let mut e = engine_with_keymaps("aaa\nbbb\nccc\n", &["n J :delete"]);
    press(&mut e, 'J');
    // Should delete, not join
    let lines = get_lines(&e);
    assert_eq!(lines[0], "bbb", "J should delete (overridden), not join");
}

#[test]
fn keymap_does_not_fire_in_wrong_mode() {
    // Define keymap for normal mode only
    let mut e = engine_with_keymaps("hello world\n", &["n K :join"]);
    // Enter insert mode
    press(&mut e, 'i');
    assert_eq!(e.mode, vimcode_core::Mode::Insert);
    // K in insert mode should NOT fire the keymap — it should insert 'K'
    press(&mut e, 'K');
    press_key(&mut e, "Escape");
    let lines = get_lines(&e);
    assert!(
        lines[0].contains('K'),
        "K in insert mode should type K, not fire keymap"
    );
}

#[test]
fn keymap_buf_cleared_on_exact_match() {
    // After a keymap fires, the buffer should be clear for the next sequence
    let mut e = engine_with_keymaps("aaa\nbbb\nccc\n", &["n gc :join"]);
    press(&mut e, 'g');
    press(&mut e, 'c');
    assert_eq!(get_lines(&e)[0], "aaa bbb", "gc should join");
    // Now try gc again — should work again
    press(&mut e, 'g');
    press(&mut e, 'c');
    assert_eq!(
        get_lines(&e)[0],
        "aaa bbb ccc",
        "second gc should join again"
    );
}

// ── :map / :unmap commands ───────────────────────────────────────────────

#[test]
fn map_command_adds_keymap() {
    let mut e = engine_with("");
    assert!(e.user_keymaps.is_empty());
    exec(&mut e, "map n K :join");
    assert_eq!(e.user_keymaps.len(), 1);
    assert_eq!(e.settings.keymaps.len(), 1);
    assert_eq!(e.settings.keymaps[0], "n K :join");
    assert!(e.message.contains("Mapped"));
}

#[test]
fn map_command_keymap_takes_effect_immediately() {
    let mut e = engine_with("aaa\nbbb\n");
    exec(&mut e, "map n K :join");
    press(&mut e, 'K');
    assert_eq!(get_lines(&e)[0], "aaa bbb");
}

#[test]
fn map_command_no_duplicates() {
    let mut e = engine_with("");
    exec(&mut e, "map n K :join");
    exec(&mut e, "map n K :join");
    assert_eq!(e.settings.keymaps.len(), 1, "should not add duplicate");
}

#[test]
fn map_command_list_shows_all() {
    let mut e = engine_with_keymaps("", &["n K :join", "v gc :delete"]);
    exec(&mut e, "map");
    assert!(e.message.contains("n K :join"), "should list first mapping");
    assert!(
        e.message.contains("v gc :delete"),
        "should list second mapping"
    );
}

#[test]
fn map_command_list_empty() {
    let mut e = engine_with("");
    exec(&mut e, "map");
    assert!(
        e.message.contains("No user keymaps"),
        "should say no keymaps: {}",
        e.message
    );
}

#[test]
fn unmap_command_removes_keymap() {
    let mut e = engine_with_keymaps("aaa\nbbb\n", &["n K :join", "n J :delete"]);
    exec(&mut e, "unmap n K");
    assert_eq!(e.settings.keymaps.len(), 1);
    assert_eq!(e.settings.keymaps[0], "n J :delete");
    assert!(e.message.contains("Unmapped"));
    // K should no longer fire the keymap (falls through to built-in)
    assert!(e.user_keymaps.len() == 1);
}

#[test]
fn unmap_command_nonexistent_shows_error() {
    let mut e = engine_with("");
    exec(&mut e, "unmap n Z");
    assert!(
        e.message.contains("No mapping found"),
        "should say not found: {}",
        e.message
    );
}

#[test]
fn map_command_bad_format_shows_usage() {
    let mut e = engine_with("");
    exec(&mut e, "map bad");
    assert!(
        e.message.contains("Usage"),
        "should show usage: {}",
        e.message
    );
}
