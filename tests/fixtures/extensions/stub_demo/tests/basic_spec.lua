-- Stubs `kubectl` with a fixture Lua script (resolved relative to this
-- spec file, not the harness process's cwd — see `t.stub_command`'s own
-- doc), triggers the extension's wrapping command, and asserts the
-- stubbed output actually reaches the screen.

t.stub_command("kubectl", "fixtures/fake_kubectl.lua")

t.command(":StubDemoFetch")
t.wait_for(function()
    return t.contains("pod-a")
end, 5)

assert(t.contains("pod-a     Running"), "expected the stubbed kubectl output to be painted")
assert(t.contains("pod-b     Running"), "expected the stubbed kubectl output to be painted")
