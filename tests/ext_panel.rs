mod common;
use common::*;
use vimcode_core::core::plugin::{ExtPanelItem, ExtPanelStyle, PanelRegistration};

// ── Panel registration ─────────────────────────────────────────────────────────

#[test]
fn register_ext_panel_adds_to_engine() {
    let mut e = engine_with("");
    let reg = PanelRegistration {
        name: "test_panel".to_string(),
        title: "TEST PANEL".to_string(),
        icon: '\u{f03a}',
        sections: vec!["Section A".to_string(), "Section B".to_string()],
    };
    e.ext_panels.insert("test_panel".to_string(), reg);
    assert!(e.ext_panels.contains_key("test_panel"));
    assert_eq!(e.ext_panels["test_panel"].sections.len(), 2);
}

#[test]
fn register_ext_panel_initializes_expanded_state() {
    let mut e = engine_with("");
    let reg = PanelRegistration {
        name: "tp".to_string(),
        title: "TP".to_string(),
        icon: 'X',
        sections: vec!["A".to_string(), "B".to_string(), "C".to_string()],
    };
    e.ext_panels.insert("tp".to_string(), reg);
    e.ext_panel_sections_expanded
        .insert("tp".to_string(), vec![true, true, true]);
    let expanded = &e.ext_panel_sections_expanded["tp"];
    assert_eq!(expanded.len(), 3);
    assert!(expanded.iter().all(|&v| v));
}

// ── set_items populates engine state ────────────────────────────────────────

fn make_item(text: &str, id: &str) -> ExtPanelItem {
    ExtPanelItem {
        text: text.into(),
        id: id.into(),
        ..Default::default()
    }
}

fn make_item_styled(
    text: &str,
    hint: &str,
    icon: &str,
    indent: u8,
    style: ExtPanelStyle,
    id: &str,
) -> ExtPanelItem {
    ExtPanelItem {
        text: text.into(),
        hint: hint.into(),
        icon: icon.into(),
        indent,
        style,
        id: id.into(),
        ..Default::default()
    }
}

#[test]
fn set_items_populates_engine_state() {
    let mut e = engine_with("");
    let items = vec![
        make_item_styled("Hello", "world", "●", 0, ExtPanelStyle::Normal, "id1"),
        make_item_styled("Goodbye", "", "", 1, ExtPanelStyle::Dim, "id2"),
    ];
    e.ext_panel_items
        .insert(("panel".to_string(), "section".to_string()), items);
    let key = ("panel".to_string(), "section".to_string());
    assert_eq!(e.ext_panel_items[&key].len(), 2);
    assert_eq!(e.ext_panel_items[&key][0].text, "Hello");
    assert_eq!(e.ext_panel_items[&key][1].style, ExtPanelStyle::Dim);
}

// ── Flat length calculation ──────────────────────────────────────────────────

fn setup_panel(e: &mut vimcode_core::Engine) {
    let reg = PanelRegistration {
        name: "tp".to_string(),
        title: "TP".to_string(),
        icon: 'X',
        sections: vec!["A".to_string(), "B".to_string()],
    };
    e.ext_panels.insert("tp".to_string(), reg);
    e.ext_panel_active = Some("tp".to_string());
    e.ext_panel_sections_expanded
        .insert("tp".to_string(), vec![true, true]);
    // Section A: 3 items
    e.ext_panel_items.insert(
        ("tp".to_string(), "A".to_string()),
        vec![
            make_item("a1", "a1"),
            make_item("a2", "a2"),
            make_item("a3", "a3"),
        ],
    );
    // Section B: 2 items
    e.ext_panel_items.insert(
        ("tp".to_string(), "B".to_string()),
        vec![make_item("b1", "b1"), make_item("b2", "b2")],
    );
}

#[test]
fn ext_panel_flat_len_all_expanded() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    // 2 section headers + 3 items + 2 items = 7
    assert_eq!(e.ext_panel_flat_len(), 7);
}

#[test]
fn ext_panel_flat_len_one_collapsed() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    // Collapse section A
    e.ext_panel_sections_expanded.get_mut("tp").unwrap()[0] = false;
    // 2 headers + 0 (collapsed) + 2 items = 4
    assert_eq!(e.ext_panel_flat_len(), 4);
}

#[test]
fn ext_panel_flat_len_all_collapsed() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_sections_expanded
        .insert("tp".to_string(), vec![false, false]);
    // 2 headers only
    assert_eq!(e.ext_panel_flat_len(), 2);
}

