//! Phase B.2 — accelerator registry + UiEvent dispatch.
//!
//! Validates the engine-owned accelerator pattern that B.2 ships:
//! `register_accelerator` → `match_accelerator` → `handle_ui_event`. See
//! `quadraui/docs/BACKEND_TRAIT_PROPOSAL.md` §11 for the design.

mod common;
use common::*;
use vimcode_core::core::engine::{
    Accelerator, AcceleratorId, AcceleratorScope, KeyBinding, UiEvent, UiEventContext,
};
use vimcode_core::quadraui::Modifiers;

#[test]
fn default_registry_has_terminal_maximize() {
    let e = engine_with("");
    let ids: Vec<&str> = e.accelerators.iter().map(|r| r.acc.id.as_str()).collect();
    assert!(
        ids.contains(&"terminal.toggle_maximize"),
        "default accelerators should include terminal.toggle_maximize, got: {ids:?}"
    );
}

#[test]
fn match_accelerator_finds_default_binding() {
    let e = engine_with("");
    // Default binding is <C-S-t>; press Ctrl+Shift+T.
    let id = e.match_accelerator(true, true, false, Some('t'), false, false, false);
    assert_eq!(
        id.as_ref().map(|i| i.as_str()),
        Some("terminal.toggle_maximize")
    );
}

#[test]
fn match_accelerator_rejects_wrong_modifiers() {
    let e = engine_with("");
    // Without Shift: should not match the default <C-S-t> binding.
    assert!(e
        .match_accelerator(true, false, false, Some('t'), false, false, false)
        .is_none());
    // Without Ctrl: should not match.
    assert!(e
        .match_accelerator(false, true, false, Some('t'), false, false, false)
        .is_none());
    // Wrong key: should not match.
    assert!(e
        .match_accelerator(true, true, false, Some('q'), false, false, false)
        .is_none());
}

#[test]
fn match_accelerator_case_insensitive_letter() {
    let e = engine_with("");
    // Both 'T' and 't' should match — the parser lowercases internally.
    let lower = e.match_accelerator(true, true, false, Some('t'), false, false, false);
    let upper = e.match_accelerator(true, true, false, Some('T'), false, false, false);
    assert!(lower.is_some());
    assert!(upper.is_some());
    assert_eq!(lower, upper);
}

#[test]
fn handle_ui_event_toggles_maximize_and_grows_panel() {
    let mut e = engine_with("");
    e.session.terminal_panel_rows = 12;
    assert!(!e.terminal_maximized);

    let ctx = UiEventContext {
        terminal_cols: 80,
        terminal_max_rows: 30,
    };
    e.handle_ui_event(
        UiEvent::Accelerator(
            AcceleratorId::new("terminal.toggle_maximize"),
            Modifiers {
                ctrl: true,
                shift: true,
                alt: false,
                cmd: false,
            },
        ),
        ctx,
    );

    assert!(e.terminal_maximized);
    assert!(e.terminal_open);
    assert_eq!(
        e.session.terminal_panel_rows, 12,
        "saved rows must not change"
    );
}

#[test]
fn handle_ui_event_idempotent_toggle() {
    let mut e = engine_with("");
    e.session.terminal_panel_rows = 10;
    let ctx = UiEventContext {
        terminal_cols: 80,
        terminal_max_rows: 30,
    };
    let ev = || {
        UiEvent::Accelerator(
            AcceleratorId::new("terminal.toggle_maximize"),
            Modifiers::default(),
        )
    };

    e.handle_ui_event(ev(), ctx);
    assert!(e.terminal_maximized);

    e.handle_ui_event(ev(), ctx);
    assert!(
        !e.terminal_maximized,
        "second toggle should restore non-maximized state"
    );
}

#[test]
fn handle_ui_event_unknown_accelerator_is_noop() {
    let mut e = engine_with("");
    e.handle_ui_event(
        UiEvent::Accelerator(
            AcceleratorId::new("nonexistent.action"),
            Modifiers::default(),
        ),
        UiEventContext {
            terminal_cols: 80,
            terminal_max_rows: 30,
        },
    );
    assert!(!e.terminal_maximized);
    assert!(!e.terminal_open);
}

#[test]
fn register_accelerator_is_idempotent_by_id() {
    let mut e = engine_with("");
    let before = e.accelerators.len();

    // Register the same id twice — should replace, not duplicate.
    e.register_accelerator(Accelerator {
        id: AcceleratorId::new("terminal.toggle_maximize"),
        binding: KeyBinding::Literal("<C-A-m>".into()),
        scope: AcceleratorScope::Global,
        label: None,
    });
    e.register_accelerator(Accelerator {
        id: AcceleratorId::new("terminal.toggle_maximize"),
        binding: KeyBinding::Literal("<C-A-n>".into()),
        scope: AcceleratorScope::Global,
        label: None,
    });

    assert_eq!(
        e.accelerators.len(),
        before,
        "re-registering same id must not duplicate"
    );
    // Latest binding wins.
    assert!(e
        .match_accelerator(true, false, true, Some('n'), false, false, false)
        .is_some());
    assert!(e
        .match_accelerator(true, false, true, Some('m'), false, false, false)
        .is_none());
}

#[test]
fn unregister_accelerator_removes_match() {
    let mut e = engine_with("");
    assert!(e
        .match_accelerator(true, true, false, Some('t'), false, false, false)
        .is_some());

    e.unregister_accelerator(&AcceleratorId::new("terminal.toggle_maximize"));

    assert!(e
        .match_accelerator(true, true, false, Some('t'), false, false, false)
        .is_none());
}

#[test]
fn match_accelerator_skips_non_global_scopes() {
    let mut e = engine_with("");
    e.unregister_accelerator(&AcceleratorId::new("terminal.toggle_maximize"));

    // Mode-scoped registration — B.2 only honours Global, so this must NOT match.
    e.register_accelerator(Accelerator {
        id: AcceleratorId::new("test.mode_scoped"),
        binding: KeyBinding::Literal("<C-S-t>".into()),
        scope: AcceleratorScope::Mode("normal".into()),
        label: None,
    });
    assert!(e
        .match_accelerator(true, true, false, Some('t'), false, false, false)
        .is_none());
}
