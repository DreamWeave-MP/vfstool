--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- Values written as JSON, YAML and TOML byte for byte the way vfstool wrote them through serde:
-- serde_json's compact JSON, serde_yaml's libyaml block style (scalar styles, quoting and tags
-- decided by the same rules), and the toml crate's pretty documents (tables after values, arrays
-- of two or more one per line, keys and strings quoted only as much as they need).
-- The value model ----------------------------------------------------------------------------------

--- A serde value: `Null` (unit), `None` (an absent option: null in JSON and YAML, left out of
--- TOML tables), booleans, numbers (whole ones below 2^53 are integers, Luau integers always are),
--- strings, arrays, ordered maps, and enum variants (`!tag` in YAML, `{ tag = value }` elsewhere).
export type Value = unknown

local Null = table.freeze { kind = 'null' }
local None = table.freeze { kind = 'none' }

--- What each array, map, variant and forced float of the model is, by identity.
local kinds = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [unknown]: string }

export type Map = { keys: { string }, values: { [string]: Value } }

local function array(items: { Value }): { Value }
  kinds[items] = 'array'
  return items
end

local function map(): Map
  local created: Map = { keys = {}, values = {} }
  kinds[created] = 'map'
  return created
end

--- Sets `key` in `target`, keeping the first position a key was given.
local function set(target: Map, key: string, value: Value): Map
  if target.values[key] == nil then table.insert(target.keys, key) end

  target.values[key] = value
  return target
end

--- A map from `{ key, value }` pairs in order, the way a struct serializes its fields.
local function struct(fields: { { unknown } }): Map
  local out = map()
  for _, pair in fields do
    set(out, pair[1] :: string, if pair[2] == nil then None else pair[2])
  end

  return out
end

local function variant(tag: string, value: Value): Value
  local tagged = { tag = tag, value = value }
  kinds[tagged] = 'variant'
  return tagged
end

--- A number that is always written as a float.
local function float(value: number): Value
  local wrapped = { value = value }
  kinds[wrapped] = 'float'
  return wrapped
end

local function kindOf(value: Value): string
  local luau = type(value)
  if luau == 'string' or luau == 'boolean' then return luau end

  if luau == 'number' then
    local number = value :: number
    if number == math.floor(number) and math.abs(number) < 2 ^ 53 then return 'int' end

    return 'float'
  end

  if typeof(value) == 'integer' then return 'int' end

  if value == Null then return 'null' end

  if value == None then return 'none' end

  local kind = kinds[value]
  if kind then return kind end

  return error('serialize: not a value', 0)
end

local function intText(value: Value): string
  if typeof(value) == 'integer' then return tostring(value) end

  return string.format('%.0f', value :: number)
end

local function floatOf(value: Value): number
  if type(value) == 'number' then return value end

  return (value :: { value: number }).value
end

-- Floats -----------------------------------------------------------------------------------------------

--- The shortest decimal digits of a finite, nonzero `value` and its exponent: `value` is
--- `digits × 10^exponent`, read off Luau's own shortest round-trip printing.
local function decimal(value: number): (string, number)
  local text = tostring(math.abs(value))
  local mantissa, exponent = string.match(text, '^([%d.]+)[eE]([-+]?%d+)$')
  local power = 0
  if mantissa then
    text = mantissa
    power = tonumber(exponent) :: number
  end

  local whole, fraction = string.match(text, '^(%d*)%.?(%d*)$')
  local digits = (whole :: string) .. (fraction :: string)
  power -= #(fraction :: string)
  digits = string.gsub(digits, '^0+', '')
  local trimmed = string.gsub(digits, '0+$', '')
  power += #digits - #trimmed
  return trimmed, power
end

--- Ryū's (and serde_json's) layout: fixed notation for decimal exponents -5 to 15, scientific
--- otherwise; `plus` writes a `+` on positive exponents, as serde_json's formatter does.
local function shortest(value: number, plus: boolean): string
  if value == 0 then return if 1 / value < 0 then '-0.0' else '0.0' end

  local sign = if value < 0 then '-' else ''
  local digits, power = decimal(value)
  local length = #digits
  local point = length + power
  if power >= 0 and point <= 16 then return `{sign}{digits}{string.rep('0', power)}.0` end

  if point > 0 and point <= 16 then
    return `{sign}{string.sub(digits, 1, point)}.{string.sub(digits, point + 1)}`
  end

  if point > -5 and point <= 0 then return `{sign}0.{string.rep('0', -point)}{digits}` end

  local exponent = point - 1
  local spelled = if exponent < 0
    then tostring(exponent)
    elseif plus then `+{exponent}`
    else tostring(exponent)
  if length == 1 then return `{sign}{digits}e{spelled}` end

  return `{sign}{string.sub(digits, 1, 1)}.{string.sub(digits, 2)}e{spelled}`