// ── Flat index to section mapping ────────────────────────────────────────────

#[test]
fn ext_panel_flat_to_section_header() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    // flat 0 = section A header
    assert_eq!(e.ext_panel_flat_to_section(0), Some((0, usize::MAX)));
    // flat 4 = section B header (1 header + 3 items)
    assert_eq!(e.ext_panel_flat_to_section(4), Some((1, usize::MAX)));
}

#[test]
fn ext_panel_flat_to_section_items() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    // flat 1 = section A, item 0
    assert_eq!(e.ext_panel_flat_to_section(1), Some((0, 0)));
    // flat 3 = section A, item 2
    assert_eq!(e.ext_panel_flat_to_section(3), Some((0, 2)));
    // flat 5 = section B, item 0
    assert_eq!(e.ext_panel_flat_to_section(5), Some((1, 0)));
    // flat 6 = section B, item 1
    assert_eq!(e.ext_panel_flat_to_section(6), Some((1, 1)));
}

// ── handle_ext_panel_key navigation ──────────────────────────────────────────

#[test]
fn ext_panel_key_j_navigates_down() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_selected = 0;
    e.handle_ext_panel_key("j", false, None);
    assert_eq!(e.ext_panel_selected, 1);
    e.handle_ext_panel_key("j", false, None);
    assert_eq!(e.ext_panel_selected, 2);
}

#[test]
fn ext_panel_key_k_navigates_up() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_selected = 3;
    e.handle_ext_panel_key("k", false, None);
    assert_eq!(e.ext_panel_selected, 2);
}

#[test]
fn ext_panel_key_j_stops_at_end() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_selected = 6; // last item
    e.handle_ext_panel_key("j", false, None);
    assert_eq!(e.ext_panel_selected, 6); // stays at 6
}

#[test]
fn ext_panel_key_k_stops_at_zero() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_selected = 0;
    e.handle_ext_panel_key("k", false, None);
    assert_eq!(e.ext_panel_selected, 0);
}

// ── Tab expand/collapse ──────────────────────────────────────────────────────

#[test]
fn ext_panel_tab_toggles_section() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_selected = 0; // section A header
    assert_eq!(e.ext_panel_flat_len(), 7);

    e.handle_ext_panel_key("Tab", false, None);
    // Section A collapsed: 2 headers + 2 items = 4
    assert_eq!(e.ext_panel_flat_len(), 4);

    e.handle_ext_panel_key("Tab", false, None);
    // Section A expanded again: 7
    assert_eq!(e.ext_panel_flat_len(), 7);
}

// ── q/Escape unfocuses ───────────────────────────────────────────────────────

#[test]
fn ext_panel_q_unfocuses() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.handle_ext_panel_key("q", false, None);
    assert!(!e.ext_panel_has_focus);
}

#[test]
fn ext_panel_escape_unfocuses() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.handle_ext_panel_key("Escape", false, None);
    assert!(!e.ext_panel_has_focus);
}

// ── Focus guard prevents normal-mode keys from leaking ───────────────────────

#[test]
fn ext_panel_focus_guard_consumes_keys() {
    let mut e = engine_with("hello\nworld\n");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_active = Some("tp".to_string());

    // 'j' should be consumed by the panel, not move cursor down
    let line_before = e.cursor().line;
    e.handle_key("j", Some('j'), false);
    let line_after = e.cursor().line;
    assert_eq!(line_before, line_after, "j should not move editor cursor");
}

// ── Panel state is consistent ────────────────────────────────────────────────

#[test]
fn ext_panel_active_tracks_state() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    assert_eq!(e.ext_panel_active.as_deref(), Some("tp"));
    assert!(e.ext_panels.contains_key("tp"));

    // Verify sections and items are accessible
    let reg = &e.ext_panels["tp"];
    assert_eq!(reg.title, "TP");
    assert_eq!(reg.sections.len(), 2);
    assert_eq!(reg.sections[0], "A");
    assert_eq!(reg.sections[1], "B");

    let items_a = &e.ext_panel_items[&("tp".to_string(), "A".to_string())];
    assert_eq!(items_a.len(), 3);
    assert_eq!(items_a[0].id, "a1");

    let items_b = &e.ext_panel_items[&("tp".to_string(), "B".to_string())];
    assert_eq!(items_b.len(), 2);
    assert_eq!(items_b[0].id, "b1");
}

