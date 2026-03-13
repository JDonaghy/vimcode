- Pressing Ctrl-D in Visual Mode when a line is selected deletes the line instead of extending the selection down

- Pressing Ctrl-U in Visual Mode when a line is selected changes the case of the current line instead of extending the selection up

- I see the error "Error: Can't open display: (null)" in tui mode regularly. This appears at different places on the screen, often at the bottom, but not always.

- After undoing a change that triggers a lot of LSP errors, they don't go away, but reverting the change should cause an update from the LSP Server and make the errors disappear.

- In the Find Panel, when I click on the text box to enter text to search for, the text is directed to the current editor buffer. So if I type "ab" it inserts the letter "b" in the buffer rather and adding "ab" to the text search box

- If an auto-suggest windows appears, sometimes it is sticky and cant be dismissed by pressing ESC. If I click away from it, or move the cursor via the keyboard, it follows the cursor position. It should never move like that and it should always be dismissable by pressing ESC.

- In diff view (`:Gdiffsplit` / `:diffthis`), when one side has many more changed lines than the other (e.g., 100 added lines on right, 1 removed on left), the left window fills with large blocks of blank padding lines. With fold filtering active (`diff_unchanged_hidden`), these padding blocks should be suppressed but are still rendered because the aligned-index tracking in `build_rendered_window` (render.rs) doesn't properly skip padding entries when advancing past folded/hidden buffer lines. The current fix attempt (advancing `aligned_idx` when `is_line_hidden`) doesn't fully resolve it — the blank space persists in practice.

- The diff toolbar (filter/navigation buttons) does not appear on both tab bars when using `:diffthis` with windows in separate editor groups. It only shows on the active group's tab bar. The `is_in_diff_view()` function was changed to check all groups, but the toolbar still only appears for one side. The toolbar should always be visible on every group tab bar that contains a diff window, regardless of which group is active.

