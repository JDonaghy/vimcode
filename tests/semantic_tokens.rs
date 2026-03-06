//! Integration tests for LSP semantic tokens.

mod common;
use common::engine_with;
use vimcode_core::core::lsp::{decode_semantic_tokens, SemanticToken, SemanticTokensLegend};

// ── decode_semantic_tokens round-trip tests ─────────────────────────────────

#[test]
fn semantic_tokens_decode_roundtrip() {
    let legend = SemanticTokensLegend {
        token_types: vec![
            "namespace".into(),
            "type".into(),
            "function".into(),
            "parameter".into(),
            "variable".into(),
        ],
        token_modifiers: vec!["declaration".into(), "readonly".into()],
    };
    // Encode: line 0, col 5, len 3, type=function(2), mods=declaration(bit0)
    //         line 0, col 10, len 4, type=parameter(3), mods=0
    //         line 3, col 2, len 6, type=variable(4), mods=readonly(bit1=2)
    let raw = vec![0, 5, 3, 2, 1, 0, 5, 4, 3, 0, 3, 2, 6, 4, 2];
    let tokens = decode_semantic_tokens(&raw, &legend);
    assert_eq!(tokens.len(), 3);

    assert_eq!(tokens[0].line, 0);
    assert_eq!(tokens[0].start_char, 5);
    assert_eq!(tokens[0].length, 3);
    assert_eq!(tokens[0].token_type, "function");
    assert_eq!(tokens[0].modifiers, vec!["declaration"]);

    assert_eq!(tokens[1].line, 0);
    assert_eq!(tokens[1].start_char, 10);
    assert_eq!(tokens[1].length, 4);
    assert_eq!(tokens[1].token_type, "parameter");
    assert!(tokens[1].modifiers.is_empty());

    assert_eq!(tokens[2].line, 3);
    assert_eq!(tokens[2].start_char, 2);
    assert_eq!(tokens[2].length, 6);
    assert_eq!(tokens[2].token_type, "variable");
    assert_eq!(tokens[2].modifiers, vec!["readonly"]);
}

#[test]
fn semantic_tokens_decode_empty_data() {
    let legend = SemanticTokensLegend {
        token_types: vec![],
        token_modifiers: vec![],
    };
    let tokens = decode_semantic_tokens(&[], &legend);
    assert!(tokens.is_empty());
}

#[test]
fn semantic_tokens_decode_partial_chunk_ignored() {
    let legend = SemanticTokensLegend {
        token_types: vec!["variable".into()],
        token_modifiers: vec![],
    };
    // 7 u32s = 1 complete chunk + 2 leftover (ignored by chunks_exact)
    let raw = vec![0, 0, 5, 0, 0, 1, 2];
    let tokens = decode_semantic_tokens(&raw, &legend);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].length, 5);
}

#[test]
fn semantic_tokens_decode_multiline_delta() {
    let legend = SemanticTokensLegend {
        token_types: vec!["keyword".into(), "function".into()],
        token_modifiers: vec![],
    };
    // line 0 col 0 len 2 type=keyword(0)
    // line 5 col 4 len 3 type=function(1)
    let raw = vec![0, 0, 2, 0, 0, 5, 4, 3, 1, 0];
    let tokens = decode_semantic_tokens(&raw, &legend);
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].line, 0);
    assert_eq!(tokens[0].start_char, 0);
    assert_eq!(tokens[0].token_type, "keyword");
    assert_eq!(tokens[1].line, 5);
    assert_eq!(tokens[1].start_char, 4);
    assert_eq!(tokens[1].token_type, "function");
}

// ── Buffer storage tests ─────────────────────────────────────────────────────

#[test]
fn buffer_state_semantic_tokens_starts_empty() {
    let e = engine_with("fn main() {}");
    let bid = e.active_buffer_id();
    let state = e.buffer_manager.get(bid).unwrap();
    assert!(state.semantic_tokens.is_empty());
}