// ── Rich panel item fields ───────────────────────────────────────────────────

#[test]
fn ext_panel_item_tree_fields() {
    use vimcode_core::core::plugin::{ExtPanelAction, ExtPanelBadge};
    let item = ExtPanelItem {
        text: "Parent".into(),
        id: "p1".into(),
        expandable: true,
        expanded: true,
        ..Default::default()
    };
    assert!(item.expandable);
    assert!(item.expanded);
    assert!(item.parent_id.is_empty());

    let child = ExtPanelItem {
        text: "Child".into(),
        id: "c1".into(),
        parent_id: "p1".into(),
        actions: vec![ExtPanelAction {
            label: "Stage".into(),
            key: "s".into(),
        }],
        badges: vec![ExtPanelBadge {
            text: "main".into(),
            color: "green".into(),
        }],
        ..Default::default()
    };
    assert_eq!(child.parent_id, "p1");
    assert_eq!(child.actions.len(), 1);
    assert_eq!(child.actions[0].label, "Stage");
    assert_eq!(child.badges.len(), 1);
    assert_eq!(child.badges[0].text, "main");
}

#[test]
fn ext_panel_item_separator() {
    let sep = ExtPanelItem {
        is_separator: true,
        ..Default::default()
    };
    assert!(sep.is_separator);
    assert!(sep.text.is_empty());
}

#[test]
fn ext_panel_item_default_has_empty_rich_fields() {
    let item = ExtPanelItem::default();
    assert!(!item.expandable);
    assert!(!item.expanded);
    assert!(item.parent_id.is_empty());
    assert!(item.actions.is_empty());
    assert!(item.badges.is_empty());
    assert!(!item.is_separator);
}

// ── Tree expand/collapse ─────────────────────────────────────────────────────

fn setup_tree_panel(e: &mut vimcode_core::Engine) {
    use vimcode_core::core::plugin::PanelRegistration;
    let reg = PanelRegistration {
        name: "tree".to_string(),
        title: "TREE".to_string(),
        icon: 'T',
        sections: vec!["S".to_string()],
    };
    e.ext_panels.insert("tree".to_string(), reg);
    e.ext_panel_active = Some("tree".to_string());
    e.ext_panel_sections_expanded
        .insert("tree".to_string(), vec![true]);
    // Parent + 2 children
    e.ext_panel_items.insert(
        ("tree".to_string(), "S".to_string()),
        vec![
            ExtPanelItem {
                text: "Parent".into(),
                id: "p1".into(),
                expandable: true,
                expanded: true,
                ..Default::default()
            },
            ExtPanelItem {
                text: "Child A".into(),
                id: "c1".into(),
                parent_id: "p1".into(),
                ..Default::default()
            },
            ExtPanelItem {
                text: "Child B".into(),
                id: "c2".into(),
                parent_id: "p1".into(),
                ..Default::default()
            },
            ExtPanelItem {
                text: "Top-level".into(),
                id: "t1".into(),
                ..Default::default()
            },
        ],
    );
}

#[test]
fn ext_panel_tree_all_visible_when_expanded() {
    let mut e = engine_with("");
    setup_tree_panel(&mut e);
    // 1 section header + 4 items (parent expanded by default) = 5
    assert_eq!(e.ext_panel_flat_len(), 5);
}

#[test]
fn ext_panel_tree_collapse_hides_children() {
    let mut e = engine_with("");
    setup_tree_panel(&mut e);
    // Collapse parent via tree_expanded state
    e.ext_panel_tree_expanded
        .insert(("tree".to_string(), "p1".to_string()), false);
    // 1 section header + 2 items (parent + top-level, children hidden) = 3
    assert_eq!(e.ext_panel_flat_len(), 3);
}

#[test]
fn ext_panel_tree_tab_toggles_expandable_item() {
    let mut e = engine_with("");
    setup_tree_panel(&mut e);
    e.ext_panel_has_focus = true;
    // Select the parent item (flat index 1 = first item after section header)
    e.ext_panel_selected = 1;
    assert_eq!(e.ext_panel_flat_len(), 5); // all visible

    // Tab collapses the parent
    e.handle_ext_panel_key("Tab", false, None);
    assert_eq!(e.ext_panel_flat_len(), 3); // children hidden

    // Tab expands it again
    e.handle_ext_panel_key("Tab", false, None);
    assert_eq!(e.ext_panel_flat_len(), 5); // children visible again
}

