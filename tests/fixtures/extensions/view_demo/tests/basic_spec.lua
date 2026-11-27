-- Drives the `view_demo` fixture's View API panel: opens it, checks both
-- the rendered screen and the structural widget tree (`t.view`), clicks
-- its button by locating the painted label text (never a hardcoded
-- coordinate), and asserts the resulting repaint reflects the click.

t.command(":ViewDemoOpen")

assert(t.contains("Counter"), "expected the 'Counter' label to be painted")
assert(t.contains("Increment"), "expected the 'Increment' button to be painted")

local view = t.view("view_demo")
assert(view ~= nil, "expected view_demo to be a registered view")
assert(view.fields[2].kind.value == "0", "expected the counter to start at 0")

t.click_text("Increment")

local after = t.view("view_demo")
assert(
    after.fields[2].kind.value == "1",
    "expected the counter field to read 1 after one click, got " .. tostring(after.fields[2].kind.value)
)
assert(t.contains("1"), "expected the incremented count to be painted on screen")
