-- REST Client (#147) — a Postman-like HTTP request builder bundled as a
-- vimcode extension.
--
-- Entirely `vimcode.*` Lua: no extension-specific Rust. Six
-- `vimcode.ui.register_view` panels, each opened as an editor-area tab via
-- its own `:RestClient*` command (or a navigation button inside the Request
-- panel):
--
--   rest_client_request      — URL, method, body, headers/params table
--                               buttons, Send, Status, navigation.
--   rest_client_headers      -- editable key/value table (Table body).
--   rest_client_params       -- editable key/value query-param table.
--   rest_client_response     -- pretty-printed response body (TextView body).
--   rest_client_history      -- past requests, newest first (List body).
--   rest_client_collections  -- saved requests (Tree body).
--   rest_client_env          -- `{{name}}` substitution variables.
--
-- Headers/params (Table-kind) and the response viewer (TextView-kind) are
-- deliberately separate registered views rather than widgets glued onto the
-- Request form — a `vimcode.ui.register_view` view is either a field stack
-- or a single list/tree/table/text_view body, never both at once
-- (`src/core/plugin_ui.rs`'s `PluginView` doc spells this out explicitly for
-- #147). The Request view is the hub: it carries only field-stack rows
-- (text/dropdown/button/read_only), and buttons on it jump to the
-- table/tree/list/text_view panels that carry everything else.
--
-- Persistence (`vimcode.storage`, global scope — not `{workspace = true}`,
-- so history/environment survive regardless of which directory vimcode was
-- started in) covers `history`, `collections` and `env_vars`. Every one of
-- them is reloaded at the top of this file, so a fresh load (a real
-- `:Plugin reload`, or vimcode restarting) picks up whatever the previous
-- load last wrote.

local method_options = { "GET", "POST", "PUT", "DELETE", "PATCH" }

local function method_index(m)
    for i, opt in ipairs(method_options) do
        if opt == m then
            return i - 1
        end
    end
    return 0
end

-- ── Persisted state ─────────────────────────────────────────────────────

local function load_stored(key, default)
    local ok, value = pcall(vimcode.storage.get, key)
    if ok and value ~= nil then
        return value
    end
    return default
end

local history = load_stored("history", {})
local collections = load_stored("collections", {})
local env_vars = load_stored("env_vars", {})

local function persist(key, value)
    pcall(vimcode.storage.set, key, value)
end

-- ── In-memory (not persisted) request-builder state ────────────────────

local current = { method = "GET", url = "", body = "" }
local headers_rows = {}
local params_rows = {}
local status_text = "idle"
local response_pretty = ""

-- ── Helpers ──────────────────────────────────────────────────────────────

-- `{{name}}` substitution against `env_vars`. An unknown variable is left
-- verbatim (rather than silently becoming empty) so a typo is visible in
-- the request that actually went out.
local function substitute(s)
    return (s:gsub("{{%s*([%w_]+)%s*}}", function(name)
        return env_vars[name] or ("{{" .. name .. "}}")
    end))
end

-- Fold `params_rows` onto a (already variable-substituted) URL as a query
-- string.
local function url_with_params(base_url)
    local parts = {}
    for _, row in ipairs(params_rows) do
        if row.key ~= "" then
            parts[#parts + 1] = substitute(row.key) .. "=" .. substitute(row.value)
        end
    end
    if #parts == 0 then
        return base_url
    end
    local sep = base_url:find("?", 1, true) and "&" or "?"
    return base_url .. sep .. table.concat(parts, "&")
end

local function headers_map()
    local map = {}
    for _, row in ipairs(headers_rows) do
        if row.key ~= "" then
            map[substitute(row.key)] = substitute(row.value)
        end
    end
    return map
end

local function pretty_json(body)
    local ok, decoded = pcall(vimcode.json.decode, body or "")
    if ok then
        local ok2, encoded = pcall(vimcode.json.encode, decoded, { pretty = true })
        if ok2 then
            return encoded
        end
    end
    return body or ""
end

local function refresh_all()
    vimcode.ui.refresh("rest_client_request")
    vimcode.ui.refresh("rest_client_headers")
    vimcode.ui.refresh("rest_client_params")
    vimcode.ui.refresh("rest_client_response")
    vimcode.ui.refresh("rest_client_history")
    vimcode.ui.refresh("rest_client_collections")
end

local function open_tab(name)
    vimcode.ui.open_view(name, { location = "tab" })
end

-- ── Send ─────────────────────────────────────────────────────────────────

local function do_send()
    local url = url_with_params(substitute(current.url))
    local has_body = current.method == "POST" or current.method == "PUT" or current.method == "PATCH"
    status_text = "sending..."
    vimcode.ui.refresh("rest_client_request")
    vimcode.http.request({
        method = current.method,
        url = url,
        headers = headers_map(),
        body = has_body and current.body or nil,
    }, function(resp)
        if resp.error then
            status_text = "ERROR: " .. tostring(resp.error)
            response_pretty = ""
        else
            local size = #(resp.body or "")
            status_text = string.format("%d · %dms · %dB", resp.status, resp.elapsed_ms or 0, size)
            response_pretty = pretty_json(resp.body)
            table.insert(history, 1, {
                method = current.method,
                url = url,
                status = resp.status,
                elapsed_ms = resp.elapsed_ms or 0,
                size = size,
            })
            -- Keep the persisted history bounded.
            while #history > 50 do
                table.remove(history)
            end
            persist("history", history)
        end
        refresh_all()
    end)
end

-- ── Request view (the hub) ──────────────────────────────────────────────

local function handle_request_event(_ctx, event)
    if event.widget_id == "url" and event.value ~= nil then
        current.url = event.value
    elseif event.widget_id == "body" and event.value ~= nil then
        current.body = event.value
    elseif event.widget_id == "method" and event.kind == "DropdownChanged" then
        current.method = method_options[event.value + 1]
    elseif event.widget_id == "add_header" and event.kind == "ButtonClicked" then
        table.insert(headers_rows, { key = "", value = "" })
        vimcode.ui.refresh("rest_client_headers")
    elseif event.widget_id == "add_param" and event.kind == "ButtonClicked" then
        table.insert(params_rows, { key = "", value = "" })
        vimcode.ui.refresh("rest_client_params")
    elseif event.widget_id == "send" and event.kind == "ButtonClicked" then
        do_send()
    elseif event.widget_id == "save" and event.kind == "ButtonClicked" then
        table.insert(collections, { method = current.method, url = current.url, body = current.body })
        persist("collections", collections)
        vimcode.ui.refresh("rest_client_collections")
    elseif event.widget_id == "open_headers" and event.kind == "ButtonClicked" then
        open_tab("rest_client_headers")
    elseif event.widget_id == "open_params" and event.kind == "ButtonClicked" then
        open_tab("rest_client_params")
    elseif event.widget_id == "open_response" and event.kind == "ButtonClicked" then
        open_tab("rest_client_response")
    elseif event.widget_id == "open_history" and event.kind == "ButtonClicked" then
        open_tab("rest_client_history")
    elseif event.widget_id == "open_collections" and event.kind == "ButtonClicked" then
        open_tab("rest_client_collections")
    elseif event.widget_id == "open_env" and event.kind == "ButtonClicked" then
        open_tab("rest_client_env")
    end
    vimcode.ui.refresh("rest_client_request")
end

vimcode.ui.register_view("rest_client_request", {
    title = "REST Client",
    icon = "R",
    fallback_icon = "R",
    render = function()
        return {
            fields = {
                { id = "hdr", type = "label", label = "REST Client" },
                {
                    id = "url",
                    type = "text",
                    label = "URL",
                    value = current.url,
                    placeholder = "https://host/path or {{base}}/path",
                },
                {
                    id = "method",
                    type = "dropdown",
                    label = "Method",
                    options = method_options,
                    selected = method_index(current.method),
                },
                { id = "body", type = "text_area", label = "Body", value = current.body, rows = 4 },
                { id = "add_header", type = "button", label = "Add Header" },
                { id = "add_param", type = "button", label = "Add Param" },
                { id = "send", type = "button", label = "Send" },
                { id = "save", type = "button", label = "Save" },
                { id = "status", type = "read_only", label = "Status", value = status_text },
                { id = "open_headers", type = "button", label = "Headers" },
                { id = "open_params", type = "button", label = "Params" },
                { id = "open_response", type = "button", label = "Response" },
                { id = "open_history", type = "button", label = "History" },
                { id = "open_collections", type = "button", label = "Collections" },
                { id = "open_env", type = "button", label = "Environment" },
            },
        }
    end,
    on_event = handle_request_event,
})

-- ── Headers / Params tables ──────────────────────────────────────────────

-- Shared by both — `rows` is the backing store, `view_name` is which
-- registered view to refresh after an edit.
local function table_view_source(rows)
    local cols = { { title = "Key", editable = true }, { title = "Value", editable = true } }
    local out = {}
    for i, row in ipairs(rows) do
        out[i] = { id = "r" .. i, cells = { row.key, row.value } }
    end
    return { kind = "table", columns = cols, rows = out }
end

local function table_view_edit(rows, event)
    if event.kind ~= "CellEdited" then
        return
    end
    local row = rows[event.row + 1]
    if not row then
        return
    end
    if event.col == 0 then
        row.key = event.value
    else
        row.value = event.value
    end
end

vimcode.ui.register_view("rest_client_headers", {
    title = "REST Headers",
    icon = "H",
    fallback_icon = "H",
    render = function()
        return table_view_source(headers_rows)
    end,
    on_event = function(_ctx, event)
        table_view_edit(headers_rows, event)
    end,
})

vimcode.ui.register_view("rest_client_params", {
    title = "REST Params",
    icon = "P",
    fallback_icon = "P",
    render = function()
        return table_view_source(params_rows)
    end,
    on_event = function(_ctx, event)
        table_view_edit(params_rows, event)
    end,
})

-- ── Response viewer ──────────────────────────────────────────────────────

vimcode.ui.register_view("rest_client_response", {
    title = "REST Response",
    icon = "V",
    fallback_icon = "V",
    render = function()
        return {
            kind = "text_view",
            text = response_pretty ~= "" and response_pretty or "(no response yet)",
            filetype = "json",
        }
    end,
})

-- ── History ──────────────────────────────────────────────────────────────

vimcode.ui.register_view("rest_client_history", {
    title = "REST History",
    icon = "T",
    fallback_icon = "T",
    render = function()
        local items = {}
        for i, h in ipairs(history) do
            items[i] = {
                id = "hist" .. i,
                text = h.method .. " " .. h.url,
                detail = tostring(h.status) .. " " .. tostring(h.elapsed_ms) .. "ms",
            }
        end
        return { kind = "list", title = "History", items = items }
    end,
    on_event = function(_ctx, event)
        if event.kind == "ItemActivated" then
            local h = history[event.index + 1]
            if h then
                current.method = h.method
                current.url = h.url
                vimcode.ui.refresh("rest_client_request")
            end
        end
    end,
})

-- ── Collections ──────────────────────────────────────────────────────────

vimcode.ui.register_view("rest_client_collections", {
    title = "REST Collections",
    icon = "C",
    fallback_icon = "C",
    render = function()
        local children = {}
        for i, c in ipairs(collections) do
            children[i] = { id = "col" .. i, label = c.method .. " " .. c.url }
        end
        return {
            kind = "tree",
            nodes = {
                { id = "root", label = "Saved Requests", expanded = true, children = children },
            },
        }
    end,
    on_event = function(_ctx, event)
        if event.kind ~= "NodeActivated" then
            return
        end
        local idx = tonumber(event.node_id:match("^col(%d+)$"))
        if idx and collections[idx] then
            current.method = collections[idx].method
            current.url = collections[idx].url
            current.body = collections[idx].body or ""
            vimcode.ui.refresh("rest_client_request")
        end
    end,
})

-- ── Environment ──────────────────────────────────────────────────────────

local function env_vars_text()
    local names = {}
    for name in pairs(env_vars) do
        names[#names + 1] = name
    end
    table.sort(names)
    local lines = {}
    for _, name in ipairs(names) do
        lines[#lines + 1] = name .. "=" .. env_vars[name]
    end
    return table.concat(lines, "\n")
end

vimcode.ui.register_view("rest_client_env", {
    title = "REST Environment",
    icon = "E",
    fallback_icon = "E",
    render = function()
        return {
            fields = {
                { id = "hdr", type = "label", label = "Environment Variables" },
                {
                    id = "vars",
                    type = "text_area",
                    label = "name=value (one per line)",
                    value = env_vars_text(),
                    rows = 6,
                },
            },
        }
    end,
    -- Reacts to `TextChanged` (fired per keystroke, #1627) as well as
    -- `TextCommitted`, exactly like the Request view's `url`/`body` fields —
    -- a `text_area`'s Enter key inserts a newline rather than committing
    -- (`Engine::handle_plugin_view_text_key`'s `is_textarea && !ctrl` arm),
    -- so waiting for `TextCommitted` alone would mean `{{name}}` variables
    -- never take effect without an explicit blur/Ctrl+Enter. Reparsing the
    -- whole buffer on every keystroke is cheap enough for a handful of
    -- `name=value` lines.
    on_event = function(_ctx, event)
        if event.widget_id ~= "vars" or event.value == nil then
            return
        end
        local next_vars = {}
        for line in event.value:gmatch("[^\n]+") do
            local name, value = line:match("^%s*([%w_]+)%s*=%s*(.-)%s*$")
            if name then
                next_vars[name] = value
            end
        end
        env_vars = next_vars
        persist("env_vars", env_vars)
    end,
})

-- ── Commands ─────────────────────────────────────────────────────────────

vimcode.command("RestClient", function()
    open_tab("rest_client_request")
end)
vimcode.command("RestClientHeaders", function()
    open_tab("rest_client_headers")
end)
vimcode.command("RestClientParams", function()
    open_tab("rest_client_params")
end)
vimcode.command("RestClientResponse", function()
    open_tab("rest_client_response")
end)
vimcode.command("RestClientHistory", function()
    open_tab("rest_client_history")
end)
vimcode.command("RestClientCollections", function()
    open_tab("rest_client_collections")
end)
vimcode.command("RestClientEnv", function()
    open_tab("rest_client_env")
end)