end

--- Rust's `Display` for a float: never an exponent.
local function display(value: number): string
  if value ~= value then return 'NaN' end

  if value == math.huge then return 'inf' end

  if value == -math.huge then return '-inf' end

  if value == 0 then return if 1 / value < 0 then '-0' else '0' end

  local sign = if value < 0 then '-' else ''
  local digits, power = decimal(value)
  local point = #digits + power
  if power >= 0 then return sign .. digits .. string.rep('0', power) end

  if point > 0 then
    return `{sign}{string.sub(digits, 1, point)}.{string.sub(digits, point + 1)}`
  end

  return `{sign}0.{string.rep('0', -point)}{digits}`
end

-- JSON -------------------------------------------------------------------------------------------------

local JsonEscapes: { [string]: string } = {
  ['"'] = '\\"',
  ['\\'] = '\\\\',
  ['\b'] = '\\b',
  ['\f'] = '\\f',
  ['\n'] = '\\n',
  ['\r'] = '\\r',
  ['\t'] = '\\t',
}

local function jsonString(text: string): string
  return '"'
    .. string.gsub(text, '[%c"\\]', function(char: string): string
      local byte = string.byte(char)
      if byte == 127 then return char end

      return JsonEscapes[char] or string.format('\\u%04x', byte)
    end)
    .. '"'
end

local function json(value: Value, out: { string })
  local kind = kindOf(value)
  if kind == 'null' or kind == 'none' then
    table.insert(out, 'null')
  elseif kind == 'boolean' then
    table.insert(out, if value then 'true' else 'false')
  elseif kind == 'int' then
    table.insert(out, intText(value))
  elseif kind == 'float' then
    local number = floatOf(value)
    table.insert(
      out,
      if number ~= number or math.abs(number) == math.huge then 'null' else shortest(number, true)
    )
  elseif kind == 'string' then
    table.insert(out, jsonString(value :: string))
  elseif kind == 'array' then
    table.insert(out, '[')
    for index, item in value :: { Value } do
      if index > 1 then table.insert(out, ',') end

      json(item, out)
    end

    table.insert(out, ']')
  elseif kind == 'map' then
    local target = value :: Map
    table.insert(out, '{')
    for index, key in target.keys do
      if index > 1 then table.insert(out, ',') end

      table.insert(out, jsonString(key))
      table.insert(out, ':')
      json(target.values[key], out)
    end

    table.insert(out, '}')
  else
    local tagged = value :: { tag: string, value: Value }
    table.insert(out, '{')
    table.insert(out, jsonString(tagged.tag))
    table.insert(out, ':')
    json(tagged.value, out)
    table.insert(out, '}')
  end
end

-- YAML -------------------------------------------------------------------------------------------------

--- serde_yaml's `digits_but_not_number`: leading zeros followed by digits, a string by YAML 1.2.
local function digitsButNotNumber(text: string): boolean
  local unsigned = string.gsub(text, '^[-+]', '', 1)
  return #unsigned > 1 and string.byte(unsigned, 1) == 48 and string.find(unsigned, '^%d+$') ~= nil
end

local function radixInteger(text: string, radix: number, pattern: string): boolean
  if text == '' or string.find(text, '^[+-]') then return false end

  local body = string.gsub(text, '^[+-]', '')
  return body ~= '' and string.find(body, pattern) ~= nil
end

--- Whether serde_yaml would read the plain scalar `text` as anything but a string: null, a
--- boolean, an integer in any radix it accepts, or a float.
local function resolvesAsNonString(text: string): boolean
  if text == '' or text == 'null' or text == 'Null' or text == 'NULL' or text == '~' then
    return true
  end

  if
    text == 'true'
    or text == 'True'
    or text == 'TRUE'
    or text == 'false'
    or text == 'False'
    or text == 'FALSE'
  then
    return true
  end

  local unpositive = string.match(text, '^%+(.*)$') or text
  local negative = string.match(text, '^%-(.*)$')
  local radixes: { [string]: string } =
    { ['0x'] = '^%x+$', ['0o'] = '^[0-7]+$', ['0b'] = '^[01]+$' }
  for prefix, pattern in radixes do
    local rest = string.match(unpositive, '^' .. prefix .. '(.*)$')
    if rest and radixInteger(rest, 0, pattern) then return true end

    local negativeRest = negative and string.match(negative, '^' .. prefix .. '(.*)$')
    if negativeRest and negativeRest ~= '' and string.find(negativeRest, pattern) then
      return true
    end
  end

  if digitsButNotNumber(text) then return false end

  if string.find(text, '^[-+]?%d+$') and not string.find(text, '^%+[-+]') then return true end

  local floatText = string.match(text, '^%+(.*)$') or text
  if string.find(floatText, '^[-+]') and string.byte(text, 1) == 43 then return false end

  if floatText == '.inf' or floatText == '.Inf' or floatText == '.INF' then return true end

  if text == '-.inf' or text == '-.Inf' or text == '-.INF' then return true end

  if text == '.nan' or text == '.NaN' or text == '.NAN' then return true end

  -- Rust's `f64::from_str`: digits with an optional point and exponent, or inf/infinity/nan words.
  local lowered = string.lower(floatText)
  local unsigned = string.gsub(lowered, '^[-+]', '', 1)
  if unsigned == 'inf' or unsigned == 'infinity' or unsigned == 'nan' then return false end

  if
    string.find(unsigned, '^%d+%.?%d*$')
    or string.find(unsigned, '^%.%d+$')
    or string.find(unsigned, '^%d+%.?%d*e[-+]?%d+$')
    or string.find(unsigned, '^%.%d+e[-+]?%d+$')
  then
    local number = tonumber(floatText)
    return number ~= nil and number == number and math.abs(number) ~= math.huge
  end

  return false
