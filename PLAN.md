# Implementation Plan: Line Numbers (Absolute & Relative)

**Goal:** Add Vim-style line numbers with both absolute (`:set number`) and relative (`:set relativenumber`) modes, controlled by a settings.json configuration file.

**Status:** Planning phase  
**Dependencies:** None (new feature)

---

## Overview

Implement line number display in the gutter area (left side of the editor), supporting:
1. **Absolute line numbers** — Sequential numbering (1, 2, 3, 4...)
2. **Relative line numbers** — Distance from cursor (3, 2, 1, 0, 1, 2, 3...)
3. **Hybrid mode** — Current line shows absolute, others show relative
4. **Configuration** — Persistent settings via settings.json file

---

## Design Decisions

### Line Number Modes
- `none` — No line numbers (default for now)
- `absolute` — Show line numbers 1, 2, 3, 4...
- `relative` — Show 3, 2, 1, 0, 1, 2, 3... (relative to cursor)
- `hybrid` — Current line shows absolute number, others show relative

### Settings File
- **Location:** `~/.config/vimcode/settings.json`
- **Format:** JSON with schema validation
- **Initial settings:**
  ```json
  {
    "line_numbers": "none",
    "relative_numbers": false
  }
  ```
- **Vim-style equivalents:**
  - `"line_numbers": "absolute"` → `:set number`
  - `"line_numbers": "relative"` → `:set relativenumber`
  - Both true → hybrid mode

### Rendering Approach
- Render in left gutter before text area
- Calculate gutter width based on max line number digits
- Update width dynamically as buffer grows
- Use monospace font matching editor font
- Slightly dimmed color (gray) for line numbers
- Highlight current line number (brighter or different color)

---

## Step 1: Settings Infrastructure

**Goal:** Create settings.json file support with loading, saving, and default values.

### Files to Create/Modify:
- `src/core/settings.rs` — New file for settings struct and I/O
- `src/core/mod.rs` — Add `pub mod settings;`
- `src/core/engine.rs` — Add settings field to Engine

### Tasks:
- [ ] Create `Settings` struct with serde Serialize/Deserialize
  - [ ] `line_numbers: LineNumberMode` enum (None, Absolute, Relative, Hybrid)
  - [ ] Default implementation
- [ ] Implement `Settings::load()` — Read from `~/.config/vimcode/settings.json`
- [ ] Implement `Settings::save()` — Write to settings file
- [ ] Create config directory if it doesn't exist
- [ ] Handle missing file gracefully (use defaults)
- [ ] Handle JSON parse errors (log warning, use defaults)
- [ ] Add `settings: Settings` field to `Engine` struct
- [ ] Initialize settings in `Engine::new()`

### Dependencies to Add:
- `serde = { version = "1.0", features = ["derive"] }`
- `serde_json = "1.0"`

### Tests:
- [ ] `test_settings_default()` — Verify default values
- [ ] `test_settings_load_missing_file()` — Graceful fallback to defaults
- [ ] `test_settings_load_save()` — Round-trip serialization
- [ ] `test_settings_invalid_json()` — Error handling

**Validation:** Settings can be loaded from file or defaults, cargo test passes.

---

## Step 2: Line Number Rendering in UI

**Goal:** Render line numbers in the left gutter area.

### Files to Modify:
- `src/main.rs` — Update rendering to include line number gutter

### Tasks:
- [ ] Calculate gutter width based on max line number digits
  - [ ] For absolute: `max_line_num.to_string().len() * char_width`
  - [ ] For relative: Always show at least 3 digits
  - [ ] Add padding (e.g., 1 char on each side)
- [ ] Offset text rendering by gutter width
- [ ] Render line numbers in gutter:
  - [ ] Use Cairo/Pango to draw numbers
  - [ ] Use monospace font (same as editor)
  - [ ] Use dimmed color (gray)
  - [ ] Right-align numbers within gutter
- [ ] Highlight current line number:
  - [ ] Brighter color or bold for current line
- [ ] Handle window splits (each window has its own line numbers)

### Rendering Logic:
```rust
// Pseudocode
let gutter_width = calculate_gutter_width(buffer_lines, settings.line_numbers);
let text_x_offset = gutter_x + gutter_width;

for (visual_row, buffer_line) in visible_lines.enumerate() {
    let line_num_text = match settings.line_numbers {
        LineNumberMode::Absolute => (buffer_line + 1).to_string(),
        LineNumberMode::Relative => calculate_relative(buffer_line, cursor_line),
        LineNumberMode::Hybrid => /* hybrid logic */,
        LineNumberMode::None => continue,
    };
    
    draw_text(line_num_text, gutter_x, y, dimmed_color);
    draw_text(buffer_text, text_x_offset, y, normal_color);
}
```

### Tests:
- [ ] Visual tests — manual verification of rendering
- [ ] Test different gutter widths (10 lines vs 1000 lines)

**Validation:** Line numbers visible in UI, text properly offset, cargo build succeeds.

---

## Step 3: Relative and Hybrid Modes

**Goal:** Implement relative and hybrid line number calculations.

### Files to Modify:
- `src/main.rs` — Update line number rendering logic

### Tasks:
- [ ] Implement relative number calculation:
  - [ ] Distance = `abs(buffer_line - cursor_line)`
  - [ ] Current line shows 0
