/// Integration tests for AI inline completions (ghost text state machine).
use vimcode_core::core::engine::Engine;

fn engine() -> Engine {
    let mut e = Engine::new();
    vimcode_core::core::session::suppress_disk_saves();
    e.settings = vimcode_core::core::settings::Settings::default();
    e
}

// ── Initial state ──────────────────────────────────────────────────────────

#[test]
fn test_ghost_text_initial_state() {
    let e = engine();
    assert!(e.ai_ghost_text.is_none());
    assert!(e.ai_ghost_alternatives.is_empty());
    assert_eq!(e.ai_ghost_alt_idx, 0);
    assert!(e.ai_completion_ticks.is_none());
    assert!(e.ai_completion_rx.is_none());
}

#[test]
fn test_ai_completions_disabled_by_default() {
    let e = engine();
    assert!(!e.settings.ai_completions);
}

// ── Timer logic ────────────────────────────────────────────────────────────

#[test]
fn test_reset_timer_sets_ticks() {
    let mut e = engine();
    e.ai_completion_reset_timer();
    assert!(e.ai_completion_ticks.is_some());
    assert!(e.ai_completion_ticks.unwrap() > 0);
}

#[test]
fn test_tick_decrements_counter() {
    let mut e = engine();
    e.ai_completion_ticks = Some(5);
    e.settings.ai_completions = true;
    e.tick_ai_completion();
    assert_eq!(e.ai_completion_ticks, Some(4));
}

#[test]
fn test_tick_clears_when_zero() {
    let mut e = engine();
    // When ticks hit 0, tick_ai_completion fires the request.
    // Since ai_completions is false, it's a no-op but clears the counter.
    e.ai_completion_ticks = Some(0);
    e.settings.ai_completions = false;
    e.tick_ai_completion();
    assert!(e.ai_completion_ticks.is_none());
}

#[test]
fn test_tick_no_redraw_when_no_rx() {
    let mut e = engine();
    // No pending rx → tick should not signal redraw
    assert!(!e.tick_ai_completion());
}

// ── Ghost text clearing ────────────────────────────────────────────────────

#[test]
fn test_ghost_clear_resets_all_fields() {
    let mut e = engine();
    e.ai_ghost_text = Some("suggestion".to_string());
    e.ai_ghost_alternatives = vec!["a".to_string(), "b".to_string()];
    e.ai_ghost_alt_idx = 1;
    e.ai_completion_ticks = Some(10);
    e.ai_ghost_clear();
    assert!(e.ai_ghost_text.is_none());
    assert!(e.ai_ghost_alternatives.is_empty());
    assert_eq!(e.ai_ghost_alt_idx, 0);
    assert!(e.ai_completion_ticks.is_none());
}

// ── Alternative cycling ────────────────────────────────────────────────────

#[test]
fn test_ghost_next_alt_wraps() {
    let mut e = engine();
    e.ai_ghost_alternatives = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    e.ai_ghost_alt_idx = 0;
    e.ai_ghost_text = Some("a".to_string());

    e.ai_ghost_next_alt();
    assert_eq!(e.ai_ghost_text.as_deref(), Some("b"));
    assert_eq!(e.ai_ghost_alt_idx, 1);

    e.ai_ghost_next_alt();
    assert_eq!(e.ai_ghost_text.as_deref(), Some("c"));
    assert_eq!(e.ai_ghost_alt_idx, 2);

    // Wrap around
    e.ai_ghost_next_alt();
    assert_eq!(e.ai_ghost_text.as_deref(), Some("a"));
    assert_eq!(e.ai_ghost_alt_idx, 0);
}

#[test]
fn test_ghost_prev_alt_wraps() {
    let mut e = engine();
    e.ai_ghost_alternatives = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    e.ai_ghost_alt_idx = 0;
    e.ai_ghost_text = Some("a".to_string());

    // Wrap backwards from 0
    e.ai_ghost_prev_alt();
    assert_eq!(e.ai_ghost_text.as_deref(), Some("c"));
    assert_eq!(e.ai_ghost_alt_idx, 2);

    e.ai_ghost_prev_alt();
    assert_eq!(e.ai_ghost_text.as_deref(), Some("b"));
    assert_eq!(e.ai_ghost_alt_idx, 1);
}

