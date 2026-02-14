# ARCHIVED: Implementation Plan - Count-Based Command Repetition

**Status:** ✅ COMPLETED  
**Date Completed:** February 14, 2026  
**Feature:** Vim-style count prefixes (e.g., `5j`, `3dd`, `10yy`)

---

## Summary

Successfully implemented full count-based command repetition across all modes (Normal, Visual, Visual Line). The feature allows users to prefix commands with numbers to repeat them, matching Vim behavior.

**Total Changes:** ~600 lines across 3 files  
**Test Coverage:** 31 new tests added (115 → 146 tests)  
**Files Modified:** 
- `src/core/engine.rs` (~550 lines added/modified)
- `src/core/cursor.rs` (added PartialEq derive)
- `src/main.rs` (~15 lines for UI display)

---

## Step 1: Core Count Infrastructure ✅ COMPLETE
**Files:** `src/core/engine.rs`, `src/main.rs`  
**Actual Changes:** ~100 lines (85 in engine.rs, 15 in main.rs)  
**Dependencies:** None (foundational)

### Tasks Completed:
- [x] Add `count: Option<usize>` field to `Engine` struct
- [x] Initialize `count: None` in `Engine::new()`
- [x] Add helper method `take_count(&mut self) -> usize` to get and consume count
- [x] Add helper method `peek_count(&self) -> Option<usize>` for UI display
- [x] Add digit capture logic in `handle_normal_key()` BEFORE pending_key check:
  - Handle digits 1-9 to accumulate count
  - Special case: `0` alone goes to column 0, but `10`, `20` etc. are valid counts
  - Enforce 10,000 maximum limit
  - Return early after capturing digit
- [x] Clear count on Escape in `handle_normal_key()`
- [x] Update UI to display count in `src/main.rs` `draw_command_line()` function:
  - Show `engine.peek_count()` when in Normal/Visual mode
  - Display as right-aligned number in command line area (Vim-style)

### Tests (6 new tests):
- [x] `test_count_accumulation()` - Test "123" accumulates to 123
- [x] `test_zero_goes_to_line_start()` - Test "0" goes to column 0
- [x] `test_count_with_zero()` - Test "10" accumulates correctly
- [x] `test_count_max_limit()` - Test count caps at 10,000
- [x] `test_count_display()` - Verify peek_count() works without consuming
- [x] `test_count_cleared_on_escape()` - Test Escape clears count

**Validation:** ✅ All 121 tests pass (115 existing + 6 new). Clippy clean. Formatted with rustfmt.

---

## Step 2: Basic Motion Commands with Count ✅ COMPLETE
**Files:** `src/core/engine.rs`  
**Actual Changes:** ~120 lines  
**Dependencies:** Step 1 (requires `count` field and `take_count()`)

### Tasks Completed:
- [x] Apply count to directional motions in `handle_normal_key()`:
  - `h`, `j`, `k`, `l`: `let count = self.take_count(); for _ in 0..count { self.move_X(); }`
- [x] Apply count to word motions: `w`, `b`, `e` - wrapped with count loop
- [x] Apply count to paragraph motions: `{`, `}` - wrapped with count loop
- [x] Apply count to arrow keys: Left, Down, Up, Right - wrapped with count loop
- [x] Apply count to scrolling Ctrl commands:
  - Ctrl-D: multiply half-page by count
  - Ctrl-U: multiply half-page by count
  - Ctrl-F: multiply full-page by count
  - Ctrl-B: multiply full-page by count

