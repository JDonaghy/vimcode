# Implementation Plan: Phase 2 - File Explorer Tree View

**Goal:** Working file tree showing CWD with navigation and file operations

**Status:** 🔨 IN PROGRESS (Phase 2A complete)  
**Priority:** HIGH  
**Estimated time:** 8-11 hours  
**Test baseline:** 232 tests (UI-based feature, minimal unit tests)

---

## Overview

Replace the empty sidebar placeholder with a functional file explorer:
- **TreeView:** Hierarchical display of files/folders
- **Double-click:** Open files in editor
- **Toolbar:** Create file, create folder, delete buttons
- **File operations:** Create, delete with error handling
- **Tree refresh:** Update tree after operations

**Phase 2 breakdown:** 8 small sub-phases (1-2 hours each)

---

## Phase 2A: Basic TreeStore ✅ COMPLETE

TreeStore with 3 columns, build_file_tree_flat() helper, sorted entries, initialized in init().

---

## Phase 2B: TreeView Widget ✅ COMPLETE

ScrolledWindow + TreeView replaces placeholder. Single column (icon+name), VSCode CSS, displays tree.

---

## Phase 2C: File Opening ✅ COMPLETE

OpenFileFromSidebar message, row_activated signal. Double-click opens files in editor.

---

## Phase 2D: Recursive Tree Building ✅ COMPLETE

Recursive build_file_tree(), parent parameter, skips dotfiles, depth limit 10, expanders enabled.

---

## Phase 2E: Toolbar + Polish ✅ COMPLETE

4 toolbar buttons (📄📁🗑️🔄). VSCode CSS: subtle selection with left border, refined hover, better spacing. Single-column fix, level_indentation=0.

---

## Phase 2F: New File Operation ✅ (2 hours)

**Goal:** Create new files from toolbar button

**Files to modify:**
- `src/main.rs` - Add message, dialog, handler

### Step 2F.1: Add CreateFile Message

**Location:** `src/main.rs` Msg enum

**Add:**
```rust
enum Msg {
    // ... existing
    OpenFileFromSidebar(PathBuf),
    CreateFile(String),     // NEW - filename relative to CWD
    RefreshFileTree,        // NEW - rebuild tree
}
```

### Step 2F.2: Add Simple Input Dialog

**Location:** `src/main.rs` - Add helper function before main()

```rust
/// Show simple text input dialog (synchronous for simplicity)
/// Returns None if cancelled, Some(text) if OK
fn show_text_input_dialog(parent: &gtk4::Window, title: &str, prompt: &str) -> Option<String> {
    let dialog = gtk4::Dialog::with_buttons(
        Some(title),
        Some(parent),
        gtk4::DialogFlags::MODAL,
        &[
            ("Cancel", gtk4::ResponseType::Cancel),
            ("OK", gtk4::ResponseType::Ok),
        ],
    );
    
    let content = dialog.content_area();
    let label = gtk4::Label::new(Some(prompt));
    label.set_margin_all(10);
    content.append(&label);
    
    let entry = gtk4::Entry::new();
    entry.set_margin_all(10);
    entry.set_width_request(300);
    content.append(&entry);
    
    dialog.set_default_response(gtk4::ResponseType::Ok);
    entry.set_activates_default(true);
    
    dialog.show();
    let response = dialog.run_future();
    
    // Note: run_future() is async - for simplicity, use blocking version
    // This is a limitation, but keeps code simple for now
    // TODO: Make this properly async in Phase 3 or later
    
    // For now, use a simpler approach: just return empty for this phase
    // We'll wire up the actual dialog in testing
    None // Placeholder - will need async handling
}
```

**Note:** GTK4 dialogs are async, which is tricky with Relm4. For Phase 2F, we'll use a **simpler approach**: prompt for filename in command line (status bar input) OR hardcode a test filename for now, then improve in Phase 3.

**Simplified approach:**
Skip dialog for now, just create "newfile.txt" as a test. We'll add proper dialogs in Phase 3.

### Step 2F.3: Wire Up New File Button

**Location:** `src/main.rs` new file button

**Add handler:**
```rust
gtk4::Button {
    set_label: "📄",
    set_tooltip_text: Some("New File"),
    set_width_request: 32,
    set_height_request: 32,
    connect_clicked[sender] => move |_| {
        // For now, create "newfile.txt" - will add proper dialog in Phase 3
        let filename = format!("newfile_{}.txt", 
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );
        sender.input(Msg::CreateFile(filename));
    }
},
```

### Step 2F.4: Handle CreateFile Message

