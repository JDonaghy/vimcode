/// Integration tests for the AI assistant panel (engine state machine).
///
/// #819 adopted quadraui's `ChatController` for the AI sidebar's input
/// buffer, cursor, history and transcript scroll — those are no longer
/// hand-rolled `Engine` fields (`ai_input`/`ai_input_cursor`/
/// `ai_input_active`/`ai_scroll_top`) but state owned by `Engine::ai_chat`
/// (a `quadraui::ChatController`), whose own text-editing/cursor-movement
/// behaviour is covered by quadraui's own test suite, not re-tested here.
/// What stays here is the business logic `Engine` still owns: sending a
/// message, polling the background response, clearing the conversation, and
/// translating a `ChatControllerEvent` into those engine actions
/// (`Engine::dispatch_ai_chat_event`).
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
    assert!(e.ai_chat.borrow().input_text().is_empty());
    assert!(!e.ai_has_focus);
    assert!(!e.ai_streaming);
    assert_eq!(e.ai_chat.borrow().transcript_scroll_top(), 0);
}

/// `ChatController`'s input has no separate "not editing" mode the way the
/// old `ai_input_active` flag did — it is always focused, so a click or
/// keystroke reaching the panel types immediately rather than needing an
/// `i`/`a`/`Return` to "activate" it first.
#[test]
fn test_ai_chat_input_has_focus_by_default() {
    let e = engine();
    assert!(e.ai_chat.borrow().input_has_focus());
}

#[test]
fn test_ai_clear_resets_state() {
    let mut e = engine();
    // Manually push a message
    e.ai_messages.push(vimcode_core::core::ai::AiMessage {
        role: "user".to_string(),
        content: "hello".to_string(),
    });
    e.ai_chat.borrow_mut().set_transcript_scroll_top(5);
    e.ai_clear();
    assert!(e.ai_messages.is_empty());
    assert_eq!(e.ai_chat.borrow().transcript_scroll_top(), 0);
    assert!(!e.ai_streaming);
}

#[test]
fn test_ai_send_empty_input_is_noop() {
    let mut e = engine();
    e.ai_send_message("  ".to_string());
    // Trimmed input is empty → no message added, no thread spawned
    assert!(e.ai_messages.is_empty());
    assert!(!e.ai_streaming);
}

/// `ChatControllerEvent::Cancelled` (Escape) leaves the panel — the
/// always-focused input has no intermediate "still focused, not editing"
/// state to fall back into the way the old two-mode `ai_input_active` did.
#[test]
fn test_ai_dispatch_cancelled_clears_focus() {
    let mut e = engine();
    e.ai_has_focus = true;
    let still_focused =
        e.dispatch_ai_chat_event(vimcode_core::quadraui::ChatControllerEvent::Cancelled);
    assert!(!still_focused);
    assert!(!e.ai_has_focus);
}

/// `ChatControllerEvent::Submit` is the panel's only path into
/// `Engine::ai_send_message` now (`ChatController` owns the input buffer);
/// the dispatch must also clear the box afterwards.
#[test]
fn test_ai_dispatch_submit_sends_and_clears_input() {
    let mut e = engine();
    e.ai_chat.borrow_mut().input_insert_str("hello world");
    let still_focused =
        e.dispatch_ai_chat_event(vimcode_core::quadraui::ChatControllerEvent::Submit {
            text: "hello world".to_string(),
        });
    assert!(still_focused);
    assert_eq!(e.ai_messages.len(), 1);
    assert_eq!(e.ai_messages[0].role, "user");
    assert_eq!(e.ai_messages[0].content, "hello world");
    assert!(e.ai_streaming);
    assert!(
        e.ai_chat.borrow().input_text().is_empty(),
        "submitting must clear the input box"
    );
}

/// Ctrl+C is the one hotkey `Engine::dispatch_ai_chat_event` binds itself
/// via `ChatControllerEvent::KeyPressed` (`ChatController` has no built-in
/// clear-conversation binding) — the same escape hatch its own doc comment
/// recommends apps use for hotkeys it doesn't consume.
#[test]
fn test_ai_dispatch_ctrl_c_clears_conversation() {
    let mut e = engine();
    e.ai_messages.push(vimcode_core::core::ai::AiMessage {
        role: "user".to_string(),
        content: "hello".to_string(),
    });
    let event = vimcode_core::quadraui::ChatControllerEvent::KeyPressed {
        key: "Char('c')".to_string(),
        modifiers: vimcode_core::quadraui::Modifiers {
            ctrl: true,
            ..Default::default()
        },
    };
    let still_focused = e.dispatch_ai_chat_event(event);
    assert!(still_focused);
    assert!(e.ai_messages.is_empty());
}

#[test]
fn test_ai_chat_scroll_transcript_by() {
    let e = engine();
    e.ai_chat.borrow_mut().scroll_transcript_by(5, 100, 20);
    assert_eq!(e.ai_chat.borrow().transcript_scroll_top(), 5);
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
