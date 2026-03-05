/// Integration tests for the AI assistant panel (engine state machine).
use vimcode_core::core::engine::Engine;

fn engine() -> Engine {
    vimcode_core::core::session::suppress_disk_saves();
    let mut e = Engine::new();
    e.settings = vimcode_core::core::settings::Settings::default();
    e
}

#[test]
fn test_ai_initial_state() {
    let e = engine();
    assert!(e.ai_messages.is_empty());
    assert!(e.ai_input.is_empty());
    assert!(!e.ai_has_focus);
    assert!(!e.ai_input_active);
    assert!(!e.ai_streaming);
    assert_eq!(e.ai_scroll_top, 0);
}

#[test]
fn test_ai_clear_resets_state() {
    let mut e = engine();
    // Manually push a message
    e.ai_messages.push(vimcode_core::core::ai::AiMessage {
        role: "user".to_string(),
        content: "hello".to_string(),
    });
    e.ai_scroll_top = 5;
    e.ai_clear();
    assert!(e.ai_messages.is_empty());
    assert_eq!(e.ai_scroll_top, 0);
    assert!(!e.ai_streaming);
}

#[test]
fn test_ai_send_empty_input_is_noop() {
    let mut e = engine();
    e.ai_input = "  ".to_string();
    e.ai_send_message();
    // Trimmed input is empty → no message added, no thread spawned
    assert!(e.ai_messages.is_empty());
    assert!(!e.ai_streaming);
}

#[test]
fn test_ai_panel_key_focus_escape() {
    let mut e = engine();
    e.ai_has_focus = true;
    e.handle_ai_panel_key("Escape", false, None);
    assert!(!e.ai_has_focus);
}

#[test]
fn test_ai_panel_key_i_activates_input() {
    let mut e = engine();
    e.ai_has_focus = true;
    assert!(!e.ai_input_active);
    e.handle_ai_panel_key("i", false, None);
    assert!(e.ai_input_active);
}

#[test]
fn test_ai_panel_key_typing_in_input() {
    let mut e = engine();
    e.ai_has_focus = true;
    e.ai_input_active = true;
    e.handle_ai_panel_key("char", false, Some('h'));
    e.handle_ai_panel_key("char", false, Some('i'));
    assert_eq!(e.ai_input, "hi");
    assert_eq!(e.ai_input_cursor, 2);
}

#[test]
fn test_ai_panel_key_backspace_in_input() {
    let mut e = engine();
    e.ai_input = "hello".to_string();
    e.ai_input_cursor = 5; // cursor at end
    e.ai_input_active = true;
    e.handle_ai_panel_key("BackSpace", false, None);
    assert_eq!(e.ai_input, "hell");
    assert_eq!(e.ai_input_cursor, 4);
}

#[test]
fn test_ai_panel_key_escape_exits_input_mode() {
    let mut e = engine();
    e.ai_has_focus = true;
    e.ai_input_active = true;
    e.handle_ai_panel_key("Escape", false, None);
    assert!(!e.ai_input_active);
    // Still has focus (only outer Escape unfocuses)
    assert!(e.ai_has_focus);
}

#[test]
fn test_ai_panel_key_scroll() {
    let mut e = engine();
    e.ai_has_focus = true;
    e.ai_scroll_top = 0;
    e.handle_ai_panel_key("j", false, None);
    assert_eq!(e.ai_scroll_top, 1);
    e.handle_ai_panel_key("k", false, None);
    assert_eq!(e.ai_scroll_top, 0);
    // k at 0 stays at 0 (saturating_sub)
    e.handle_ai_panel_key("k", false, None);
    assert_eq!(e.ai_scroll_top, 0);
}

#[test]
fn test_ai_panel_key_g_scrolls_to_top() {
    let mut e = engine();
    e.ai_has_focus = true;
    e.ai_scroll_top = 10;
    e.handle_ai_panel_key("g", false, None);
    assert_eq!(e.ai_scroll_top, 0);
}

