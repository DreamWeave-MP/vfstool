--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- The messages the native binding raises, word for word: argument and receiver checks, the
-- crate's own errors with their `dream.vfs: ` prefix, and the host's I/O errors as the crate
-- printed them, without `@dream/fs`'s function name and path.

local function fail(message: string): never return error(message, 0) end

--- `dream.vfs: <message>`.
local function vfs(message: string): never return fail(`dream.vfs: {message}`) end

--- The binder's message for an argument it couldn't convert: `what` is the function's debug name,
--- `position` the argument's place after any receiver, and `stack` its stack slot.
local function argument(
  what: string,
  position: number,
  stack: number,
  expected: string,
  problem: string
): never
  return fail(
    `{what}: bad argument #{position} (expected {expected}): Lua stack index {stack}: {problem}`
  )
end

--- The binder's message for a receiver that isn't one.
local function receiver(what: string, expected: string, value: unknown): never
  return fail(`invalid argument #1 to '{what}' ({expected} expected, got {typeof(value)})`)
end

local function method(class: string, name: string): string return `dream.vfs.{class}.{name}` end

local function str(what: string, value: unknown, position: number, stack: number): string
  if type(value) == 'string' then return value end

  return argument(what, position, stack, 'string', `expected string, got {typeof(value)}`)
end

--- A `&str` argument: a string that is UTF-8.
local function text(what: string, value: unknown, position: number, stack: number): string
  local found = str(what, value, position, stack)
  if not utf8.len(found) then
    fail(`{what}: bad argument #{position} (expected string): Lua string is not valid UTF-8`)
  end

  return found
end

--- The binder's message for a userdata argument of the wrong class.
local function userdata(
  what: string,
  position: number,
  stack: number,
  class: string,
  value: unknown
): never
  return fail(
    `{what}: bad argument #{position} (expected {class}): invalid argument #{stack} to '{what}' ({class} expected, got {typeof(
      value
    )})`
  )
end

local function boolean(what: string, value: unknown, position: number, stack: number): boolean
  if type(value) == 'boolean' then return value end

  return argument(what, position, stack, 'boolean', `expected boolean, got {typeof(value)}`)
end

local function optionalBoolean(
  what: string,
  value: unknown,
  position: number,
  stack: number
): boolean?
  if value == nil then return nil end

  return boolean(what, value, position, stack)
end

local function optionalString(
  what: string,
  value: unknown,
  position: number,
  stack: number
): string?
  if value == nil then return nil end

  return str(what, value, position, stack)
end

local function buf(what: string, value: unknown, position: number, stack: number): buffer
  if type(value) == 'buffer' then return value end

  return argument(what, position, stack, 'buffer', `expected buffer, got {typeof(value)}`)
end

--- An exact `i64`, as the binder's `Exact<i64>` reads one: a whole number or an integer.
local function exact(what: string, value: unknown, position: number, stack: number): number
  local number: number
  if type(value) == 'number' then
    number = value
  elseif typeof(value) == 'integer' then
    number = integer.tonumber(value :: integer)
  else
    return argument(what, position, stack, 'integer', `expected integer, got {typeof(value)}`)
  end

  if number ~= math.floor(number) or number < -2 ^ 63 or number >= 2 ^ 63 then
    return argument(what, position, stack, 'integer', `number {number} is not an exact i64`)
  end

  return number
end

local function optionalExact(what: string, value: unknown, position: number, stack: number): number?
  if value == nil then return nil end

  return exact(what, value, position, stack)
end

--- What a table argument read through `as_table` says when it isn't one.
local function tableArgument(name: string, value: unknown, stack: number): never
  return vfs(`{name}: Lua stack index {stack}: expected table, got {typeof(value)}`)
end

--- The host's I/O error as the crate printed it: the OS's words, without `@dream/fs`'s function
--- name and path in front of them.
local function osError(message: unknown): string
  local text = tostring(message)
  local os = string.match(text, '^dream%.fs%.[%w.]+: .-: ([^:]-%(os error %d+%))$')
  if os then return os end

  local fallback = string.match(text, '^dream%.fs%.[%w.]+: .*: ([^:]+)$')
  return fallback or text
