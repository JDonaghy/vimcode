# Implementation Plan: High-Priority Vim Motions & Operators

**Goal:** Implement essential Vim motions and operators to complete the core editing experience.

**Status:** In progress (Steps 1-4 complete)  
**Dependencies:** None  
**Test baseline:** 210 tests passing

---

## Overview

This plan implements the next tier of high-priority Vim features:

1. **Character find motions** — `f`, `F`, `t`, `T` with `;` and `,` repeat
2. **More delete/change operators** — `dw`, `cw`, `c`, `C`, `s`, `S`
3. **Text objects** — `iw`, `aw`, `i"`, `a(`, `i{`, etc.
4. **Repeat command** — `.` to repeat last change
5. **Visual block mode** — `Ctrl-V` for rectangular selections
6. **Additional motions** — `ge` (back to end of word), `%` (matching bracket)
7. **Reverse search** — `?` for backward search

---

## Step 1: Character Find Motions ✅ COMPLETE

11 tests added.

---

## Step 2: Delete/Change Operators ✅ COMPLETE

16 tests added.

---

## Step 3: Additional Motions (`ge`, `%`) ✅ COMPLETE

12 tests added.

---

## Step 4: Text Objects (`iw`, `aw`, `i"`, `a(`, etc.) ✅ COMPLETE

17 tests added. Implemented word/quote/bracket text objects with d/c/y operators and visual mode support.

---

## Step 5: Repeat Command (`.`) ✅ COMPLETE

4 tests added. Basic implementation for insert (`i`,`a`,`o`) and delete (`x`,`dd`) operations with count support (`3.`). Edge cases deferred.

---

## Step 6: Visual Block Mode (`Ctrl-V`)

**Goal:** Add rectangular/column selection mode.

### Implementation
- Add `VisualBlock` variant to `Mode` enum
- In `handle_normal_key()`, add `Ctrl-V` (0x16) case
- Store selection anchor (line, col)
- Calculate rectangular region:
  - From `(anchor_line, anchor_col)` to `(cursor_line, cursor_col)`
  - Include all lines in range, columns in range
  - Create `Vec<(line, col_start, col_end)>` for each line
- Render rectangular highlight:
  - Modify drawing code to handle block selections
- Operators in visual block mode:
  - `d` — delete rectangular region from each line
  - `c` — change rectangular region, enter insert mode
  - `y` — yank rectangular region
  - `I` — insert at start of each line in block
  - `A` — append at end of each line in block

### Testing
- Test entering visual block mode
- Test rectangular selection across lines
- Test delete in block mode
- Test yank and paste of block
- Test insert/append in block mode
- Test with varying line lengths
- Test navigation extends block

**Estimated:** 12-15 tests

---

## Step 7: Reverse Search (`?`)

**Goal:** Add backward search with `?` key.

### Implementation
- Add `search_direction: SearchDirection` to Engine
  - Enum: `Forward`, `Backward`
- On `?` key, enter Search mode with `Backward` direction
- Modify `find_search_matches()` to support direction
- Modify `n` and `N` to respect direction:
  - `n` — next match in search direction
  - `N` — previous match (opposite direction)
- Update status message: "?pattern" vs "/pattern"

### Testing
- Test `?` search finds matches backward
- Test `n` after `?` goes backward
- Test `N` after `?` goes forward
- Test wrapping at start of file
- Test alternating `/` and `?` searches

**Estimated:** 8-10 tests

---

## Implementation Order

1. **Step 1:** Character find motions — Foundation for text navigation
2. **Step 2:** More delete/change operators — Builds on existing operator logic
3. **Step 3:** Additional motions (`ge`, `%`) — Simpler than text objects
4. **Step 4:** Text objects — More complex, benefits from operator infrastructure
5. **Step 5:** Repeat command (`.`) — Requires tracking from previous steps
6. **Step 7:** Reverse search (`?`) — Independent feature
7. **Step 6:** Visual block mode — Most complex, benefits from all operator work

---

## Success Criteria

- [x] `f`, `F`, `t`, `T` motions work with `;` and `,` repeat
- [x] `dw`, `cw`, `s`, `S`, `C` operators functional
- [x] `ge` and `%` motions work correctly
- [x] Text objects `iw`, `aw`, `i"`, `a(`, etc. work with operators
- [x] `.` repeats last change operation (basic implementation)
- [ ] `Ctrl-V` visual block mode with rectangular selections
- [ ] `?` reverse search with proper `n`/`N` behavior
- [ ] All operations work with counts
- [ ] All operations integrate with undo/redo
- [ ] All operations work with named registers
- [ ] All tests pass, clippy clean
- [ ] No performance regression

---

## Notes

- Each step is designed to be independently testable
- Steps build on each other (operators → text objects → repeat)
- Maintain strict separation: core logic in `src/core/`, UI in `src/main.rs`
- Add tests incrementally with each step
- Run `cargo test` and `cargo clippy` after each step
