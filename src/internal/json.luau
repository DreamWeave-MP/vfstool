--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- JSON read the way serde_json reads it into a `Value`: strict syntax, whitespace only between
-- tokens, numbers kept as integers when they are written as integers and fit 64 bits, and nesting
-- deeper than 128 refused. Values carry their kind, so `1` and `1.0` stay different, as serde_json
-- keeps them.

--- A decoded value: `null`, a boolean, a string, an integer (by its exact decimal spelling), a
--- float, an array, or an object (keys in document order, later duplicates winning).
export type Value =
  { kind: 'null' }
  | { kind: 'bool', value: boolean }
  | { kind: 'string', value: string }
  | { kind: 'int', value: string }
  | { kind: 'float', value: number }
  | { kind: 'array', items: { Value } }
  | { kind: 'object', keys: { string }, values: { [string]: Value } }

local Null: Value = table.freeze { kind = 'null' } :: Value

local MaxDepth = 128

local Escapes = table.freeze {
  [34] = '"',
  [92] = '\\',
  [47] = '/',
  [98] = '\b',
  [102] = '\f',
  [110] = '\n',
  [114] = '\r',
  [116] = '\t',
}

type Parser = { text: string, at: number }

local function skip(parser: Parser)
  parser.at = string.find(parser.text, '[^ \t\n\r]', parser.at) or (#parser.text + 1)
end

-- Every reader answers nil for text that isn't JSON, and its caller passes the nil up.

local function hex4(parser: Parser): number?
  local digits = string.match(parser.text, '^%x%x%x%x', parser.at)
  if not digits then return nil end

  parser.at += 4
  return tonumber(digits, 16)
end

--- A `\u` escape's code point, joining a surrogate pair.
local function codePoint(parser: Parser): number?
  local unit = hex4(parser)
  if not unit then return nil end

  if unit >= 0xDC00 and unit <= 0xDFFF then return nil end

  if unit < 0xD800 or unit > 0xDBFF then return unit end

  if string.sub(parser.text, parser.at, parser.at + 1) ~= '\\u' then return nil end

  parser.at += 2
  local low = hex4(parser)
  if not low or low < 0xDC00 or low > 0xDFFF then return nil end

  return 0x10000 + (unit - 0xD800) * 0x400 + (low - 0xDC00)
end

local function readString(parser: Parser): string?
  local text = parser.text
  local parts = {}
  parser.at += 1

  while true do
    local stop = string.find(text, '["\\%c]', parser.at)
    if not stop then return nil end

    table.insert(parts, string.sub(text, parser.at, stop - 1))
    local byte = string.byte(text, stop)
    if byte == 34 then
      parser.at = stop + 1
      break
    end

    -- A control character must be escaped.
    if byte ~= 92 then return nil end

    local kind = string.byte(text, stop + 1)
    parser.at = stop + 2
    if kind == 117 then
      local unit = codePoint(parser)
      if not unit then return nil end

      table.insert(parts, utf8.char(unit))
      continue
    end

    local escaped = kind and Escapes[kind]
    if not escaped then return nil end

    table.insert(parts, escaped)
  end

  local value = table.concat(parts)
  if not utf8.len(value) then return nil end

  return value
end

local function readNumber(parser: Parser): Value?
  local spelled = string.match(parser.text, '^-?%d+%.?%d*[eE]?[-+]?%d*', parser.at)
  if not spelled then return nil end

  local integerPart, fraction, exponent = string.match(spelled, '^(-?%d+)(%.?%d*)([eE]?[-+]?%d*)$')
  if not integerPart or not fraction or not exponent then return nil end

  -- No leading zeros, a digit after a point, digits after an exponent.
  if string.match(integerPart, '^-?0%d') then return nil end

  if
    fraction == '.'
    or exponent == 'e'
    or exponent == 'E'
    or string.match(exponent, '^[eE][-+]$')
  then
    return nil
  end

  parser.at += #spelled
  if fraction == '' and exponent == '' then
    local magnitude = string.gsub(integerPart, '^-', '')
    local negative = string.byte(integerPart, 1) == 45
    local limit = if negative then '9223372036854775808' else '18446744073709551615'
    if #magnitude < #limit or (#magnitude == #limit and magnitude <= limit) then
      if negative and magnitude == '0' then return { kind = 'float', value = -0.0 } end

      return { kind = 'int', value = integerPart }
    end
  end

  return { kind = 'float', value = tonumber(spelled) :: number }
end

local readValue: (Parser, number) -> Value?

local function readArray(parser: Parser, depth: number): Value?
  parser.at += 1
  local items = {}
  skip(parser)
  if string.byte(parser.text, parser.at) == 93 then
    parser.at += 1
    return { kind = 'array', items = items }
  end

  while true do
    local item = readValue(parser, depth)
    if not item then return nil end

    table.insert(items, item)
    skip(parser)
    local byte = string.byte(parser.text, parser.at)
    parser.at += 1
    if byte == 93 then break end

    if byte ~= 44 then return nil end
  end

  return { kind = 'array', items = items }
end

local function readObject(parser: Parser, depth: number): Value?
  parser.at += 1
  local keys, values = {}, {}
  skip(parser)
  if string.byte(parser.text, parser.at) == 125 then
    parser.at += 1
    return { kind = 'object', keys = keys, values = values }
  end

  while true do
    skip(parser)
    if string.byte(parser.text, parser.at) ~= 34 then return nil end

    local key = readString(parser)
    if not key then return nil end

    skip(parser)
    if string.byte(parser.text, parser.at) ~= 58 then return nil end

    parser.at += 1
    local value = readValue(parser, depth)
    if not value then return nil end

    if values[key] == nil then table.insert(keys, key) end

    values[key] = value
    skip(parser)
    local byte = string.byte(parser.text, parser.at)
    parser.at += 1
    if byte == 125 then break end

    if byte ~= 44 then return nil end
  end

  return { kind = 'object', keys = keys, values = values }
end

function readValue(parser: Parser, depth: number): Value?
  if depth > MaxDepth then return nil end

  skip(parser)
  local text = parser.text
  local byte = string.byte(text, parser.at)
  if byte == 123 then return readObject(parser, depth + 1) end

  if byte == 91 then return readArray(parser, depth + 1) end

  if byte == 34 then
    local value = readString(parser)
    return if value then { kind = 'string', value = value } else nil
  end

  if byte == 116 and string.sub(text, parser.at, parser.at + 3) == 'true' then
    parser.at += 4
    return { kind = 'bool', value = true }
  end

  if byte == 102 and string.sub(text, parser.at, parser.at + 4) == 'false' then
    parser.at += 5
    return { kind = 'bool', value = false }
  end

  if byte == 110 and string.sub(text, parser.at, parser.at + 3) == 'null' then
    parser.at += 4
    return Null
  end

  if byte == 45 or (byte and byte >= 48 and byte <= 57) then return readNumber(parser) end

  return nil
end

--- The document's value, or nil when it isn't one JSON value (trailing whitespace allowed).
local function decode(text: string): Value?
  local parser = { text = text, at = 1 }
  local result = readValue(parser, 0)
  skip(parser)
  if parser.at <= #text then return nil end

  return result
end

--- Whether two values are equal the way serde_json's `Value` compares them.
local function equal(left: Value, right: Value): boolean
  if left.kind == 'array' and right.kind == 'array' then
    if #left.items ~= #right.items then return false end

    for index, item in left.items do
      if not equal(item, right.items[index]) then return false end
    end

    return true
  end

  if left.kind == 'object' and right.kind == 'object' then
    if #left.keys ~= #right.keys then return false end

    for key, value in left.values do
      local other = right.values[key]
      if other == nil or not equal(value, other) then return false end
    end

    return true
  end

  if left.kind == 'null' and right.kind == 'null' then return true end

  if left.kind == 'bool' and right.kind == 'bool' then return left.value == right.value end

  if left.kind == 'string' and right.kind == 'string' then return left.value == right.value end

  if left.kind == 'int' and right.kind == 'int' then return left.value == right.value end

  if left.kind == 'float' and right.kind == 'float' then return left.value == right.value end

  return false
end

return table.freeze {
  Null = Null,
  decode = decode,
  equal = equal,
}
