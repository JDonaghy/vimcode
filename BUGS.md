# Known Bugs


## GTK Scrollbar / Tab Group Divider Overlap

When two or more editor tab groups are side by side, the vertical scrollbar of the left group extends slightly beyond the group boundary into the adjacent group's space. This is because the GTK scrollbar overlay widget is not clipped to the window rect.

Additionally, a capture-phase gesture on the overlay intercepts divider drags before the scrollbar receives them (6px hit zone). This means it is easy to accidentally start resizing the tab groups when intending to click-to-scroll on the scrollbar near the divider boundary.

## LSP Semantic Token Highlighting Disappears After Hover Popup

LSP semantic token highlighting in the main editor often stops appearing after using hover popups (editor hover via `gh` or `K`). The highlighting may not return until the file is re-opened or the LSP server is restarted. Likely related to the hover popup flow triggering a code path that clears or fails to re-request semantic tokens. Partially addressed in Session 201 (removed aggressive token clearing on edit, guarded against empty-response overwrites and missing-legend overwrites), but the issue persists — possibly a different trigger path connected to the hover popup lifecycle.
