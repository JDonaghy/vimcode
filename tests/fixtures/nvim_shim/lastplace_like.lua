-- A minimal, self-contained reproduction of ethanholz/nvim-lastplace's core
-- structure (MIT-licensed; https://github.com/ethanholz/nvim-lastplace),
-- written for issue #1213's Phase-0 spike.
--
-- This is NOT a vendored copy of nvim-lastplace — during the spike's
-- investigation the real, unmodified plugin was fetched and loaded directly
-- against both a real Neovim and vimcode+contrib/nvim-shim (see the PR
-- report for that run's results). This fixture exists so the *automated*,
-- hermetic regression test (`tests/nvim_shim_spike.rs`) doesn't need network
-- access or a vendored third-party file to demonstrate the same finding: it
-- reproduces, line for line in spirit, nvim-lastplace's two load-bearing
-- patterns —
--
--   1. a `BufRead` autocmd whose callback registers a *second*,
--      buffer-scoped `BufWinEnter` autocmd (nvim-lastplace's `setup()`,
--      the `vim.fn.has("nvim-0.7")` branch), and
--   2. restoring the cursor from the `'"'` mark via `vim.fn.line([['"]])`
--      (nvim-lastplace's `set_cursor_position`).
--
-- It uses only real, public `vim.*` API surface — nothing here is
-- vimcode-specific — so this exact file runs unmodified against a real
-- Neovim (see the oracle driver in `tests/nvim_shim_spike.rs`) as well as
-- against vimcode with `contrib/nvim-shim/init.lua` loaded first.

local M = {}

local group = vim.api.nvim_create_augroup("SpikeLastplace", { clear = true })

vim.api.nvim_create_autocmd("BufRead", {
    group = group,
    callback = function(opts)
        -- nvim-lastplace's exact shape: register a second autocmd *inside*
        -- the first one's callback, scoped to the buffer that was just read.
        vim.api.nvim_create_autocmd("BufWinEnter", {
            group = group,
            buffer = opts.buf,
            callback = function()
                M.restore_cursor(opts.buf)
            end,
        })
    end,
})

function M.restore_cursor(buf)
    if vim.tbl_contains({ "quickfix", "nofile", "help" }, vim.api.nvim_buf_get_option(buf, "buftype")) then
        return
    end
    local last_line = vim.fn.line([['"]])
    local buf_last_line = vim.fn.line("$")
    if last_line > 0 and last_line <= buf_last_line then
        vim.api.nvim_win_set_cursor(0, { last_line, 0 })
    end
end

_G.SpikeLastplace = M
return M
