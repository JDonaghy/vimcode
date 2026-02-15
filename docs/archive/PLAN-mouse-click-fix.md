# Implementation Plan: Phase 0.5 - Fix Mouse Click Positioning

**Goal:** Fix mouse click coordinate-to-position conversion for accurate cursor placement

**Status:** 🔴 ACTIVE - Critical bug fix  
**Priority:** HIGH (regression from Phase 0)  
**Estimated time:** 3-5 hours  
**Test baseline:** 214 tests → 214+ tests (all existing pass, possibly add more)

---

## Problem Analysis

Current `handle_mouse_click()` at src/main.rs:860 has three critical issues:

### Issue 1: Hardcoded Approximations
```rust
let line_height = 24.0; // Approximate, should match font metrics
let char_width = 9.0; // Approximate for Monospace 14
```
**Effect:** Clicks land too far right, wrong line as window grows

### Issue 2: Hardcoded Window Dimensions
```rust
let width = 800.0;
let height = 600.0;
```
**Effect:** Wrong calculations when window is resized

### Issue 3: No Pango Layout Access
Cannot measure actual text width for tabs, proportional characters, etc.

**User Report:** "Clicking always takes you too far to the right and sometimes to the wrong line"

---

## Implementation Phases

### Phase 0.5A: Pass Real Dimensions ✅

**Completed:** Added width/height to MouseClick message, updated GestureClick handler, handle_mouse_click() signature, and caller. Removed hardcoded 800x600. All mouse tests pass.

---

### Phase 0.5B: Use Real Font Metrics ✅

**Completed:** Created Pango context in handle_mouse_click(), calculated real line_height and char_width from font metrics. Removed hardcoded 24.0 and 9.0. All tests pass.

---

### Phase 0.5C: Improve Column Calculation ✅

**Completed:** Pixel-perfect column detection using Pango layout measurement. Handles tabs (4 spaces), unicode, empty lines, clicks past line end. All tests pass.

---

### Phase 0.5D: Add Comprehensive Tests ✅ (1 hour)

**Goal:** Ensure mouse clicking is robust with edge case coverage

**Files to modify:**
- `src/core/engine.rs` - Add more tests after line 7647

**New tests to add:**

```rust
#[test]
fn test_mouse_click_line_end() {
    // Click past last character should clamp to end
}

#[test]
fn test_mouse_click_past_last_line() {
    // Click below last line should clamp to last line
}

#[test]
fn test_mouse_click_with_line_numbers_absolute() {
    // Gutter width changes with line numbers on
}

#[test]
fn test_mouse_click_with_line_numbers_relative() {
    // Gutter width consistent with relative numbers
}

#[test]
fn test_mouse_click_in_gutter() {
    // Click in gutter should be ignored
}

#[test]
fn test_mouse_click_empty_buffer() {
    // Click in empty buffer goes to (0, 0)
}

#[test]
fn test_mouse_click_tab_bar() {
    // Click in tab bar ignored (future: switch tabs)
}

#[test]
fn test_mouse_click_status_bar() {
    // Click in status bar ignored
}

#[test]
fn test_mouse_click_window_separator() {
    // Click on separator ignored
}

#[test]
fn test_mouse_click_after_resize() {
    // Clicking after window resize uses correct dimensions
}
```

**Testing:**
- Run all tests: `cargo test`
- Run mouse tests only: `cargo test test_mouse`
- Verify count: Should be 214 + N new tests (probably 224 total)

**Success criteria:**
- All new tests pass
- All existing tests still pass (214 → 224+)
- Edge cases covered
- No clippy warnings: `cargo clippy -- -D warnings`

---

## Implementation Order

**Must complete in sequence:**

1. ✅ **Phase 0.5A** - Pass real dimensions (blocks 0.5B)
2. ✅ **Phase 0.5B** - Use real font metrics (main fix)
3. ⏸️ **Phase 0.5C** - Improve column calc (optional, do if needed)
4. ✅ **Phase 0.5D** - Add comprehensive tests (validates fixes)

**Critical path:** 0.5A → 0.5B → 0.5D (skip 0.5C unless needed)

**Manual testing after each phase:**
```bash
cargo build
cargo run -- src/main.rs  # Open a file
# Click at various positions:
# - Start of line
# - Middle of line  
# - End of line
# - Empty lines
# - Different window sizes
```

---

## Success Criteria

### User-visible:
- ✅ Clicking moves cursor to correct position
- ✅ No more "too far to the right" issue
- ✅ No more "wrong line" issue
- ✅ Works at any window size
- ✅ Works with/without line numbers

### Technical:
- ✅ No hardcoded dimensions or font metrics
- ✅ Matches draw_editor() calculations exactly
- ✅ All 214+ tests pass
- ✅ Clippy clean
- ✅ No performance regression

---

## Next Steps

After Phase 0.5 is complete and verified:
- **Phase 1:** Activity Bar + Collapsible Sidebar (see PLAN_phase1.md)
- **Phase 2:** File Explorer Tree View (see PLAN_phase2.md)
- **Phase 3:** Integration & Polish (see PLAN_phase3.md)
- **Phase 4:** Settings Persistence - DEFERRED (see PLAN_phase4.md)

---

## Architecture Notes

**No core/ changes needed** - This is purely a UI fix in src/main.rs

**Design principle:** The coordinate-to-position conversion must exactly mirror the position-to-coordinate calculation in draw_editor(). Any mismatch causes clicking bugs.

**Debugging tip:** Add debug prints in handle_mouse_click() to see:
```rust
eprintln!("Click: ({}, {}) → line={}, col={}, line_height={}, char_width={}", 
          x, y, line, col, line_height, char_width);
```

**Known limitation:** Phase 0.5B uses `approximate_char_width()` which is "close enough" for monospace fonts but not pixel-perfect. Phase 0.5C fixes this if needed.