**Location:** `src/main.rs` update() function

**Add match arm:**
```rust
Msg::CreateFile(name) => {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let file_path = cwd.join(&name);
    
    // Validate filename
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        self.engine.borrow_mut().message = "Invalid filename".to_string();
        self.redraw = !self.redraw;
        return;
    }
    
    // Check if already exists
    if file_path.exists() {
        self.engine.borrow_mut().message = format!("File already exists: {}", name);
        self.redraw = !self.redraw;
        return;
    }
    
    // Create file
    match std::fs::File::create(&file_path) {
        Ok(_) => {
            self.engine.borrow_mut().message = format!("Created: {}", name);
            
            // Trigger tree refresh
            sender.input(Msg::RefreshFileTree);
            
            // Open the new file
            sender.input(Msg::OpenFileFromSidebar(file_path));
        }
        Err(e) => {
            self.engine.borrow_mut().message = format!("Error creating file: {}", e);
        }
    }
    self.redraw = !self.redraw;
}

Msg::RefreshFileTree => {
    // Rebuild tree from CWD
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    
    // Clear existing tree
    // Note: Need access to tree_store - must store in App struct
    // For now, placeholder - will implement after restructuring
    
    self.redraw = !self.redraw;
}
```

### Step 2F.5: Store TreeStore in App Struct

**Location:** `src/main.rs` App struct

**Problem:** We need to access tree_store in update() to refresh it, but it's only in init().

**Solution:** Store it in App struct (requires Rc<RefCell<>> for GTK objects)

```rust
struct App {
    engine: Rc<RefCell<Engine>>,
    redraw: bool,
    sidebar_visible: bool,
    active_panel: SidebarPanel,
    tree_store: Option<gtk4::TreeStore>,  // NEW
}
```

**In init():**
```rust
let model = App {
    engine: engine.clone(),
    redraw: false,
    sidebar_visible: true,
    active_panel: SidebarPanel::Explorer,
    tree_store: Some(tree_store.clone()),  // NEW
};
```

**In RefreshFileTree handler:**
```rust
Msg::RefreshFileTree => {
    if let Some(ref store) = self.tree_store {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        
        // Clear tree
        store.clear();
        
        // Rebuild
        build_file_tree(store, None, &cwd);
    }
    self.redraw = !self.redraw;
}
```

### Testing Phase 2F

**Manual:**
```bash
cargo build
cargo run
```

**Test sequence:**
1. Click 📄 button → creates "newfile_<timestamp>.txt"
2. File appears in tree
3. File opens in editor automatically
4. File is empty (correct)
5. Type some text, save with `:w`
6. Click 📄 again → creates another newfile
7. Check directory → files actually exist on disk

**Edge cases:**
- Try creating file with no write permission in directory → shows error
- Tree refreshes and shows new file

**Success criteria:**
- ✅ New file button creates file
- ✅ Tree refreshes automatically
- ✅ File opens in editor
- ✅ File exists on disk
- ✅ Errors handled gracefully
- ✅ Status bar shows feedback

---

## Phase 2G: New Folder Operation ✅ (1 hour)

**Goal:** Create new folders from toolbar button

**Files to modify:**
- `src/main.rs` - Add message, wire button, handle

### Step 2G.1: Add CreateFolder Message

**Location:** `src/main.rs` Msg enum

**Add:**
```rust
enum Msg {
    // ... existing
    CreateFile(String),
    CreateFolder(String),  // NEW
    RefreshFileTree,
}
```

### Step 2G.2: Wire Up New Folder Button

**Location:** `src/main.rs` new folder button

```rust
gtk4::Button {
    set_label: "📁",
    set_tooltip_text: Some("New Folder"),
    set_width_request: 32,
    set_height_request: 32,
    connect_clicked[sender] => move |_| {
        // For now, create "newfolder_<timestamp>"
        let foldername = format!("newfolder_{}", 
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );
        sender.input(Msg::CreateFolder(foldername));
    }
},
```

### Step 2G.3: Handle CreateFolder Message

**Location:** `src/main.rs` update() function

```rust
Msg::CreateFolder(name) => {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let folder_path = cwd.join(&name);
    
    // Validate folder name
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        self.engine.borrow_mut().message = "Invalid folder name".to_string();
        self.redraw = !self.redraw;
        return;
    }
    
    // Check if already exists
    if folder_path.exists() {
        self.engine.borrow_mut().message = format!("Folder already exists: {}", name);
        self.redraw = !self.redraw;
        return;
    }
    
    // Create folder
    match std::fs::create_dir(&folder_path) {
        Ok(_) => {
            self.engine.borrow_mut().message = format!("Created folder: {}", name);
            sender.input(Msg::RefreshFileTree);
        }
        Err(e) => {
            self.engine.borrow_mut().message = format!("Error creating folder: {}", e);
        }
    }
    self.redraw = !self.redraw;
}
```