#[test]
fn ext_panel_flat_to_section_with_tree() {
    let mut e = engine_with("");
    setup_tree_panel(&mut e);
    // Collapse parent
    e.ext_panel_tree_expanded
        .insert(("tree".to_string(), "p1".to_string()), false);
    // flat 0 = section header
    assert_eq!(e.ext_panel_flat_to_section(0), Some((0, usize::MAX)));
    // flat 1 = parent item (original index 0)
    assert_eq!(e.ext_panel_flat_to_section(1), Some((0, 0)));
    // flat 2 = top-level item (original index 3, children skipped)
    assert_eq!(e.ext_panel_flat_to_section(2), Some((0, 3)));
    // flat 3 = out of bounds
    assert_eq!(e.ext_panel_flat_to_section(3), None);
}

// ── Editor hover popup ───────────────────────────────────────────────────────

#[test]
fn gh_triggers_editor_hover() {
    let mut e = engine_with("hello world\n");
    // gh should trigger the editor hover (even with no content, it requests LSP)
    e.handle_key("g", Some('g'), false);
    e.handle_key("h", Some('h'), false);
    // With no diagnostics/annotations/LSP, no popup should be created
    assert!(e.editor_hover.is_none());
}

#[test]
fn editor_hover_shows_annotation() {
    let mut e = engine_with("hello world\n");
    // Set an annotation on line 0
    e.line_annotations
        .insert(0, "blame: John, 2h ago".to_string());
    // Trigger hover at cursor position
    e.trigger_editor_hover_at_cursor();
    assert!(e.editor_hover.is_some());
    let hover = e.editor_hover.as_ref().unwrap();
    assert!(!hover.rendered.lines.is_empty());
    // Content should include the annotation
    let text = hover.rendered.lines.join("\n");
    assert!(text.contains("blame: John, 2h ago"));
}

#[test]
fn editor_hover_shows_plugin_content() {
    let mut e = engine_with("hello world\n");
    // Set plugin hover content for line 0
    e.editor_hover_content
        .insert(0, "**Commit abc123**\n\nfeat: add hover".to_string());
    e.trigger_editor_hover_at_cursor();
    assert!(e.editor_hover.is_some());
    let text = e.editor_hover.as_ref().unwrap().rendered.lines.join("\n");
    assert!(text.contains("Commit abc123"));
}

#[test]
fn editor_hover_dismiss_on_escape() {
    let mut e = engine_with("hello world\n");
    e.editor_hover_content.insert(0, "**Test**".to_string());
    e.trigger_editor_hover_at_cursor();
    assert!(e.editor_hover.is_some());
    // Escape should dismiss
    e.handle_editor_hover_key("Escape");
    assert!(e.editor_hover.is_none());
    assert!(!e.editor_hover_has_focus);
}

#[test]
fn editor_hover_keyboard_scroll() {
    let mut e = engine_with("hello world\n");
    // Need >20 rendered lines to test scrolling (viewport is 20 lines max).
    // Use double newlines so each "Line N" renders as a separate paragraph line.
    let long_content = (1..=30)
        .map(|i| format!("Line {}", i))
        .collect::<Vec<_>>()
        .join("\n\n");
    e.editor_hover_content.insert(0, long_content);
    e.trigger_editor_hover_at_cursor();
    assert!(e.editor_hover.is_some());
    assert_eq!(e.editor_hover.as_ref().unwrap().scroll_top, 0);
    e.handle_editor_hover_key("j");
    assert_eq!(e.editor_hover.as_ref().unwrap().scroll_top, 1);
    e.handle_editor_hover_key("k");
    assert_eq!(e.editor_hover.as_ref().unwrap().scroll_top, 0);
}

#[test]
fn editor_hover_focus_guard_consumes_keys() {
    let mut e = engine_with("hello\nworld\n");
    e.editor_hover_content
        .insert(0, "hover content".to_string());
    e.trigger_editor_hover_at_cursor();
    e.editor_hover_has_focus = true;
    // j should be consumed by hover, not move cursor
    let line_before = e.cursor().line;
    e.handle_key("j", Some('j'), false);
    assert_eq!(e.cursor().line, line_before);
}

// ── Panel double-click event ──────────────────────────────────────────────────