end

--- libyaml's view of a scalar: whether it spans lines, and which styles may write it.
type Analysis = {
  multiline: boolean,
  flowPlain: boolean,
  blockPlain: boolean,
  singleQuoted: boolean,
  block: boolean,
  length: number,
}

local function isBreakAt(text: string, at: number): boolean
  local byte = string.byte(text, at)
  if byte == 13 or byte == 10 then return true end

  if byte == 0xC2 then return string.byte(text, at + 1) == 0x85 end

  if byte == 0xE2 and string.byte(text, at + 1) == 0x80 then
    local third = string.byte(text, at + 2)
    return third == 0xA8 or third == 0xA9
  end

  return false
end

local function isBlankzAt(text: string, at: number): boolean
  local byte = string.byte(text, at)
  return byte == nil or byte == 32 or byte == 9 or byte == 0 or isBreakAt(text, at)
end

local function widthAt(text: string, at: number): number
  local byte = string.byte(text, at) or 0
  if byte < 0x80 then return 1 end

  if bit32.band(byte, 0xE0) == 0xC0 then return 2 end

  if bit32.band(byte, 0xF0) == 0xE0 then return 3 end

  if bit32.band(byte, 0xF8) == 0xF0 then return 4 end

  return 0
end

local function isPrintableAt(text: string, at: number): boolean
  local byte = string.byte(text, at) :: number
  if byte == 0x0A or (byte >= 0x20 and byte <= 0x7E) then return true end

  local second = string.byte(text, at + 1) or 0
  if byte == 0xC2 then return second >= 0xA0 end

  if byte >= 0xC3 and byte <= 0xEC then return true end

  if byte == 0xED then return second <= 0x9F end

  if byte == 0xEE then return true end

  if byte == 0xEF then
    local third = string.byte(text, at + 2) or 0
    if second == 0xBB then return third ~= 0xBF end

    if second == 0xBF then return third ~= 0xBE and third ~= 0xBF end

    return true
  end

  return byte >= 0xF0 and byte <= 0xF4
end

local FirstIndicators = table.freeze {
  [35] = true,
  [44] = true,
  [91] = true,
  [93] = true,
  [123] = true,
  [125] = true,
  [38] = true,
  [42] = true,
  [33] = true,
  [124] = true,
  [62] = true,
  [39] = true,
  [34] = true,
  [37] = true,
  [64] = true,
  [96] = true,
}

local FlowIndicators =
  table.freeze { [44] = true, [63] = true, [91] = true, [93] = true, [123] = true, [125] = true }