#[test]
fn buffer_state_semantic_tokens_can_be_set() {
    let mut e = engine_with("let x = 42;");
    let bid = e.active_buffer_id();
    let state = e.buffer_manager.get_mut(bid).unwrap();
    state.semantic_tokens.push(SemanticToken {
        line: 0,
        start_char: 4,
        length: 1,
        token_type: "variable".into(),
        modifiers: vec!["declaration".into()],
    });
    assert_eq!(state.semantic_tokens.len(), 1);
    assert_eq!(state.semantic_tokens[0].token_type, "variable");
}

#[test]
fn buffer_state_semantic_tokens_cleared_on_set() {
    let mut e = engine_with("let x = 42;");
    let bid = e.active_buffer_id();
    let state = e.buffer_manager.get_mut(bid).unwrap();
    state.semantic_tokens.push(SemanticToken {
        line: 0,
        start_char: 0,
        length: 3,
        token_type: "keyword".into(),
        modifiers: vec![],
    });
    assert_eq!(state.semantic_tokens.len(), 1);
    state.semantic_tokens.clear();
    assert!(state.semantic_tokens.is_empty());
}

// ── Engine pending state tests ───────────────────────────────────────────────

#[test]
fn engine_semantic_tokens_pending_starts_empty() {
    let e = engine_with("hello");
    assert!(e.lsp_pending_semantic_tokens.is_empty());
}

#[test]
fn engine_semantic_tokens_pending_can_be_set() {
    let mut e = engine_with("hello");
    e.lsp_pending_semantic_tokens
        .insert(42, std::path::PathBuf::from("/tmp/test.rs"));
    assert_eq!(
        e.lsp_pending_semantic_tokens.get(&42).map(|p| p.as_path()),
        Some(std::path::Path::new("/tmp/test.rs"))
    );
}

#[test]
fn engine_semantic_tokens_pending_multiple_requests() {
    let mut e = engine_with("hello");
    e.lsp_pending_semantic_tokens
        .insert(1, std::path::PathBuf::from("/tmp/a.rs"));
    e.lsp_pending_semantic_tokens
        .insert(2, std::path::PathBuf::from("/tmp/b.rs"));
    assert_eq!(e.lsp_pending_semantic_tokens.len(), 2);
    // Removing one leaves the other.
    let removed = e.lsp_pending_semantic_tokens.remove(&1);
    assert_eq!(removed.unwrap().to_str().unwrap(), "/tmp/a.rs");
    assert_eq!(e.lsp_pending_semantic_tokens.len(), 1);
    assert!(e.lsp_pending_semantic_tokens.contains_key(&2));
}

// ── Legend parsing tests ─────────────────────────────────────────────────────

#[test]
fn semantic_tokens_legend_types_and_modifiers() {
    let legend = SemanticTokensLegend {
        token_types: vec!["namespace".into(), "type".into(), "class".into()],
        token_modifiers: vec!["declaration".into(), "static".into()],
    };
    assert_eq!(legend.token_types.len(), 3);
    assert_eq!(legend.token_modifiers.len(), 2);
    assert_eq!(legend.token_types[0], "namespace");
    assert_eq!(legend.token_modifiers[1], "static");
}

#[test]
fn semantic_tokens_out_of_range_type_index() {
    let legend = SemanticTokensLegend {
        token_types: vec!["keyword".into()],
        token_modifiers: vec![],
    };
    // type index 99 is out of bounds → empty string
    let raw = vec![0, 0, 3, 99, 0];
    let tokens = decode_semantic_tokens(&raw, &legend);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_type, "");
}

#[test]
fn semantic_tokens_multiple_modifiers_bitmask() {
    let legend = SemanticTokensLegend {
        token_types: vec!["variable".into()],
        token_modifiers: vec![
            "declaration".into(),
            "readonly".into(),
            "static".into(),
            "deprecated".into(),
        ],
    };
    // bits = 0b1010 = 10 → "readonly" (bit1) + "deprecated" (bit3)
    let raw = vec![0, 0, 5, 0, 10];
    let tokens = decode_semantic_tokens(&raw, &legend);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].modifiers, vec!["readonly", "deprecated"]);
}