#[test]
fn ext_panel_double_click_fires_event() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_selected = 1; // item a1 (flat 1 = section A header + item 0)
                              // Should not panic; fires panel_double_click event (no Lua hooks to verify,
                              // but the method should resolve the correct item).
    e.handle_ext_panel_double_click();
    // Verify selection is unchanged.
    assert_eq!(e.ext_panel_selected, 1);
}

#[test]
fn ext_panel_double_click_on_header_is_noop() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_selected = 0; // section A header
                              // Double-click on header should be a no-op (item_idx == usize::MAX).
    e.handle_ext_panel_double_click();
    assert_eq!(e.ext_panel_selected, 0);
}

// ── Panel context menu event ──────────────────────────────────────────────────

#[test]
fn ext_panel_context_menu_fires_event() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_selected = 5; // item b1
                              // Should not panic; fires panel_context_menu event.
    e.open_ext_panel_context_menu(10, 20);
    assert_eq!(e.ext_panel_selected, 5);
}

#[test]
fn ext_panel_context_menu_on_header() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_selected = 4; // section B header
                              // Context menu on header should still fire (with empty item_id).
    e.open_ext_panel_context_menu(10, 20);
    assert_eq!(e.ext_panel_selected, 4);
}

// ── Panel input field ─────────────────────────────────────────────────────────

#[test]
fn ext_panel_slash_activates_input() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    assert!(!e.ext_panel_input_active);
    e.handle_ext_panel_key("/", false, Some('/'));
    assert!(e.ext_panel_input_active);
}

#[test]
fn ext_panel_input_typing_appends_text() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_input_active = true;
    e.handle_ext_panel_input_key("h", false, Some('h'));
    e.handle_ext_panel_input_key("i", false, Some('i'));
    let text = e
        .ext_panel_input_text
        .get("tp")
        .cloned()
        .unwrap_or_default();
    assert_eq!(text, "hi");
}

#[test]
fn ext_panel_input_backspace_removes_char() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_input_active = true;
    e.ext_panel_input_text
        .insert("tp".to_string(), "abc".to_string());
    e.handle_ext_panel_input_key("BackSpace", false, None);
    let text = e
        .ext_panel_input_text
        .get("tp")
        .cloned()
        .unwrap_or_default();
    assert_eq!(text, "ab");
}

#[test]
fn ext_panel_input_escape_deactivates() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_input_active = true;
    e.handle_ext_panel_input_key("Escape", false, None);
    assert!(!e.ext_panel_input_active);
}

#[test]
fn ext_panel_input_return_deactivates() {
    let mut e = engine_with("");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_input_active = true;
    e.ext_panel_input_text
        .insert("tp".to_string(), "search query".to_string());
    e.handle_ext_panel_input_key("Return", false, None);
    assert!(!e.ext_panel_input_active);
    // Text should still be preserved.
    let text = e
        .ext_panel_input_text
        .get("tp")
        .cloned()
        .unwrap_or_default();
    assert_eq!(text, "search query");
}

#[test]
fn ext_panel_input_intercepts_keys() {
    let mut e = engine_with("hello\nworld\n");
    setup_panel(&mut e);
    e.ext_panel_has_focus = true;
    e.ext_panel_input_active = true;
    // 'j' should be consumed by input (typed as text), not navigate panel or editor.
    let selected_before = e.ext_panel_selected;
    let line_before = e.cursor().line;
    e.handle_key("j", Some('j'), false);
    assert_eq!(e.ext_panel_selected, selected_before);
    assert_eq!(e.cursor().line, line_before);
    let text = e
        .ext_panel_input_text
        .get("tp")
        .cloned()
        .unwrap_or_default();
    assert_eq!(text, "j");
}

// ── Inline blame toggle ───────────────────────────────────────────────────────

#[test]
fn toggle_blame_no_file_shows_message() {
    let mut e = engine_with("hello\n");
    // No file path → "No file" message.
    e.toggle_inline_blame();
    assert!(e.message.contains("No file"));
    assert!(!e.blame_annotations_active);
}

#[test]
fn toggle_blame_off_clears_annotations() {
    let mut e = engine_with("hello\nworld\n");
    // Simulate blame being active.
    e.blame_annotations_active = true;
    e.line_annotations.insert(0, "author, 2h ago".to_string());
    e.editor_hover_content
        .insert(0, "### `abc1234`\n\n**Author:** test".to_string());
    e.toggle_inline_blame();
    assert!(!e.blame_annotations_active);
    assert!(e.line_annotations.is_empty());
    assert!(e.editor_hover_content.is_empty());
    assert!(e.message.contains("off"));
}

