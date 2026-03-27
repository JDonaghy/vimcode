# src/core/engine/visual.rs — 884 lines

Visual mode key handling and multi-cursor support.

## Key Methods
- `handle_visual_key(key, ctrl, unicode)` — visual/visual-line/visual-block mode key handler
- Supports: motions (hjkl, w/b/e, 0/$, gg/G, f/t, %), operators (d/c/y/>/</=), text objects, mode switching (v/V/Ctrl-V), search (*/# /n/N), case toggle (~), increment (Ctrl-A/X)
- Multi-cursor: `Alt-D` adds cursor at next match; all cursors receive identical keystrokes; Escape collapses
