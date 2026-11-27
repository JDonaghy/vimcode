-- Drives the `view_demo` fixture's View API panel: opens it, checks both
-- the rendered screen and the structural widget tree (`t.view`), clicks
-- its button by locating the painted label text (never a hardcoded
-- coordinate), and asserts the resulting repaint reflects the click. Also
-- exercises `t.keys`, `t.screen`, `t.cell` and the raw `t.click`, which
-- `t.command`/`t.contains`/`t.click_text` alone never touch.

t.keys("<Esc>")
t.command(":ViewDemoOpen")

assert(t.contains("Counter"), "expected the 'Counter' label to be painted")
assert(t.contains("Increment"), "expected the 'Increment' button to be painted")

local screen = t.screen()
assert(type(screen) == "string", "t.screen() should return the rendered frame as a string")
assert(screen:find("Counter") ~= nil, "t.screen() should include the painted header")

local corner = t.cell(0, 0)
assert(corner ~= nil, "t.cell(0, 0) should return a painted cell table")
assert(type(corner.char) == "string", "t.cell(...).char should be a one-character string")

-- Row 0 is the tab bar, not a field this spec cares about — a direct hit
-- test through the real click path, deliberately away from "Increment"
-- so it doesn't interfere with the counter assertions below.
t.click(0, 0)

local view = t.view("view_demo")
assert(view ~= nil, "expected view_demo to be a registered view")
assert(view.fields[2].kind.value == "Count: 0", "expected the counter to start at 0")

t.click_text("Increment")

local after = t.view("view_demo")
assert(
    after.fields[2].kind.value == "Count: 1",
    "expected the counter field to read 1 after one click, got " .. tostring(after.fields[2].kind.value)
)
-- A unique label, not the bare digit "1" the gutter already paints on
-- every frame regardless of the click — this is real evidence the click
-- caused a repaint, not a tautology that was already true beforehand.
assert(t.contains("Count: 1"), "expected the incremented count to be painted on screen")