#[test]
fn test_ghost_cycle_empty_alternatives_is_noop() {
    let mut e = engine();
    // With no alternatives, cycling should not panic or change anything
    e.ai_ghost_next_alt();
    e.ai_ghost_prev_alt();
    assert!(e.ai_ghost_text.is_none());
}

// ── Accept ghost text ──────────────────────────────────────────────────────

#[test]
fn test_accept_ghost_inserts_text() {
    let mut e = engine();
    // Set up a buffer with content and cursor
    e.handle_key("i", Some('i'), false); // enter insert mode
                                         // Type "hello " first
    for ch in "hello ".chars() {
        e.handle_key("", Some(ch), false);
    }
    // Ghost text appears after typing stops (debounce), set it now
    e.ai_ghost_text = Some("world".to_string());
    // Now accept ghost text
    e.ai_accept_ghost();
    let line = e.buffer().content.line(0).to_string();
    assert!(line.contains("hello world"), "got: {line}");
}

#[test]
fn test_accept_ghost_empty_is_noop() {
    let mut e = engine();
    e.ai_ghost_text = Some(String::new());
    e.ai_accept_ghost();
    // No changes, ghost is cleared
    assert!(e.ai_ghost_text.is_none());
}

#[test]
fn test_accept_ghost_clears_state() {
    let mut e = engine();
    e.ai_ghost_text = Some("test".to_string());
    e.ai_ghost_alternatives = vec!["test".to_string(), "other".to_string()];
    e.ai_ghost_alt_idx = 1;
    e.ai_accept_ghost();
    assert!(e.ai_ghost_text.is_none());
    assert!(e.ai_ghost_alternatives.is_empty());
    assert_eq!(e.ai_ghost_alt_idx, 0);
}

// ── Ghost text cleared on non-Tab keys in insert mode ─────────────────────

#[test]
fn test_ghost_cleared_on_non_matching_char_in_insert() {
    let mut e = engine();
    e.handle_key("i", Some('i'), false); // enter insert mode
    e.ai_ghost_text = Some("suggestion".to_string());
    // Typing a character that does NOT match the ghost prefix clears it
    e.handle_key("", Some('x'), false);
    assert!(e.ai_ghost_text.is_none());
}

#[test]
fn test_ghost_consumed_on_matching_char_in_insert() {
    let mut e = engine();
    e.handle_key("i", Some('i'), false);
    // Ghost starts with `"` — simulate user typing `"` after AI included it
    e.ai_ghost_text = Some("\"PlayerObject\":".to_string());
    e.ai_ghost_alternatives = vec!["\"PlayerObject\":".to_string()];
    e.handle_key("\"", Some('"'), false);
    // Ghost should have consumed the leading `"`, not been cleared
    assert_eq!(e.ai_ghost_text.as_deref(), Some("PlayerObject\":"));
}

#[test]
fn test_ghost_cleared_when_consumed_to_empty() {
    let mut e = engine();
    e.handle_key("i", Some('i'), false);
    e.ai_ghost_text = Some("x".to_string());
    e.ai_ghost_alternatives = vec!["x".to_string()];
    // Typing the only char in ghost should clear it (nothing left to show)
    e.handle_key("x", Some('x'), false);
    assert!(e.ai_ghost_text.is_none());
}

#[test]
fn test_ghost_accepted_via_tab_in_insert() {
    let mut e = engine();
    e.handle_key("i", Some('i'), false);
    // Disable expand_tab to simplify the test
    e.settings.expand_tab = false;
    e.ai_ghost_text = Some("suffix".to_string());
    e.handle_key("Tab", None, false);
    // Ghost text should be consumed
    assert!(e.ai_ghost_text.is_none());
    let line = e.buffer().content.line(0).to_string();
    assert!(line.contains("suffix"), "expected 'suffix' in: {line}");
}

