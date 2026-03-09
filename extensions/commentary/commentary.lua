-- VimCode Commentary
-- Toggle line comments, inspired by tpope's vim-commentary.
-- https://github.com/tpope/vim-commentary

-- Comment strings by filetype / language ID.
local comment_strings = {
    rust       = "//",
    go         = "//",
    c          = "//",
    cpp        = "//",
    csharp     = "//",
    java       = "//",
    javascript = "//",
    typescript = "//",
    typescriptreact = "//",
    javascriptreact = "//",
    php        = "//",
    swift      = "//",
    kotlin     = "//",
    scala      = "//",
    dart       = "//",
    python     = "#",
    ruby       = "#",
    shellscript = "#",
    bash       = "#",
    yaml       = "#",
    toml       = "#",
    dockerfile = "#",
    perl       = "#",
    r          = "#",
    lua        = "--",
    sql        = "--",
    haskell    = "--",
    elm        = "--",
    html       = "<!--",
    xml        = "<!--",
    css        = "/*",
    jsonc      = "//",
    tex        = "%",
    latex      = "%",
    erlang     = "%",
    lisp       = ";",
    scheme     = ";",
    clojure    = ";",
    vim        = '"',
}

-- Multi-char comment closing strings (for block-style comments).
local comment_end = {
    html = " -->",
    xml  = " -->",
    css  = " */",
}

--- Get the comment prefix and suffix for the current filetype.
local function get_comment_style()
    local ft = vimcode.state.filetype()
    local cs = comment_strings[ft] or "#"
    local ce = comment_end[ft] or ""
    return cs, ce
end

--- Check whether a single line (string) is commented with the given prefix.
local function is_line_commented(text, cs)
    local stripped = text:match("^%s*(.*)")
    if stripped == "" then
        return true -- treat blank lines as "commented" (skip them)
    end
    if stripped:sub(1, #cs + 1) == cs .. " " then
        return true
    end
    if stripped:sub(1, #cs) == cs then
        return true
    end
    return false
end

--- Toggle comment on lines [start_line, end_line] (1-indexed).
local function toggle_comment_range(start_line, end_line)
    local cs, ce = get_comment_style()
    local count = vimcode.buf.line_count()
    if start_line < 1 then start_line = 1 end
    if end_line > count then end_line = count end
    if start_line > end_line then return end

    -- First pass: determine whether we are commenting or uncommenting.
    -- If ALL non-blank lines are already commented, we uncomment.
    local all_commented = true
    local has_content = false
    for i = start_line, end_line do
        local line = vimcode.buf.line(i)
        if line then
            local stripped = line:match("^%s*(.*)")
            if stripped ~= "" then
                has_content = true
                if not is_line_commented(line, cs) then
                    all_commented = false
                    break
                end
            end
        end
    end

    if not has_content then return end -- all blank — nothing to do

    -- Second pass: apply.
    for i = start_line, end_line do
        local line = vimcode.buf.line(i)
        if line then
            local indent, rest = line:match("^(%s*)(.*)")
            if rest == "" then
                -- Skip blank/whitespace-only lines.
            elseif all_commented then
                -- Uncomment: strip comment prefix (and optional trailing suffix).
                if ce ~= "" and rest:sub(-#ce) == ce then
                    rest = rest:sub(1, -#ce - 1)
                end
                if rest:sub(1, #cs + 1) == cs .. " " then
                    rest = rest:sub(#cs + 2)
                elseif rest:sub(1, #cs) == cs then
                    rest = rest:sub(#cs + 1)
                end
                vimcode.buf.set_line(i, indent .. rest)
            else
                -- Comment: add prefix (and optional suffix).
                vimcode.buf.set_line(i, indent .. cs .. " " .. rest .. ce)
            end
        end
    end
end

-- ── :Commentary command ─────────────────────────────────────────────────────

vimcode.command("Commentary", function(args)
    local cursor = vimcode.buf.cursor()
    local n = tonumber(args) or 1
    toggle_comment_range(cursor.line, cursor.line + n - 1)
end)