--- `yaml_emitter_analyze_scalar`.
local function analyze(text: string): Analysis
  local length = utf8.len(text) or #text
  if text == '' then
    return {
      multiline = false,
      flowPlain = false,
      blockPlain = true,
      singleQuoted = true,
      block = false,
      length = 0,
    }
  end

  local blockIndicators, flowIndicators = false, false
  local lineBreaks, special = false, false
  local leadingSpace, leadingBreak, trailingSpace, trailingBreak = false, false, false, false
  local breakSpace, spaceBreak = false, false
  local previousSpace, previousBreak = false, false
  local prefix = string.sub(text, 1, 3)
  if prefix == '---' or prefix == '...' then
    blockIndicators, flowIndicators = true, true
  end

  local precededByWhitespace = true
  local at = 1
  local followedByWhitespace = isBlankzAt(text, at + widthAt(text, at))
  while at <= #text do
    local byte = string.byte(text, at) :: number
    if at == 1 then
      if FirstIndicators[byte] then
        flowIndicators, blockIndicators = true, true
      end

      if byte == 63 or byte == 58 then
        flowIndicators = true
        if followedByWhitespace then blockIndicators = true end
      end

      if byte == 45 and followedByWhitespace then
        flowIndicators, blockIndicators = true, true
      end
    else
      if FlowIndicators[byte] then flowIndicators = true end

      if byte == 58 then
        flowIndicators = true
        if followedByWhitespace then blockIndicators = true end
      end

      if byte == 35 and precededByWhitespace then
        flowIndicators, blockIndicators = true, true
      end
    end

    if not isPrintableAt(text, at) then special = true end

    local width = widthAt(text, at)
    if isBreakAt(text, at) then lineBreaks = true end

    if byte == 32 then
      if at == 1 then leadingSpace = true end

      if at + width - 1 == #text then trailingSpace = true end

      if previousBreak then breakSpace = true end

      previousSpace, previousBreak = true, false
    elseif isBreakAt(text, at) then
      if at == 1 then leadingBreak = true end

      if at + width - 1 == #text then trailingBreak = true end

      if previousSpace then spaceBreak = true end

      previousSpace, previousBreak = false, true
    else
      previousSpace, previousBreak = false, false
    end

    precededByWhitespace = isBlankzAt(text, at)
    at += math.max(width, 1)
    if at <= #text then followedByWhitespace = isBlankzAt(text, at + widthAt(text, at)) end
  end

  local result = {
    multiline = lineBreaks,
    flowPlain = true,
    blockPlain = true,
    singleQuoted = true,
    block = true,
    length = length,
  }

  if leadingSpace or leadingBreak or trailingSpace or trailingBreak then
    result.flowPlain, result.blockPlain = false, false
  end

  if trailingSpace then result.block = false end

  if breakSpace then
    result.flowPlain, result.blockPlain, result.singleQuoted = false, false, false
  end

  if spaceBreak or special then
    result.flowPlain, result.blockPlain, result.singleQuoted, result.block =
      false, false, false, false
  end

  if lineBreaks then
    result.flowPlain, result.blockPlain = false, false
  end

  if flowIndicators then result.flowPlain = false end

  if blockIndicators then result.blockPlain = false end

  return result
end

--- An emitter writing libyaml's block style, tracking its column and whitespace state.
type Emitter = {
  out: { string },
  column: number,
  indent: number,
  indents: { number },
  whitespace: boolean,
  indention: boolean,
  mappingContext: boolean,
  simpleKey: boolean,
  flow: number,
}

local function put(emitter: Emitter, text: string)
  table.insert(emitter.out, text)
  emitter.column += utf8.len(text) or #text
end

local function putBreak(emitter: Emitter)
  table.insert(emitter.out, '\n')
  emitter.column = 0
end

local function writeIndent(emitter: Emitter)
  local indent = math.max(emitter.indent, 0)
  if
    not emitter.indention
    or emitter.column > indent
    or (emitter.column == indent and not emitter.whitespace)
  then
    putBreak(emitter)
  end

  if emitter.column < indent then put(emitter, string.rep(' ', indent - emitter.column)) end

  emitter.whitespace = true
  emitter.indention = true
end

local function indicator(
  emitter: Emitter,
  text: string,
  needWhitespace: boolean,
  isWhitespace: boolean,
  isIndention: boolean
)
  if needWhitespace and not emitter.whitespace then put(emitter, ' ') end

  put(emitter, text)
  emitter.whitespace = isWhitespace
  emitter.indention = emitter.indention and isIndention
end

local function increaseIndent(emitter: Emitter, flow: boolean, indentless: boolean)
  table.insert(emitter.indents, emitter.indent)
  if emitter.indent < 0 then
    emitter.indent = if flow then 2 else 0
  elseif not indentless then
    emitter.indent += 2
  end
end

local function popIndent(emitter: Emitter) emitter.indent = table.remove(emitter.indents) :: number end

--- Writes one character (or a break) of a scalar, as libyaml's WRITE and WRITE_BREAK do.
local function writeChar(emitter: Emitter, text: string, at: number): number
  local width = math.max(widthAt(text, at), 1)
  if string.byte(text, at) == 10 then
    putBreak(emitter)
  elseif isBreakAt(text, at) then
    table.insert(emitter.out, string.sub(text, at, at + width - 1))
    emitter.column = 0
  else
    put(emitter, string.sub(text, at, at + width - 1))
  end

  return at + width
end

