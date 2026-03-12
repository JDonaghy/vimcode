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

#[test]
fn set_items_populates_engine_state() {
    let mut e = engine_with("");
    let items = vec![
        ExtPanelItem {
            text: "Hello".to_string(),
            hint: "world".to_string(),
            icon: "●".to_string(),
            indent: 0,
            style: ExtPanelStyle::Normal,
            id: "id1".to_string(),
        },
        ExtPanelItem {
            text: "Goodbye".to_string(),
            hint: "".to_string(),
            icon: "".to_string(),
            indent: 1,
            style: ExtPanelStyle::Dim,
            id: "id2".to_string(),
        },
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
            ExtPanelItem {
                text: "a1".into(),
                hint: "".into(),
                icon: "".into(),
                indent: 0,
                style: ExtPanelStyle::Normal,
                id: "a1".into(),
            },
            ExtPanelItem {
                text: "a2".into(),
                hint: "".into(),
                icon: "".into(),
                indent: 0,
                style: ExtPanelStyle::Normal,
                id: "a2".into(),
            },
            ExtPanelItem {
                text: "a3".into(),
                hint: "".into(),
                icon: "".into(),
                indent: 0,
                style: ExtPanelStyle::Normal,
                id: "a3".into(),
            },
        ],
    );
    // Section B: 2 items
    e.ext_panel_items.insert(
        ("tp".to_string(), "B".to_string()),
        vec![
            ExtPanelItem {
                text: "b1".into(),
                hint: "".into(),
                icon: "".into(),
                indent: 0,
                style: ExtPanelStyle::Normal,
                id: "b1".into(),
            },
            ExtPanelItem {
                text: "b2".into(),
                hint: "".into(),
                icon: "".into(),
                indent: 0,
                style: ExtPanelStyle::Normal,
                id: "b2".into(),
            },
        ],
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
