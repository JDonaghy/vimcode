# Git Insights

Inline git blame annotations and per-line blame, file history — powered by a Lua script that hooks into VimCode's cursor-move event.

**Scripts**: `blame.lua`
**Requires**: `git` on PATH

## Install

```
:ExtInstall git-insights
```

After installing, move the cursor to any line in a file tracked by git and the blame annotation appears inline at the end of the line.
