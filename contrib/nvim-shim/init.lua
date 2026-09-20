--! Pure-Lua `vim.*` prelude over `vimcode.*` — go/no-go spike for issue #1213
--! (Phase 0 of #1212, "nvim-shim: run real Neovim plugins on vimcode").
--
-- STATUS: SPIKE / DISPOSABLE. This is not a shipped feature and is not part
-- of the `vimcode.*` API surface (per #1213's constraints). It exists to be
-- loaded as an ordinary vimcode plugin (see `src/core/engine/plugins.rs`'s
-- loader, which scans `~/.config/vimcode/plugins/` for `.lua` files / `init.lua`
-- dirs and simply `exec()`s them into one shared, persistent `mlua::Lua` VM —
-- there is no dedicated "prelude" load slot, so this file just has to sort
-- before the plugin(s) that need it, e.g. `00-nvim-shim.lua`) and measure how
-- far a real, unmodified Neovim plugin gets before something in `vimcode.*`
-- breaks it. See the PR description for the full go/no-go report; every
-- deliberate inaccuracy below is tagged `-- LIE:` and is also listed there.
--
-- Location: `contrib/` is NOT on any vimcode plugin load path (the only load
-- path is `~/.config/vimcode/plugins/` — see `plugin_init` in
-- `src/core/engine/plugins.rs`). This file is loaded directly by the spike's
-- test harness (`tests/nvim_shim_spike.rs`) via `PluginManager::load_plugins_dir`
-- pointed at a temp copy, exactly like a real install would. Nothing under
-- `src/` changed to accommodate it (constraint: zero Rust changes).
--
-- ─── What `vimcode.*` actually exposes (see src/core/plugin.rs) ────────────
--
-- Every Lua callback (`vimcode.on`, `vimcode.command`, `vimcode.keymap`) runs
-- against a `PluginCallContext` snapshot of "the" active buffer/cursor/mode —
-- there is no buffer *handle* concept, no window concept, and no viewport
-- (topline/botline) at all. Every `vim.api.nvim_buf_*`/`nvim_win_*` call in
-- this file therefore silently operates on "whatever vimcode currently calls
-- the active buffer", which is usually but not always what a real Neovim
-- `bufnr` argument would have meant.
--
-- Registration functions (`vimcode.on`/`vimcode.command`/`vimcode.keymap`)
-- only work while the plugin *file* is being loaded — they write into a
-- `PluginRegistrations` accumulator that is installed in Lua `app_data` only
-- for the duration of `PluginManager::load_one_plugin`, then removed
-- (`src/core/plugin.rs:358,363`). Calling any of them again later, e.g. from
-- inside an event-hook callback (the exact pattern `nvim-lastplace` uses —
-- register a `BufRead` autocmd whose callback registers a second,
-- buffer-scoped `BufWinEnter` autocmd), finds no `PluginRegistrations` in
-- `app_data`, so the registration call below is a **silent no-op**: no Lua
-- error, no vimcode error, the callback just never fires. This is the
-- spike's headline finding — see the PR report.

local vim = _G.vim or {}
_G.vim = vim
vim.api = vim.api or {}
vim.fn = vim.fn or {}
vim.g = vim.g or {}
vim.keymap = vim.keymap or {}

-- ─── 5.4 compat shims (#1212 §5: vimcode's mlua is real Lua 5.4; Neovim's
-- runtime is LuaJIT/5.1). Plugins that assume LuaJIT-isms need these to even
-- parse/run. ──────────────────────────────────────────────────────────────
_G.unpack = _G.unpack or table.unpack
_G.loadstring = _G.loadstring or load
_G.bit = _G.bit
    or {
        band = function(a, b)
            return a & b
        end,
        bor = function(a, b)
            return a | b
        end,
        bxor = function(a, b)
            return a ~ b
        end,
        bnot = function(a)
            return ~a
        end,
        lshift = function(a, b)
            return a << b
        end,
        rshift = function(a, b)
            return a >> b
        end,
        tobit = function(a)
            return a | 0
        end,
    }
-- LIE: there is no JIT here at all (this is real Lua 5.4, not LuaJIT), and
-- `os`/`arch`/`version` are made up strings, not probed. Any plugin that
-- branches on `jit.os == "Windows"` etc. gets a fixed, possibly wrong answer.
_G.jit = _G.jit
    or {
        os = "vimcode-shim",
        arch = "shim",
        version = "Lua 5.4 (vimcode nvim-shim, not LuaJIT)",
    }

-- ─── package.path ───────────────────────────────────────────────────────
-- LIE / KNOWN GAP: real Neovim's `package.path` is seeded from `runtimepath`
-- so `require('some-plugin')` finds `lua/some-plugin/init.lua` under any
-- installed plugin's directory. vimcode has no runtimepath concept and the
-- Lua sandbox mlua builds (`StdLib::ALL_SAFE`) does *not* include the `debug`
-- library, so a loaded chunk cannot even ask "what file am I?" — there is no
-- `debug.getinfo(1, "S").source` to seed a script-relative path from. The
-- best this prelude can do is extend `package.path` with the process's
-- current working directory and hope a plugin's `require()` targets happen
-- to resolve from there (true for `cargo test`, not guaranteed for a real
-- vimcode session launched from an arbitrary cwd). A real Phase 1 fix needs
-- either a `vimcode.plugin_dir()` Rust-side API or the loader passing the
-- plugin's own directory in as an upvalue/global before `exec()`.
package.path = package.path .. ";./?.lua;./?/init.lua"

-- ─── vim.o / vim.bo over vimcode.opt (name-mismatch map) ────────────────
--
-- `vimcode.opt.get/set` key on vimcode's *internal* setting names
-- (`src/core/settings.rs`), not Neovim's option names — e.g. vimcode uses
-- `shift_width` where Neovim uses `shiftwidth`. Only the keys
-- `Engine::settings_snapshot` whitelists (`src/core/engine/plugins.rs`) are
-- reachable at all; anything else silently reads back `""` — indistinguishable
-- from "option exists and is empty" from Lua's side. LIE: unmapped/unlisted
-- options return `nil` from this shim rather than erroring, which is what
-- Neovim would do for a genuinely unknown option, but here it also covers
-- perfectly real options vimcode just doesn't expose yet (e.g. `signcolumn`,
-- `list`, `foldmethod`).
local OPT_NAME_MAP = {
    shiftwidth = "shift_width",
    sw = "shift_width",
    expandtab = "expand_tab",
    et = "expand_tab",
    autoindent = "auto_indent",
    ai = "auto_indent",
    number = "line_numbers",
    nu = "line_numbers",
    hlsearch = "hlsearch",
    hls = "hlsearch",
    ignorecase = "ignorecase",
    ic = "ignorecase",
    smartcase = "smartcase",
    scs = "smartcase",
    incsearch = "incremental_search",
    scrolloff = "scrolloff",
    so = "scrolloff",
    textwidth = "textwidth",
    tw = "textwidth",
    colorcolumn = "colorcolumn",
    cc = "colorcolumn",
    wrap = "wrap",
    swapfile = "swapfile",
    swf = "swapfile",
    updatetime = "updatetime",
    ut = "updatetime",
    splitbelow = "splitbelow",
    sb = "splitbelow",
    splitright = "splitright",
    spr = "splitright",
    tabstop = "tabstop",
    ts = "tabstop",
}

local function opt_coerce(raw)
    if raw == "" then
        return nil
    end
    if raw == "true" then
        return true
    end
    if raw == "false" then
        return false
    end
    local n = tonumber(raw)
    if n then
        return n
    end
    return raw
end

local function opt_get(key)
    return opt_coerce(vimcode.opt.get(OPT_NAME_MAP[key] or key))
end

local function opt_set(key, value)
    vimcode.opt.set(OPT_NAME_MAP[key] or key, tostring(value))
end

vim.o = setmetatable({}, {
    __index = function(_, k)
        return opt_get(k)
    end,
    __newindex = function(_, k, v)
        opt_set(k, v)
    end,
})

-- LIE: vimcode has no per-buffer option storage distinct from global
-- settings, and no `buftype` concept at all (`PluginCallContext` has no such
-- field). `vim.bo.filetype`/`vim.bo.ft` map onto the one real read-only field
-- vimcode does expose (`vimcode.state.filetype()`); `vim.bo.buftype`/`bt`
-- always reads back `""` (i.e. "normal buffer"), which is wrong for
-- quickfix/help/nofile buffers a real plugin would want to skip.
vim.bo = setmetatable({}, {
    __index = function(_, k)
        if k == "filetype" or k == "ft" then
            return vimcode.state.filetype()
        end
        if k == "buftype" or k == "bt" then
            return "" -- LIE: always "normal", see comment above
        end
        return opt_get(k)
    end,
    __newindex = function(_, k, v)
        if k == "filetype" or k == "ft" or k == "buftype" or k == "bt" then
            return -- LIE: silent no-op, no setter path exists
        end
        opt_set(k, v)
    end,
})

-- ─── vim.api ─────────────────────────────────────────────────────────────

vim.api.nvim_buf_get_lines = function(_buf, start, stop, _strict)
    local t = vimcode.buf.get_lines(start, stop)
    local out = {}
    for i = 1, (t.n or #t) do
        out[i] = t[i]
    end
    return out
end

vim.api.nvim_buf_set_lines = function(_buf, start, stop, _strict, lines)
    vimcode.buf.set_lines(start, stop, lines)
end

vim.api.nvim_buf_line_count = function(_buf)
    return vimcode.buf.line_count()
end

vim.api.nvim_win_get_cursor = function(_win)
    local c = vimcode.buf.cursor()
    -- Neovim cursor col is 0-indexed byte offset; vimcode's is 1-indexed.
    return { c.line, math.max(0, c.col - 1) }
end

vim.api.nvim_win_set_cursor = function(_win, pos)
    vimcode.buf.set_cursor(pos[1], pos[2] + 1)
end

-- LIE: no buffer-handle concept — "the current buffer" is the only buffer
-- vimcode's plugin ABI can address, so this always returns 0.
vim.api.nvim_get_current_buf = function()
    return 0
end

-- LIE: no augroup concept. Returns a fake id; grouping/clearing is a no-op.
local NEXT_FAKE_ID = 1
vim.api.nvim_create_augroup = function(_name, _opts)
    NEXT_FAKE_ID = NEXT_FAKE_ID + 1
    return NEXT_FAKE_ID
end

-- Map from Neovim autocmd event names onto the handful of string events
-- `Engine::plugin_event` actually fires (see `src/core/engine/plugins.rs`,
-- `lsp_ops.rs:42-44`). Anything not in this table is registered under its
-- raw Neovim name — vimcode will simply never fire it, so the callback is
-- dead code, not an error. That is itself a LIE by omission: Neovim has ~100
-- autocmd events; vimcode's plugin system has about a dozen string hooks
-- total, so most of a real plugin's autocmd surface silently never runs.
local AUTOCMD_EVENT_MAP = {
    BufRead = "open",
    BufReadPost = "open",
    BufNewFile = "BufNew",
    BufEnter = "BufEnter",
    BufWritePost = "save",
    BufWrite = "save",
    VimEnter = "VimEnter",
    InsertEnter = "InsertEnter",
    InsertLeave = "InsertLeave",
}

-- LIE: returns a fake autocmd id; `nvim_del_autocmd`/group-clear are not
-- implemented at all (not needed by anything this spike drove far enough to
-- reach). See the module doc comment above for the load-time-only
-- registration limitation this function is built on top of.
vim.api.nvim_create_autocmd = function(event, opts)
    opts = opts or {}
    local events = type(event) == "table" and event or { event }
    for _, ev in ipairs(events) do
        local mapped = AUTOCMD_EVENT_MAP[ev] or ev
        vimcode.on(mapped, function(arg)
            -- LIE: "buffer = N" scoping is approximated as "always current
            -- buffer" (N ~= 0 is rejected outright) since there is no buffer
            -- handle to compare against.
            if opts.buffer and opts.buffer ~= 0 then
                return
            end
            if opts.callback then
                opts.callback({ buf = 0, file = arg, match = arg, event = ev })
            end
        end)
    end
    NEXT_FAKE_ID = NEXT_FAKE_ID + 1
    return NEXT_FAKE_ID
end

vim.api.nvim_create_user_command = function(name, fn, _opts)
    vimcode.command(name, function(argstr)
        argstr = argstr or ""
        local fargs = {}
        for w in argstr:gmatch("%S+") do
            table.insert(fargs, w)
        end
        fn({ args = argstr, fargs = fargs, name = name, bang = false })
    end)
end

vim.api.nvim_buf_get_option = function(buf, name)
    if name == "filetype" then
        return vimcode.state.filetype()
    end
    if name == "buftype" then
        return "" -- LIE: see vim.bo.buftype above
    end
    return opt_get(name)
end

-- vim.cmd / nvim_command
--
-- LIE (significant): `vimcode.command_run` queues the ex command onto
-- `ctx.run_commands`, applied by `Engine::apply_plugin_ctx` *after* the
-- whole Lua callback returns (`src/core/engine/plugins.rs`) — it is not
-- executed synchronously the way Neovim's `vim.cmd`/`nvim_command` is. Any
-- plugin that issues a command and then immediately reads buffer/cursor
-- state back in the same callback (a very common pattern — this is the same
-- "write now, read stale" shape as the kill test) will observe pre-command
-- state. `nvim-lastplace`'s final `normal! g\`"` happens to be safe because
-- nothing reads state afterward in the same callback, but that is luck, not
-- something this shim guarantees for plugins in general.
vim.cmd = function(command)
    if type(command) == "table" then
        local parts = { command.cmd }
        for _, a in ipairs(command.args or {}) do
            table.insert(parts, a)
        end
        command = table.concat(parts, " ")
    end
    vimcode.command_run(command)
end
vim.api.nvim_command = vim.cmd

-- ─── vim.fn ─────────────────────────────────────────────────────────────

vim.fn.line = function(expr)
    if expr == "." then
        return vimcode.buf.cursor().line
    elseif expr == "$" then
        return vimcode.buf.line_count()
    elseif expr == "w0" then
        -- LIE: no viewport (topline) in PluginCallContext at all; approximate
        -- with the whole buffer's first line.
        return 1
    elseif expr == "w$" then
        -- LIE: no viewport (botline) either; approximate with the last line.
        return vimcode.buf.line_count()
    else
        local mark = expr:match("^'(.)$")
        if mark then
            local m = vimcode.state.mark(mark)
            return m and m.line or 0
        end
    end
    return 0
end

vim.fn.col = function(expr)
    if expr == "." then
        return vimcode.buf.cursor().col
    elseif expr == "$" then
        local line = vimcode.buf.line(vim.fn.line(".")) or ""
        return #line + 1
    end
    return 0
end

vim.fn.getline = function(n)
    if n == "." then
        n = vim.fn.line(".")
    end
    return vimcode.buf.line(n) or ""
end

vim.fn.setline = function(n, text)
    if n == "." then
        n = vim.fn.line(".")
    end
    vimcode.buf.set_line(n, text)
    return 0
end

vim.fn.expand = function(expr)
    local path = vimcode.buf.path() or ""
    if expr == "%" then
        return path
    elseif expr == "%:t" then
        return path:match("([^/]+)$") or path
    elseif expr == "%:h" then
        return path:match("(.*)/") or "."
    elseif expr == "%:p" then
        -- LIE: not guaranteed absolute; vimcode's buf_path is whatever the
        -- buffer was opened with.
        return path
    end
    return "" -- LIE: <cword>/<afile>/etc. unimplemented, silently empty
end

vim.fn.getcwd = function()
    return vimcode.cwd()
end

vim.fn.mode = function()
    local m = vimcode.state.mode()
    local map = {
        Normal = "n",
        Insert = "i",
        Visual = "v",
        VisualLine = "V",
        VisualBlock = "\22",
        Replace = "R",
    }
    return map[m] or "n"
end

-- LIE: advertises modern-Neovim autocmd-API feature flags unconditionally so
-- plugins take their `nvim_create_autocmd` code path instead of a legacy
-- `vim.cmd([[augroup ...]])` string-command path — deliberately, since the
-- former is the one this shim can actually intercept. vimcode is not
-- Neovim 0.7+ in any other sense (`has("nvim")` is also true, which is a lie
-- in its own right: this is not Neovim at all).
vim.fn.has = function(feature)
    local known = {
        ["nvim"] = 1,
        ["nvim-0.5"] = 1,
        ["nvim-0.5.1"] = 1,
        ["nvim-0.7"] = 1,
    }
    return known[feature] or 0
end

-- LIE: no fold model is exposed via `vimcode.*` at all; always "no fold".
vim.fn.foldclosed = function(_expr)
    return -1
end

-- ─── vim.tbl_* / vim.split / vim.notify / vim.inspect ────────────────────
-- Reimplemented in pure Lua (not sourced from vimcode.* — these are generic
-- table/string helpers Neovim ships and plugins assume exist).

function vim.tbl_contains(t, value)
    for _, v in pairs(t) do
        if v == value then
            return true
        end
    end
    return false
end

function vim.tbl_keys(t)
    local keys = {}
    for k in pairs(t) do
        table.insert(keys, k)
    end
    return keys
end

function vim.tbl_values(t)
    local values = {}
    for _, v in pairs(t) do
        table.insert(values, v)
    end
    return values
end

function vim.tbl_isempty(t)
    return next(t) == nil
end

function vim.tbl_deep_extend(behavior, ...)
    local result = {}
    for _, t in ipairs({ ... }) do
        for k, v in pairs(t) do
            if type(v) == "table" and type(result[k]) == "table" then
                result[k] = vim.tbl_deep_extend(behavior, result[k], v)
            elseif not (behavior == "keep" and result[k] ~= nil) then
                result[k] = v
            end
        end
    end
    return result
end

function vim.split(s, sep, _opts)
    local parts = {}
    if sep == nil or sep == " " or sep == "%s" then
        for w in s:gmatch("%S+") do
            table.insert(parts, w)
        end
    else
        local pattern = "([^" .. sep .. "]+)"
        for w in s:gmatch(pattern) do
            table.insert(parts, w)
        end
    end
    return parts
end

-- LIE: `vimcode.message` sets one status-line string; there is no severity
-- concept, no notification history, and nothing distinguishes ERROR from
-- INFO. `level` is accepted and ignored.
function vim.notify(msg, _level)
    vimcode.message(tostring(msg))
end

-- A minimal pretty-printer, NOT Neovim's real `vim.inspect` (no cycle
-- detection beyond a flat "seen" set, no configurable indent/depth, no
-- metatable-aware formatting). Good enough for `print(vim.inspect(x))`
-- debugging inside a plugin; not a drop-in replacement.
function vim.inspect(v, opts)
    opts = opts or {}
    local seen = {}
    local function ser(x, indent)
        if type(x) == "table" then
            if seen[x] then
                return "<cycle>"
            end
            seen[x] = true
            local parts = {}
            for k, val in pairs(x) do
                table.insert(parts, indent .. "  [" .. tostring(k) .. "] = " .. ser(val, indent .. "  "))
            end
            if #parts == 0 then
                return "{}"
            end
            return "{\n" .. table.concat(parts, ",\n") .. "\n" .. indent .. "}"
        elseif type(x) == "string" then
            return string.format("%q", x)
        else
            return tostring(x)
        end
    end
    return ser(v, "")
end

-- ─── vim.keymap ───────────────────────────────────────────────────────────

vim.keymap.set = function(mode, lhs, rhs, _opts)
    local modes = type(mode) == "table" and mode or { mode }
    for _, m in ipairs(modes) do
        vimcode.keymap(m, lhs, function()
            if type(rhs) == "function" then
                rhs()
            else
                vimcode.feedkeys(rhs)
            end
        end)
    end
end

return vim
