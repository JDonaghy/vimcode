# Implementation Plan: Phase 4 - VSCode-like File Explorer with Preview Mode

**Goal:** Transform file explorer to match VSCode behavior while preserving Vim power features

**Status:** 🔄 IN PROGRESS  
**Priority:** HIGH  
**Estimated time:** 10-12 hours over 3 days  
**Test baseline:** 232 tests → 242+ tests (10 new unit tests expected)

---

## Overview

Add VSCode-style file opening behavior to VimCode's file explorer:
- **Single-click files**: Opens in preview mode (italic, dimmed tab, reusable, auto-closes)
- **Double-click files**: Opens permanently (or promotes preview)
- **Edit/save file**: Promotes preview to permanent
- **Single-click folders**: Expands/collapses only (no file opening)
- **Preview indicator**: Italic + dimmed tab label, "[Preview]" in `:ls`
- **One global preview**: Replaces previous preview buffer, auto-closes on replace
- **Power users preserved**: Can still `:vsplit` + `:e` for multi-file splits in tabs

---

## User Experience

### VSCode-Like Behavior

**File tree interactions:**
- **Single-click file** → Opens in preview mode (italic tab)
- **Single-click another file** → Replaces preview (first file's buffer auto-closes)
- **Double-click file** → Opens permanently OR promotes preview to permanent
- **Single-click folder** → Expands/collapses (no file opening)

**Preview promotion triggers:**
- Editing the file (any text modification)
- Saving the file (`:w`)
- Double-clicking the file again

**Visual indicators:**
- Preview tabs: Italic + dimmed text color
- Permanent tabs: Normal + full color
- `:ls` command: Shows "[Preview]" suffix for preview buffers

### Vim Power User Features Preserved

**Tab model:**
- Tabs work like VSCode (one primary file per tab)
- Tab label shows active window's file
- BUT power users can still `:vsplit` then `:e otherfile.rs`
- Result: Multiple files in one tab, tab label updates with `Ctrl-W w`

**Buffer commands:**
- All buffer commands still work: `:bn`, `:bp`, `:b#`, `:ls`, `:bd`
- Preview buffers appear in `:ls` with "[Preview]" marker
- Preview buffers auto-close when replaced (not when manually navigating)

**Splits:**
- `:vsplit` and `:split` still work normally
- `:e` in split opens file in that window
- Window cycling (`Ctrl-W w`) does NOT promote preview (only editing does)

---

## Implementation Phases

### Phase 1: Research & Architecture (READ-ONLY)

**Task 1.1: Verify GTK TreeView Click Events** ⏳
- Research `connect_button_press_event` vs `connect_row_activated`
- Check if `GestureClick` can be used with TreeView
- Verify we can detect folder vs file at click position
- Ensure single-click doesn't interfere with expand/collapse

**Task 1.2: Verify Pango Italic Support** ⏳
- Check current tab rendering code in `draw_editor()`
- Verify `pango::Style::Italic` works with current font
- Test if we can also dim color (RGB values)
- Ensure italic text doesn't break tab width calculations

**Task 1.3: Map All Text Modification Entry Points** ⏳
- Find ALL locations where text can be modified (for preview promotion)
- Search for: `insert_char()`, `insert_newline()`, `backspace()`, `x`, `dd`, `D`, delete operators, paste, change operators, visual mode operations
- **Decision made:** Undo/redo should NOT promote preview (read-only navigation)

**Task 1.4: Understand Tab Closing Logic** ⏳
- How does `:tabclose` work currently?
- What happens to buffers when last window showing them closes?
- How does `delete_buffer()` work (force flag, dirty check)?

**Task 1.5: Analyze Buffer Creation Path** ⏳
- Understand `BufferManager::open_file()` flow
- Ensure existing code paths default to permanent mode
- Plan how to add `open_file_with_mode()`

---

### Phase 2: Core Data Model Changes

**Task 2.1: Add Preview Flag to BufferState** ⏳

**File:** `src/core/buffer_manager.rs`

Add field:
```rust
pub struct BufferState {
    pub buffer: Buffer,
    pub file_path: Option<PathBuf>,
    pub dirty: bool,
    pub preview: bool,  // NEW: false by default (permanent)
    // ... existing fields
}
```

Initialize `preview: false` in all `BufferState::new()` calls.

**Task 2.2: Add Preview Tracking to Engine** ⏳

**File:** `src/core/engine.rs`

Add field:
```rust
pub struct Engine {
    // ... existing fields
    pub preview_buffer_id: Option<BufferId>,  // NEW: Tracks current preview
}
```

Initialize `preview_buffer_id: None` in `Engine::new()`.

**Task 2.3: Create OpenMode Enum** ⏳

**File:** `src/core/engine.rs`

Add type:
```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenMode {
    Preview,      // Single-click: reusable, auto-close old preview
    Permanent,    // Double-click/edit: keep forever
}
```

---

### Phase 3: Core Logic Implementation

**Task 3.1: Implement `open_file_with_mode()`** ⏳

**File:** `src/core/engine.rs`

New method:
```rust
pub fn open_file_with_mode(
    &mut self, 
    path: &Path, 
    mode: OpenMode
) -> Result<BufferId, io::Error>
```

**Logic:**
1. Call `buffer_manager.open_file(path)` to get/create buffer
2. If `mode == OpenMode::Preview`:
   - If old preview exists and is different buffer, delete it (`force=true`)
   - Mark new buffer as preview
   - Store in `preview_buffer_id`
3. If `mode == OpenMode::Permanent`:
   - Mark buffer as NOT preview
   - Clear `preview_buffer_id` if it was this buffer

**Task 3.2: Implement `promote_preview_if_needed()`** ⏳

**File:** `src/core/engine.rs`

New method:
```rust
fn promote_preview_if_needed(&mut self)
```

**Logic:**
1. Get current active buffer
2. If `preview_buffer_id == Some(current_buffer)`:
   - Set `buffer_state.preview = false`
   - Set `preview_buffer_id = None`

**Task 3.3: Add Promotion Calls to Text Modifications** ⏳

**File:** `src/core/engine.rs`

Call `promote_preview_if_needed()` from:
- `insert_char()`
- `delete_char()`
- `delete_line()`
- `insert_newline()`
- Paste operations
- All change operators
- Visual mode delete/change

**NOT from:** Undo/redo (decision: these are navigation, not modifications)

**Task 3.4: Update `:ls` Command** ⏳

**File:** `src/core/engine.rs`

Modify `list_buffers()` to add "[Preview]" suffix:
```rust
// Example output:
//   1 %a + "main.rs" line 42 [Preview]
//   2  a  "lib.rs" line 1
```

**Task 3.5: Write Unit Tests** ⏳

**File:** `src/core/engine.rs`

Add tests:
1. `test_open_file_preview_mode()` - Opens with preview flag
2. `test_open_file_permanent_mode()` - Opens without preview flag
3. `test_preview_replaces_previous()` - Second preview closes first
4. `test_preview_same_file_twice()` - Doesn't close/reopen same file
5. `test_edit_promotes_preview()` - Insert char promotes
6. `test_save_promotes_preview()` - Save promotes
7. `test_double_click_promotes_preview()` - Opening permanent promotes existing preview
8. `test_preview_buffer_deleted()` - Old preview truly deleted from buffer list
9. `test_undo_does_not_promote()` - Undo doesn't affect preview status
10. `test_ls_shows_preview_flag()` - Buffer list includes "[Preview]"

---

### Phase 4: UI Integration

**Task 4.1: Add New Messages** ⏳

**File:** `src/main.rs`

Add messages:
```rust
enum Msg {
    // ... existing
    OpenFilePreview(PathBuf),      // NEW: Single-click
    OpenFilePermanent(PathBuf),    // NEW: Double-click (rename existing)
    ToggleFolder(gtk4::TreePath),  // NEW: Single-click folder
}
```

**Task 4.2: Implement TreeView Click Handlers** ⏳

**File:** `src/main.rs`

Add single-click handler (research needed for exact GTK API):
```rust
// Use connect_button_press_event or GestureClick
// Detect single-click vs double-click
// Get TreePath at click position
// Check if file or folder
// Send appropriate message
```

Update double-click handler:
```rust
// Keep connect_row_activated, change to use OpenFilePermanent
```

**Task 4.3: Create Helper Function** ⏳

**File:** `src/main.rs`

Add function:
```rust
fn get_file_path_from_tree_path(
    tree_view: &gtk4::TreeView, 
    tree_path: &gtk4::TreePath
) -> Option<PathBuf>
```

**Task 4.4: Implement Message Handlers** ⏳

**File:** `src/main.rs`

Handler for `OpenFilePreview`:
- Call `engine.open_file_with_mode(path, OpenMode::Preview)`
- Switch active window to buffer
- Reset cursor and scroll
- Highlight in tree
- Focus editor

Handler for `OpenFilePermanent`:
- Similar to above but with `OpenMode::Permanent`

Handler for `ToggleFolder`:
- Expand/collapse TreeView row

---

### Phase 5: Visual Feedback

**Task 5.1: Update Tab Rendering for Italic** ⏳

**File:** `src/main.rs` - `draw_editor()` function

Changes to tab rendering:
1. Get buffer state for active window's buffer
2. Check `buffer_state.preview` flag
3. If preview:
   - Set font to italic: `font_desc.set_style(pango::Style::Italic)`
   - Dim color: Use `cr.set_source_rgb(0.5, 0.5, 0.5)` instead of normal
4. If not preview:
   - Normal font and color

**Task 5.2: Test Italic Rendering** ⏳

Verify:
- Italic text renders correctly
- Dimmed color is visible but still readable
- Layout doesn't break (tab width stays consistent)
- Works on different systems

---

### Phase 6: Edge Cases & Cleanup

**Task 6.1: Buffer Cleanup on Preview Replace** ⏳

**File:** `src/core/engine.rs` - `open_file_with_mode()`

Logic:
```rust
if let Some(old_preview_id) = self.preview_buffer_id {
    if old_preview_id != buffer_id {
        // Delete old preview buffer (force=true)
        let _ = self.delete_buffer(old_preview_id, true);
    }
}
```

**Task 6.2: Tab Close Cleanup** ⏳

**File:** `src/core/engine.rs` - `close_tab()` or equivalent

Logic: When closing tab with preview buffer, delete it:
```rust
if let Some(preview_id) = self.preview_buffer_id {
    if tab_contains_buffer(tab, preview_id) {
        let _ = self.delete_buffer(preview_id, true);
        self.preview_buffer_id = None;
    }
}
```

**Task 6.3: Save Promotion** ⏳

**File:** `src/core/engine.rs` - Save methods

Update `save_current_buffer()` to promote preview:
```rust
pub fn save_current_buffer(&mut self) -> Result<(), io::Error> {
    let buffer_id = self.active_buffer_id();
    self.buffer_manager.save_buffer(buffer_id)?;
    
    // Promote preview
    if self.preview_buffer_id == Some(buffer_id) {
        if let Some(state) = self.buffer_manager.buffers.get_mut(&buffer_id) {
            state.preview = false;
        }
        self.preview_buffer_id = None;
    }
    
    Ok(())
}
```

---

### Phase 7: Testing

**Task 7.1: Manual Test Scenarios** ⏳

**Scenario 1: Basic preview replacement**
1. Single-click file1.rs → Opens in preview (italic tab)
2. Verify `:ls` shows "[Preview]"
3. Single-click file2.rs → file1 preview replaced
4. Verify `:ls` no longer shows file1.rs

**Scenario 2: Double-click permanent**
1. Double-click file3.rs → Opens permanent (normal tab)
2. Single-click file4.rs → Opens preview (now 2 tabs)
3. Single-click file5.rs → Replaces file4 preview

**Scenario 3: Edit promotion**
1. Single-click file6.rs → Preview
2. Press `i` then type "hello"
3. Verify tab no longer italic
4. Verify `:ls` doesn't show "[Preview]"

**Scenario 4: Save promotion**
1. Single-click file8.rs → Preview
2. Make edit
3. Type `:w` → Saves and promotes

**Scenario 5: Folder clicks**
1. Single-click collapsed folder → Expands
2. Single-click expanded folder → Collapses
3. Single-click file in folder → Opens preview
4. Verify folder didn't collapse

**Scenario 6: Power user splits**
1. Open file9.rs (permanent)
2. Type `:vsplit` then `:e file10.rs`
3. Verify both files in one tab, two windows
4. Type `Ctrl-W w` to switch windows
5. Verify tab label updates to show active window's file

**Scenario 7: Preview in closing tab**
1. Open file13.rs (permanent)
2. Type `:tabnew` → New tab
3. Single-click file14.rs → Preview in new tab
4. Type `:tabclose` → Close tab with preview
5. Verify `:ls` no longer shows file14.rs

**Task 7.2: Edge Case Tests** ⏳

**Edge 1:** Open same file twice (shouldn't close/reopen)
**Edge 2:** Preview becomes dirty (should auto-promote from edit)
**Edge 3:** Double-click preview (should promote to permanent)
**Edge 4:** Close preview manually with `:bd`
**Edge 5:** Multiple tabs with preview (only one global preview)

**Task 7.3: Regression Tests** ⏳

Run full test suite:
```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Expected: All 232 existing tests pass + 10 new tests = 242 total

---

### Phase 8: Documentation

**Task 8.1: Update PROJECT_STATE.md** ⏳

Add to "File Explorer" section:
- Single-click files opens preview mode
- Double-click opens permanently
- Preview promotion triggers
- Visual indicators (italic, dimmed, `:ls` flag)

**Task 8.2: Update HISTORY.md** ⏳

Add Session 18 entry with full implementation details.

**Task 8.3: Update README.md** ⏳

Add to Key Commands section:
- Tree single-click behavior
- Tree double-click behavior
- Preview mode explanation

**Task 8.4: Update PLAN_ARCHIVE** ✅

Archive Phase 3 plan with completion summary.

---

## Open Questions

1. **Italic rendering fallback:** If GTK/Pango doesn't support italic, use dimmed color only?
   - **Decision needed**

2. **Preview indicator in tab:** Besides italic+dim, add visual symbol? (e.g., `~file.rs`)
   - **Decision needed**

3. **Status bar preview indicator:** Show preview status in status bar?
   - **Decision needed**

4. **Preview after tab switch:** If you switch tabs then back, should preview still exist?
   - **Decision needed**

5. **:tabnew behavior:** Should `:tabnew file.rs` open in permanent or preview mode?
   - **Recommendation:** Permanent (explicit command)

---

## Success Criteria

✅ Single-click opens preview (italic, dimmed tab)  
✅ Preview auto-replaces previous preview  
✅ Double-click opens permanent  
✅ Edit promotes preview to permanent  
✅ Save promotes preview to permanent  
✅ `:ls` shows "[Preview]" indicator  
✅ Preview buffers auto-close when replaced  
✅ Folders expand/collapse on single-click  
✅ Power users can still `:vsplit` + `:e`  
✅ Tab label shows active window's file  
✅ All 232 existing tests still pass  
✅ 10 new unit tests pass  
✅ Clippy clean  
✅ All manual scenarios pass  

---

## Risk Assessment

**High Risk:**
- GTK single-click handling (complex event handling needed)
- Italic font support (might not render on all systems)

**Medium Risk:**
- Buffer cleanup timing (avoid race conditions)
- Tab close logic (careful testing needed)

**Low Risk:**
- Core preview logic (straightforward)
- Promotion on edit (clear call points)

**Mitigation:**
- Research GTK APIs thoroughly before implementation
- Test italic rendering early
- Comprehensive unit tests
- Manual testing focused on edge cases

---

## Next Steps

1. Answer open questions (5 questions above)
2. Begin Phase 1: Research & Architecture
3. Proceed systematically through phases
4. Test thoroughly at each phase
5. Update documentation upon completion

---

## Notes

- Phase 3 (Integration & Polish) archived to `PLAN_ARCHIVE_phase3_integration_polish.md`
- Current plan builds on completed Phase 3 work
- Preserves all existing Vim functionality
- Adds VSCode-like UX for file exploration
- Maintains VimCode's hybrid philosophy