### Testing Phase 2G

**Manual:**
```bash
cargo build
cargo run
```

**Test sequence:**
1. Click 📁 button → creates "newfolder_<timestamp>"
2. Folder appears in tree (sorted before files)
3. Click expand arrow → folder is empty
4. Click 📁 again → creates another folder
5. Navigate to new folder in terminal, create file inside
6. Click 🔄 refresh → file appears in tree under folder

**Success criteria:**
- ✅ New folder button creates folder
- ✅ Tree refreshes automatically
- ✅ Folder appears with folder icon
- ✅ Folder is expandable
- ✅ Folder exists on disk
- ✅ Errors handled gracefully

---

## Phase 2H: Delete Operation ✅ (1-2 hours)

**Goal:** Delete files/folders from toolbar button with confirmation

**Files to modify:**
- `src/main.rs` - Add message, wire button, handle with confirmation

### Step 2H.1: Add DeletePath Message

**Location:** `src/main.rs` Msg enum

```rust
enum Msg {
    // ... existing
    CreateFolder(String),
    DeletePath(PathBuf),  // NEW
    RefreshFileTree,
}
```

### Step 2H.2: Wire Up Delete Button

**Location:** `src/main.rs` delete button

**Need to get selected item from tree first:**

```rust
gtk4::Button {
    set_label: "🗑️",
    set_tooltip_text: Some("Delete"),
    set_width_request: 32,
    set_height_request: 32,
    connect_clicked[sender, file_tree_view] => move |_| {
        // Get selected row
        if let Some(selection) = file_tree_view.selection().selected() {
            let (model, iter) = selection;
            let path_str: String = model.value(&iter, 2).get().unwrap_or_default();
            let path = PathBuf::from(path_str);
            sender.input(Msg::DeletePath(path));
        }
    }
},
```

### Step 2H.3: Handle DeletePath with Confirmation

**Location:** `src/main.rs` update() function

```rust
Msg::DeletePath(path) => {
    // Get filename for confirmation message
    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    
    // For now, skip confirmation dialog (async complexity)
    // Just delete with warning in status bar
    // TODO: Add proper confirmation dialog in Phase 3
    
    let is_dir = path.is_dir();
    let result = if is_dir {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    };
    
    match result {
        Ok(_) => {
            let item_type = if is_dir { "folder" } else { "file" };
            self.engine.borrow_mut().message = 
                format!("Deleted {}: {}", item_type, filename);
            
            // If deleted file was open, close its buffer
            // Find buffer by path and delete it
            let mut engine = self.engine.borrow_mut();
            let buffer_to_delete = engine.buffer_manager
                .buffers
                .iter()
                .find(|(_, state)| state.file_path.as_ref() == Some(&path))
                .map(|(id, _)| *id);
            
            if let Some(buffer_id) = buffer_to_delete {
                // Switch away if it's the active buffer
                if engine.active_buffer_id() == buffer_id {
                    // Switch to previous buffer if available
                    if let Some(other_id) = engine.buffer_manager
                        .buffers
                        .keys()
                        .find(|&&id| id != buffer_id)
                        .copied()
                    {
                        engine.active_window_mut().buffer_id = other_id;
                    }
                }
                
                // Delete the buffer
                let _ = engine.buffer_manager.delete_buffer(buffer_id);
            }
            
            drop(engine);
            sender.input(Msg::RefreshFileTree);
        }
        Err(e) => {
            self.engine.borrow_mut().message = 
                format!("Error deleting {}: {}", filename, e);
        }
    }
    self.redraw = !self.redraw;
}
```

### Step 2H.4: Wire Up Refresh Button

**Location:** `src/main.rs` refresh button (already added in Phase 2E)

```rust
gtk4::Button {
    set_label: "🔄",
    set_tooltip_text: Some("Refresh"),
    set_width_request: 32,
    set_height_request: 32,
    connect_clicked[sender] => move |_| {
        sender.input(Msg::RefreshFileTree);
    }
},
```

### Testing Phase 2H

**Manual:**
```bash
cargo build
cargo run
```

**Test sequence:**
1. Create test file: Click 📄 button
2. Select file in tree (single click)
3. Click 🗑️ button → file deleted
4. File disappears from tree
5. File gone from disk (verify in terminal)
6. Create folder: Click 📁 button
7. Select folder in tree
8. Click 🗑️ → folder deleted

