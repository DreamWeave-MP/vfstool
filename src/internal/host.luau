--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- The `host` table: files and directories on any host path, with no VFS in between, over
-- `@dream/fs`; and `Writer`, the buffered writer `host.openWrite` and `vfs:openWrite` return.
local check = require './check'
local file = require './file'
local fs = require '@dream/fs'
local paths = require './paths'
local views = require './views'

export type Writer = {
  write: (self: Writer, data: buffer | string, offset: number?, length: number?) -> number,
  writeAt: (
    self: Writer,
    position: number,
    data: buffer | string,
    offset: number?,
    length: number?
  ) -> number,
  seek: (self: Writer, position: number) -> (),
  tell: (self: Writer) -> number,
  flush: (self: Writer) -> (),
  truncate: (self: Writer, length: number) -> (),
  close: (self: Writer) -> file.VfsFile?,
}

type WriterState = {
  handle: dream_fs_Writer?,
  append: boolean,
  name: string,
  --- What a VFS-bound writer does when it closes: makes its file the key's winner.
  register: (() -> file.VfsFile)?,
}

local writers = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [unknown]: WriterState }

local function writerOf(value: unknown, method: string): WriterState
  local state = writers[value]
  if not state then check.receiver(check.method('Writer', method), 'dream.vfs.Writer', value) end

  return state
end

local function opened(state: WriterState, what: string): dream_fs_Writer
  local handle = state.handle
  if not handle then
    check.vfs(`Writer.{what}: the writer over {paths.lossy(state.name)} is closed`)
  end

  return handle :: dream_fs_Writer
end

--- The bytes of `data` the `offset` and `length` arguments select, checked as the crate did.
local function slice(
  what: string,
  data: buffer | string,
  offset: number?,
  length: number?
): (buffer | string, number, number)
  local size = if type(data) == 'string' then #data else buffer.len(data)
  local at = offset or 0
  if at < 0 then check.vfs(`{what}: offset {at} is negative`) end

  if at > size then check.vfs(`{what}: offset {at} past the end of the data (size {size})`) end

  local space = size - at
  local count = length or space
  if count < 0 then check.vfs(`{what}: length {count} is negative`) end

  if count > space then
    check.vfs(`{what}: length {count} does not fit the data (space {space} after offset {at})`)
  end

  return data, at, count
end

local function bytesArgument(
  what: string,
  value: unknown,
  position: number,
  stack: number
): buffer | string
  if type(value) == 'string' or type(value) == 'buffer' then return value :: buffer | string end

  return check.argument(
    what,
    position,
    stack,
    'string or buffer',
    `expected buffer, got {typeof(value)}`
  )
end

local WriterMethods = {}
local WriterMeta = {
  __index = WriterMethods,
  __tostring = function(self: Writer): string
    return `dream.vfs.Writer({paths.lossy(writers[self].name)})`
  end,
}

function WriterMethods.write(
  self: Writer,
  data: buffer | string,
  offset: number?,
  length: number?
): number
  local state = writerOf(self, 'write')
  local what = check.method('Writer', 'write')
  local bytes = bytesArgument(what, data, 1, 2)
  local at, count = check.optionalExact(what, offset, 2, 3), check.optionalExact(what, length, 3, 4)
  local _, from, size = slice('Writer.write', bytes, at, count)
  local handle = opened(state, 'write')
  check.answer(handle:write(bytes, from, size))
  return size
end

function WriterMethods.writeAt(
  self: Writer,
  position: number,
  data: buffer | string,
  offset: number?,
  length: number?
): number
  local state = writerOf(self, 'writeAt')
  local what = check.method('Writer', 'writeAt')
  local to = check.exact(what, position, 1, 2)
  local bytes = bytesArgument(what, data, 2, 3)
  local at, count = check.optionalExact(what, offset, 3, 4), check.optionalExact(what, length, 4, 5)
  if to < 0 then check.vfs(`Writer.writeAt: position {to} is negative`) end

  local _, from, size = slice('Writer.writeAt', bytes, at, count)
  if state.append then
    check.vfs(
      `Writer.writeAt: the writer over {paths.lossy(state.name)} appends, so it cannot write at a position`
    )
  end

  local handle = opened(state, 'writeAt')
  check.answer(handle:writeAt(to, bytes, from, size))
  return size