local function writePlain(emitter: Emitter, text: string)
  if not emitter.whitespace and (#text ~= 0 or emitter.flow ~= 0) then put(emitter, ' ') end

  local breaks = false
  local at = 1
  while at <= #text do
    if string.byte(text, at) == 32 then
      at = writeChar(emitter, text, at)
    elseif isBreakAt(text, at) then
      if not breaks and string.byte(text, at) == 10 then putBreak(emitter) end

      at = writeChar(emitter, text, at)
      emitter.indention = true
      breaks = true
    else
      if breaks then writeIndent(emitter) end

      at = writeChar(emitter, text, at)
      emitter.indention = false
      breaks = false
    end
  end

  emitter.whitespace = false
  emitter.indention = false
end

local function writeSingleQuoted(emitter: Emitter, text: string)
  indicator(emitter, '\'', true, false, false)
  local breaks = false
  local at = 1
  while at <= #text do
    if string.byte(text, at) == 32 then
      at = writeChar(emitter, text, at)
    elseif isBreakAt(text, at) then
      if not breaks and string.byte(text, at) == 10 then putBreak(emitter) end

      at = writeChar(emitter, text, at)
      emitter.indention = true
      breaks = true
    else
      if breaks then writeIndent(emitter) end

      if string.byte(text, at) == 39 then put(emitter, '\'') end

      at = writeChar(emitter, text, at)
      emitter.indention = false
      breaks = false
    end
  end

  if breaks then writeIndent(emitter) end

  indicator(emitter, '\'', false, false, false)
  emitter.whitespace = false
  emitter.indention = false
end

local DoubleEscapes = table.freeze {
  [0x00] = '0',
  [0x07] = 'a',
  [0x08] = 'b',
  [0x09] = 't',
  [0x0A] = 'n',
  [0x0B] = 'v',
  [0x0C] = 'f',
  [0x0D] = 'r',
  [0x1B] = 'e',
  [0x22] = '"',
  [0x5C] = '\\',
  [0x85] = 'N',
  [0xA0] = '_',
  [0x2028] = 'L',
  [0x2029] = 'P',
}

local function writeDoubleQuoted(emitter: Emitter, text: string)
  indicator(emitter, '"', true, false, false)
  local at = 1
  while at <= #text do
    local byte = string.byte(text, at) :: number
    local isBom = string.sub(text, at, at + 2) == '\u{feff}'
    if not isPrintableAt(text, at) or isBom or isBreakAt(text, at) or byte == 34 or byte == 92 then
      local width = math.max(widthAt(text, at), 1)
      local code = utf8.codepoint(text, at) or byte
      at += width
      local short = DoubleEscapes[code]
      if short then
        put(emitter, '\\' .. short)
      elseif code <= 0xFF then
        put(emitter, string.format('\\x%02X', code))
      elseif code <= 0xFFFF then
        put(emitter, string.format('\\u%04X', code))
      else
        put(emitter, string.format('\\U%08X', code))
      end
    else
      at = writeChar(emitter, text, at)
    end
  end

  indicator(emitter, '"', false, false, false)
  emitter.whitespace = false
  emitter.indention = false
end

local function writeLiteral(emitter: Emitter, text: string)
  indicator(emitter, '|', true, false, false)
  local first = string.byte(text, 1)
  if first == 32 or isBreakAt(text, 1) then indicator(emitter, '2', false, false, false) end

  -- The chomping hint: `-` without a final break, `+` with more than one (or only one).
  local last = #text
  while last > 1 and bit32.band(string.byte(text, last), 0xC0) == 0x80 do
    last -= 1
  end

  local chomp: string?
  if not isBreakAt(text, last) then
    chomp = '-'
  elseif last == 1 then
    chomp = '+'
  else
    local before = last - 1
    while before > 1 and bit32.band(string.byte(text, before), 0xC0) == 0x80 do
      before -= 1
    end

    if isBreakAt(text, before) then chomp = '+' end
  end

  if chomp then indicator(emitter, chomp, false, false, false) end

  putBreak(emitter)
  emitter.indention = true
  emitter.whitespace = true
  local breaks = true
  local at = 1
  while at <= #text do
    if isBreakAt(text, at) then
      at = writeChar(emitter, text, at)
      emitter.indention = true
      breaks = true
    else
      if breaks then writeIndent(emitter) end

      at = writeChar(emitter, text, at)
      emitter.indention = false
      breaks = false
    end
  end
end

--- A scalar's text and the style serde_yaml asks for: `plain` for numbers, booleans and null;
--- `literal` for strings with a newline; `single` for strings that would read as something else;
--- `any` for the rest, which libyaml then picks for.
local function scalarOf(value: Value): (string, string)
  local kind = kindOf(value)
  if kind == 'null' or kind == 'none' then return 'null', 'plain' end

  if kind == 'boolean' then return if value then 'true' else 'false', 'plain' end

  if kind == 'int' then return intText(value), 'plain' end

  if kind == 'float' then
    local number = floatOf(value)
    if number ~= number then return '.nan', 'plain' end

    if number == math.huge then return '.inf', 'plain' end

    if number == -math.huge then return '-.inf', 'plain' end

    return shortest(number, false), 'plain'
  end

  local text = value :: string
  if string.find(text, '\n', 1, true) then return text, 'literal' end

  if resolvesAsNonString(text) or digitsButNotNumber(text) then return text, 'single' end

  return text, 'any'
end

--- `yaml_emitter_select_scalar_style`, then the scalar written.
local function emitScalar(emitter: Emitter, text: string, requested: string)
  local analysis = analyze(text)
  local style = if requested == 'any' then 'plain' else requested
  if emitter.simpleKey and analysis.multiline then style = 'double' end

  if style == 'plain' then
    if
      (emitter.flow ~= 0 and not analysis.flowPlain)
      or (emitter.flow == 0 and not analysis.blockPlain)
    then
      style = 'single'
    end

    if analysis.length == 0 and (emitter.flow ~= 0 or emitter.simpleKey) then style = 'single' end
  end

  if style == 'single' and not analysis.singleQuoted then style = 'double' end

  if style == 'literal' and (not analysis.block or emitter.flow ~= 0 or emitter.simpleKey) then
    style = 'double'
  end

  increaseIndent(emitter, true, false)
  if style == 'plain' then
    writePlain(emitter, text)
  elseif style == 'single' then
    writeSingleQuoted(emitter, text)
  elseif style == 'double' then
    writeDoubleQuoted(emitter, text)
  else
    writeLiteral(emitter, text)
  end

  popIndent(emitter)
end

local function isEmptyCollection(value: Value): boolean
  local kind = kindOf(value)
  if kind == 'array' then return #(value :: { Value }) == 0 end

  if kind == 'map' then return #(value :: Map).keys == 0 end

  return false
end

local emitNode: (Emitter, Value, boolean, boolean, boolean) -> ()

--- Whether a key can be written plainly before its `:`, as `check_simple_key` decides.
local function simpleKey(text: string, requested: string): boolean
  if requested == 'literal' then return false end

  local analysis = analyze(text)
  return not analysis.multiline and analysis.length <= 128
end

local function writeTag(emitter: Emitter, tag: string)
  if not emitter.whitespace then put(emitter, ' ') end

  put(emitter, '!')
  local escaped = string.gsub(
    string.sub(tag, 2),
    '[^%w;/%?:@&=%+%$,_%.~%*\'%(%)%[%]%-]',
    function(char: string): string return string.format('%%%02X', string.byte(char)) end
  )
  put(emitter, escaped)
  emitter.whitespace = false
  emitter.indention = false
end

local function emitCollection(emitter: Emitter, value: Value, tag: string?)
  if tag then writeTag(emitter, tag) end

  local kind = kindOf(value)
  if isEmptyCollection(value) then
    indicator(emitter, if kind == 'array' then '[' else '{', true, true, false)
    indicator(emitter, if kind == 'array' then ']' else '}', false, false, false)
    return
  end

  if kind == 'array' then
    increaseIndent(emitter, false, emitter.mappingContext and not emitter.indention)
    for _, item in value :: { Value } do
      writeIndent(emitter)
      indicator(emitter, '-', true, false, true)
      emitNode(emitter, item, false, true, false)
    end

    popIndent(emitter)
    return
  end

  local target = value :: Map
  increaseIndent(emitter, false, false)
  for _, key in target.keys do
    writeIndent(emitter)
    local keyText, keyStyle = scalarOf(key)
    if simpleKey(keyText, keyStyle) then
      emitter.simpleKey = true
      emitter.mappingContext = true
      emitScalar(emitter, keyText, keyStyle)
      emitter.simpleKey = false
      indicator(emitter, ':', false, false, false)
    else
      indicator(emitter, '?', true, false, true)
      emitNode(emitter, key, false, false, true)
      writeIndent(emitter)
      indicator(emitter, ':', true, false, true)
    end

    emitNode(emitter, target.values[key], false, false, true)
  end

  popIndent(emitter)
end

function emitNode(
  emitter: Emitter,
  value: Value,
  _root: boolean,
  _sequence: boolean,
  mapping: boolean
)
  emitter.mappingContext = mapping
  emitter.simpleKey = false
  local kind = kindOf(value)
  if kind == 'variant' then
    local tagged = value :: { tag: string, value: Value }
    local inner = tagged.value
    local tag = '!' .. string.gsub(tagged.tag, '^!', '')
    local innerKind = kindOf(inner)
    if innerKind == 'array' or innerKind == 'map' then
      emitCollection(emitter, inner, tag)
    else
      writeTag(emitter, tag)
      local text, style = scalarOf(inner)
      emitScalar(emitter, text, style)
    end

    return
  end

  if kind == 'array' or kind == 'map' then
    emitCollection(emitter, value, nil)
    return
  end

  local text, style = scalarOf(value)
  emitScalar(emitter, text, style)
end

local function yaml(value: Value): string
  local emitter: Emitter = {
    out = {},
    column = 0,
    indent = -1,
    indents = {},
    whitespace = true,
    indention = true,
    mappingContext = false,
    simpleKey = false,
    flow = 0,
  }
  emitNode(emitter, value, true, false, false)
  -- The document's end: a break unless the last line is already empty.
  writeIndent(emitter)
  local text = table.concat(emitter.out)
  if string.sub(text, -1) ~= '\n' then
    text ..= '\n'
  end

  return text
end

-- TOML -------------------------------------------------------------------------------------------------

local function tomlFail(message: string): never error(`dream.vfs: {message}`, 0) end

local function stringMetrics(text: string)
  local maxSingle, maxDouble, single, double = 0, 0, 0, 0
  local escapeCodes, escape, newline = false, false, false
  for index = 1, #text do
    local byte = string.byte(text, index)
    if byte == 39 then
      single += 1
      maxSingle = math.max(maxSingle, single)
    else
      single = 0
    end

    if byte == 34 then
      double += 1
      maxDouble = math.max(maxDouble, double)
    else
      double = 0
    end

    if byte == 92 then
      escape = true
    elseif byte == 9 then
      -- A tab is always allowed.
    elseif byte == 10 then
      newline = true
    elseif byte <= 0x1F or byte == 0x7F then
      escapeCodes = true
    end
  end

  return maxSingle, maxDouble, escapeCodes, escape, newline
end

local TomlEscapes = table.freeze {
  [8] = '\\b',
  [9] = '\\t',
  [10] = '\\n',
  [12] = '\\f',
  [13] = '\\r',
  [92] = '\\\\',
}

--- `write_toml_value` for an escaped (basic) string body.
local function tomlEscaped(text: string, multiline: boolean): string
  local out = {}
  local quotes = 0
  local limit = if multiline then 2 else 0
  for index = 1, #text do
    local byte = string.byte(text, index)
    if byte == 34 then
      quotes += 1
      if quotes > limit then
        table.insert(out, '\\"')
        quotes = 0
        continue
      end
    else
      quotes = 0
    end

    if byte == 10 and multiline then
      table.insert(out, '\n')
    elseif TomlEscapes[byte] then
      table.insert(out, TomlEscapes[byte])
    elseif byte <= 0x1F or byte == 0x7F then
      table.insert(out, string.format('\\u%04X', byte))
    else
      table.insert(out, string.char(byte))
    end
  end

  return table.concat(out)
end

local function tomlString(text: string): string
  local maxSingle, maxDouble, escapeCodes, escape, newline = stringMetrics(text)
  if not escapeCodes and not escape and maxDouble == 0 and not newline then
    return `"{tomlEscaped(text, false)}"`
  end

  if not escapeCodes and maxSingle == 0 and not newline then return `'{text}'` end

  local prefix = if newline then '\n' else ''
  if not escapeCodes and not escape and maxDouble <= 2 then
    return `"""{prefix}{tomlEscaped(text, true)}"""`
  end

  if not escapeCodes and maxSingle <= 2 then return `'''{prefix}{text}'''` end

  if newline then return `"""{prefix}{tomlEscaped(text, true)}"""` end

  return `"{tomlEscaped(text, false)}"`
end

local function tomlKey(key: string): string
  if key ~= '' and string.find(key, '^[%w_%-]+$') then return key end

  local escapeCodes, escape, single, double = false, false, false, false
  for index = 1, #key do
    local byte = string.byte(key, index)
    if byte == 39 then
      single = true
    elseif byte == 34 then
      double = true
    elseif byte == 92 then
      escape = true
    elseif byte ~= 9 and (byte <= 0x1F or byte == 0x7F) then
      escapeCodes = true
    end
  end

  if not escapeCodes and not escape and not double then return `"{tomlEscaped(key, false)}"` end

  if not escapeCodes and not single then return `'{key}'` end

  return `"{tomlEscaped(key, false)}"`
end

--- What the toml crate does with a value in a table: a key-value line, a sub-table, an array of
--- tables, or nothing (an absent option).
local function strategy(value: Value): string
  local kind = kindOf(value)
  if kind == 'none' then return 'skip' end

  if kind == 'map' or kind == 'variant' then return 'table' end

  if kind == 'array' then
    local items = value :: { Value }
    if #items == 0 then return 'value' end

    for _, item in items do
      if strategy(item) ~= 'table' then return 'value' end
    end

    return 'arrayOfTables'
  end

  return 'value'
end

local function asMap(value: Value): Map
  if kindOf(value) == 'variant' then
    local tagged = value :: { tag: string, value: Value }
    local wrapped = map()
    set(wrapped, tagged.tag, tagged.value)
    return wrapped
  end

  return value :: Map
end

local function tomlFloat(number: number): string
  if number ~= number then return 'nan' end

  if number == 0 then return if 1 / number < 0 then '-0.0' else '0.0' end

  local text = display(number)
  if number % 1 == 0 and math.abs(number) ~= math.huge then return text .. '.0' end

  return text
end

local function tomlValue(value: Value, out: { string })
  local kind = kindOf(value)
  if kind == 'null' then tomlFail 'unsupported unit type' end

  if kind == 'none' then tomlFail 'unsupported None value' end

  if kind == 'boolean' then
    table.insert(out, if value then 'true' else 'false')
  elseif kind == 'int' then
    table.insert(out, intText(value))
  elseif kind == 'float' then
    table.insert(out, tomlFloat(floatOf(value)))
  elseif kind == 'string' then
    table.insert(out, tomlString(value :: string))
  elseif kind == 'array' then
    local items = value :: { Value }
    local multiline = #items >= 2
    table.insert(out, '[')
    for index, item in items do
      if multiline then
        table.insert(out, '\n    ')
      elseif index > 1 then
        table.insert(out, ', ')
      end

      tomlValue(item, out)
      if multiline then table.insert(out, ',') end
    end

    if multiline then table.insert(out, '\n') end

    table.insert(out, ']')
  else
    local target = asMap(value)
    local parts = {}
    for _, key in target.keys do
      local item = target.values[key]
      if kindOf(item) == 'none' then continue end

      local encoded = {}
      tomlValue(item, encoded)
      table.insert(parts, `{tomlKey(key)} = {table.concat(encoded)}`)
    end

    table.insert(out, if #parts == 0 then '{}' else `\{ {table.concat(parts, ', ')} \}`)
  end
end

type TomlTable = { key: { string }?, body: { string }, hasChildren: boolean, array: boolean }

--- The document serializer: a table's values go in its body, its sub-tables and arrays of tables
--- become tables of their own, listed in the order they were made.
local function tomlTable(tables: { TomlTable }, current: TomlTable, value: Map)
  for _, key in value.keys do
    local item = value.values[key]
    local plan = strategy(item)
    if plan == 'skip' then continue end

    local encodedKey = tomlKey(key)
    if plan == 'value' then
      local encoded = {}
      tomlValue(item, encoded)
      table.insert(current.body, `{encodedKey} = {table.concat(encoded)}\n`)
    elseif plan == 'arrayOfTables' then
      current.hasChildren = true
      for _, element in item :: { Value } do
        local path = table.clone(current.key or {})
        table.insert(path, encodedKey)
        local child: TomlTable = { key = path, body = {}, hasChildren = false, array = true }
        table.insert(tables, child)
        tomlTable(tables, child, asMap(element))
      end
    else
      current.hasChildren = true
      local path = table.clone(current.key or {})
      table.insert(path, encodedKey)
      local child: TomlTable = { key = path, body = {}, hasChildren = false, array = false }
      table.insert(tables, child)
      tomlTable(tables, child, asMap(item))
    end
  end
end

local function toml(value: Value): string
  local kind = kindOf(value)
  if kind ~= 'map' and kind ~= 'variant' then
    if kind == 'none' then tomlFail 'unsupported None value' end

    -- serde_json keeps a non-negative integer as a `u64`, a negative one as an `i64`.
    local names =
      { null = 'unit', boolean = 'bool', float = 'f64', string = 'str', array = 'array' }
    local named = names[kind]
    if kind == 'int' then
      local number = if typeof(value) == 'integer'
        then integer.tonumber(value :: integer)
        else value :: number
      named = if number < 0 then 'i64' else 'u64'
    end

    tomlFail(`unsupported {named} type`)
  end

  local root: TomlTable = { key = nil, body = {}, hasChildren = false, array = false }
  local tables = { root }
  tomlTable(tables, root, asMap(value))
  local rendered = {}
  for _, entry in tables do
    local body = table.concat(entry.body)
    local required = if entry.key == nil
      then body ~= ''
      else entry.array or body ~= '' or not entry.hasChildren
    if not required then continue end

    local text = body
    if entry.key then
      local open = if entry.array then '[[' else '['
      local close = if entry.array then ']]' else ']'
      text = `{open}{table.concat(entry.key, '.')}{close}\n{body}`
    end

    table.insert(rendered, text)
  end

  return table.concat(rendered, '\n')
end

-- Entry points -----------------------------------------------------------------------------------------

--- `value` in `format`; raises the crate's message for an unknown format or what TOML can't hold.
local function encode(value: Value, format: string): string
  if format == 'json' then
    local out = {}
    json(value, out)
    return table.concat(out)
  end

  if format == 'yaml' then return yaml(value) end

  if format == 'toml' then return toml(value) end

  return error(
    `dream.vfs: unknown serialization format '{format}' (expected json, yaml, or toml)`,
    0
  )
end

return table.freeze {
  Null = Null,
  None = None,
  array = array,
  map = map,
  set = set,
  struct = struct,
  variant = variant,
  float = float,
  kindOf = kindOf,
  encode = encode,
  shortest = shortest,
  display = display,
}
