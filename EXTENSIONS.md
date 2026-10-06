# VimCode Extension Development Guide

This document covers everything needed to build VimCode extensions, including the Lua plugin API
and the TOML manifest format. An extension bundles an LSP server config, optional DAP (debugger)
adapter, and optional Lua scripts into a single named package.

> **Registry:** Official extensions live in the [vimcode-ext](https://github.com/JDonaghy/vimcode-ext)
> repository. VimCode fetches the registry on startup and caches it locally. You can also develop
> and test extensions locally before submitting them to the registry.

## Quick Start

Create a directory under `~/.config/vimcode/extensions/` with a `manifest.toml` and optional
Lua scripts:

```
~/.config/vimcode/extensions/my-extension/
├── manifest.toml    # Required: extension metadata + LSP/DAP config
├── my_script.lua    # Optional: Lua plugin script
└── README.md        # Optional: shown in Extensions panel on Enter
```

Install with `:ExtInstall my-extension`, manage with `:ExtList`, `:ExtDisable`, `:ExtEnable`,
`:ExtRemove`.

## Local Extension Development

You can develop and test extensions entirely locally without publishing to the registry:

1. **Create the extension directory:**
   ```bash
   mkdir -p ~/.config/vimcode/extensions/my-extension
   ```

2. **Write a `manifest.toml`** (see schema below):
   ```toml
   name = "my-extension"
   display_name = "My Extension"
   description = "What it does"
   version = "0.1.0"
   scripts = ["my_script.lua"]
   ```

3. **Write your Lua scripts** in the same directory.

4. **Install it:** Open VimCode, open the Extensions panel — your local extension appears
   in the AVAILABLE list. Press `i` or run `:ExtInstall my-extension` to activate it.
   Alternatively, you can directly run `:ExtInstall my-extension` without opening the panel.

5. **Iterate:** Edit your scripts, then `:Plugin reload` to reload without restarting.

Local extensions override registry extensions with the same name, so you can fork and modify
an existing extension by copying it to `~/.config/vimcode/extensions/<name>/`.

### Updating an Existing Extension

When modifying a published extension (e.g. `git-insights`), always update the **registry repo first**, then sync to the local cache:

1. **Edit in the `vimcode-ext` repo** (`~/src/vimcode-ext/<extension>/`)
2. **Commit and push** the changes to the registry repo
3. **Copy the updated files** to the local cache (`~/.config/vimcode/extensions/<extension>/`)
4. **Reload** in VimCode with `:Plugin reload`

This ensures the registry repo is the source of truth. Never edit the local cache without propagating changes back to the repo — the local cache will be overwritten on reinstall.

### Submitting to the Registry

When your extension is ready, submit a PR to
[vimcode-ext](https://github.com/JDonaghy/vimcode-ext) adding your extension directory
(manifest.toml + scripts + README.md) and an entry in `registry.json`. Once merged,
it becomes available to all VimCode users via the Extensions panel.

### Self-Hosted Registry

Set `extension_registry_url` in `~/.config/vimcode/settings.json` to point to your own
`registry.json` URL. The format is the same as the official registry — a JSON array of
extension manifest objects.

---

## Manifest Format (`manifest.toml`)

Every extension needs a `manifest.toml` describing its capabilities.

### Complete Schema

```toml
# ── Required ──────────────────────────────────────────────
name = "my-extension"                       # Identifier (lowercase, hyphens ok)
display_name = "My Extension"               # Human-readable name

# ── Optional metadata ────────────────────────────────────
description = "What this extension does"    # One-liner
version = "1.0.0"                           # Semver

# ── File / language activation ────────────────────────────
file_extensions = [".py", ".pyi"]           # File types that activate this extension
language_ids = ["python"]                   # LSP language identifiers
workspace_markers = ["pyproject.toml"]      # Files indicating project root

# ── LSP server ────────────────────────────────────────────
[lsp]
binary = "pyright-langserver"               # Primary binary name (must be on PATH)
install = "npm install -g pyright"          # Shell command to install (shown to user)
fallback_binaries = ["pylsp"]               # Tried in order if primary not found
args = ["--stdio"]                          # Arguments passed to the LSP binary

# ── DAP debugger adapter ──────────────────────────────────
[dap]
adapter = "debugpy"                         # Adapter registry name
binary = "python"                           # Executable to launch
install = "pip install debugpy"             # Install command
transport = "stdio"                         # "stdio" or "tcp"
args = ["-m", "debugpy.adapter"]            # Launch arguments

# ── Lua scripts ───────────────────────────────────────────
scripts = ["my_script.lua"]                 # Filenames of bundled Lua scripts

# ── Comment style override ────────────────────────────────
[comment]
line = "//"                                 # Single-line comment prefix
block_open = "/*"                           # Block comment open
block_close = "*/"                          # Block comment close

# ── Board panel data provider (#522) ──────────────────────
[board]
refresh_command = ["my-tool", "board", "--json"]  # Argv run to refresh the Board panel;
                                                   # must print a BoardModel JSON document
                                                   # on stdout and exit zero
poll_interval_secs = 30                     # Seconds between automatic refreshes
tick_command = ["my-tool", "notify"]        # Optional: argv run periodically to nudge a
                                             # daemon-less pipeline forward, opt-in via the
                                             # "Board Auto-Tick" setting (default off, #523)
tick_interval_secs = 300                    # Seconds between tick_command runs (default 300)

# Provider-declared, named board actions (#523) — the card context menu's
# contents, a stage keybinding table, and (via `OpenIssue`/`OpenReview`-
# named entries) what `quadraui::BoardAction`'s corresponding variants
# dispatch to.
[[board.actions]]
name = "assign"                             # Action id (also the :context menu key)
label = "Dispatch Work"                     # Shown in the menu / confirmation dialog
command = ["my-tool", "assign", "{id}"]     # Argv; "{id}" is replaced with the card id
# stages = ["col:ready"]                    # Optional: column ids this is valid in
                                             # (omit/empty = valid in every stage)
# key = "d"                                 # Optional: single-key binding for the stage
confirm = true                              # Confirm before running (irreversible/metered)

[[board.actions]]
name = "OpenReview"                         # Matches a `quadraui::BoardAction::OpenReview`
label = "Start Review"
command = ["my-tool", "review", "{id}"]     # Must print a review target as JSON — see below

[board.verdict_commands]                    # Review verdicts (#526), keyed by the
approve = ["my-tool", "verdict", "{id}",    # generic "approve"/"request-changes"/
  "--ok", "--body-file", "{body_file}"]     # "comment" tokens; "{body_file}" is a
request-changes = ["my-tool", "verdict",    # temp file holding the composed review
  "{id}", "--changes", "--body-file",       # body (never inline)
  "{body_file}"]
```

**`OpenReview` is the one special-cased action name (#525).** Every other
action is fire-and-forget: vimcode runs the argv, surfaces its exit status
and trimmed stdout on the status line, and refreshes the board.
`OpenReview` instead expects its command to print a **review target** as
JSON on stdout, which vimcode resolves into a local git diff and opens as
an in-editor multi-file review (the same surface `:Review` uses):

```json
{ "branch": "issue-42-my-work", "base": "develop", "host": "worker-3" }
```

`branch` and `base` are required — both are resolved as **local** git
revisions (three-dot diff), so the branch must already exist in the
workspace vimcode has open. `host` is optional provenance: when the
provider's roster spans more than one machine, it is painted in the review
footer alongside the branch and worktree path, so a human editing files
there can tell it isn't their own checkout.

### Field Reference

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | String | Yes | Unique identifier, used in `:ExtInstall` |
| `display_name` | String | Yes | Shown in the Extensions sidebar |
| `description` | String | No | Short description |
| `version` | String | No | Semver version |
| `file_extensions` | String[] | No | File extensions (e.g., `[".py", ".pyi"]`) |
| `language_ids` | String[] | No | LSP language IDs (e.g., `["python"]`) |
| `workspace_markers` | String[] | No | Files/dirs indicating project root |
| `scripts` | String[] | No | Lua script filenames to load |

#### `[lsp]` Section

| Field | Type | Description |
|-------|------|-------------|
| `binary` | String | Primary LSP server binary name |
| `install` | String | Shell command to install the server |
| `fallback_binaries` | String[] | Alternative binaries tried in order |
| `args` | String[] | Command-line arguments (e.g., `["--stdio"]`) |
| `dependencies` | String[] | System binaries required on PATH (checked before starting) |
| `ignore_error_sources` | String[] | Diagnostic sources whose errors are excluded from explorer counts (e.g., `["rust-analyzer"]` — its internal analysis produces false positives; real errors come from `rustc`) |

#### `[dap]` Section

| Field | Type | Description |
|-------|------|-------------|
| `adapter` | String | Adapter name for DAP registry |
| `binary` | String | Executable to launch |
| `install` | String | Shell command to install |
| `transport` | String | `"stdio"` (default) or `"tcp"` |
| `args` | String[] | Launch arguments |

#### `[comment]` Section

Override comment style for languages handled by this extension.

| Field | Type | Description |
|-------|------|-------------|
| `line` | String | Line comment prefix (e.g., `"//"`, `"#"`, `"--"`) |
| `block_open` | String | Block comment open (e.g., `"/*"`) |
| `block_close` | String | Block comment close (e.g., `"*/"`) |

#### `[board]` Section (#522, actions #523)

Declares this extension as a data provider for the Board panel. Generic — no
particular provider is named or assumed; any external tool that emits
vimcode's board JSON contract (a `BoardModel`: columns of cards with inline
status badges, matching quadraui's `Board` component) on stdout works here.

| Field | Type | Description |
|-------|------|-------------|
| `refresh_command` | String[] | Argv run for a board refresh. `refresh_command[0]` is the binary, the rest are arguments. Must print a `BoardModel` JSON document to stdout and exit zero. |
| `poll_interval_secs` | Integer | Seconds between automatic background refreshes (default `30`). |
| `verdict_commands` | Table | Maps a review verdict — `"approve"`, `"request-changes"`, or `"comment"` (generic code-review terms, #526) — to an argv template run when that verdict is reported from the change-review surface. `{id}` substitutes the reviewed card's id; `{body_file}` substitutes the path to a temp file holding the composed review body (never sent inline — review bodies contain newlines, code fences and quotes unsafe to splice into an argv). A verdict with no entry is simply not offered. |
| `tick_command` | String[] | Optional argv run periodically as a fire-and-forget nudge to a daemon-less pipeline (e.g. a "notify" command) — so it doesn't stall just because vimcode is the only client with the board open. Opt-in: only runs when the user enables the "Board Auto-Tick" setting (`board_tick_enabled`, **default off** — a passive viewer must not silently dispatch metered work). |
| `tick_interval_secs` | Integer | Seconds between `tick_command` runs, when enabled (default `300`). |
| `actions` | Array of tables (`[[board.actions]]`) | Provider-declared named actions a card can be dispatched through — the right-click context menu's contents, a stage keybinding table, and the target of `quadraui::BoardAction::OpenIssue`/`OpenReview` (matched by `name`) when no more specific handling applies. See the field reference below. |

Each `[[board.actions]]` entry:

| Field | Type | Description |
|-------|------|-------------|
| `name` | String | Action id. Also matched against `OpenIssue`/`OpenReview` to wire those `BoardAction` variants to a provider command. |
| `label` | String | Optional human-readable label for the context menu / confirmation dialog (falls back to `name`). |
| `command` | String[] | Argv to run. The literal token `{id}` in any argument is substituted with the acted-on card's id. An entry with an empty `command` is declared-but-not-runnable. Stdout is surfaced on the status line, **except** for `name = "OpenReview"`, whose stdout must be a review-target JSON document (see above). |
| `stages` | String[] | Optional column ids this action is valid in. Empty/omitted means valid in every stage. |
| `key` | String | Optional single-key binding while the Board panel has focus and a card matching one of `stages` is selected (e.g. `"P"`/`"S"`/`"F"` for Test verdicts). `R` is reserved by the host for "open the review for the selected card" and cannot be rebound. |
| `confirm` | Bool | Whether firing this action needs a Yes/No confirmation first (default `false`) — set this for irreversible or metered actions (dispatch work, merge). |

If no provider extension is installed/configured, the Board panel reports
"no board provider configured" — every other part of the editor is
unaffected. A missing or failing provider binary degrades to a message in
the panel, never a crash.

---

## Example Manifests

### Language Extension (Python)

```toml
name = "python"
display_name = "Python Language Support"
file_extensions = [".py", ".pyi", ".pyw"]
language_ids = ["python"]
workspace_markers = ["pyproject.toml", "setup.py", "setup.cfg", "requirements.txt"]

[lsp]
binary = "pyright-langserver"
install = "npm install -g pyright"
fallback_binaries = ["basedpyright-langserver", "pylsp", "jedi-language-server"]
args = ["--stdio"]

[dap]
adapter = "debugpy"
binary = "python"
transport = "stdio"
args = ["-m", "debugpy.adapter"]
```

### Tooling Extension (Git Insights)

```toml
name = "git-insights"
display_name = "Git Insights"
description = "Inline git blame annotations and file history"
file_extensions = []
language_ids = []
scripts = ["blame.lua", "history.lua", "show.lua", "line_history.lua",
           "diff.lua", "stash.lua", "repo_log.lua", "git_log_panel.lua"]
```

### Minimal LSP-Only Extension

```toml
name = "terraform"
display_name = "Terraform Language Support"
file_extensions = [".tf", ".tfvars", ".hcl"]
language_ids = ["terraform", "terraform-vars"]
workspace_markers = ["main.tf", "terraform.tfstate"]

[lsp]
binary = "terraform-ls"
install = "brew install hashicorp/tap/terraform-ls"
args = ["serve"]
```

---

## Lua Plugin API

Lua scripts have access to the `vimcode` global object. Scripts run in Lua 5.4.

**Two kinds of call.** Most of the original API (`vimcode.buf.*`, `vimcode.opt.*`,
`vimcode.state.*`, `vimcode.message`, `vimcode.command_run`, `vimcode.panel.*`, …) is
**queued**: reads come from a snapshot taken before your callback ran, and writes are
applied after it returns. The newer namespaces (`vimcode.buffer.*`, `vimcode.window.*`,
`vimcode.keymap.set`/`list`, `vimcode.loop.*`, `vimcode.schedule`/`defer`,
`vimcode.picker.*`, `vimcode.decor.*`, `vimcode.syntax.*`, `vimcode.undo.*`,
`vimcode.diagnostics.*`, `vimcode.ui.open_view`, `vimcode.http.*`, `vimcode.fire_event`)
are **immediate**: they act on the live editor at call time.

Immediate calls only work while vimcode is running one of your callbacks (a command,
event hook, keymap, timer, process or HTTP callback, picker or view callback). Calling
one at the top level of your script, during load, raises
`"…: no live editor"`. Do that work from a `vimcode.on("VimEnter", …)` hook instead.
(The exception is `vimcode.keymap.set`, which also works at load time. `vimcode.json.*`
and `vimcode.storage.*` work anywhere.)

Buffers and windows are plain integer **handles**, and `0` always means "current".

### Registration Functions

Call these at the top level of your script (during load time):

```lua
-- Register a custom command (callable via :MyCommand args)
vimcode.command("MyCommand", function(args)
    -- args is a string containing everything after the command name
    vimcode.message("Got: " .. args)
end)

-- Register an event hook (see the Events Reference)
vimcode.on("BufWrite", function(path)
    vimcode.message("Saved: " .. path)
end)

-- Register a key mapping (legacy form): consulted *after* built-in keys,
-- so it cannot take over a key vimcode already binds.
-- Modes: "n" (normal), "i" (insert), "v" (visual), "c" (command)
vimcode.keymap("n", "<leader>h", function()
    vimcode.message("Hello from keymap!")
end)
```

To own a key that has a built-in meaning, or for expr, buffer-local, operator-pending
or visual-only maps, use `vimcode.keymap.set` (see [Events and Keymaps](#events-and-keymaps)).

### Core Functions

```lua
vimcode.message(text)           -- Display a status bar message
vimcode.cwd()                   -- Get current working directory (string)
vimcode.command_run(cmd)        -- Execute a VimCode command (e.g., "w", "q", "split")
vimcode.feedkeys(keys)          -- Queue keystrokes, Vim notation: "dw", "<Esc>", "<C-a>", "<CR>"
vimcode.eval(expr)              -- Evaluate a small Vim-style expression (see below)
vimcode.open_url(url)           -- Open a URL in the system browser
vimcode.set_operatorfunc(fn)    -- Register a function for g@{motion} (receives "line"/"char")
vimcode.fire_event(name)        -- Fire the "User" event with `name` as its argument
```

`vimcode.eval` understands only these forms and returns `nil` for anything else:
`@r` (register contents, string), `&option` (setting value, string), `line('.')`,
`col('.')`, `line('$')` (integers), and `mode()` (string).

`feedkeys`, `command_run`, `message` and `open_url` are queued: they run after your
callback returns.

**Operator functions.** `g@{motion}` sets the `'[` and `']` marks to the motion's span
(1-indexed, inclusive) before calling the function registered with
`vimcode.set_operatorfunc`, so the function reads its range with
`vimcode.state.mark("[")` / `vimcode.state.mark("]")`:

```lua
-- ys{motion}: wrap the motion's text in quotes (single-line spans)
vimcode.keymap.set("n", "ys", function()
    vimcode.set_operatorfunc(function(_)
        local a, b = vimcode.state.mark("["), vimcode.state.mark("]")
        local buf = vimcode.buffer.current()
        local line = vimcode.buffer.get_lines(buf, a.line - 1, a.line)[1]
        vimcode.buffer.set_lines(buf, a.line - 1, a.line, {
            line:sub(1, a.col - 1) .. '"' .. line:sub(a.col, b.col) .. '"' .. line:sub(b.col + 1),
        })
    end)
    vimcode.feedkeys("g@")
end)
```

### Buffer Functions (`vimcode.buf.*`)

Line numbers are **1-indexed**, except `get_lines`/`set_lines`, which are 0-indexed
with an exclusive end (Neovim's `nvim_buf_get_lines` convention).

```lua
vimcode.buf.lines()             -- All buffer lines as a table
vimcode.buf.line(n)             -- Get line n (returns string or nil)
vimcode.buf.set_line(n, text)   -- Replace line n with text
vimcode.buf.insert_line(n, text)-- Insert new line before position n
vimcode.buf.delete_line(n)      -- Delete line n
vimcode.buf.get_lines(s, e)     -- Lines [s, e), 0-indexed; negative counts from the end.
                                --   Strings keep their trailing "\n".
vimcode.buf.set_lines(s, e, lines) -- Replace lines [s, e) with the `lines` table
vimcode.buf.line_count()        -- Total number of lines
vimcode.buf.path()              -- File path (string or nil for unnamed buffers)
vimcode.buf.cursor()            -- Returns {line=N, col=M} (1-indexed)
vimcode.buf.set_cursor(line, col) -- Move cursor to position
vimcode.buf.annotate_line(n, text) -- Add virtual text annotation to line n
vimcode.buf.clear_annotations() -- Clear all line annotations
vimcode.buf.open_scratch(name, content, opts) -- Open a scratch buffer
  -- opts (optional table): readonly=bool, filetype=string, split="vertical"|"horizontal"
```

Writes made through `vimcode.buf.*` are **queued** and applied *after* your
callback returns, and reads come from a snapshot taken *before* it ran — so a
read never sees a write made in the same callback. When you need
read-after-write, a buffer you did not open, or a handle to pass around, use the
immediate API below.

### Immediate Buffer / Window API (`vimcode.buffer.*`, `vimcode.window.*`)

These take effect **at call time**, against the live editor. Buffers and windows
are plain integer handles, and `0` always means "current".

```lua
vimcode.buffer.current()              -- Handle of the active buffer
vimcode.buffer.is_valid(buf)          -- Does this handle name a live buffer?
vimcode.buffer.line_count(buf)        -- Number of lines
vimcode.buffer.get_lines(buf, s, e)   -- Lines [s, e) as a table, no trailing "\n"
vimcode.buffer.set_lines(buf, s, e, lines) -- Replace lines [s, e); applied immediately
vimcode.buffer.create(opts)           -- New buffer -> handle
  -- opts (optional table): scratch=bool, name=string
vimcode.buffer.set_write_handler(buf, fn) -- :w on `buf` calls fn(buf) instead of
                                      --   writing to disk; false if `buf` is invalid

vimcode.window.current()              -- Handle of the focused window
vimcode.window.is_valid(win)
vimcode.window.get_buf(win)           -- Buffer shown in this window
vimcode.window.set_buf(win, buf)      -- Show `buf` in `win` (fires `BufLeave`, `BufEnter`)
vimcode.window.get_cursor(win)        -- {line=N, col=M} (also [1]=N, [2]=M), 1-indexed
vimcode.window.set_cursor(win, pos)   -- pos: {line=N, col=M} or {N, M}, 1-indexed
```

Conventions, and how they differ from `vimcode.buf.*`:

- `get_lines`/`set_lines` are **0-indexed with an exclusive end**, and a negative
  index counts back from the end of the buffer (same as `vimcode.buf.get_lines`).
- `get_lines` strips the trailing newline; `vimcode.buf.get_lines` does not.
- `line_count` counts *logical* lines, so a buffer ending in a newline does not
  report an extra empty line, and
  `set_lines(b, 0, line_count(b), get_lines(b, 0, line_count(b)))` is a no-op.
  `vimcode.buf.line_count()` is one larger on such buffers.
- Cursors are 1-indexed, like `vimcode.buf.cursor()`.
- All of a plugin's immediate edits to one buffer in one callback collapse into a
  single undo step.
- If you mix the two tiers in one callback, the queued write is applied last and
  therefore wins.
- Calling these outside a callback (e.g. at the top level of your script, during
  load) is an error: there is no live editor to talk to yet. Do the work from a
  `vimcode.on("VimEnter", …)` hook instead.

**Write handler.** `set_write_handler` claims a buffer's writes (the shape oil.nvim's
`BufWriteCmd` uses): `:w` fires `BufWritePre`, then calls your function with the buffer
handle instead of writing the file. Persist the content however you like. The buffer is
marked unmodified afterwards.

Example — build a scratch report and show it:

```lua
vimcode.command("Report", function(_)
  local src = vimcode.buffer.current()
  local n = vimcode.buffer.line_count(src)
  local out = vimcode.buffer.create({ scratch = true, name = "Report" })
  vimcode.buffer.set_lines(out, 0, -1, { "lines: " .. n, "" })
  for _, line in ipairs(vimcode.buffer.get_lines(src, 0, math.min(n, 20))) do
    local at = vimcode.buffer.line_count(out)
    vimcode.buffer.set_lines(out, at, at, { "  " .. line })
  end
  vimcode.window.set_buf(0, out)
end)
```

### Settings Functions (`vimcode.opt.*`)

```lua
vimcode.opt.get("tabstop")      -- Query a setting value (returns string)
vimcode.opt.set("tabstop", "4") -- Set a setting value
```

Available settings include: `number`, `relativenumber`, `tabstop`, `shiftwidth`, `expandtab`,
`autoindent`, `wrap`, `hlsearch`, `ignorecase`, `smartcase`, `scrolloff`, `cursorline`,
`colorcolumn`, `textwidth`, `splitbelow`, `splitright`, `colorscheme`, and more.

### State Functions (`vimcode.state.*`)

```lua
vimcode.state.mode()            -- Current mode: "Normal", "Insert", "Visual", etc.
vimcode.state.filetype()        -- Buffer language ID: "rust", "python", etc.
vimcode.state.register("a")     -- Get register: {content="...", linewise=false} or nil
vimcode.state.set_register("a", "text", false) -- Set register (char, content, linewise)
vimcode.state.mark("a")         -- Get mark position: {line=N, col=M} or nil
                                --   ("[" / "]" give the last g@ motion's span)
```

### Git Functions (`vimcode.git.*`)

```lua
-- Get blame info for a single line (returns table or nil)
local blame = vimcode.git.blame_line(10)
-- blame = {hash="abc123", author="Name", date=1700000000,
--          relative_date="3 days ago", message="Fix bug", not_committed=false}

-- Get structured blame for every line in the current buffer
local all_blame = vimcode.git.blame_file()
-- all_blame = {{hash="abc123", author="Name", ...}, ...}

-- Get recent commits for the current file (simple)
local log = vimcode.git.log_file(20)
-- log = {{hash="abc123", message="Fix bug"}, ...}

-- Get detailed commits for the current file (with author, date, stat)
local detailed = vimcode.git.file_log_detailed(20)
-- detailed = {{hash="abc123", author="Name", date="3 days ago", message="Fix bug", stat="1 file changed"}, ...}

-- Get commits that touched a specific line range
local line_commits = vimcode.git.line_log(10, 20, 50)
-- line_commits = {{hash="abc123", author="Name", date="3 days ago", message="Fix"}, ...}

-- Get repo-wide commit log
local repo_log = vimcode.git.log(100)
-- repo_log = {{hash="abc123", message="Fix bug"}, ...}

-- Show full commit details
local show = vimcode.git.show("abc123")  -- string or nil

-- Diff against a ref (branch, tag, HEAD, etc.)
local diff = vimcode.git.diff_ref("main")  -- string or nil

-- Repository info
local root = vimcode.git.repo_root()  -- string or nil
local branch = vimcode.git.branch()   -- string or nil

-- Stash operations
local stashes = vimcode.git.stash_list()
-- stashes = {{index=0, message="WIP", branch="main"}, ...}
local result = vimcode.git.stash_push("save my work")  -- string
local result = vimcode.git.stash_pop(0)                 -- string
local diff = vimcode.git.stash_show(0)                   -- string or nil

-- List all branches with tracking info
local branches = vimcode.git.branches()
-- branches = {{name="main", tracking="origin/main", is_current=true}, ...}

-- List files changed in a commit
local files = vimcode.git.commit_files("abc123")
-- files = {"src/main.rs", "src/lib.rs", ...}

-- Get diff for a specific file at a commit
local diff = vimcode.git.diff_file("abc123", "src/main.rs")  -- string or nil

-- Get file contents at a specific commit
local content = vimcode.git.show_file("abc123", "src/main.rs")  -- string or nil

-- Get detailed commit info (author, date, message, stat)
local detail = vimcode.git.commit_detail("abc123")
-- detail = {hash="abc123", author="Name", date="2026-03-22", message="Fix bug", stat="2 files changed, 10 insertions(+)"}

-- Open a side-by-side diff for a file at a commit (uses engine diff infrastructure)
vimcode.git.open_diff("abc123", "src/main.rs")
```

### Events and Keymaps

`vimcode.on(event, fn)` registers a hook; every event and its argument is listed in the
[Events Reference](#events-reference). A hook always receives a single string. Where an
event carries a buffer or window handle, convert it with `tonumber(arg)`.

```lua
vimcode.keymap.set(mode, lhs, fn, opts) -- Map lhs; consulted *before* built-in keys
vimcode.keymap.list()                   -- Every active map: {{mode=, lhs=, buffer=, desc=}, ...}
vimcode.fire_event(name)                -- Fire "User" with `name` as the argument
```

- `mode` is one of `"n"`, `"i"`, `"v"` (all visual modes), `"x"` (visual only),
  `"o"` (operator-pending, e.g. after `d`) or `"c"`.
- `lhs` uses Vim key notation, including `<leader>`.
- `opts` (optional table):
  - `expr = true`: the callback returns a key string, which is fed back through the
    normal key path (`return "dd"` deletes a line).
  - `buffer = handle`: the map only applies while that buffer is active. A buffer-local
    map must be registered from inside a callback, such as `BufEnter`, because no
    buffer exists at load time. `buffer` is ignored at load time.
  - `desc = "..."`: a description, returned by `keymap.list()`.
- `keymap.list()` covers config-defined and Lua maps alike. `buffer` is `nil` for a
  global map and `desc` is `nil` when none was given.

```lua
vimcode.keymap.set("n", "<leader>w", function()
    vimcode.command_run("w")
end, { desc = "Save file" })

vimcode.keymap.set("n", "Q", function() return "dd" end, { expr = true })

vimcode.on("FileType", function(ft)
    if ft == "markdown" then
        vimcode.keymap.set("n", "<leader>p", function()
            vimcode.message("preview")
        end, { buffer = vimcode.buffer.current(), desc = "Markdown preview" })
    end
end)

-- Signal other plugins; they listen with vimcode.on("User", ...)
vimcode.on("VimEnter", function() vimcode.fire_event("MyPluginReady") end)
```

### Async Shell Execution

Run shell commands in a background thread with results delivered via event hooks:

```lua
-- Basic usage
vimcode.async_shell("git status", "my_result_event")

-- With options
vimcode.async_shell("grep -n pattern", "search_done", {
    stdin = "input text",   -- Optional: pipe to stdin
    cwd = "/path/to/dir"    -- Optional: working directory
})

-- Handle the result
vimcode.on("my_result_event", function(output)
    vimcode.message("Result: " .. output)
    -- Exit status of the command behind this event (integer), or nil if
    -- no result has arrived yet
    local code = vimcode.async_shell_exit_code("my_result_event")
end)
```

For streamed output, stdin, or killing a process, use `vimcode.loop.spawn` below.

### Timers and Processes (`vimcode.loop.*`, `vimcode.schedule`, `vimcode.defer`)

Callbacks run on the main thread and may use the whole API, including the immediate
`vimcode.buffer.*` calls.

```lua
vimcode.schedule(fn)                -- Run fn() once, on the next idle tick
vimcode.defer(ms, fn)               -- Run fn() once, after ms milliseconds

local t = vimcode.loop.timer(ms, fn, opts) -- Run fn() after ms; opts: {["repeat"]=true}
t:stop()                            -- Cancel it

local p = vimcode.loop.spawn(cmd, args, opts) -- Start a process (no shell)
  -- args: table of strings, or nil
  -- opts (optional): cwd=string, env={NAME="value", ...},
  --   on_stdout=fn(chunk), on_stderr=fn(chunk), on_exit=fn(code, signal)
p:write(data)                       -- Write to its stdin
p:close_stdin()
p:kill()                            -- Kill it; on_exit still fires
```

- `repeat` is a Lua keyword, so the option must be written `{ ["repeat"] = true }`.
- `on_stdout`/`on_stderr` receive output in chunks as it arrives, not necessarily
  whole lines.
- `on_exit` fires once with `(code, signal)`. Exactly one is non-nil: a normal exit
  carries `code`, and a death by signal (such as `:kill()` on Unix) carries `signal`.
- `spawn` raises an error if the process cannot be started.
- Unloading a plugin stops its timers and kills its processes.

```lua
local p = vimcode.loop.spawn("rg", { "--line-number", "TODO" }, {
    cwd = vimcode.cwd(),
    on_stdout = function(chunk) vimcode.message(chunk) end,
    on_exit = function(code, signal)
        vimcode.message("rg exited: " .. tostring(code or ("signal " .. signal)))
    end,
})
vimcode.defer(5000, function() p:kill() end)
```

### Picker API (`vimcode.picker.*`)

Opens the built-in fuzzy picker with items your plugin supplies.

```lua
local picker = vimcode.picker.open({
    title = "Pick one",
    items = {
        { display = "first",  data = 1 },
        { display = "second", detail = "more info", icon = "*",
          data = { any = "value" }, preview = { file = "/path/to/file", line = 10 } },
    },
    on_select = function(data) end,  -- called with the chosen item's `data`
    on_cancel = function() end,      -- picker dismissed
    on_query  = function(query) end, -- query text changed
})

picker:set_items(items)   -- Replace the whole list
picker:append(items)      -- Add items; keeps the query and the selection
picker:set_loading(bool)  -- Mark the list as still loading
picker:close()            -- Close the picker
```

Item fields (all optional):

| Field | Meaning |
|-------|---------|
| `display` | Text shown in the list |
| `filter_text` | Text matched against the query, instead of `display` |
| `detail` | Secondary text |
| `icon` | Icon string |
| `data` | Any Lua value, passed back to `on_select` unchanged (`nil` if absent) |
| `preview` | `{file = path, line = n}` or `{buffer = handle, line = n}`. `line` is 1-indexed and optional. A buffer preview shows the buffer's live text. |

- The picker always fuzzy-filters locally. `on_query` is only for a dynamic source
  (such as live grep) that wants to re-run on each keystroke and refill the list
  with `set_items`/`append`.
- The handle methods can be called with `.` or `:`. They are immediate calls, so a
  `loop.spawn` `on_stdout` callback can stream results in with `picker:append(...)`.
- `set_loading` records state only. It is not drawn yet.
- `open` raises an error if called outside a callback.

### Decorations (`vimcode.decor.*`)

Namespaced extmarks: highlights, virtual text and gutter signs anchored to buffer
positions. Marks move with inserted and deleted lines, and across undo and redo.

```lua
local ns = vimcode.decor.namespace(name)   -- Namespace id; same name -> same id
vimcode.decor.set_hl(name, opts)           -- Define or update a highlight group
local id = vimcode.decor.set_mark(buf, ns, opts) -- Add a mark -> id (nil if buf invalid)
vimcode.decor.get_mark(buf, ns, id)        -- The mark as a table, or nil
vimcode.decor.del_mark(buf, ns, id)        -- -> bool
vimcode.decor.clear(buf, ns, start, end)   -- Remove ns's marks; optional row range [start, end)
```

`set_hl` options: `fg`, `bg` (`"#rrggbb"` or `"#rrggbbaa"`), `bold`, `italic`,
`underline` (booleans), and `link` (another group name). A `link` resolves to another
`set_hl` group first, then to a theme role matched case-insensitively (for example
`"Comment"`), and is re-resolved when the colour scheme changes.

`set_mark` options:

| Key | Meaning |
|-----|---------|
| `row`, `col` | Anchor, 0-indexed. `col` counts characters. Default 0. |
| `end_row`, `end_col` | End of a range mark (exclusive `end_col`). Default: the anchor, which makes a point mark. |
| `hl_group` | Highlight the range `[row,col)..(end_row,end_col)`. A point mark highlights nothing. |
| `virt_text` | List of chunks: `{{"text", "Group"}, ...}` or `{{text = "...", hl_group = "..."}, ...}`. The first chunk's group styles all of it. |
| `virt_text_pos` | `"overlay"` (draw over the text at `col`, same width) or `"inline"` (insert at `col`, shifting later text). Virtual text without one of these is not drawn. |
| `sign_text` | A gutter sign, shown in the breakpoint column. Only the first character is drawn, and a breakpoint takes priority. |
| `sign_hl` | Stored and returned by `get_mark`, but not drawn yet. |

`get_mark` returns `row`, `col`, `end_row`, `end_col`, plus whichever of `hl_group`,
`virt_text` (as `{text=, hl_group=}` chunks), `virt_text_pos`, `sign_text` and
`sign_hl` the mark has, so you can pass it back to `set_mark`. It returns `nil` for a
mark in another namespace, so a plugin only sees its own marks.

`virt_text_pos = "eol"` is accepted and stored, but end-of-line virtual text is not
drawn yet. Use `vimcode.buf.annotate_line` for end-of-line text.

```lua
vimcode.command("MarkWord", function(_)
    local ns = vimcode.decor.namespace("my-plugin")
    vimcode.decor.set_hl("MyWarn", { fg = "#1e1e1e", bg = "#e5c07b", bold = true })
    vimcode.decor.set_hl("MyNote", { link = "Comment" })
    local c = vimcode.window.get_cursor(0)
    vimcode.decor.set_mark(0, ns, {
        row = c.line - 1, col = c.col - 1, end_col = c.col + 4,
        hl_group = "MyWarn", sign_text = "!",
        virt_text = { { "<- here", "MyNote" } }, virt_text_pos = "inline",
    })
end)
```

### Syntax Tree, Undo Tree and Diagnostics

Read-only access to what the editor already tracks. All rows are 0-indexed.

```lua
-- Smallest tree-sitter node at (row, col); col is a 0-indexed *byte* offset
local node = vimcode.syntax.node_at(buf, row, col)
-- node = {type="identifier", language="rust",
--         range={start_row=, start_col=, end_row=, end_col=},
--         parent={type=, range=}}          -- parent is one level only, may be nil

-- Run a tree-sitter query; optional row range {start_row, end_row} (exclusive end)
local caps = vimcode.syntax.query(buf, "(function_item name: (identifier) @name)", {0, 100})
-- caps = {{name="name", type="identifier", range={...}}, ...}

-- Undo tree, oldest first
local nodes = vimcode.undo.tree(buf)
-- nodes = {{seq=, parent=, time=, current=}, ...}
--   parent: parent's seq (nil for the root); time: ms since the Unix epoch;
--   current: true for the state the buffer is in now
vimcode.undo.jump(buf, seq)   -- Restore the buffer to undo state `seq` -> bool

-- LSP diagnostics for a buffer (buf optional, defaults to the current one)
local diags = vimcode.diagnostics.get(buf)
-- diags = {{range={start_row=, start_col=, end_row=, end_col=},
--           severity="error"|"warning"|"information"|"hint",
--           message="...", source="rustc", code="E0308"}, ...}
```

- `node_at` and `query` raise an error when the buffer has no tree-sitter parser for
  its language, the position is out of range, or the query does not compile.
  Syntax ranges are byte columns with an exclusive end.
- `undo.tree` and `diagnostics.get` return an empty table for an invalid buffer. A
  diagnostic's `source` and `code` may be `nil`.
- `DiagnosticChanged` fires with the file path whenever a buffer's diagnostics are
  replaced.

### Editor Hover (`vimcode.editor.set_hover`)

```lua
vimcode.editor.set_hover(line, markdown)  -- line is 1-indexed
```

Attaches markdown to a buffer line. It appears in the same editor hover popup as LSP
hover and diagnostics: on a keyboard hover, or when the mouse rests past the end of
that line's text (over its annotation). The content is keyed by line number. Markdown
links can use `command:` URIs (next section). Inline git blame uses this mechanism.

### Command URIs

Hover popups and panel hovers support `command:` URIs in markdown links. When a user clicks (or presses Enter on) a command URI link, VimCode dispatches it to the matching plugin command registered via `vimcode.command()`.

**Format:** `[Label](command:CommandName?args)`

The `?args` portion is optional and is percent-decoded before being passed to the command handler. This enables interactive hover popups with clickable action links.

```lua
-- Register commands that can be invoked from hover popup links
vimcode.command("GitShow", function(hash)
    vimcode.command_run("Gshow " .. hash)
end)

vimcode.command("CopyHash", function(hash)
    vimcode.state.set_register("+", hash, false)
    vimcode.message("Copied " .. hash)
end)

-- Use command URIs in hover markdown
local md = "**Commit info**\n\n"
    .. "[Open Commit](command:GitShow?abc1234)"
    .. " | [Copy Hash](command:CopyHash?abc1234)"
vimcode.editor.set_hover(line, md)
```

Built-in LSP commands (`command:definition`, `command:type_definition`, `command:implementation`, `command:references`) are handled internally and take precedence over plugin commands of the same name.

### Panel API (`vimcode.panel.*`)

Extensions can register custom sidebar panels that appear in the activity bar. This is the same mechanism used by the git-insights extension's Git Log panel.

```lua
-- Register a custom sidebar panel (call at load time)
vimcode.panel.register("my_panel", {
    title = "My Panel",                  -- Sidebar header
    icon = "X",                          -- Single character for activity bar icon
    fallback_icon = "M",                 -- Used when Nerd Fonts are disabled
    sections = {"Section A", "Section B"}, -- Named collapsible sections
})

-- Populate a section with items (typically from a panel_focus hook)
vimcode.panel.set_items("my_panel", "Section A", {
    {text = "Item 1", id = "one", hint = "description", icon = "*"},
    {text = "Item 2", id = "two", hint = "extra info", style = "dim",
     badges = {{text = "main", color = "green"}},
     actions = {{label = "Open", key = "o"}}},
    {is_separator = true},
    {text = "Group", id = "grp", style = "header", expandable = true, expanded = false},
    {text = "Child", id = "c1", parent_id = "grp", indent = 1},
})

-- Hover markdown for an item, and the "?" help popup for the panel
vimcode.panel.set_hover("my_panel", "one", "**Item 1**\n\nDetails")
vimcode.panel.set_help("my_panel", { {"o", "Open item"}, {"d", "Delete item"} })

-- Parse event argument from panel_select/panel_action hooks
local info = vimcode.panel.parse_event(arg)
-- info = {panel="my_panel", section="Section A", id="one", key="o", index=0}

-- Programmatically navigate to a panel item (focus panel, select section, expand item)
vimcode.panel.reveal("my_panel", "Section A", "item_id")
-- Useful for blame-to-log navigation: reveal a commit in the Git Log panel

-- Panel input field
vimcode.panel.get_input("my_panel")         -- Current input text
vimcode.panel.set_input("my_panel", "text") -- Set input text
```

**Item fields:** `text`, `id`, `hint`, `icon`, `style`, `indent` (number),
`expandable`, `expanded`, `parent_id` (children are hidden while their parent is
collapsed), `actions` (`{{label=, key=}}`, drawn as clickable badges), `badges`
(`{{text=, color=}}`, colour as a name or `"#rrggbb"`), and `is_separator`.

**Panel navigation keys** (when panel has focus):

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate items |
| `Tab` | Expand/collapse section or tree node |
| `Enter` | Fire `panel_select` event for current item |
| `/` | Activate panel input field (search/filter) |
| `?` | Show the help popup (if `set_help` was called) |
| `q` / `Escape` | Unfocus panel (or deactivate input field) |
| Other keys | Fire `panel_action` event with the key |

**Panel input field:** Press `/` to activate an inline input field at the top of the panel. Typing fires `panel_input` events on every keystroke for live filtering. Press `Escape` to deactivate or `Return` to confirm and deactivate. Plugins can read/write the input text via `vimcode.panel.get_input(name)` and `vimcode.panel.set_input(name, text)`.

**Panel events:**

| Event | Argument | When |
|-------|----------|------|
| `panel_focus` | panel name | Panel gains focus in sidebar |
| `panel_select` | `"panel\|section\|id\|\|index"` | Enter pressed on item |
| `panel_action` | `"panel\|section\|id\|key\|index"` | Other key pressed on item |
| `panel_expand` | `"panel\|section\|id\|\|index"` | Tree node expanded via Tab |
| `panel_collapse` | `"panel\|section\|id\|\|index"` | Tree node collapsed via Tab |
| `panel_double_click` | `"panel\|section\|id\|\|index"` | Double-click on item |
| `panel_context_menu` | `"panel\|section\|id\|\|index"` | Right-click on item |
| `panel_input` | `"panel\|\|\|text\|"` | Input field text changed or confirmed |

**Item styles:**

| Style | Effect |
|-------|--------|
| `"normal"` | Default foreground color (also used for an unknown style) |
| `"header"` | Section-header styling |
| `"dim"` | Muted/grey text |
| `"accent"` | Accent colour |

### View API (`vimcode.ui.*`) — declarative widget panels (#146)

`vimcode.panel.*` above paints a **tree of rows**. `vimcode.ui.register_view`
paints a **form of widgets** (text fields, dropdowns, toggles, buttons) or a single
**list, tree, table or text view**, from a declarative tree your `render` callback
returns, and routes widget events back to your `on_event` callback. A view lives in the
sidebar, and `vimcode.ui.open_view` also opens it as an editor tab.

```lua
local method = 1   -- 0-based index into the options list below
local status = ""

vimcode.ui.register_view("my_view", {
    title = "My View",             -- sidebar header + activity-bar tooltip
    icon = "",                    -- single character for the activity bar
    fallback_icon = "M",           -- used when Nerd Fonts are disabled

    -- Called when the panel is focused and after every `on_event`.
    -- Return the widget tree to paint. `ctx.view` is the view's name.
    render = function(ctx)
        return {
            id = "main",
            schema_version = 1,
            fields = {
                { type = "label",  id = "hdr",    label = "Request" },
                { type = "text",   id = "url",    label = "URL",
                  value = "https://example.com", placeholder = "https://" },
                { type = "dropdown", id = "method", label = "Method",
                  options = { "GET", "POST" }, selected = method },
                { type = "toggle", id = "tls",    label = "Verify TLS",
                  value = true },
                { type = "button", id = "send",   label = "Send" },
                { type = "read_only", id = "st",  label = "Status",
                  value = status },
            },
        }
    end,

    -- Called with the widget the user activated. Plain data — no closures.
    on_event = function(ctx, event)
        if event.kind == "ButtonClicked" and event.widget_id == "send" then
            status = "sent"
        elseif event.kind == "DropdownChanged" and event.widget_id == "method" then
            method = event.value          -- 0-based index
        end
    end,
})

-- Ask for a re-render from anywhere (e.g. an async_shell callback):
vimcode.ui.refresh("my_view")

-- Open the view as an editor-area tab (from inside a callback):
vimcode.ui.open_view("my_view", { location = "tab" })
```

`register_view` must be called at **load time** (like `vimcode.command` and
`vimcode.keymap`). The `render` / `on_event` callbacks are stored and invoked
later, and may use the whole `vimcode.*` API — including the immediate
`vimcode.buffer.*` / `vimcode.window.*` API.

`open_view(name, opts)` is an immediate call. `"tab"` is the only `location` (and the
default). An unknown view name or any other location raises an error. The tab behaves
like any other tab: close it with `:tabclose`, split it or move it between groups.

**Field types:**

| `type` | Extra keys | Emits |
|--------|-----------|-------|
| `"label"` | — | nothing (section header) |
| `"read_only"` | `value` | nothing |
| `"text"` | `value`, `placeholder` | `TextChanged`, `TextCommitted` (`event.value` = string) |
| `"password"` | `value`, `placeholder` | `TextChanged`, `TextCommitted` |
| `"text_area"` | `value`, `placeholder`, `rows` | `TextChanged`, `TextCommitted` |
| `"toggle"` | `value` | `ToggleChanged` (`event.value` = bool) |
| `"button"` | — | `ButtonClicked` |
| `"dropdown"` | `options`, `selected` | `DropdownChanged` (`event.value` = 0-based index) |
| `"segmented"` | `options`, `selected` | `SegmentedChanged` (`event.value` = 0-based index) |
| `"buttons"` | `buttons = {{id=,label=,disabled=}}` | `ButtonClicked` per button id |
| `"toggles"` | `toggles = {{id=,label=,value=}}` | `ToggleChanged` per toggle id |

Every field also accepts `label`, `hint`, `disabled`, `error` and `warning`
(`error` / `warning` render an indicator plus the message instead of the hint).

**Text entry.** A focused `text`, `password` or `text_area` field takes typing,
cursor movement (`Left`/`Right`/`Home`/`End`), selection and clipboard (`Ctrl-A`,
`Ctrl-C`, `Ctrl-X`, `Ctrl-V`). vimcode owns the edit state. Your callback sees
`TextChanged` on every keystroke and `TextCommitted` when the edit is committed:

- `text` / `password`: `Enter` commits. Leaving the field without `Enter` discards
  the edit, and the next paint shows your declared `value` again.
- `text_area`: `Enter` inserts a newline. `Ctrl-Enter`, or leaving the field, commits.

**View bodies (#1631).** Instead of `fields`, `render` can return a single body with
`kind` set:

| `kind` | Keys | Events (`event.*`) |
|--------|------|--------------------|
| `"list"` | `title`, `items = {{id=, text=, detail=}}` (`id` required) | `ItemSelected`, `ItemActivated` (`index`, 0-based) |
| `"tree"` | `nodes = {{id=, label=, expanded=, children={...}}}` (`id` required) | `NodeSelected`, `NodeActivated`, `Expanded`, `Collapsed` (`node_id`) |
| `"table"` | `columns = {{title=, editable=}}` (`title` required), `rows = {{id=, cells={...}}}` (`cells` required) | `ItemSelected`, `ItemActivated` (`index`), `CellEdited` (`row`, `col`, both 0-based, and `value`) |
| `"text_view"` | `text`, `filetype` | none (scrollable, read-only text) |

"Activated" means `Enter` or a double-click. A view is either a field stack or one
body, never both.

```lua
vimcode.ui.register_view("my_list", {
    title = "Recent",
    render = function()
        return { kind = "list", items = {
            { id = "a", text = "first", detail = "1m ago" },
            { id = "b", text = "second" },
        } }
    end,
    on_event = function(_, event)
        if event.kind == "ItemActivated" then
            vimcode.message("row " .. (event.index + 1))
        end
    end,
})
```

**Your widget ids are yours.** Internally they are namespaced
(`plugin:<view>:<id>`) so two extensions can both call a button `"send"`, but
`event.widget_id` is always the id *you* wrote.

**Your state is yours.** Vimcode never edits the values you declared — it reports
what the activation implies (`ToggleChanged` carries the *new* value) and paints
whatever your next `render` returns. Selection and scroll position are vimcode's
and survive a re-render.

**Keys (when the view has focus):** `Tab`/`Shift-Tab` move between interactive
fields (labels, read-only and disabled rows are skipped) and always work, even from
inside a text field. Outside a text field, `j`/`k` also move, `Enter`/`Space` activate
the focused field, and `g`/`G` jump to the first/last field. In the sidebar,
`q`/`Escape` unfocus and `h`/`Left` return to the activity bar.

**`schema_version`** declares which vocabulary your tree uses. Omit it for the
current version (1). A view declaring a version newer than the running vimcode
understands is refused with a status-line error instead of being half-rendered.

### HTTP, JSON and Storage (`vimcode.http`, `vimcode.json`, `vimcode.storage`)

```lua
local req = vimcode.http.request({
    method = "POST",                 -- default "GET"
    url = "https://api.example.com/items",  -- required
    headers = { ["Content-Type"] = "application/json" },
    body = vimcode.json.encode({ name = "x" }),
    timeout_ms = 10000,              -- default 30000
}, function(resp)
    if resp.error then
        vimcode.message("failed: " .. resp.error)
    else
        -- resp.status (number), resp.headers (table), resp.body (string),
        -- resp.elapsed_ms (number)
        local data = vimcode.json.decode(resp.body)
    end
end)
req:cancel()   -- Kill the request; the callback is not called
```

- `http.request` is asynchronous and immediate (call it from a callback). The callback
  runs once on the main thread with either `{status, headers, body, elapsed_ms}` or
  `{error}`. An HTTP error status such as 404 is a normal response, not an `error`.
- Requests run through `curl`, which must be on `PATH`. Only `http` and `https` URLs
  are allowed. A header name or value containing CR or LF raises an error.
- A repeated response header keeps only its last value.

```lua
vimcode.json.encode(value, { pretty = true })  -- -> string (opts optional)
vimcode.json.decode(str)                       -- -> Lua value; raises on bad JSON
vimcode.json.null           -- Sentinel for JSON null inside a table
vimcode.json.empty_object   -- Sentinel that encodes as {}
```

- A table with keys `1..n` encodes as an array. Any other table encodes as an object.
  An empty table encodes as `[]`. Use `vimcode.json.empty_object` to get `{}`.
- `decode` returns `vimcode.json.null` for a JSON `null` and
  `vimcode.json.empty_object` for `{}`, so both round-trip. Compare with `==`.
- Functions, NaN and infinity cannot be encoded.

```lua
vimcode.storage.get(key, opts)          -- Stored value, or nil
vimcode.storage.set(key, value, opts)   -- Store a JSON-encodable value -> true
vimcode.storage.delete(key, opts)       -- -> true if the key existed
vimcode.storage.keys(opts)              -- Sorted list of keys
-- opts (optional): { workspace = true } stores per working directory
-- instead of globally
```

Storage is private to each plugin, persists across restarts and is written atomically.
Values go through the same conversion as `vimcode.json`. Storage works at load time,
so a plugin can restore its state at the top of its script:

```lua
local history = vimcode.storage.get("history") or {}
```

`contrib/extensions/rest-client/` is a complete extension built on `vimcode.ui`,
`vimcode.http`, `vimcode.json` and `vimcode.storage`.

### Comment Style Override

Override comment syntax for a language (useful for custom/niche languages):

```lua
vimcode.set_comment_style("haskell", {
    line = "--",
    block_open = "{-",
    block_close = "-}"
})
```

---

## Events Reference

Every hook receives one string argument. "Handle" means a buffer or window handle as a
decimal string; use `tonumber(arg)`.

| Event | Argument | When |
|-------|----------|------|
| `VimEnter` | `""` | Editor initialization complete |
| `open` | file path | File opened in editor |
| `BufNew` | file path | File opened in editor (same moment as `open`) |
| `BufEnter` | file path, or display name for an unnamed/scratch buffer | File opened, or a window switched buffers via `vimcode.window.set_buf` |
| `BufLeave` | buffer handle | A window switched away from this buffer via `vimcode.window.set_buf` (fires before `BufEnter`) |
| `FileType` | filetype (language id, e.g. `"rust"`) | File opened and its language is known |
| `BufWritePre` | file path (`""` for a buffer with no file) | Before a buffer is written, including writes taken over by `set_write_handler` |
| `save` | file path | After buffer is written to disk |
| `BufWrite` | file path | After buffer is written to disk (fires right after `save`) |
| `TextChanged` | buffer handle | A keystroke outside Insert/Replace mode changed the buffer |
| `TextChangedI` | buffer handle | A keystroke in Insert/Replace mode changed the buffer |
| `cursor_move` | `"line,col"` (1-indexed) | Cursor moved, Normal mode only (150 ms debounce) |
| `CursorMoved` | window handle | Cursor moved, outside Insert/Replace mode (150 ms debounce) |
| `CursorMovedI` | window handle | Cursor moved in Insert/Replace mode (150 ms debounce) |
| `WinEnter` | window handle | A window gained focus |
| `WinLeave` | window handle | A window lost focus (fires before `WinEnter`) |
| `InsertEnter` | new mode name (`"Insert"` or `"Replace"`) | Entered Insert or Replace mode |
| `InsertLeave` | old mode name (`"Insert"` or `"Replace"`) | Left Insert or Replace mode |
| `ModeChanged` | `"Old:New"` | Any mode change (e.g., `"Normal:Insert"`) |
| `ColorScheme` | scheme name | `:colorscheme` switched themes |
| `DiagnosticChanged` | file path | LSP diagnostics for a file were replaced |
| `User` | name passed to `vimcode.fire_event` | A plugin called `vimcode.fire_event(name)` |
| `git_branch_changed` | new branch name (or `""`) | External git branch change detected (rate-limited to once per 2s) |
| `panel_focus` | panel name | Extension panel gains focus |
| `panel_select` | `"panel\|section\|id\|\|index"` | Enter pressed on extension panel item |
| `panel_action` | `"panel\|section\|id\|key\|index"` | Other key pressed on extension panel item |
| `panel_expand` | `"panel\|section\|id\|\|index"` | Tree node expanded via Tab |
| `panel_collapse` | `"panel\|section\|id\|\|index"` | Tree node collapsed via Tab |
| `panel_double_click` | `"panel\|section\|id\|\|index"` | Double-click on panel item |
| `panel_context_menu` | `"panel\|section\|id\|\|index"` | Right-click on panel item |
| `panel_input` | `"panel\|\|\|text\|"` | Panel input field text changed |
| Custom | shell output | `async_shell()` callback event |

Mode names in `ModeChanged`, `InsertEnter` and `InsertLeave` are `Normal`, `Insert`,
`Replace`, `Command`, `Search`, `Visual`, `VisualLine` and `VisualBlock`.

An event triggered by your own callback — e.g. `BufEnter` from
`vimcode.window.set_buf` — is **deferred**: it runs as soon as your callback
returns and its queued effects have been applied, not in the middle of it. Your
hook for such an event therefore sees the finished state of the change that
caused it.

---

## Complete Example: Word Count Extension

```lua
-- ~/.config/vimcode/extensions/wordcount/wordcount.lua

-- Count words in the current buffer
vimcode.command("WordCount", function(_)
    local lines = vimcode.buf.lines()
    local count = 0
    for _, line in ipairs(lines) do
        for _ in line:gmatch("%S+") do
            count = count + 1
        end
    end
    vimcode.message("Word count: " .. count)
end)

-- Count words in selection (visual mode)
vimcode.command("WordCountSelection", function(args)
    -- args contains "start_line,end_line" when called from visual mode
    local s, e = args:match("(%d+),(%d+)")
    if not s then
        vimcode.message("No selection")
        return
    end
    local count = 0
    for i = tonumber(s), tonumber(e) do
        local line = vimcode.buf.line(i)
        if line then
            for _ in line:gmatch("%S+") do
                count = count + 1
            end
        end
    end
    vimcode.message("Selected word count: " .. count)
end)

-- Show word count on save
vimcode.on("save", function(_)
    local lines = vimcode.buf.lines()
    local count = 0
    for _, line in ipairs(lines) do
        for _ in line:gmatch("%S+") do
            count = count + 1
        end
    end
    vimcode.message("Saved (" .. count .. " words)")
end)
```

With manifest:

```toml
name = "wordcount"
display_name = "Word Count"
description = "Word counting commands and save-time word count display"
scripts = ["wordcount.lua"]
```

## Complete Example: Inline Git Blame

```lua
-- blame.lua — show inline blame annotations as you move the cursor

vimcode.on("cursor_move", function(pos)
    local line, _ = pos:match("(%d+),(%d+)")
    line = tonumber(line)
    if not line then return end

    local blame = vimcode.git.blame_line(line)
    if blame and not blame.not_committed then
        local text = blame.author .. " • " .. blame.relative_date .. " • " .. blame.message
        vimcode.buf.clear_annotations()
        vimcode.buf.annotate_line(line, text)
    else
        vimcode.buf.clear_annotations()
    end
end)
```

## Complete Example: Auto-Format on Save

```lua
-- autoformat.lua — run formatter on save for specific filetypes

local formatters = {
    rust = "rustfmt",
    python = "black -q -",
    javascript = "prettier --stdin-filepath %",
    go = "gofmt",
}

vimcode.on("save", function(path)
    local ft = vimcode.state.filetype()
    if formatters[ft] then
        -- Use built-in LSP format if available, otherwise shell out
        vimcode.command_run("Lformat")
    end
end)
```

`save` fires *after* the file is written, so this formats the buffer that was just
saved and leaves it modified until the next `:w`. LSP formatting is asynchronous, so
`BufWritePre` cannot make it land before the write either. For true format-on-save
with an LSP server, use the built-in `format_on_save` setting.

## Complete Example: TODO Highlighter

Highlights `TODO`/`FIXME` with a gutter sign, keeps the marks current as you edit,
and lists them in a picker (events + decor + keymap + picker):

```lua
-- todo.lua
local ns
local words = { "TODO", "FIXME" }

local function scan(buf)
    if not ns then  -- decor calls need a live editor, so set up lazily
        ns = vimcode.decor.namespace("todo")
        vimcode.decor.set_hl("TodoWord", { fg = "#1e1e1e", bg = "#e5c07b", bold = true })
    end
    vimcode.decor.clear(buf, ns)
    local hits = {}
    local lines = vimcode.buffer.get_lines(buf, 0, vimcode.buffer.line_count(buf))
    for i, line in ipairs(lines) do
        for _, word in ipairs(words) do
            local s = line:find(word, 1, true)
            if s then
                local col = utf8.len(line, 1, s - 1) or (s - 1) -- decor cols count characters
                vimcode.decor.set_mark(buf, ns, { row = i - 1, col = col,
                    end_col = col + #word, hl_group = "TodoWord", sign_text = "!" })
                hits[#hits + 1] = { display = i .. ": " .. line, data = i,
                    preview = { buffer = buf, line = i } }
            end
        end
    end
    return hits
end

vimcode.on("BufEnter", function() scan(vimcode.buffer.current()) end)
vimcode.on("TextChanged", function(buf) scan(tonumber(buf)) end)
vimcode.on("InsertLeave", function() scan(vimcode.buffer.current()) end)

vimcode.keymap.set("n", "<leader>td", function()
    vimcode.picker.open({
        title = "TODOs",
        items = scan(vimcode.buffer.current()),
        on_select = function(line) vimcode.window.set_cursor(0, { line = line, col = 1 }) end,
    })
end, { desc = "List TODOs" })
```

---

## Plugin Loading Details

### Load Order

1. User plugins from `~/.config/vimcode/plugins/` (alphabetical)
2. Extension scripts from `~/.config/vimcode/extensions/<name>/` (for installed extensions)

### Plugin Formats

- **Single file**: `~/.config/vimcode/plugins/my_plugin.lua`
- **Directory**: `~/.config/vimcode/plugins/my_plugin/init.lua`

### Disabling Plugins

```vim
:Plugin disable my_plugin
:Plugin enable my_plugin
:Plugin list
:Plugin reload
```

Or set in `~/.config/vimcode/settings.json`:

```json
{
    "disabled_plugins": ["my_plugin"]
}
```

### Extension Commands

```vim
:ExtInstall <name>     " Install an extension
:ExtRemove <name>      " Uninstall an extension
:ExtEnable <name>      " Enable a disabled extension
:ExtDisable <name>     " Disable without removing
:ExtList               " List all extensions and status
:ExtRefresh            " Refresh the extension registry
```

---

## Tips for AI-Assisted Development

When asking an AI to write a VimCode extension:

1. **Specify the manifest fields** — name, display_name, file_extensions, language_ids
2. **For LSP extensions** — provide the binary name and install command
3. **For script extensions** — describe the behavior, which events to hook, and which commands to register
4. **Line numbers and cursors are 1-indexed**, except `get_lines`/`set_lines` ranges and `vimcode.decor`/`syntax`/`diagnostics` rows, which are 0-indexed
5. **Never block the editor**: use `vimcode.loop.spawn()` (or `vimcode.async_shell()`) for processes and `vimcode.http.request()` for network I/O
6. **The cursor_move event** only fires in Normal mode; use `CursorMoved`/`CursorMovedI` for every mode
7. **Buffer modifications** via `vimcode.buf.*` (`set_line`/`insert_line`/`delete_line`/`set_lines`) are applied after the callback returns; `vimcode.buffer.*` applies them immediately
8. **Immediate APIs** (`vimcode.buffer`, `window`, `decor`, `picker`, `loop`, `http`, …) only work inside callbacks, not at script load time
9. **Test with** `:Plugin reload` to reload scripts without restarting the editor
