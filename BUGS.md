# Known Bugs

In a .md file some key words have a different font color from most words even when no Lsp Server is running. E.g. the word "use" is highlighted in blue in vscode theme. Also words in double quotes are in orange.



## GTK Scrollbar / Tab Group Divider Overlap

When two or more editor tab groups are side by side, the vertical scrollbar of the left group extends slightly beyond the group boundary into the adjacent group's space. This is because the GTK scrollbar overlay widget is not clipped to the window rect.

Additionally, a capture-phase gesture on the overlay intercepts divider drags before the scrollbar receives them (6px hit zone). This means it is easy to accidentally start resizing the tab groups when intending to click-to-scroll on the scrollbar near the divider boundary.
