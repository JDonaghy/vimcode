mod common;
use common::*;
use vimcode_core::Mode;

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
    let buf_id = e.active_buffer_id();
    if let Some(state) = e.buffer_manager.get_mut(buf_id) {
        state.lsp_language_id = Some("rust".to_string());
    }

    // gcc with count 2 should comment 2 lines (native :Commentary)
    press(&mut e, '2');
    press(&mut e, 'g');
    press(&mut e, 'c');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "// let x = 1;", "line 1 should be commented");
    assert_eq!(lines[1], "// let y = 2;", "line 2 should be commented");
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
    // These should be silently ignored (bad format). Since #1151, "x" is a
    // valid mode letter (Visual-only) and a bare (non-`:`-prefixed) rhs like
    // "join" is a valid key-to-keys mapping ("map K to j,o,i,n"), so neither
    // of those is invalid anymore — an unrecognised mode letter is.
    let e = engine_with_keymaps(
        "hello\n",
        &[
            "z K :join", // invalid mode
            "n K",       // missing rhs
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

// ── :map family — vim per-mode ex commands (#1151) ──────────────────────────
//
// Before #1151, vimcode's only mapping command was `:map <mode> <keys>
// :<excmd>` — mode as a positional argument, target always an ex command.
// That is not vim's `:map`. These tests exercise vim's real syntax: mode
// comes from the *command name* (`:nnoremap`, `:imap`, …), and `{rhs}` can be
// a raw key sequence, not just `:excmd`.

#[test]
fn nnoremap_command_adds_keymap() {
    let mut e = engine_with("");
    assert!(e.user_keymaps.is_empty());
    exec(&mut e, "nnoremap K :join");
    assert_eq!(e.user_keymaps.len(), 1);
    assert_eq!(e.settings.keymaps.len(), 1);
    // Persisted in vimcode's storage format: mode "n" + noremap "!".
    assert_eq!(e.settings.keymaps[0], "n! K :join");
    assert!(e.message.contains("Mapped"));
}

#[test]
fn nnoremap_command_keymap_takes_effect_immediately() {
    let mut e = engine_with("aaa\nbbb\n");
    exec(&mut e, "nnoremap K :join");
    press(&mut e, 'K');
    assert_eq!(get_lines(&e)[0], "aaa bbb");
}

#[test]
fn nnoremap_command_no_duplicates() {
    let mut e = engine_with("");
    exec(&mut e, "nnoremap K :join");
    exec(&mut e, "nnoremap K :join");
    assert_eq!(e.settings.keymaps.len(), 1, "should not add duplicate");
}

#[test]
fn bare_map_command_expands_to_vims_combined_modes() {
    // vim's bare `:map`/`:noremap` targets Normal+Visual+Select+Operator-pending.
    let mut e = engine_with("aaa\nbbb\n");
    exec(&mut e, "noremap Q :join");
    assert_eq!(
        e.settings.keymaps.len(),
        4,
        "bare :noremap should expand to n,v,s,o: {:?}",
        e.settings.keymaps
    );
    press(&mut e, 'Q');
    assert_eq!(get_lines(&e)[0], "aaa bbb", "n-mode expansion should fire");
}

#[test]
fn bang_map_command_expands_to_insert_and_cmdline() {
    // vim's `:noremap!`/`:map!` (bang, no mode letter) target Insert+Command-line.
    let mut e = engine_with("");
    exec(&mut e, "noremap! zz <Esc>");
    assert_eq!(
        e.settings.keymaps.len(),
        2,
        ":noremap! should expand to i,c: {:?}",
        e.settings.keymaps
    );
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
fn nunmap_command_removes_keymap() {
    let mut e = engine_with_keymaps("aaa\nbbb\n", &["n K :join", "n J :delete"]);
    exec(&mut e, "nunmap K");
    assert_eq!(e.settings.keymaps.len(), 1);
    assert_eq!(e.settings.keymaps[0], "n J :delete");
    assert!(e.message.contains("Unmapped"));
    // K should no longer fire the keymap (falls through to built-in)
    assert_eq!(e.user_keymaps.len(), 1);
}

#[test]
fn unmap_command_nonexistent_shows_error() {
    let mut e = engine_with("");
    exec(&mut e, "nunmap Z");
    assert!(
        e.message.contains("No mapping found"),
        "should say not found: {}",
        e.message
    );
}

#[test]
fn nnoremap_command_bad_format_shows_usage() {
    let mut e = engine_with("");
    exec(&mut e, "nnoremap bad"); // no rhs
    assert!(
        e.message.contains("Usage"),
        "should show usage: {}",
        e.message
    );
}

#[test]
fn nmapclear_removes_only_that_modes_mappings() {
    let mut e = engine_with_keymaps("", &["n K :join", "v gc :delete"]);
    exec(&mut e, "nmapclear");
    assert_eq!(e.settings.keymaps.len(), 1);
    assert_eq!(e.settings.keymaps[0], "v gc :delete");
}

#[test]
fn bare_mapclear_matches_bare_maps_scope_not_everything() {
    // Bare `:mapclear` targets vim's combined n,v,s,o (`:map`'s scope) —
    // it must NOT also sweep i/c mappings, which need the bang form
    // (`:mapclear!`, matching `:map!`'s scope) instead.
    let mut e = engine_with_keymaps("", &["n K :join", "i! jk <Esc>"]);
    exec(&mut e, "mapclear");
    assert_eq!(
        e.settings.keymaps,
        vec!["i! jk <Esc>".to_string()],
        "bare :mapclear must leave insert-mode mappings alone"
    );
}

#[test]
fn bang_mapclear_targets_insert_and_cmdline_only() {
    let mut e = engine_with_keymaps("", &["n K :join", "i! jk <Esc>"]);
    exec(&mut e, "mapclear!");
    assert_eq!(
        e.settings.keymaps,
        vec!["n K :join".to_string()],
        "bang :mapclear! must leave normal-mode mappings alone"
    );
}

// ── Key-to-keys remapping (#1151) ────────────────────────────────────────────
//
// The single most common line in any vimrc, `inoremap jk <Esc>`, was
// unexpressible before #1151 — every `UserKeymap` target was an ex command.

#[test]
fn inoremap_jk_expands_to_escape() {
    let mut e = engine_with("");
    exec(&mut e, "inoremap jk <Esc>");
    press(&mut e, 'i');
    assert_eq!(e.mode, Mode::Insert);
    press(&mut e, 'j');
    press(&mut e, 'k');
    assert_eq!(
        e.mode,
        Mode::Normal,
        "jk should have exited insert mode via <Esc> expansion"
    );
}

#[test]
fn inoremap_jk_single_j_falls_through_to_insert() {
    // A single 'j' not followed by 'k' within the sequence should just be
    // typed, not swallowed — same prefix-buffering contract as ex-command
    // keymaps already had.
    let mut e = engine_with("");
    exec(&mut e, "inoremap jk <Esc>");
    press(&mut e, 'i');
    press(&mut e, 'j');
    press(&mut e, 'x');
    assert_eq!(e.mode, Mode::Insert);
    assert_eq!(get_lines(&e)[0], "jx");
}

// ── noremap vs map: recursion semantics + depth guard (#1151) ───────────────

#[test]
fn nmap_recurses_into_a_second_mapping() {
    // Recursive (`map`): a -> b, b -> x (delete char under cursor). Pressing
    // 'a' should chase through 'b' into 'x' and delete a character.
    let mut e = engine_with_keymaps("abc\n", &["n a b", "n b x"]);
    press(&mut e, 'a');
    assert_eq!(
        get_lines(&e)[0],
        "bc",
        "recursive nmap should chase a -> b -> x (delete char)"
    );
}

#[test]
fn nnoremap_does_not_recurse_into_a_second_mapping() {
    // Non-recursive (`noremap`): a -> b (noremap), and b is separately
    // mapped to x. Pressing 'a' must take rhs 'b' literally — the built-in
    // word-back motion, which doesn't touch the buffer — not chase into the
    // 'b' -> 'x' mapping (which would delete a char).
    let mut e = engine_with_keymaps("abc def\n", &["n! a b", "n b x"]);
    let before = get_lines(&e)[0].clone();
    press(&mut e, 'a');
    assert_eq!(
        get_lines(&e)[0],
        before,
        "noremap must not chase into b's own mapping"
    );
}

#[test]
fn recursive_mapping_cycle_hits_maxmapdepth_guard() {
    // `nmap a b` + `nmap b a` is a cycle with no base case. Vim's
    // `maxmapdepth` (default 1000) stops it with E223 instead of hanging.
    let mut e = engine_with_keymaps("hello\n", &["n a b", "n b a"]);
    press(&mut e, 'a');
    assert!(
        e.message.contains("E223") || e.message.to_lowercase().contains("recursive"),
        "expected a recursive-mapping error, got: {:?}",
        e.message
    );
    // The buffer must be untouched — the guard should abort cleanly, not
    // partially execute something from mid-cycle.
    assert_eq!(get_lines(&e)[0], "hello");
}

// ── <leader> expansion (#1151) ───────────────────────────────────────────────

#[test]
fn leader_expands_in_lhs() {
    let mut e = engine_with("aaa\nbbb\n");
    assert_eq!(e.settings.leader, ' ', "test assumes the default leader");
    exec(&mut e, "nnoremap <leader>w :join");
    press(&mut e, ' ');
    press(&mut e, 'w');
    assert_eq!(
        get_lines(&e)[0],
        "aaa bbb",
        "<leader>w should have fired :join"
    );
}

#[test]
fn leader_expansion_rebuilds_on_settings_change() {
    // Changing settings.leader and rebuilding must re-expand existing
    // <leader> keymaps against the *new* leader, not the one at definition
    // time — rebuild_user_keymaps re-parses from the stored (unexpanded)
    // settings.keymaps string every time.
    let mut e = engine_with_keymaps("aaa\nbbb\n", &["n! <leader>w :join"]);
    e.settings.leader = ',';
    e.rebuild_user_keymaps();
    press(&mut e, ',');
    press(&mut e, 'w');
    assert_eq!(get_lines(&e)[0], "aaa bbb");
}

// ── Operator-pending (`o`) maps (#1151) ──────────────────────────────────────

#[test]
fn operator_pending_map_extends_a_motion() {
    // onoremap p i( — a classic vim idiom: 'p' while an operator is pending
    // behaves like the "inside parens" text object, so "dp" deletes inside
    // the parens the cursor is in.
    let mut e = engine_with_keymaps("foo(bar)baz\n", &["o! p i("]);
    e.view_mut().cursor = vimcode_core::Cursor { line: 0, col: 5 }; // on 'b' of "bar"
    press(&mut e, 'd');
    press(&mut e, 'p');
    assert_eq!(
        get_lines(&e)[0],
        "foo()baz",
        "d + o-mapped p should behave like di( "
    );
}

#[test]
fn operator_pending_map_does_not_fire_in_plain_normal_mode() {
    // The same 'p' mapping must not fire outside operator-pending. If it
    // wrongly fired, its rhs "i(" would enter Insert mode and type a literal
    // '('; the built-in 'p' (paste) with nothing yanked leaves the buffer
    // and mode untouched instead.
    let mut e = engine_with_keymaps("foo(bar)baz\n", &["o! p i("]);
    e.view_mut().cursor = vimcode_core::Cursor { line: 0, col: 5 };
    press(&mut e, 'p');
    assert_eq!(
        e.mode,
        Mode::Normal,
        "bare 'p' (no pending operator) must not fire the o-mode map"
    );
    assert_eq!(get_lines(&e)[0], "foo(bar)baz");
}