### Tests (7 new tests):
- [x] `test_count_hjkl_motions()` - Test 5l, 2j, 3h, 1k
- [x] `test_count_arrow_keys()` - Test arrow key equivalents with count
- [x] `test_count_word_motions()` - Test 3w, 2b, 2e
- [x] `test_count_paragraph_motions()` - Test 2}, 1{
- [x] `test_count_scroll_commands()` - Test 2 Ctrl-D, 3 Ctrl-F, etc.
- [x] `test_count_motion_bounds_checking()` - Test 100l, 100j boundary cases
- [x] `test_count_large_values()` - Test 10w with many words

**Validation:** ✅ All 128 tests pass (121 existing + 7 new). Clippy clean. Formatted with rustfmt.

---

## Step 3: Line Operations with Count (yy, dd, x, D) ✅ COMPLETE
**Files:** `src/core/engine.rs`  
**Actual Changes:** ~210 lines  
**Dependencies:** Step 1 (requires `count` field)

### Tasks Completed:
- [x] Add `delete_lines(count, changed)` method:
  - Delete `count` lines starting from current line
  - Handle bounds (don't delete more lines than available)
  - Save deleted content to register (linewise)
  - Update cursor position
  - Properly handle newline structure
- [x] Add `yank_lines(count)` method:
  - Yank `count` lines starting from current line
  - Handle bounds
  - Save to register (linewise)
  - Set appropriate message ("X lines yanked")
- [x] Update `dd` command in `handle_pending_key()`:
  - `let count = self.take_count();`
  - Call `self.delete_lines(count, changed);`
- [x] Update `yy` and `Y` commands:
  - `let count = self.take_count();`
  - Call `self.yank_lines(count);`
- [x] Update `x` command:
  - Get count, calculate chars to delete (min of count and remaining chars on line)
  - Delete count chars in one operation
  - Save to register (characterwise)
- [x] Update `D` command:
  - Enhanced `delete_to_end_of_line_with_count()` method
  - Count=1: delete to EOL excluding newline
  - Count>1: delete to EOL + (count-1) full lines below
  - Complex two-pass deletion to preserve newline structure

### Tests (8 new tests):
- [x] `test_count_x_delete_chars()` - Test 3x deletes 3 chars
- [x] `test_count_x_bounds()` - Test 100x stops at line end
- [x] `test_count_dd_delete_lines()` - Test 3dd deletes 3 lines
- [x] `test_count_yy_yank_lines()` - Test 2yy yanks 2 lines
- [x] `test_count_Y_yank_lines()` - Test 3Y yanks 3 lines
- [x] `test_count_D_delete_to_eol()` - Test 2D deletes to EOL + 1 line
- [x] `test_count_dd_last_lines()` - Test delete past EOF
- [x] `test_count_yy_last_lines()` - Test yank past EOF

**Validation:** ✅ All 136 tests pass (128 existing + 8 new). Clippy clean. Formatted with rustfmt.

---

## Step 4: Special Commands and Mode Changes ✅ COMPLETE
**Files:** `src/core/engine.rs`  
**Actual Changes:** ~150 lines  
**Dependencies:** Step 1 (requires `count` field)

### Tasks Completed:
- [x] Update `G` command:
  - Use `peek_count()` to distinguish between no count vs explicit count
  - If count present: go to line N (1-indexed)
  - If no count: go to last line (existing behavior)
- [x] Update `gg` command in `handle_pending_key()`:
  - Use `peek_count()` to check if count was provided
  - If count present: go to line N (1-indexed)
  - If no count: go to first line (existing behavior)
- [x] Apply count to paste commands:
  - `p`: wrap with count loop
  - `P`: wrap with count loop
- [x] Apply count to search navigation:
  - `n`: wrap with count loop
  - `N`: wrap with count loop
- [x] Apply count to `o` and `O`:
  - Insert count newlines using `"\n".repeat(count)`
  - Clear count before entering insert mode
- [x] Clear count on mode changes:
  - All insert mode triggers: `i`, `a`, `A`, `I`, `o`, `O`
  - Command mode: `:`
  - Search mode: `/`
  - Note: Visual mode PRESERVES count for use with motions

### Tests (6 new tests):
- [x] `test_count_G_goto_line()` - Test "42G" goes to line 42
- [x] `test_count_gg_goto_line()` - Test "2gg" goes to line 2
- [x] `test_count_paste()` - Test "3p" pastes 3 times
- [x] `test_count_search_next()` - Test "3n" jumps 3 matches
- [x] `test_count_o_insert_lines()` - Test "3o" inserts 3 newlines
- [x] `test_count_cleared_on_insert_mode()` - Test count clears when entering insert

**Validation:** ✅ All 142 tests pass (136 existing + 6 new). Clippy clean. Formatted with rustfmt.

---

## Step 5: Visual Mode and Final Integration ✅ COMPLETE
**Files:** `src/core/engine.rs`, `src/core/cursor.rs`  
**Actual Changes:** ~80 lines  
**Dependencies:** Steps 1, 2 (requires count infrastructure and motion updates)

### Tasks Completed:
- [x] Add digit accumulation in `handle_visual_key()` (similar to normal mode)
- [x] Apply count to visual mode motions in `handle_visual_key()`:
  - Wrapped all motion commands with count loop: `h`, `j`, `k`, `l`, `w`, `b`, `e`, `{`, `}`
  - Handle `gg` with count (go to line N or first line)
  - Handle arrow keys with count
  - Handle Ctrl-D/U/F/B with count (multiply scroll distance)
- [x] Ensure count is cleared when exiting visual mode to normal mode (Escape, v, V)
- [x] Verify count doesn't interfere with visual operators (`y`, `d`, `c`)
- [x] Add PartialEq derive to Cursor struct (needed for tests)

### Tests (4 new tests):
- [x] `test_count_visual_motion()` - Test "5j" in visual mode extends selection 5 lines
- [x] `test_count_visual_word()` - Test "3w" in visual mode extends by 3 words
- [x] `test_count_visual_line_mode()` - Test "5j" in visual line mode
- [x] `test_count_not_applied_to_visual_operators()` - Test "3" then "d" deletes selection once

### Test Updated:
- [x] `test_count_cleared_on_mode_changes()` - Updated to reflect new behavior where count is preserved when entering visual mode (but cleared on exit)

**Validation:** ✅ All 146 tests pass (142 existing + 4 new). Clippy clean. Formatted with rustfmt.

---

## Implementation Notes

### Count Semantics
- Count before operator: `3dd` (delete 3 lines)
- Count before motion: `5j` (move 5 lines down)
- Special cases:
  - `42G` - goto line 42
  - `0` - goto column 0 (unless preceded by digit)
  - `10`, `20` - counts with zero accumulate correctly
- Count is preserved when entering visual mode (allows `5v3j` pattern)
- Count is cleared when exiting visual mode or entering insert/command/search modes

### UI Display
- Count appears in command line area (bottom-right), Vim-style
- Displayed when in Normal, Visual, or VisualLine modes
- Cleared after use or on mode change

### Technical Details
- Maximum count: 10,000 (user-friendly message on overflow)
- Helper methods: `take_count()` (consume), `peek_count()` (query without consuming)
- Digit capture happens before pending_key check to allow multi-key sequences like `10dd`

---

## Final Test Results

- **Total tests:** 146 (up from 115)
- **New tests added:** 31
- **Status:** ✅ All passing
- **Clippy:** ✅ Clean (no warnings)
- **Rustfmt:** ✅ Formatted

---

## Success Criteria - All Met ✅

- ✅ All motion commands support count prefixes
- ✅ All line operations (yy, dd, x, D) support count
- ✅ Special commands (G, gg, p, P, n, N, o, O) support count
- ✅ Visual mode motions support count
- ✅ Count displays in command line area
- ✅ Count clears appropriately (after use, on mode change, on Escape)
- ✅ Count capped at 10,000
- ✅ Zero special case handled correctly
- ✅ 146 tests passing
- ✅ Clippy and rustfmt pass

---

**Feature Complete:** Count-based command repetition is fully implemented and tested.
