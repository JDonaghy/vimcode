# Documentation Maintenance

Load this file after completing any feature or significant change.

## Files to Update

After completing any feature or significant change, update ALL of these:
- **`README.md`** — the primary user-facing reference; keep the feature tables, key reference, and command list accurate and complete; update the test count in the intro line
- **`PROJECT_STATE.md`** — internal progress tracker; update session date, test counts, file sizes, recent work entry, and roadmap checkboxes
- **`GitHub Issues`** — close completed issues, create new ones for planned work; update milestones as needed
- **`PLAN.md`** — session-level coordination doc for in-flight multi-stage features. Update after every session that advances a stage: mark completed stages with their commit SHA, adjust scope notes, bump the date. When the active wave finishes, move the section to a completed list or delete (git history retains it).
- **`EXTENSIONS.md`** — extension development guide; update if any Lua API functions, events, manifest fields, or plugin loading behavior change

## README.md Update Rules

- Add new keys/commands to the appropriate Key Reference table
- Add new `:` commands to the Command Mode table
- Add new git commands to the git commands table
- Add new settings to the settings table
- Update architecture section if new files are added or line counts change significantly
- Do NOT add speculative/planned features — only document what is implemented

## Code Summaries (`SUMMARIES/`)

The `SUMMARIES/` directory contains concise summaries of every major source file. These save tokens by letting you understand file contents without reading thousands of lines.

**When to read:** At session start or before working on a file you haven't read yet — check the summary first to understand structure and find the right methods.

**When to update:** After modifying any source file that has a summary, update the corresponding summary to reflect:
- New or removed public methods/functions
- New or removed structs/enums/types
- Changed line count (update the number)
- Changed file purpose or responsibilities

**Format:** Each summary file covers one source file and contains: purpose, line count, key types, and key public methods. Keep entries to one line each — no implementation details.

**Naming:** `SUMMARIES/gtk_mod.md`, `SUMMARIES/engine_keys.md`, `SUMMARIES/render.md`, etc. (path segments joined with `_`, no extension in name).