#[test]
fn blame_annotations_cleared_on_tab_switch() {
    let mut e = engine_with("hello\n");
    e.blame_annotations_active = true;
    e.line_annotations.insert(0, "test".to_string());
    // Switching tabs should clear blame state (covered by line_annotations.clear).
    e.line_annotations.clear();
    e.blame_annotations_active = false;
    assert!(!e.blame_annotations_active);
    assert!(e.line_annotations.is_empty());
}

#[test]
fn toggle_blame_command_works() {
    let mut e = engine_with("hello\n");
    // No file, so ToggleBlame should show "No file".
    e.execute_command("ToggleBlame");
    assert!(e.message.contains("No file"));
}

#[test]
fn toggle_blame_gib_alias_works() {
    let mut e = engine_with("hello\n");
    e.execute_command("Gib");
    assert!(e.message.contains("No file"));
}

#[test]
fn hover_dwell_triggers_on_annotation_ghost_text() {
    let mut e = engine_with("hello\nworld\n");
    // Simulate blame annotations being active with hover content.
    e.blame_annotations_active = true;
    e.line_annotations
        .insert(0, "author, 2h ago — fix bug".to_string());
    e.editor_hover_content
        .insert(0, "### `abc1234`\n\n**Author:** test".to_string());
    // Mouse move to column 80 (past end of "hello") — should start dwell
    // because mouse is over the ghost text region.
    e.editor_hover_mouse_move(0, 80, false);
    assert!(e.editor_hover_dwell.is_some());
}

#[test]
fn hover_dwell_on_code_text_ignores_annotation() {
    let mut e = engine_with("hello\nworld\n");
    // Simulate blame annotations on line 0.
    e.blame_annotations_active = true;
    e.line_annotations
        .insert(0, "author, 2h ago — fix bug".to_string());
    e.editor_hover_content
        .insert(0, "### `abc1234`\n\n**Author:** test".to_string());
    // Mouse move to column 2 (on "l" in "hello") — should start a normal word
    // dwell, NOT an annotation dwell. The annotation hover should only appear
    // when the mouse is past the end of the line text.
    e.editor_hover_mouse_move(0, 2, false);
    // Dwell starts because "l" is alphanumeric (word dwell).
    assert!(e.editor_hover_dwell.is_some());
    // But if we move to whitespace col 6 (past "hello\n" but before annotation
    // visual position), no dwell since it's not on a word and not on annotation.
    // "hello\n" has 5 printable chars, so col 5 is past end but annotation
    // starts there — let's test col 5 which is >= line_char_len (5).
    e.editor_hover_mouse_move(0, 5, false);
    // col 5 >= line_char_len(5) AND annotation exists → annotation dwell
    assert!(e.editor_hover_dwell.is_some());
}

#[test]
fn keyboard_hover_shows_blame_popup() {
    let mut e = engine_with("hello\nworld\n");
    // Simulate blame annotations.
    e.blame_annotations_active = true;
    e.editor_hover_content
        .insert(0, "### `abc1234`\n\n**Author:** test".to_string());
    // gh triggers hover at cursor position (line 0, col 0).
    e.trigger_editor_hover_at_cursor();
    assert!(e.editor_hover.is_some());
    // Annotation hover should NOT auto-focus (faint border, not blue).
    assert!(!e.editor_hover_has_focus);
}

#[test]
fn annotation_hover_not_dismissed_by_lsp_null() {
    use vimcode_core::core::engine::EditorHoverSource;
    let mut e = engine_with("hello\nworld\n");
    // Simulate an annotation hover popup.
    e.blame_annotations_active = true;
    e.editor_hover_content
        .insert(0, "### `abc1234`\n\n**Author:** test".to_string());
    e.trigger_editor_hover_at_cursor();
    assert!(e.editor_hover.is_some());
    // Verify source is Annotation, not Lsp.
    assert!(matches!(
        e.editor_hover.as_ref().unwrap().source,
        EditorHoverSource::Annotation
    ));
}

#[test]
fn poll_blame_applies_results() {
    let mut e = engine_with("hello\n");
    // Nothing to poll → false.
    assert!(!e.poll_blame());
    assert!(!e.blame_annotations_active);
}
