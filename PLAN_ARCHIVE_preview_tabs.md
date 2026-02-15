# Phase 4: Preview Mode — COMPLETE ✅

**Goal:** VSCode-style preview tabs
**Status:** ✅ COMPLETE
**Tests:** 242 passing (10 new)

## What Was Implemented

### Core Features
- Single-click files → preview (italic/dimmed tab, replaceable)
- Double-click → permanent (or promotes preview)
- Edit/save → auto-promotes to permanent
- `:ls` shows `[Preview]` suffix
- One global preview buffer

### Implementation
- `BufferState.preview: bool` field
- `OpenMode` enum (Preview/Permanent)
- `Engine.preview_buffer_id` tracking
- `open_file_with_mode()` method
- Auto-promote on text modification & save
- GestureClick single-click handler
- Italic font + dimmed colors in tabs
- Tab bar always visible (even with 1 tab)
- Double-click opens in new tab
- Undo clears dirty flag at oldest change
- Tab clicking to switch tabs

### Files Modified
- `src/core/buffer_manager.rs` — preview field
- `src/core/engine.rs` — core logic + 10 tests
- `src/core/mod.rs` — re-export OpenMode
- `src/main.rs` — UI handlers, tab rendering

## Commit
```
39e9b18 feat: add VSCode-style preview mode for file explorer
```

## Next Steps
- TBD (choose from roadmap)