end

function WriterMethods.seek(self: Writer, position: number)
  local state = writerOf(self, 'seek')
  local to = check.exact(check.method('Writer', 'seek'), position, 1, 2)
  if to < 0 then check.vfs(`Writer.seek: position {to} is negative`) end

  local handle = opened(state, 'seek')
  check.answer(handle:seek(to))
end

function WriterMethods.tell(self: Writer): number
  local state = writerOf(self, 'tell')
  local handle = state.handle
  return if handle then handle:tell() else 0
end

function WriterMethods.flush(self: Writer)
  local handle = opened(writerOf(self, 'flush'), 'flush')
  check.answer(handle:flush())
end

function WriterMethods.truncate(self: Writer, length: number)
  local state = writerOf(self, 'truncate')
  local to = check.exact(check.method('Writer', 'truncate'), length, 1, 2)
  if to < 0 then check.vfs(`Writer.truncate: length {to} is negative`) end

  local handle = opened(state, 'truncate')
  check.answer(handle:truncate(to))
end

function WriterMethods.close(self: Writer): file.VfsFile?
  local state = writerOf(self, 'close')
  local handle = state.handle
  if not handle then return nil end

  state.handle = nil
  check.answer(handle:close())
  local register = state.register
  return if register then register() else nil
end

table.freeze(WriterMethods)

--- `{ append?, truncate? }` for `context`.
local function openOptions(context: string, value: unknown): { append: boolean, truncate: boolean? }
  local given = check.options(context, value)
  local append = check.option(context, given, 'append', 'boolean') == true
  local truncate = check.option(context, given, 'truncate', 'boolean') :: boolean?
  check.finish(context, given, { 'append', 'truncate' })
  return { append = append, truncate = truncate }
end

--- A writer over the host file `path`, opened as `options` say; `register` runs on close.
local function newWriter(
  path: string,
  options: { append: boolean, truncate: boolean? },
  register: (() -> file.VfsFile)?
): Writer
  local handle =
    check.answer(fs.openWrite(path, { append = options.append, truncate = options.truncate }))
  local writer = (setmetatable({}, WriterMeta) :: unknown) :: Writer
  writers[writer] = { handle = handle, append = options.append, name = path, register = register }
  return check.handle(writer)
end

-- The host table ------------------------------------------------------------------------------------

local function name(function_: string): string return `dream.vfs.host.{function_}` end

local function recursive(context: string, value: unknown): boolean
  local given = check.options(context, value)
  local result = check.option(context, given, 'recursive', 'boolean') == true
  check.finish(context, given, { 'recursive' })
  return result
end

local function readFile(path: string): buffer
  local target = check.str(name 'readFile', path, 1, 1)
  return check.answer(fs.readFile(target))
end

local function readFileString(path: string): string
  local target = check.str(name 'readFileString', path, 1, 1)
  return check.answer(fs.readFileString(target))
end

local function readAt(path: string, offset: number, length: number): buffer
  local what = name 'readAt'
  local target = check.str(what, path, 1, 1)
  local at, count = check.exact(what, offset, 2, 2), check.exact(what, length, 3, 3)
  if at < 0 then check.vfs(`host.readAt: offset {at} is negative`) end

  if count < 0 then check.vfs(`host.readAt: length {count} is negative`) end

  local reader = check.answer(fs.open(target))
  local size = reader:size()
  if at > size then check.vfs(`host.readAt: offset {at} past the end (size {size})`) end

  local out = check.answer(reader:readAt(at, count))
  reader:close()
  return out
end

