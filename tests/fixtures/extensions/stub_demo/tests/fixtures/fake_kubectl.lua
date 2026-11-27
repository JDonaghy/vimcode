-- Fake `kubectl` stub: a real, standalone Lua script run by `vimcode
-- --run-lua-stub` (see `t.stub_command` in src/test_extension.rs), never
-- loaded by the extension itself. Ignores its arguments and prints a
-- fixed pod list, matching the shape `kubectl get pods` prints.
io.write("NAME      STATUS\n")
io.write("pod-a     Running\n")
io.write("pod-b     Running\n")
os.exit(0)
