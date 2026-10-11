--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- `analyze_pair`: whether two versions of a file differ in what they do or only in how they are
-- written. The class comes from the extension, else from the bytes; INI, TOML and JSON compare
-- by their parsed values, text by its trimmed non-blank lines, anything else byte for byte.
local json = require './json'
local paths = require './paths'

export type Delta = { kind: string, changeSummary: { string }? }

--- Rust's `char::is_whitespace` beyond ASCII, as UTF-8.
local UnicodeSpaces = table.freeze {
  '\u{85}',
  '\u{a0}',
  '\u{1680}',
  '\u{2000}',
  '\u{2001}',
  '\u{2002}',
  '\u{2003}',
  '\u{2004}',
  '\u{2005}',
  '\u{2006}',
  '\u{2007}',
  '\u{2008}',
  '\u{2009}',
  '\u{200a}',
  '\u{2028}',
  '\u{2029}',
  '\u{202f}',
  '\u{205f}',
  '\u{3000}',
}

local function isSpaceAt(text: string, at: number): number?
  local byte = string.byte(text, at)
  if not byte then return nil end

  if byte == 32 or (byte >= 9 and byte <= 13) then return 1 end

  if byte < 0xC2 then return nil end

  for _, space in UnicodeSpaces do
    if string.sub(text, at, at + #space - 1) == space then return #space end
  end

  return nil
end

--- `str::trim`: Unicode whitespace off both ends.
local function trim(text: string): string
  local first = 1
  while true do
    local width = isSpaceAt(text, first)
    if not width then break end

    first += width
  end

  local last = #text
  while last >= first do
    -- Step back to the start of the last character, then see whether it is a space.
    local start = last
    while start > first and bit32.band(string.byte(text, start), 0xC0) == 0x80 do
      start -= 1
    end

    local width = isSpaceAt(text, start)
    if not width or start + width - 1 ~= last then break end

    last = start - 1
  end

  return string.sub(text, first, last)
end

--- `str::lines`: split at `\n`, a `\r` before it dropped, no line after a final newline.
local function lines(text: string): { string }
  local out = {}
  local start = 1
  while start <= #text do
    local stop = string.find(text, '\n', start, true)
    local line = string.sub(text, start, (stop or (#text + 1)) - 1)
    if string.byte(line, -1) == 13 then line = string.sub(line, 1, -2) end

    table.insert(out, line)
    if not stop then break end

    start = stop + 1
  end

  return out
end

local function isUtf8(text: string): boolean return utf8.len(text) ~= nil end

local function normalizedText(text: string): string
  local kept = {}
  for _, line in lines(text) do
    local trimmed = trim(line)
    if trimmed ~= '' then table.insert(kept, trimmed) end
  end

  return table.concat(kept, '\n')
end

local function behavior(summary: string): Delta
  return { kind = 'behaviorChanging', changeSummary = { summary } }
end

local function binary(left: string, right: string): Delta
  if left == right then return { kind = 'noOpEquivalent' } end

  return behavior 'binary payload differs'
end

local function text(left: string, right: string): Delta
  if not isUtf8(left) or not isUtf8(right) then return { kind = 'unknown' } end

  if left == right then return { kind = 'noOpEquivalent' } end

  if normalizedText(left) == normalizedText(right) then return { kind = 'cosmeticOnly' } end

  return behavior 'text content differs after normalization'
end

--- `parse_ini_like`, as one canonical string: sections and keys lowercased, values trimmed, the
--- last value of a key kept, everything sorted.
local function iniCanonical(source: string): string
  local sections: { [string]: { [string]: string } } = {}
  local section = 'global'

  for _, line in lines(source) do
    local trimmed = trim(line)
    local first = string.byte(trimmed, 1)
    if trimmed == '' or first == 59 or first == 35 then continue end

    if first == 91 and string.byte(trimmed, -1) == 93 then
      section = string.lower(trim(string.sub(trimmed, 2, -2)))
      continue
    end

    local equals = string.find(trimmed, '=', 1, true)
    if not equals then continue end

    local values = sections[section]
    if not values then
      values = {}
      sections[section] = values
    end

    local key = string.lower(trim(string.sub(trimmed, 1, equals - 1)))
    values[key] = trim(string.sub(trimmed, equals + 1))
  end

  local names = {}
  for name in sections do
    table.insert(names, name)
  end

  table.sort(names)
  local parts = {}
  for _, name in names do
    local keys = {}
    for key in sections[name] do
      table.insert(keys, key)
    end

    table.sort(keys)
    table.insert(parts, `[{#name}]{name}`)
    for _, key in keys do
      local value = sections[name][key]
      table.insert(parts, `{#key}:{key}={#value}:{value}`)
    end
  end

  return table.concat(parts, '\n')
end

local function ini(left: string, right: string): Delta
  if not isUtf8(left) or not isUtf8(right) then return { kind = 'unknown' } end

  if left == right then return { kind = 'noOpEquivalent' } end

  if iniCanonical(left) == iniCanonical(right) then return { kind = 'cosmeticOnly' } end

  return behavior 'INI keys/values differ'
end

type TomlLayout = {
  [unknown]: {
    keys: { string }?,
    floats: { [string | number]: boolean },
    datetimes: { [string | number]: boolean }?,
  },
}

--- A decoded TOML value as one canonical string, integers, floats, datetimes and strings told
--- apart by the layout.
local function tomlCanonical(
  value: unknown,
  float: boolean,
  layout: TomlLayout,
  datetime: boolean?
): string
  local kind = type(value)
  if kind == 'string' then return `{if datetime then 'd' else 's'}{#(value :: string)}:{value}` end

  if kind == 'boolean' then return if value then 'T' else 'F' end

  if kind == 'number' then
    local number = value :: number
    if float then
      if number ~= number then return 'fnan' end

      return `f{string.format('%.17g', number + 0.0)}`
    end

    return `i{string.format('%.0f', number)}`
  end

  local described = layout[value] or { floats = {}, datetimes = {}, keys = nil }
  local floats = described.floats
  local datetimes = described.datetimes or {}
  local keys = described.keys
  local parts = {}
  if keys then
    local sorted = table.clone(keys)
    table.sort(sorted)
    for _, key in sorted do
      local item = (value :: { [string]: unknown })[key]
      table.insert(
        parts,
        `{#key}:{key}={tomlCanonical(item, floats[key] == true, layout, datetimes[key] == true)}`
      )
    end

    return `\{{table.concat(parts, ',')}\}`
  end

  for index, item in value :: { unknown } do
    parts[index] = tomlCanonical(item, floats[index] == true, layout, datetimes[index] == true)
  end

  return `[{table.concat(parts, ',')}]`
end

local function tomlValue(source: string): string?
  if not isUtf8(source) then return nil end

  -- jess.toml.decode throws for a document that isn't TOML.
  local ok, value, layout = pcall(jess.toml.decode, source)
  if not ok then return nil end

  return tomlCanonical(value, false, (layout :: unknown) :: TomlLayout)
end

local function toml(left: string, right: string): Delta
  if left == right then return { kind = 'noOpEquivalent' } end

  local a, b = tomlValue(left), tomlValue(right)
  if not a or not b then return { kind = 'unknown' } end

  if a == b then return { kind = 'cosmeticOnly' } end

  return behavior 'TOML semantic values differ'
end

local function jsonDelta(left: string, right: string): Delta
  if left == right then return { kind = 'noOpEquivalent' } end

  local a, b = json.decode(left), json.decode(right)
  if not a or not b then return { kind = 'unknown' } end

  if json.equal(a, b) then return { kind = 'cosmeticOnly' } end

  return behavior 'JSON semantic values differ'
end

local Classes: { [string]: string } = {
  ini = 'ini',
  cfg = 'ini',
  toml = 'toml',
  json = 'json',
  lua = 'luaScript',
  mws = 'mwScriptLike',
  mwscript = 'mwScriptLike',
  txt = 'text',
  md = 'text',
}

local function class(path: string, left: string, right: string): string
  local extension = paths.extension(path)
  local known = extension and isUtf8(extension) and Classes[string.lower(extension)]
  if known then return known end

  if string.find(left, '\0', 1, true) or string.find(right, '\0', 1, true) then return 'binary' end

  if isUtf8(left) and isUtf8(right) then return 'text' end

  return 'unknown'
end

--- The asset class of `path` and how `right` differs from `left`.
local function analyze(path: string, left: buffer | string, right: buffer | string): (string, Delta)
  local a = if type(left) == 'buffer' then buffer.tostring(left) else left
  local b = if type(right) == 'buffer' then buffer.tostring(right) else right
  local kind = class(path, a, b)
  if kind == 'ini' then return kind, ini(a, b) end

  if kind == 'toml' then return kind, toml(a, b) end

  if kind == 'json' then return kind, jsonDelta(a, b) end

  if kind == 'luaScript' or kind == 'mwScriptLike' or kind == 'text' then
    return kind, text(a, b)
  end

  if kind == 'binary' then return kind, binary(a, b) end

  return kind, { kind = 'unknown' }
end

return table.freeze {
  analyze = analyze,
  trim = trim,
  lines = lines,
}