- [ ] Implement hybrid mode:
  - [ ] Current line shows absolute number
  - [ ] Other lines show relative distance
- [ ] Handle edge cases:
  - [ ] First line
  - [ ] Last line
  - [ ] Empty buffers

### Relative Number Logic:
```rust
fn calculate_relative_line_num(buffer_line: usize, cursor_line: usize) -> String {
    let distance = buffer_line.abs_diff(cursor_line);
    distance.to_string()
}

fn calculate_hybrid_line_num(buffer_line: usize, cursor_line: usize) -> String {
    if buffer_line == cursor_line {
        (buffer_line + 1).to_string() // Absolute (1-indexed)
    } else {
        calculate_relative_line_num(buffer_line, cursor_line)
    }
}
```

### Tests:
- [ ] Test relative calculation with cursor at different positions
- [ ] Test hybrid mode shows absolute for current line
- [ ] Test edge cases (first line, last line)

**Validation:** All line number modes render correctly, manual testing confirms Vim-like behavior.

---

## Step 4: Settings Commands (Optional for v1)

**Goal:** Allow changing line number settings via `:set` commands.

### Files to Modify:
- `src/core/engine.rs` — Add `:set` command handling

### Tasks:
- [ ] Implement `:set number` — Enable absolute line numbers
- [ ] Implement `:set nonumber` — Disable line numbers
- [ ] Implement `:set relativenumber` — Enable relative line numbers
- [ ] Implement `:set norelativenumber` — Disable relative line numbers
- [ ] Implement `:set number!` — Toggle absolute
- [ ] Implement `:set relativenumber!` — Toggle relative
- [ ] Save settings to file after `:set` command
- [ ] Update UI immediately after setting change

### Command Mapping:
| Command | Effect |
|---------|--------|
| `:set number` | `settings.line_numbers = Absolute` |
| `:set nonumber` | `settings.line_numbers = None` |
| `:set relativenumber` | `settings.line_numbers = Relative` |
| `:set norelativenumber` | `settings.line_numbers = Absolute` (if number was set) |
| Both set | `settings.line_numbers = Hybrid` |

### Tests:
- [ ] `test_set_number()` — Enable absolute numbers
- [ ] `test_set_relativenumber()` — Enable relative numbers
- [ ] `test_set_nonumber()` — Disable numbers
- [ ] `test_set_hybrid()` — Enable both (hybrid mode)

**Validation:** `:set` commands work, settings persist after restart.

---

## Step 5: Dynamic Gutter Width

**Goal:** Gutter width adjusts dynamically as line count changes.

### Files to Modify:
- `src/main.rs` — Make gutter width calculation dynamic

### Tasks:
- [ ] Recalculate gutter width on buffer change:
  - [ ] When lines are added (insert, paste, open)
  - [ ] When lines are deleted (dd, x, etc.)
- [ ] Cache gutter width per window to avoid recalculating every frame
- [ ] Trigger redraw when gutter width changes

### Optimization:
- Calculate width once per buffer modification
- Store in Window or View state
- Only recalculate when line count crosses digit boundary (9→10, 99→100, etc.)

### Tests:
- [ ] Test gutter width increases when crossing digit boundaries
- [ ] Test gutter width decreases when deleting lines

**Validation:** Gutter width updates correctly, no performance issues.

---

## Implementation Order

1. **Step 1:** Settings infrastructure (can be developed/tested independently)
2. **Step 2:** Basic rendering with absolute numbers (requires Step 1)
3. **Step 3:** Relative and hybrid modes (requires Step 2)
4. **Step 4:** `:set` commands (optional, can be added later)
5. **Step 5:** Dynamic gutter width (polish, can be deferred)

---

## Success Criteria

- [ ] Settings.json file loads and saves correctly
- [ ] Line numbers render in left gutter
- [ ] Absolute mode shows 1, 2, 3, 4...
- [ ] Relative mode shows distance from cursor
- [ ] Hybrid mode shows absolute for current line, relative for others
- [ ] Gutter width adjusts based on line count
- [ ] Current line number highlighted
- [ ] Settings persist across restarts
- [ ] (Optional) `:set number` and `:set relativenumber` commands work
- [ ] All existing tests still pass
- [ ] No performance degradation

---

## Technical Notes

### Color Scheme
- **Normal line numbers:** Gray (rgba 0.5, 0.5, 0.5, 1.0)
- **Current line number:** Brighter (rgba 0.8, 0.8, 0.8, 1.0) or yellow
- **Background:** Match editor background

### Font
- Use same monospace font as editor text
- Same size as editor text

### Padding
- 1 character padding on left and right of gutter
- Vertical alignment matches text lines

### Multi-Window Support
- Each window renders its own line numbers
- Cursor line is per-window, so current line highlight is per-window

---

## Future Enhancements (Out of Scope for v1)

- [ ] Sign column (for breakpoints, errors, git changes)
- [ ] Fold column
- [ ] Customizable colors via settings.json
- [ ] Line number click to select line
- [ ] Different fonts/sizes for line numbers

---

## Estimated Effort

- **Step 1 (Settings):** 1-2 hours
- **Step 2 (Basic Rendering):** 2-3 hours
- **Step 3 (Relative/Hybrid):** 1-2 hours
- **Step 4 (Commands):** 1-2 hours (optional)
- **Step 5 (Dynamic Width):** 1 hour (polish)

**Total:** ~6-10 hours for complete implementation
