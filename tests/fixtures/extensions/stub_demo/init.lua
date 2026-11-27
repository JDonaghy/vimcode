-- Fixture extension for `tests/test_extension_harness.rs`: wraps a
-- `kubectl`-shaped CLI the way the real k8s/sql-tool extensions do, so a
-- spec can prove the wrapping logic without the real tool installed —
-- `t.stub_command("kubectl", ...)` puts a fake one on `PATH` first.

local output = ""

vimcode.ui.register_view("stub_demo", {
    title = "Stub Demo",
    icon = "S",
    fallback_icon = "S",
    render = function(ctx)
        return {
            id = "main",
            schema_version = 1,
            fields = {
                { type = "label", id = "hdr", label = "kubectl get pods" },
                { type = "read_only", id = "out", label = "Output", value = output },
            },
        }
    end,
})

vimcode.command("StubDemoFetch", function()
    vimcode.loop.spawn("kubectl", { "get", "pods" }, {
        on_stdout = function(chunk)
            output = output .. chunk
        end,
        on_exit = function(code, signal)
            vimcode.ui.refresh("stub_demo")
            vimcode.ui.open_view("stub_demo", { location = "tab" })
        end,
    })
end)
