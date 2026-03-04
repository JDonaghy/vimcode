-- Git Insights: inline blame annotation on current line
-- Shows author, relative date, and commit message as virtual text.
-- Annotations are hidden automatically while in insert mode (the engine
-- suppresses cursor_move in insert mode, and the render layer clears them).

local last_line = -1
local last_path = ""

vimcode.on("cursor_move", function(_)
  local cur = vimcode.buf.cursor()
  local path = vimcode.buf.path()

  -- Reset when switching to a different file (tab switch, window switch, etc.)
  if path ~= last_path then
    last_path = path
    last_line = -1
  end

  if cur.line == last_line then return end
  last_line = cur.line
  vimcode.buf.clear_annotations()
  local info = vimcode.git.blame_line(cur.line)
  if info then
    local text
    if info.not_committed then
      text = "   Not committed yet"
    else
      text = "   " .. info.author .. " \u{2022} " .. info.relative_date .. " \u{2022} " .. info.message
    end
    vimcode.buf.annotate_line(cur.line, text)
  end
end)

-- :GitLog — show recent commits for current file in status bar
vimcode.command("GitLog", function(_)
  local entries = vimcode.git.log_file(10)
  if #entries == 0 then
    vimcode.message("No git history for this file")
    return
  end
  local lines = {}
  for _, e in ipairs(entries) do
    table.insert(lines, e.hash .. " " .. e.message)
  end
  vimcode.message(table.concat(lines, " | "))
end)
