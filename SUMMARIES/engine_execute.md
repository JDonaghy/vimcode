# src/core/engine/execute.rs — 4,536 lines

Ex-command dispatcher. Parses and executes all `:` commands entered in command mode.

## Key Methods
- `execute_command(cmd)` — main dispatcher; giant match over ~100+ command names
- Handles: `:w`, `:q`, `:e`, `:sp`, `:vs`, `:bn`, `:bp`, `:bd`, `:tabnew`, `:tabclose`, `:set`, `:colorscheme`, `:norm`, `:grep`, `:vimgrep`, `:copen`, `:cn`, `:cp`, `:Gdiff`, `:Gblame`, `:Gstatus`, `:Gpush`, `:Gpull`, `:Gfetch`, `:Gbranches`, `:term`, `:LspInfo`, `:LspRestart`, `:LspInstall`, `:DapInstall`, `:Plugin`, `:Settings`, `:Keymaps`, `:AI`, `:ExtRemove`, `:ExtRefresh`, `:map`/`:nmap`/`:imap`/`:vmap`, `:retab`, `:saveas`, `:windo`/`:bufdo`/`:tabdo`, `:fold`, `:Rename`, `:Lformat`, `:CodeAction`, `:hover`, `:DiffPeek`, `:Explore`, etc.
- `handle_status_action(action) -> Option<EngineAction>` — handles clickable status bar segment actions; returns `Some(EngineAction::ToggleSidebar)` for sidebar toggle (backend must dispatch), handles panel/menu toggle directly
- `parse_ex_address(chars, i, current) -> Option<isize>` / `parse_ex_range(chars) -> (Option<(isize,isize)>, usize)` — the full ex address grammar (`N . $ % 'm /pat/ ?pat? \\/ +N -N a,b a;b`); `-1` is Vim's line 0
- `try_execute_ranged_command(cmd)` — `:[range]` for `:d`, `:y`, `:j`, `:>`, `:<`, `:t`/`:co`, `:m`, and a bare range that moves the cursor
- `try_execute_substitute(cmd)` / `run_substitute(range, pat, repl, flags)` — `:s`, `:&`, `:&&`, `:~`; any delimiter, all flags, counts, `|` chaining
- `try_execute_global(cmd)` / `execute_global_command(range, after, delim, invert)` — `:g` / `:v`
- `try_execute_norm(cmd)` / `execute_norm_range(start, end, keys)` — `:[range]normal`
- `compile_vim_pattern(pattern, smartcase_applies)` / `collect_match_spans(re, text)` — the shared search-pattern entry points (see `src/core/vim_regex.rs`)
- `run_search()`, `search_next()`, `search_prev()`, `submit_search(raw, count)` — `/` and `?`, with `:h search-offset` support
- `splice_buffer_text(new_text)` — replace the buffer's text as one undo step, touching only the differing region
- `ex_copy_move(start, end, dest, is_move)` — shared `:t` / `:co` / `:m`
- Falls through to plugin command dispatch if no built-in match
