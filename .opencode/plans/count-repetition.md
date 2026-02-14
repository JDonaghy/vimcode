# Implementation Plan: Count-Based Command Repetition

**Goal:** Add Vim-style count prefixes (e.g., `5j`, `3dd`, `10yy`) with a 10,000 limit, displayed in the command line area.

**Estimated Total Changes:** ~510 lines across 2 files  
**Test Coverage:** 20+ new tests

---

## Step 1: Core Count Infrastructure
**Files:** `src/core/engine.rs`, `src/main.rs`  
**Estimated Changes:** ~80 lines  
**Dependencies:** None (foundational)

### Tasks:
- [ ] Add `count: Option<usize>` field to `Engine` struct (line ~56)
- [ ] Initialize `count: None` in `Engine::new()` (line ~84)
- [ ] Add helper method `take_count(&mut self) -> usize` to get and consume count (after line 2127)
- [ ] Add helper method `peek_count(&self) -> Option<usize>` for UI display (after line 2127)
- [ ] Add digit capture logic in `handle_normal_key()` after pending_key check (line ~790):
  - Handle digits 1-9 to accumulate count
  - Special case: `0` alone goes to column 0, but `10`, `20` etc. are valid counts
  - Enforce 10,000 maximum limit
  - Return early after capturing digit
- [ ] Update UI to display count in `src/main.rs` `draw_command_line()` function (line ~708):
  - Show `engine.peek_count()` when in Normal/Visual mode
  - Display as plain number in command line area

### Tests (add to `src/core/engine.rs` tests module):
- [ ] `test_count_accumulation()` - Test "123" accumulates to 123
- [ ] `test_zero_goes_to_line_start()` - Test "0" goes to column 0
- [ ] `test_count_with_zero()` - Test "10j" works correctly
- [ ] `test_count_max_limit()` - Test count caps at 10,000
- [ ] `test_count_display()` - Verify peek_count() works without consuming

**Validation:** Run `cargo test` and manually type digits in normal mode - they should appear in command line.

---

## Step 2: Basic Motion Commands with Count
**Files:** `src/core/engine.rs`  
**Estimated Changes:** ~120 lines  
**Dependencies:** Step 1 (requires `count` field and `take_count()`)

### Tasks:
- [ ] Apply count to directional motions in `handle_normal_key()`:
  - `h` (line ~793): `let count = self.take_count(); for _ in 0..count { self.move_left(); }`
  - `j` (line ~794): `let count = self.take_count(); for _ in 0..count { self.move_down(); }`
  - `k` (line ~795): `let count = self.take_count(); for _ in 0..count { self.move_up(); }`
  - `l` (line ~796): `let count = self.take_count(); for _ in 0..count { self.move_right(); }`
- [ ] Apply count to word motions:
  - `w` (line ~896): wrap with count loop
  - `b` (line ~897): wrap with count loop
  - `e` (line ~898): wrap with count loop
- [ ] Apply count to paragraph motions:
  - `{` (line ~899): wrap with count loop
  - `}` (line ~900): wrap with count loop
- [ ] Apply count to arrow keys (lines ~954-962):
  - Left, Down, Up, Right: wrap with count loop
  - Home, End: consume count but don't use it
- [ ] Apply count to scrolling Ctrl commands (lines ~741-776):
  - Ctrl-D: multiply half-page by count
  - Ctrl-U: multiply half-page by count
  - Ctrl-F: multiply full-page by count
  - Ctrl-B: multiply full-page by count

### Tests:
- [ ] `test_count_motion_j()` - Test "5j" moves down 5 lines
- [ ] `test_count_motion_h()` - Test "3h" moves left 3 chars
- [ ] `test_count_motion_w()` - Test "2w" moves forward 2 words
- [ ] `test_count_motion_paragraph()` - Test "3}" moves forward 3 paragraphs
- [ ] `test_count_exceeds_bounds()` - Test "999j" stops at EOF without crashing
- [ ] `test_multi_digit_count()` - Test "123j" moves 123 lines
- [ ] `test_count_ctrl_d()` - Test "3<Ctrl-D>" scrolls 3 half-pages

**Validation:** Run `cargo test` and manually test `5j`, `10k`, `3w` in editor.

---

## Step 3: Line Operations with Count (yy, dd, x, D)
**Files:** `src/core/engine.rs`  
**Estimated Changes:** ~180 lines  
**Dependencies:** Step 1 (requires `count` field)