#[test]
fn test_ai_command_sets_input_and_sends() {
    let mut e = engine();
    // :AI <message> should push user message and start streaming
    e.execute_command("AI hello world");
    // Message should be pushed and streaming started
    assert_eq!(e.ai_messages.len(), 1);
    assert_eq!(e.ai_messages[0].role, "user");
    assert_eq!(e.ai_messages[0].content, "hello world");
    assert!(e.ai_streaming);
    assert!(e.ai_has_focus);
}

#[test]
fn test_ai_clear_command() {
    let mut e = engine();
    e.ai_messages.push(vimcode_core::core::ai::AiMessage {
        role: "user".to_string(),
        content: "test".to_string(),
    });
    e.execute_command("AiClear");
    assert!(e.ai_messages.is_empty());
}

#[test]
fn test_ai_input_cursor_arrow_keys() {
    let mut e = engine();
    e.ai_input = "hello".to_string();
    e.ai_input_cursor = 5;
    e.ai_input_active = true;
    e.handle_ai_panel_key("Left", false, None);
    assert_eq!(e.ai_input_cursor, 4);
    e.handle_ai_panel_key("Right", false, None);
    assert_eq!(e.ai_input_cursor, 5);
    e.handle_ai_panel_key("Home", false, None);
    assert_eq!(e.ai_input_cursor, 0);
    e.handle_ai_panel_key("End", false, None);
    assert_eq!(e.ai_input_cursor, 5);
    // Left saturates at 0
    e.handle_ai_panel_key("Home", false, None);
    e.handle_ai_panel_key("Left", false, None);
    assert_eq!(e.ai_input_cursor, 0);
}

#[test]
fn test_ai_input_cursor_insert_at_middle() {
    let mut e = engine();
    e.ai_input = "hllo".to_string();
    e.ai_input_cursor = 1; // insert 'e' between 'h' and 'l'
    e.ai_input_active = true;
    e.handle_ai_panel_key("", false, Some('e'));
    assert_eq!(e.ai_input, "hello");
    assert_eq!(e.ai_input_cursor, 2);
}

#[test]
fn test_ai_insert_text_paste() {
    let mut e = engine();
    e.ai_input = "hi".to_string();
    e.ai_input_cursor = 2;
    e.ai_insert_text(" world");
    assert_eq!(e.ai_input, "hi world");
    assert_eq!(e.ai_input_cursor, 8);
}

#[test]
fn test_ai_poll_no_rx_returns_false() {
    let mut e = engine();
    assert!(!e.poll_ai());
}

#[test]
fn test_ai_poll_receives_ok_response() {
    let mut e = engine();
    // Manually wire up the channel
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    e.ai_rx = Some(rx);
    e.ai_streaming = true;
    // Nothing in channel yet
    assert!(!e.poll_ai());
    // Send a response
    tx.send(Ok("Nice to meet you!".to_string())).unwrap();
    assert!(e.poll_ai());
    assert!(!e.ai_streaming);
    assert_eq!(e.ai_messages.len(), 1);
    assert_eq!(e.ai_messages[0].role, "assistant");
    assert_eq!(e.ai_messages[0].content, "Nice to meet you!");
}

#[test]
fn test_ai_poll_receives_error_response() {
    let mut e = engine();
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    e.ai_rx = Some(rx);
    e.ai_streaming = true;
    tx.send(Err("API error".to_string())).unwrap();
    assert!(e.poll_ai());
    assert!(!e.ai_streaming);
    // Error goes to message bar, not ai_messages
    assert!(e.ai_messages.is_empty());
    assert!(e.message.contains("AI error"));
}

#[test]
fn test_ai_settings_defaults() {
    let settings = vimcode_core::core::settings::Settings::default();
    assert_eq!(settings.ai_provider, "anthropic");
    assert!(settings.ai_api_key.is_empty());
    assert!(settings.ai_model.is_empty());
    assert!(settings.ai_base_url.is_empty());
}
