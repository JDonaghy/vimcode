# Git Insights

Inline git blame annotations — see who last changed each line, when, and why.

**Requires**: `git` on PATH

## How It Works

Move the cursor to any line in a git-tracked file and the blame annotation appears inline at the end of the line, showing the author, relative date, and commit message.

Annotations are suppressed in Insert mode to avoid distraction while editing.

## Features

- Inline blame annotations on the current line
- Automatic update as you navigate
- Clean display: uncommitted changes show no annotation
- Works with unsaved buffers (uses `--contents -` to blame working copy)
