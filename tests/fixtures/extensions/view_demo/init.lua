-- Fixture extension for `tests/test_extension_harness.rs`: a minimal
-- `vimcode.ui.register_view` panel with one button that increments a
-- counter, exercising the current View API end to end (register, open,
-- render, click-driven `on_event`).

local count = 0

vimcode.ui.register_view("view_demo", {
    title = "View Demo",
    icon = "V",
    fallback_icon = "V",
    render = function(ctx)
        return {
            id = "main",
            schema_version = 1,
            fields = {
                { type = "label", id = "hdr", label = "Counter" },
                { type = "read_only", id = "count", label = "Count", value = "Count: " .. tostring(count) },
                { type = "button", id = "inc", label = "Increment" },
            },
        }
    end,
    on_event = function(ctx, event)
        if event.kind == "ButtonClicked" and event.widget_id == "inc" then
            count = count + 1
        end
    end,
})

vimcode.command("ViewDemoOpen", function()
    vimcode.ui.open_view("view_demo", { location = "tab" })
end)
