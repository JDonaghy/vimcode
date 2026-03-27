# src/core/engine/execute.rs — 2,962 lines

Ex-command dispatcher. Parses and executes all `:` commands entered in command mode.

## Key Methods
- `execute_command(cmd)` — main dispatcher; giant match over ~100+ command names
- Handles: `:w`, `:q`, `:e`, `:sp`, `:vs`, `:bn`, `:bp`, `:bd`, `:tabnew`, `:tabclose`, `:set`, `:colorscheme`, `:norm`, `:grep`, `:vimgrep`, `:copen`, `:cn`, `:cp`, `:Gdiff`, `:Gblame`, `:Gstatus`, `:Gpush`, `:Gpull`, `:Gfetch`, `:term`, `:LspInfo`, `:LspRestart`, `:LspInstall`, `:DapInstall`, `:Plugin`, `:Settings`, `:Keymaps`, `:AI`, `:ExtRemove`, `:ExtRefresh`, `:map`/`:nmap`/`:imap`/`:vmap`, `:retab`, `:saveas`, `:windo`/`:bufdo`/`:tabdo`, `:fold`, `:Rename`, `:Lformat`, `:CodeAction`, `:hover`, `:DiffPeek`, `:Explore`, etc.
- Range parsing: `%`, `'<,'>`, `N,M`, `.`, `$`, relative `+N/-N`
- Falls through to plugin command dispatch if no built-in match