// ── Poll receives completion from channel ──────────────────────────────────

#[test]
fn test_tick_receives_completion_from_channel() {
    let mut e = engine();
    let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<String>, String>>();
    e.ai_completion_rx = Some(rx);
    e.settings.ai_completions = true;

    // Nothing yet — no redraw
    assert!(!e.tick_ai_completion());

    // Send a completion
    tx.send(Ok(vec!["auto_complete".to_string()])).unwrap();
    // tick_ai_completion picks it up
    assert!(e.tick_ai_completion());
    assert_eq!(e.ai_ghost_text.as_deref(), Some("auto_complete"));
    assert_eq!(e.ai_ghost_alternatives.len(), 1);
}

#[test]
fn test_tick_ignores_error_response() {
    let mut e = engine();
    let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<String>, String>>();
    e.ai_completion_rx = Some(rx);
    e.settings.ai_completions = true;
    tx.send(Err("network error".to_string())).unwrap();
    // Error is silently ignored (no redraw signal)
    assert!(!e.tick_ai_completion());
    assert!(e.ai_ghost_text.is_none());
}

#[test]
fn test_tick_ignores_empty_alternatives() {
    let mut e = engine();
    let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<String>, String>>();
    e.ai_completion_rx = Some(rx);
    e.settings.ai_completions = true;
    tx.send(Ok(vec![])).unwrap();
    // Empty vec → no ghost text set, no redraw
    assert!(!e.tick_ai_completion());
    assert!(e.ai_ghost_text.is_none());
}

// ── Prefix overlap stripping ───────────────────────────────────────────────

#[test]
fn test_completion_strips_repeated_prefix_char() {
    // AI returns `"PlayerObject":` but cursor is already after `"` in the buffer.
    // The engine should strip the repeated `"` so the ghost shows `PlayerObject":`.
    let mut e = engine();
    let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<String>, String>>();
    e.ai_completion_rx = Some(rx);
    e.settings.ai_completions = true;
    // Simulate: prefix tail ended with `"`
    e.ai_completion_prefix_tail = "case \"".to_string();
    tx.send(Ok(vec!["\"PlayerObject\":".to_string()])).unwrap();
    assert!(e.tick_ai_completion());
    // The leading `"` should have been stripped
    assert_eq!(e.ai_ghost_text.as_deref(), Some("PlayerObject\":"));
}

#[test]
fn test_completion_no_overlap_unchanged() {
    // AI returns `world` and buffer ends with `hello ` — no overlap, unchanged.
    let mut e = engine();
    let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<String>, String>>();
    e.ai_completion_rx = Some(rx);
    e.settings.ai_completions = true;
    e.ai_completion_prefix_tail = "hello ".to_string();
    tx.send(Ok(vec!["world".to_string()])).unwrap();
    assert!(e.tick_ai_completion());
    assert_eq!(e.ai_ghost_text.as_deref(), Some("world"));
}

#[test]
fn test_completion_strips_multi_char_overlap() {
    // AI returns `fn foo()` when prefix ends with `fn ` — strip `fn `.
    let mut e = engine();
    let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<String>, String>>();
    e.ai_completion_rx = Some(rx);
    e.settings.ai_completions = true;
    e.ai_completion_prefix_tail = "fn ".to_string();
    tx.send(Ok(vec!["fn foo()".to_string()])).unwrap();
    assert!(e.tick_ai_completion());
    assert_eq!(e.ai_ghost_text.as_deref(), Some("foo()"));
}

// ── settings.ai_completions round-trip ────────────────────────────────────

#[test]
fn test_ai_completions_setting_get_set() {
    let mut settings = vimcode_core::core::settings::Settings::default();
    assert_eq!(settings.get_value_str("ai_completions"), "false");
    settings.set_value_str("ai_completions", "true").unwrap();
    assert!(settings.ai_completions);
    assert_eq!(settings.get_value_str("ai_completions"), "true");
}
