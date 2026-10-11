--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- vfstool's globs: `*` within a component, `**` across them, `**/` for any number of leading
-- folders, `?` one character; matched against the whole path folded like a host path. A glob is
-- compiled to the same Rust regular expression the crate built.
local paths = require './paths'

--- The regular expression a glob means, anchored at both ends.
local function pattern(glob: string): string
  local out = { '^' }
  local chars = {}
  for _, code in utf8.codes(glob) do
    table.insert(chars, utf8.char(code))
  end

  local index = 1
  while index <= #chars do
    local char = chars[index]
    if char == '*' then
      if chars[index + 1] == '*' then
        if chars[index + 2] == '/' then
          table.insert(out, '(?:.*/)?')
          index += 3
        else
          table.insert(out, '.*')
          index += 2
        end
      else
        table.insert(out, '[^/]*')
        index += 1
      end
    elseif char == '?' then
      table.insert(out, '[^/]')
      index += 1
    else
      table.insert(out, jess.regex.escape(char))
      index += 1
    end
  end

  table.insert(out, '$')
  return table.concat(out)
end

export type Matcher = (path: string) -> boolean

--- A matcher for `glob`, or nil when it doesn't compile (the crate then matched nothing).
local function compile(glob: string): Matcher?
  -- jess.regex.new throws for a pattern it can't compile.
  local ok, regex = pcall(jess.regex.new, pattern(paths.lossy(paths.normalizeHost(glob))), nil)
  if not ok then return nil end

  return function(path: string): boolean
    return (regex :: Regex):isMatch(paths.lossy(paths.normalizeHost(path)))
  end
end

local function matches(glob: string, path: string): boolean
  local matcher = compile(glob)
  return matcher ~= nil and matcher(path)
end

return table.freeze {
  pattern = pattern,
  compile = compile,
  matches = matches,
}