### Tasks:
- [ ] Add `delete_lines(count, changed)` method (after line ~2152):
  - Delete `count` lines starting from current line
  - Handle bounds (don't delete more lines than available)
  - Save deleted content to register (linewise)
  - Update cursor position
  - Set appropriate message ("X lines deleted")
- [ ] Add `yank_lines(count)` method (after `delete_lines()`):
  - Yank `count` lines starting from current line
  - Handle bounds
  - Save to register (linewise)
  - Set appropriate message ("X lines yanked")
- [ ] Update `dd` command in `handle_pending_key()` (line ~990):
  - `let count = self.take_count();`
  - Call `self.delete_lines(count, changed);`
- [ ] Update `yy` command in `handle_pending_key()` (line ~997):
  - `let count = self.take_count();`
  - Call `self.yank_lines(count);`
- [ ] Update `x` command (line ~870):
  - Get count, calculate chars to delete (min of count and remaining chars on line)
  - Delete count chars in one operation
  - Save to register (characterwise)
- [ ] Update `D` command (line ~904):
  - Wrap with count loop (delete to end, move down, repeat)
  - Handle last line properly

### Tests:
- [ ] `test_count_yank_lines()` - Test "3yy" yanks 3 lines
- [ ] `test_count_delete_lines()` - Test "5dd" deletes 5 lines
- [ ] `test_count_delete_char()` - Test "4x" deletes 4 chars
- [ ] `test_count_delete_to_eol()` - Test "2D" deletes to end of 2 lines
- [ ] `test_count_yank_partial()` - Test "100yy" on 5-line buffer yanks only 5 lines
- [ ] `test_count_with_register()` - Test "\"a3yy" yanks to register 'a'

**Validation:** Run `cargo test` and manually test `3dd`, `5yy`, `10x` in editor.

---

## Step 4: Special Commands and Mode Changes
**Files:** `src/core/engine.rs`  
**Estimated Changes:** ~80 lines  
**Dependencies:** Step 1 (requires `count` field)

### Tasks:
- [ ] Update `G` command (line ~912):
  - If count present: go to line N (1-indexed)
  - If no count: go to last line (existing behavior)
- [ ] Update `gg` command in `handle_pending_key()` (line ~978):
  - If count present: go to line N (1-indexed)
  - If no count: go to first line (existing behavior)
- [ ] Apply count to paste commands:
  - `p` (line ~926): wrap with count loop
  - `P` (line ~929): wrap with count loop
- [ ] Apply count to search navigation:
  - `n` (line ~935): wrap with count loop
  - `N` (line ~936): wrap with count loop
- [ ] Apply count to `o` and `O` (lines ~836, ~851):
  - Insert count newlines instead of just one
- [ ] Clear count on mode changes (add `self.count = None;`):
  - All insert mode triggers: `i`, `a`, `A`, `I`, `o`, `O` (lines ~797-863)
  - Visual mode triggers: `v`, `V` (lines ~937, ~941)
  - Command mode: `:` (line ~945)
  - Search mode: `/` (line ~949)
- [ ] Clear count on Escape in `handle_key()` (add after line ~696)

### Tests:
- [ ] `test_count_G_goto_line()` - Test "42G" goes to line 42
- [ ] `test_count_gg_goto_line()` - Test "2gg" goes to line 2
- [ ] `test_count_paste()` - Test "3p" pastes 3 times
- [ ] `test_count_search_next()` - Test "3n" jumps 3 matches
- [ ] `test_count_cleared_on_insert_mode()` - Test count clears when entering insert
- [ ] `test_count_cleared_on_escape()` - Test Escape clears count

**Validation:** Run `cargo test` and manually test `42G`, `3p`, count clearing behavior.

---

## Step 5: Visual Mode and Final Integration
**Files:** `src/core/engine.rs`  
**Estimated Changes:** ~50 lines  
**Dependencies:** Steps 1, 2 (requires count infrastructure and motion updates)

### Tasks:
- [ ] Apply count to visual mode motions in `handle_visual_key()` (around line 1294):
  - Wrap all motion commands with count loop: `h`, `j`, `k`, `l`, `w`, `b`, `e`, `{`, `}`
  - Handle `gg` with count (line ~1317)
  - Handle arrow keys with count
  - Handle Ctrl-D/U/F/B with count
- [ ] Ensure count is cleared when exiting visual mode to normal mode
- [ ] Verify count doesn't interfere with visual operators (`y`, `d`, `c`)

### Tests:
- [ ] `test_count_visual_motion()` - Test "5j" in visual mode extends selection 5 lines
- [ ] `test_count_visual_word()` - Test "3w" in visual mode extends by 3 words
- [ ] `test_count_visual_line_mode()` - Test "5j" in visual line mode
- [ ] `test_count_not_applied_to_visual_operators()` - Test "3" then "d" deletes selection once

### Final Validation:
- [ ] Run full test suite: `cargo test` (should have 135+ tests passing)
- [ ] Run linter: `cargo clippy -- -D warnings` (must pass)
- [ ] Run formatter: `cargo fmt --check` (must pass)
- [ ] Manual testing checklist:
  - [ ] Test `5j`, `10k`, `3w`, `2b` (motions)
  - [ ] Test `3dd`, `5yy`, `10x` (operations)
  - [ ] Test `42G`, `1gg` (goto line)
  - [ ] Test `3p`, `5n` (paste, search)
  - [ ] Test count display in command line
  - [ ] Test count clears on mode change
  - [ ] Test count with register: `"a3yy`
  - [ ] Test visual mode with count: `v5jd`
  - [ ] Test count limit: type 99999, verify caps at 10000
  - [ ] Test 0 vs 10/20: `0` goes to col 0, `10j` moves 10 lines

**Validation:** All 135+ tests pass, clippy clean, manual testing complete.

---

## Notes

- **Independence:** Steps 2-5 can be implemented in parallel after Step 1 is complete
- **Step 1 is foundational** - it must be completed first
- **Each step is testable independently** - write and run tests after each step
- **Incremental commits recommended** - commit after each step passes tests
- **Count semantics:**
  - Count before operator: `3dd` (delete 3 lines)
  - Count before motion: `5j` (move 5 lines)
  - Special: `42G` (goto line 42), `0` (goto column 0 unless preceded by digit)
- **UI Display:** Count appears in command line area (bottom), cleared after use

---

## Success Criteria

- [ ] All motion commands support count prefixes
- [ ] All line operations (yy, dd, x, D, o, O) support count
- [ ] Special commands (G, gg, p, P, n, N) support count
- [ ] Visual mode motions support count
- [ ] Count displays in command line area
- [ ] Count clears appropriately (after use, on mode change, on Escape)
- [ ] Count capped at 10,000
- [ ] Zero special case handled correctly
- [ ] All 135+ tests pass
- [ ] Clippy and rustfmt pass
- [ ] Manual testing confirms Vim-like behavior