local function writeFile(path: string, data: buffer | string, options: unknown?): number
  local what = name 'writeFile'
  local target = check.str(what, path, 1, 1)
  local bytes = bytesArgument(what, data, 2, 2)
  local given = check.options('host.writeFile', options)
  local append = check.option('host.writeFile', given, 'append', 'boolean') == true
  check.finish('host.writeFile', given, { 'append' })
  return check.answer(fs.writeFile(target, bytes, { append = append }))
end

local function open(path: string): file.Reader
  return file.openHost(check.str(name 'open', path, 1, 1))
end

local function openWrite(path: string, options: unknown?): Writer
  local target = check.str(name 'openWrite', path, 1, 1)
  return newWriter(target, openOptions('host.openWrite', options), nil)
end

local function stat(path: string): {
  size: number,
  isFile: boolean,
  isDir: boolean,
  modified: number?,
  readonly: boolean,
}?
  local target = check.str(name 'stat', path, 1, 1)
  local found, message = fs.stat(target)
  if message then check.io(message) end

  if not found then return nil end

  return {
    size = found.size,
    isFile = found.isFile,
    isDir = found.isDir,
    modified = if found.modifiedSeconds then found.modified else nil,
    readonly = found.readonly,
  }
end

local function exists(path: string): boolean
  local target = check.str(name 'exists', path, 1, 1)
  return check.answer(fs.exists(target))
end

export type HostEntry = { path: string, isDir: boolean, size: number }

local function list(path: string, options: unknown?): views.View<HostEntry>
  local root = check.str(name 'list', path, 1, 1)
  local deep = recursive('host.list', options)
  if not deep then check.answer(fs.list(root)) end

  local found, message = fs.walk(root, {
    followLinks = true,
    metadata = true,
    maxDepth = if deep then nil else 1,
  })

  if not found then return check.vfs(check.walkError(message)) end

  local rows = (table.create(#found.paths) :: unknown) :: { HostEntry }
  for index, rel in found.paths do
    rows[index] = {
      path = paths.join(root, rel),
      isDir = found.kinds[index] == 'dir',
      size = if found.kinds[index] == 'dir' then 0 else (found.sizes :: { number })[index],
    }
  end

  table.sort(rows, function(a: HostEntry, b: HostEntry): boolean return a.path < b.path end)
  return views.lazy(#rows, function(index: number): HostEntry
    local row = rows[index]
    return { path = row.path, isDir = row.isDir, size = row.size }
  end)
end

local function mkdir(path: string, options: unknown?)
  local target = check.str(name 'mkdir', path, 1, 1)
  check.answer(fs.mkdir(target, { recursive = recursive('host.mkdir', options) }))
end

local function remove(path: string, options: unknown?)
  local target = check.str(name 'remove', path, 1, 1)
  check.answer(fs.remove(target, { recursive = recursive('host.remove', options) }))
end

local function rename(from: string, to: string)
  local what = name 'rename'
  local source, destination = check.str(what, from, 1, 1), check.str(what, to, 2, 2)
  check.answer(fs.rename(source, destination))
end

local function copy(from: string, to: string): number
  local what = name 'copy'
  local source, destination = check.str(what, from, 1, 1), check.str(what, to, 2, 2)
  return check.answer(fs.copy(source, destination))
end

local function canonicalize(path: string): string
  local target = check.str(name 'canonicalize', path, 1, 1)
  return check.answer(fs.canonicalize(target))
end

local Table = table.freeze {
  readFile = readFile,
  readFileString = readFileString,
  readAt = readAt,
  writeFile = writeFile,
  open = open,
  openWrite = openWrite,
  stat = stat,
  exists = exists,
  list = list,
  mkdir = mkdir,
  remove = remove,
  rename = rename,
  copy = copy,
  canonicalize = canonicalize,
}

return table.freeze {
  Table = Table,
  newWriter = newWriter,
  openOptions = openOptions,
  slice = slice,
}