**Test edge cases:**
- Delete open file → buffer closes, switches to another buffer
- Delete folder with files inside → removes all (use with caution!)
- Delete with no selection → nothing happens
- Delete system file with no permission → shows error

**IMPORTANT:** No confirmation dialog yet! Deletion is immediate. Add warning in status bar.

**Success criteria:**
- ✅ Delete button removes selected item
- ✅ Tree refreshes automatically
- ✅ File/folder removed from disk
- ✅ Open buffers closed if file deleted
- ✅ Errors handled gracefully
- ✅ No crashes on edge cases

**Known limitation:** No undo, no confirmation dialog. Use carefully!

---

## Testing Phase 2 (Complete)

### Manual Testing Checklist

**Tree Display:**
- [ ] Files and folders shown with icons
- [ ] Sorted correctly (folders first, alphabetical)
- [ ] Indentation shows hierarchy (16px per level)
- [ ] Expand/collapse works with arrows
- [ ] Keyboard navigation works (arrows, Enter)
- [ ] Scrolling works for long lists
- [ ] Selection visible (blue highlight)

**File Operations:**
- [ ] Double-click file opens in editor
- [ ] Double-click folder expands/collapses
- [ ] New file button creates file + opens it
- [ ] New folder button creates folder
- [ ] Delete button removes file/folder
- [ ] Refresh button reloads tree
- [ ] Operations reflected on disk
- [ ] Tree refreshes after operations

**Error Handling:**
- [ ] Invalid filenames rejected
- [ ] Existing files not overwritten
- [ ] Permission errors shown in status bar
- [ ] Empty filenames rejected
- [ ] Paths with slashes rejected
- [ ] No crashes on any operation

**Integration:**
- [ ] Opened files show in buffer list (`:ls`)
- [ ] Can switch between files with `:b`
- [ ] Deleted files close their buffers
- [ ] Multiple operations in sequence work
- [ ] Editor functionality unaffected

### Automated Testing

```bash
cargo test          # All tests pass
cargo clippy -- -D warnings  # No warnings
cargo fmt --check   # Formatted
```

**New tests to add:**
Most of Phase 2 is UI/filesystem operations, hard to unit test. Consider adding integration tests if needed, but manual testing is primary validation.

---

## Success Criteria

### Phase 2 Complete When:

**User-visible:**
- ✅ File tree displays CWD contents
- ✅ Folders expand/collapse to show hierarchy
- ✅ Double-click opens files in editor
- ✅ Toolbar buttons functional:
  - ✅ 📄 Creates new file
  - ✅ 📁 Creates new folder
  - ✅ 🗑️ Deletes selected item
  - ✅ 🔄 Refreshes tree
- ✅ Tree updates after operations
- ✅ Errors shown in status bar
- ✅ All editor features work unchanged

**Technical:**
- ✅ All existing tests pass (224+)
- ✅ No clippy warnings
- ✅ No crashes or panics
- ✅ Performance acceptable (< 1s load time for typical projects)
- ✅ File operations actually modify disk

---

## Next Steps

After Phase 2 complete:
- **Phase 3:** Integration & Polish (see PLAN_phase3.md)
  - Ctrl-Shift-E keybinding
  - Focus management (Escape from tree)
  - Active file highlighting in tree
  - Better error messages
  - Optional: proper input dialogs
  - 3-5 hours estimated

---

## Known Limitations

**To address in Phase 3:**
1. No confirmation dialog for delete (immediate deletion)
2. No proper input dialogs (uses timestamp-based names)
3. No "Save changes?" prompt when deleting open files
4. No undo for file operations
5. Dotfiles are skipped (hidden files not shown)

**To address in Phase 5:**
1. No file watching (external changes not detected)
2. No .gitignore respect
3. No workspace concept (always shows CWD)
4. No drag-and-drop
5. No context menu (right-click)

---

## Architecture Notes

**All changes in src/main.rs** - No core/ modifications needed

**Why TreeStore?** GTK's TreeStore handles hierarchical data naturally. Alternative would be custom model, but TreeStore is simpler.

**Why synchronous file ops?** Keeps code simple for now. File operations are fast enough for typical use. Could make async in Phase 5 if needed.

**Why skip dotfiles?** Reduces clutter, matches most IDEs' default behavior. Can make configurable later.

**Refresh strategy:** Full tree rebuild on each refresh. Could optimize with incremental updates, but full rebuild is simpler and fast enough for typical projects.
