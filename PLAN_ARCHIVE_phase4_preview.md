# Implementation Plan: Phase 4 - Settings Persistence

**Goal:** Remember sidebar state across application restarts

**Status:** ⏸️ DEFERRED (implement anytime after Phase 1)  
**Priority:** LOW (nice-to-have, not blocking)  
**Estimated time:** 1-2 hours  
**Test baseline:** No new tests needed (optional: 2-3 tests)

---

## Overview

**DEFERRED:** This phase is not required for core functionality. Can be implemented anytime after Phase 1 is complete.

Add sidebar state persistence so user preferences survive app restarts:
- Sidebar visible/hidden state
- Active panel (Explorer, Search, etc.)
- Sidebar width (future enhancement)

**Current behavior:** Sidebar always starts visible with Explorer active

**Desired behavior:** Sidebar remembers last state

---

## Why Deferred?

1. **Not user-critical:** Users can toggle sidebar each session (Ctrl-B)
2. **Simple workaround:** Always starts visible, which is reasonable default
3. **Independent:** Doesn't block any other features
4. **Easy to add later:** Clean separation, just add save/load logic

**When to implement:**
- After Phases 0-3 complete and stable
- When polish pass is desired
- When user requests it
- Never, if not needed

---

## Implementation (When Ready)

### Files to Modify
- `src/core/settings.rs` - Add SidebarSettings struct
- `src/main.rs` - Load on init, save on quit

### Step 4.1: Add SidebarSettings Struct

**Location:** `src/core/settings.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarSettings {
    pub visible: bool,
    pub active_panel: String,  // "explorer", "search", "git", "settings", "none"
    pub width: u32,            // For future: customizable width
}

impl Default for SidebarSettings {
    fn default() -> Self {
        Self {
            visible: true,
            active_panel: "explorer".to_string(),
            width: 300,
        }
    }
}
```

### Step 4.2: Add to Settings Struct

**Location:** `src/core/settings.rs` Settings struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub line_numbers: LineNumberMode,
    pub sidebar: SidebarSettings,  // NEW
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            line_numbers: LineNumberMode::Absolute,
            sidebar: SidebarSettings::default(),  // NEW
        }
    }
}
```

### Step 4.3: Load Settings on Init

**Location:** `src/main.rs` init() function

```rust
fn init(...) -> ComponentParts<Self> {
    load_css();
    
    let engine = match file_path {
        Some(ref path) => Engine::open(path),
        None => Engine::new(),
    };
    
    // Load sidebar settings
    let sidebar_settings = engine.settings.sidebar.clone();
    let sidebar_visible = sidebar_settings.visible;
    let active_panel = match sidebar_settings.active_panel.as_str() {
        "explorer" => SidebarPanel::Explorer,
        "search" => SidebarPanel::Search,
        "git" => SidebarPanel::Git,
        "settings" => SidebarPanel::Settings,
        _ => SidebarPanel::None,
    };
    
    let engine = Rc::new(RefCell::new(engine));
    
    // ... build widgets
    
    let model = App {
        engine: engine.clone(),
        redraw: false,
        sidebar_visible,  // From settings
        active_panel,     // From settings
        tree_store: Some(tree_store.clone()),
        tree_has_focus: false,
        file_tree_view: Some(widgets.file_tree_view.clone()),
        drawing_area: Some(widgets.drawing_area.clone()),
    };
    
    // ... rest of init
}
```

### Step 4.4: Save Settings on Quit

**Location:** `src/main.rs` - Add cleanup before quit

**Problem:** Need to intercept quit to save settings

**Option 1:** Save on every change (simple but more I/O)
**Option 2:** Save on window close signal (proper but more complex)

**Option 1 implementation (simpler):**

In ToggleSidebar handler:
```rust
Msg::ToggleSidebar => {
    self.sidebar_visible = !self.sidebar_visible;
    
    // Save to settings
    let mut engine = self.engine.borrow_mut();
    engine.settings.sidebar.visible = self.sidebar_visible;
    let _ = engine.settings.save(&Settings::default_path());
    drop(engine);
    
    self.redraw = !self.redraw;
}
```

In SwitchPanel handler:
```rust
Msg::SwitchPanel(panel) => {
    // ... existing logic
    
    // Save to settings
    let mut engine = self.engine.borrow_mut();
    engine.settings.sidebar.visible = self.sidebar_visible;
    engine.settings.sidebar.active_panel = match self.active_panel {
        SidebarPanel::Explorer => "explorer".to_string(),
        SidebarPanel::Search => "search".to_string(),
        SidebarPanel::Git => "git".to_string(),
        SidebarPanel::Settings => "settings".to_string(),
        SidebarPanel::None => "none".to_string(),
    };
    let _ = engine.settings.save(&Settings::default_path());
    drop(engine);
    
    self.redraw = !self.redraw;
}
```

**Option 2 implementation (better):**

Connect to window close signal:
```rust
view! {
    gtk4::Window {
        set_title: Some("VimCode"),
        set_default_size: (800, 600),
        
        connect_close_request[sender] => move |_| {
            sender.input(Msg::SaveAndQuit);
            gtk4::glib::Propagation::Proceed
        },
        
        // ... rest of window
    }
}
```

Add SaveAndQuit message handler:
```rust
Msg::SaveAndQuit => {
    // Save sidebar state
    let mut engine = self.engine.borrow_mut();
    engine.settings.sidebar.visible = self.sidebar_visible;
    engine.settings.sidebar.active_panel = match self.active_panel {
        SidebarPanel::Explorer => "explorer".to_string(),
        SidebarPanel::Search => "search".to_string(),
        SidebarPanel::Git => "git".to_string(),
        SidebarPanel::Settings => "settings".to_string(),
        SidebarPanel::None => "none".to_string(),
    };
    let _ = engine.settings.save(&Settings::default_path());
    // Don't quit here - let GTK handle it
}
```

### Testing

**Manual:**
```bash
cargo build
cargo run
```

1. Toggle sidebar with Ctrl-B → closes
2. Quit app (`:q`)
3. Restart app → sidebar still closed
4. Toggle sidebar open
5. Switch to Settings panel (when implemented)
6. Quit and restart → Settings panel active

**Check settings file:**
```bash
cat ~/.config/vimcode/settings.json
```

Should see:
```json
{
  "line_numbers": "Absolute",
  "sidebar": {
    "visible": false,
    "active_panel": "explorer",
    "width": 300
  }
}
```

### Success Criteria

- ✅ Sidebar state persists across restarts
- ✅ Active panel remembered
- ✅ Settings file updated correctly
- ✅ Invalid settings.json handled gracefully (falls back to defaults)
- ✅ No performance impact
- ✅ No crashes

---

## Future Enhancements

When implementing Phase 4, consider also adding:

1. **Sidebar width persistence:**
   - Add resize handle to sidebar
   - Save custom width
   - Load width on startup

2. **Per-workspace settings:**
   - Different sidebar state per project
   - Store in `.vimcode/` folder in project root
   - Fall back to global settings

3. **More preferences:**
   - Show/hide dotfiles
   - Tree sort order
   - Toolbar button visibility

---

## Notes

**Why low priority:**
- Most users are fine with default state
- One keypress (Ctrl-B) restores preference
- Not a blocker for any feature
- Code is simple, can add anytime

**When this becomes important:**
- User explicitly requests it
- Multiple testers report it as annoying
- Polish pass before release
- After all critical features complete

**Estimated effort:** 1-2 hours total (very simple)