end

--- Raises an I/O error the way the crate's `io_error` did.
local function io(message: unknown): never return vfs(osError(message)) end

--- What a `@dream/fs` call answered: its value, or, when the OS refused it (`nil` and a message),
--- the crate's `dream.vfs: <os error>` raised.
local function answer<T>(value: T?, message: string?): T
  if value == nil then return io(message) end

  return value
end

--- walkdir's own text for a traversal error, `IO error for operation on <path>: <os error>`,
--- out of `@dream/fs`'s message.
local function walkError(message: unknown): string
  local text = tostring(message)
  return string.match(text, '(IO error for operation on .*)$') or osError(text)
end

--- An option table, read the way l3i's `Options` reads one: nil when absent, its own error when it
--- isn't a table or has a key that isn't a string. `finish` refuses the keys nobody asked for.
local function options(context: string, value: unknown): { [string]: unknown }?
  if value == nil then return nil end

  if type(value) ~= 'table' then
    return fail(`{context}: options must be a table, got {typeof(value)}`)
  end

  for key in value :: { [unknown]: unknown } do
    if type(key) ~= 'string' then
      return fail(`{context}: option keys must be strings, got {typeof(key)}`)
    end
  end

  return value :: { [string]: unknown }
end

--- Refuses any key of `given` outside `known`, naming every known one, after the fields were read.
local function finish(context: string, given: { [string]: unknown }?, known: { string })
  if not given then return end

  for key in given do
    if not table.find(known, key) then
      local sorted = table.clone(known)
      table.sort(sorted)
      fail(`{context}: unknown option '{key}'; known options are {table.concat(sorted, ', ')}`)
    end
  end
end

--- An option's value of the expected Luau type, or nil; `expected` is the binder's type name.
local function option(
  context: string,
  given: { [string]: unknown }?,
  key: string,
  expected: string
): unknown
  if not given then return nil end

  local value = given[key]
  if value == nil then return nil end

  local kind = type(value)
  if expected == 'integer' then
    if kind == 'number' or typeof(value) == 'integer' then return value end
  elseif kind == expected then
    return value
  end

  return fail(`{context}.{key}: expected {expected}, got {typeof(value)}`)
end

--- A required string field of an option table.
local function requiredString(context: string, given: { [string]: unknown }, key: string): string
  local value = given[key]
  if value == nil then return fail(`{context}: missing required option '{key}'`) end

  if type(value) ~= 'string' then
    return fail(`{context}.{key}: expected string, got {typeof(value)}`)
  end

  return value
end

--- An array of strings, each checked, under `path` in messages.
--- `value` as strings, or the first one that isn't raised through `raise` (`fail` unless given).
local function stringArray(
  path: string,
  value: { unknown },
  raise: ((message: string) -> never)?
): { string }
  local out = (table.create(#value) :: unknown) :: { string }

  for index, item in value do
    if type(item) ~= 'string' then
      return (raise or fail)(`{path}[{index}]: expected string, got {typeof(item)}`)
    end

    out[index] = item
  end

  return out
end

--- Every value that is one of the module's handles (a VFS, an index, a reader...): what a native
--- binding would have made userdata, which `serialize` refuses.
local handles = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [unknown]: boolean }

local function handle<T>(value: T): T
  handles[value] = true
  return value
end

local function isHandle(value: unknown): boolean return handles[value] == true end

return table.freeze {
  handle = handle,
  isHandle = isHandle,
  fail = fail,
  vfs = vfs,
  argument = argument,
  receiver = receiver,
  method = method,
  str = str,
  text = text,
  userdata = userdata,
  boolean = boolean,
  optionalBoolean = optionalBoolean,
  optionalString = optionalString,
  buf = buf,
  exact = exact,
  optionalExact = optionalExact,
  tableArgument = tableArgument,
  osError = osError,
  io = io,
  answer = answer,
  walkError = walkError,
  options = options,
  finish = finish,
  option = option,
  requiredString = requiredString,
  stringArray = stringArray,
}
