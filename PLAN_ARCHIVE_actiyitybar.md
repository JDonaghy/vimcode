# Implementation Plan: Phase 1 - Activity Bar + Collapsible Sidebar

**Goal:** VSCode-style icon bar and collapsible sidebar panel (empty initially)

**Status:** Phase 1 COMPLETE ✅ (1A/1B/1C/1D/1E all done)  
**Priority:** CRITICAL  
**Test baseline:** 232 tests passing

---

## Overview

Add the foundational UI structure for a VSCode-like sidebar:
- **Activity bar:** 48px vertical icon panel (always visible)
- **Sidebar:** 300px collapsible panel with smooth animation
- **Keybinding:** Ctrl-B to toggle sidebar
- **Panel switching:** Click icons to switch between panels (only Explorer enabled initially)

**No file tree yet** - that's Phase 2. This phase is pure UI structure.

---

## Phase 1A: Basic Layout Structure ✅ (1-2 hours)

**Goal:** Restructure window layout with activity bar + sidebar + editor

**Files to modify:**
- `src/main.rs` - App struct, enums, view! macro

### Step 1.1: Add State to App Struct

**Location:** `src/main.rs` lines 17-20

**Current:**
```rust
struct App {
    engine: Rc<RefCell<Engine>>,
    redraw: bool,
}
```

**New:**
```rust
struct App {
    engine: Rc<RefCell<Engine>>,
    redraw: bool,
    sidebar_visible: bool,      // NEW
    active_panel: SidebarPanel, // NEW
}
```

### Step 1.2: Add SidebarPanel Enum

**Location:** Before App struct (line 17)

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
enum SidebarPanel {
    Explorer,
    Search,
    Git,
    Settings,
    None,
}
```

### Step 1.3: Add Message Types

**Location:** `src/main.rs` Msg enum (lines 23-35)

**Add these variants:**
```rust
enum Msg {
    KeyPress { ... },
    Resize,
    MouseClick { ... },
    ToggleSidebar,              // NEW
    SwitchPanel(SidebarPanel),  // NEW
}
```

### Step 1.4: Restructure Window Layout

**Location:** `src/main.rs` view! macro (lines 43-82)

**Replace the entire gtk4::Box with:**
```rust
view! {
    gtk4::Window {
        set_title: Some("VimCode"),
        set_default_size: (800, 600),

        #[name = "main_hbox"]
        gtk4::Box {
            set_orientation: gtk4::Orientation::Horizontal,

            // Activity Bar (48px, always visible)
            #[name = "activity_bar"]
            gtk4::Box {
                set_orientation: gtk4::Orientation::Vertical,
                set_width_request: 48,
                set_css_classes: &["activity-bar"],

                // Placeholder for buttons (added in Phase 1C)
                gtk4::Label {
                    set_label: "📁\n🔍\n🌿\n⚙️",
                    set_margin_all: 5,
                }
            },

            // Sidebar (collapsible with Revealer)
            #[name = "sidebar_revealer"]
            gtk4::Revealer {
                set_transition_type: gtk4::RevealerTransitionType::SlideRight,
                set_transition_duration: 200,
                
                #[watch]
                set_reveal_child: model.sidebar_visible,

                gtk4::Box {
                    set_orientation: gtk4::Orientation::Vertical,
                    set_width_request: 300,
                    set_css_classes: &["sidebar"],

                    // Placeholder - will add file tree in Phase 2
                    gtk4::Label {
                        set_label: "Explorer Panel (Empty)",
                        set_margin_all: 10,
                    },
                }
            },

            // Editor area (existing DrawingArea)
            gtk4::Box {
                set_orientation: gtk4::Orientation::Vertical,
                set_hexpand: true,

                #[name = "drawing_area"]
                gtk4::DrawingArea {
                    // Keep all existing DrawingArea configuration
                    set_hexpand: true,
                    set_vexpand: true,
                    set_focusable: true,
                    grab_focus: (),

                    add_controller = gtk4::EventControllerKey { ... },
                    add_controller = gtk4::GestureClick { ... },
                    
                    #[watch]
                    set_css_classes: { ... },
                }
            }
        }
    }
}
```

### Step 1.5: Initialize New Fields

**Location:** `src/main.rs` init() function (lines 84-126)

**Update App initialization:**
```rust
let model = App {
    engine: engine.clone(),
    redraw: false,
    sidebar_visible: true,  // NEW - start visible
    active_panel: SidebarPanel::Explorer, // NEW
};
```

### Testing Phase 1A

**Manual:**
```bash
cargo build
cargo run
```

**Expected:**
- Activity bar visible on left (48px with emoji label)
- Sidebar visible (300px with "Explorer Panel" text)
- Editor area to the right (resizes to fit)
- No functionality yet - just layout

**Verify:**
- Window renders correctly
- No crashes
- All existing editor functionality works (typing, commands, etc.)
- Run: `cargo test` - all 214+ tests still pass

**Success criteria:**
- ✅ Layout structure in place
- ✅ Three horizontal sections visible
- ✅ No regressions
- ✅ All tests pass

---

## Phase 1B: Ctrl-B Toggle Functionality ✅

Ctrl-B detection added to EventControllerKey, ToggleSidebar handler implemented. Sidebar toggles with 200ms animation. 232 tests pass, clippy clean.

---

## Phase 1C: Activity Bar Buttons ✅

Four buttons (📁🔍🌿⚙️) added to activity bar. Explorer button functional with tooltips, others disabled. SwitchPanel handler toggles visibility. 232 tests pass, clippy clean.

---

## Phase 1D: Active Panel Indicator ✅

Explorer button uses #[watch] for dynamic CSS. Shows "active" class when sidebar visible. All buttons use "activity-button" class. 232 tests pass, clippy clean.

---

## Phase 1E: CSS Styling ✅

Created load_css() function with VSCode dark theme colors. Called in init() before widgets. Activity bar (#252526) with borders, hover/active states. 232 tests pass, clippy clean.

---

## Testing Phase 1 (Complete)

### Manual Testing Checklist

**Layout:**
- [ ] Activity bar 48px wide, full height
- [ ] Sidebar 300px wide when open
- [ ] Editor area fills remaining space
- [ ] Window resizes correctly
- [ ] Min window size reasonable (400x300)

**Functionality:**
- [ ] Ctrl-B toggles sidebar with animation
- [ ] Click 📁 button toggles sidebar
- [ ] Click 📁 twice: open → close → open
- [ ] Disabled buttons don't respond
- [ ] Tooltips appear on hover
- [ ] Active indicator shows/hides correctly

**Visual:**
- [ ] Dark theme colors match VSCode
- [ ] Borders visible between sections
- [ ] Hover states work on buttons
- [ ] Active state clearly visible
- [ ] Animation smooth (200ms)
- [ ] No flickering or visual glitches

**Editor:**
- [ ] Can still type in editor
- [ ] Mouse clicking still works
- [ ] Commands still work (`:w`, `:q`, etc.)
- [ ] Visual mode still works
- [ ] Undo/redo still works
- [ ] All editor features unaffected

### Automated Testing

```bash
# All tests should still pass
cargo test

