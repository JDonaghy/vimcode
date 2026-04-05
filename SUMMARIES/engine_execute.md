# src/core/engine/execute.rs — 3,008 lines

Ex-command dispatcher. Parses and executes all `:` commands entered in command mode.

## Key Methods
- `execute_command(cmd)` — main dispatcher; giant match over ~100+ command names
- Handles: `:w`, `:q`, `:e`, `:sp`, `:vs`, `:bn`, `:bp`, `:bd`, `:tabnew`, `:tabclose`, `:set`, `:colorscheme`, `:norm`, `:grep`, `:vimgrep`, `:copen`, `:cn`, `:cp`, `:Gdiff`, `:Gblame`, `:Gstatus`, `:Gpush`, `:Gpull`, `:Gfetch`, `:Gbranches`, `:term`, `:LspInfo`, `:LspRestart`, `:LspInstall`, `:DapInstall`, `:Plugin`, `:Settings`, `:Keymaps`, `:AI`, `:ExtRemove`, `:ExtRefresh`, `:map`/`:nmap`/`:imap`/`:vmap`, `:retab`, `:saveas`, `:windo`/`:bufdo`/`:tabdo`, `:fold`, `:Rename`, `:Lformat`, `:CodeAction`, `:hover`, `:DiffPeek`, `:Explore`, etc.
- `handle_status_action(action) -> Option<EngineAction>` — handles clickable status bar segment actions; returns `Some(EngineAction::ToggleSidebar)` for sidebar toggle (backend must dispatch), handles panel/menu toggle directly
- Range parsing: `%`, `'<,'>`, `N,M`, `.`, `$`, relative `+N/-N`
- Falls through to plugin command dispatch if no built-in match