# No warnings
cargo clippy -- -D warnings

# Code formatted
cargo fmt --check
```

### New Tests to Add

**Location:** `src/main.rs` or separate test file

Since sidebar is pure UI, tests would be integration tests (not unit tests). Consider these optional:

```rust
// Note: These would require GTK test harness - may skip for now
// Manual testing is sufficient for UI-only features

#[test]
fn test_sidebar_toggles() {
    // Create app, send ToggleSidebar message
    // Verify sidebar_visible changes
}

#[test]
fn test_switch_panel() {
    // Send SwitchPanel(Explorer) message
    // Verify active_panel changes
}
```

**Recommendation:** Skip automated tests for Phase 1, rely on manual testing. Add integration tests in Phase 3 if needed.

---

## Success Criteria

### Phase 1 Complete When:

**User-visible:**
- ✅ Activity bar visible with 4 icon buttons
- ✅ Sidebar toggles with Ctrl-B (smooth animation)
- ✅ Explorer button toggles sidebar
- ✅ Active panel visually indicated
- ✅ VSCode-like dark theme applied
- ✅ All editor features work unchanged

**Technical:**
- ✅ All existing tests pass (214+)
- ✅ Clippy clean
- ✅ Code formatted
- ✅ No performance regression
- ✅ Sidebar structure ready for file tree (Phase 2)

---

## Next Steps

After Phase 1 complete:
- **Phase 2:** File Explorer Tree View (see PLAN_phase2.md)
  - Will replace "Explorer Panel (Empty)" placeholder with TreeView
  - Add file operations (open, create, delete)
  - 8-11 hours estimated

---

## Architecture Notes

**All changes in src/main.rs** - No core/ modifications needed

**Why Revealer?** GTK4's Revealer widget handles smooth slide animations automatically. We just set `reveal_child` and it animates.

**Why #[watch]?** Relm4's `#[watch]` macro automatically updates widget properties when model changes. Perfect for reactive UI.

**Why CSS classes?** Cleaner than setting colors in code. Easy to adjust theme later.

**No settings persistence yet** - Deferred to Phase 4. Sidebar state doesn't persist across restarts until then.
