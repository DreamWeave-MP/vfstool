--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- `VfsFile`: a loose file on disk or an entry inside an archive, and `Reader`, the cursor a
-- script parses one through. A file's positional reads copy from a backing it keeps after the
-- first one: a memory map of a loose file (`@dream/fs`'s reader), or the decoded bytes of an
-- archive entry, so a sequential parse allocates nothing per call.
local check = require './check'
local fs = require '@dream/fs'
local paths = require './paths'
local types = require './types'

type Entry = types.Entry

--- Where a file's bytes come from once it has been read positionally.
type Backing = {
  size: number,
  map: dream_fs_Reader?,
  bytes: buffer?,
}

export type VfsFile = {
  isLoose: (self: VfsFile) -> boolean,
  isArchive: (self: VfsFile) -> boolean,
  path: (self: VfsFile) -> string,
  fileName: (self: VfsFile) -> string?,
  fileStem: (self: VfsFile) -> string?,
  parentArchivePath: (self: VfsFile) -> string?,
  parentArchiveName: (self: VfsFile) -> string?,
  size: (self: VfsFile) -> number,
  readAll: (self: VfsFile) -> string,
  readAllBuffer: (self: VfsFile) -> buffer,
  readInto: (self: VfsFile, target: buffer, offset: number?) -> number,
  readAt: (
    self: VfsFile,
    target: buffer,
    fileOffset: number,
    length: number?,
    bufferOffset: number?
  ) -> number,
  readRange: (self: VfsFile, fileOffset: number, length: number) -> buffer,
  open: (self: VfsFile) -> Reader,
}

export type Reader = {
  read: (self: Reader, length: number) -> buffer,
  readInto: (self: Reader, target: buffer, bufferOffset: number?, length: number?) -> number,
  seek: (self: Reader, position: number) -> (),
  skip: (self: Reader, count: number) -> (),
  tell: (self: Reader) -> number,
  size: (self: Reader) -> number,
  close: (self: Reader) -> (),
}

type FileState = { entry: Entry, backing: Backing? }
type ReaderState = { backing: Backing?, size: number, position: number, name: string }

local files = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [VfsFile]: FileState }
local readers = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [Reader]: ReaderState }

local function fileState(file: unknown, method: string): FileState
  local state = files[file :: VfsFile]
  if not state then check.receiver(check.method('VfsFile', method), 'dream.vfs.VfsFile', file) end

  return state
end

local function readerState(reader: unknown, method: string): ReaderState
  local state = readers[reader :: Reader]
  if not state then check.receiver(check.method('Reader', method), 'dream.vfs.Reader', reader) end

  return state
end

local function isArchive(entry: Entry): boolean return entry.archive ~= nil end

--- An archive entry's last component, as the crate's `last_component` finds it in the lossy name.
local function lastComponent(entry: Entry): string?
  local name = paths.lossy(entry.path)
  local last = string.match(name, '[^/\\]*$') or name
  if last == '' or last == '.' or last == '..' then return nil end

  return last
end

-- Reading ------------------------------------------------------------------------------------------

local function readArchived(handle: types.Archive, index: number): buffer
  local described = handle:entry(index)
  local known = described and described.size
  if described and known then
    local out = buffer.create(known)
    handle:readInto(described, out, 0)
    return out
  end

  return buffer.fromstring(handle:readEntry(index))
end

--- The decoded bytes of an archive entry, or nil and why it can't be read.
local function decodeEntry(entry: Entry): (buffer?, string?)
  local ref = entry.archive :: types.ArchiveRef
  -- @dream-archive raises for an entry it can't read: corrupt data, or a compression it lacks.
  local ok, result = pcall(readArchived, ref.handle, entry.index :: number)
  if not ok then return nil, tostring(result) end

  return result :: buffer, nil
end

--- The decoded bytes of an archive entry; one that can't be read raises as the crate's
--- `InvalidData` printed it.
local function decode(entry: Entry): buffer
  local bytes, problem = decodeEntry(entry)
  if not bytes then return check.vfs(problem :: string) end

  return bytes
end

local function openBacking(entry: Entry): Backing
  if isArchive(entry) then
    local bytes = decode(entry)
    return { size = buffer.len(bytes), bytes = bytes }
  end

  local map = check.answer(fs.open(entry.path))
  return { size = map:size(), map = map }
end

local function backingOf(state: FileState): Backing
  local backing = state.backing
  if backing then return backing end

  local opened = openBacking(state.entry)
  state.backing = opened
  return opened
end

--- Copies up to `length` bytes from `offset` of the backing into `target` at `targetOffset`.
local function copyFrom(
  backing: Backing,
  target: buffer,
  targetOffset: number,
  offset: number,
  length: number
): number
  local count = math.min(length, backing.size - offset)
  if count <= 0 then return 0 end

  local bytes = backing.bytes
  if bytes then
    buffer.copy(target, targetOffset, bytes, offset, count)
    return count
  end

  return check.answer(
    (backing.map :: dream_fs_Reader):readAtInto(target, offset, count, targetOffset)
  )
end

local function knownSize(entry: Entry): number?
  if isArchive(entry) then
    local ref = entry.archive :: types.ArchiveRef
    local described = ref.handle:entry(entry.index :: number)
    return described and described.size
  end

  local stat, message = fs.stat(entry.path)
  if stat then return stat.size end

  if message then return check.io(message) end

  -- Nothing there: the error the crate's metadata call raised, as the host words it.
  local _, missing = fs.readAt(entry.path, 0, 0)
  return check.io(missing)
end

local function sizeOf(state: FileState): number
  local backing = state.backing
  if backing then return backing.size end

  return knownSize(state.entry) or backingOf(state).size
end

-- Argument windows, each naming the argument it refuses -------------------------------------------

local function nonNegative(what: string, name: string, value: number): number
  if value < 0 then check.vfs(`{what}: {name} {value} is negative`) end

  return value
end

local function withinFile(what: string, name: string, offset: number, size: number)
  if offset > size then check.vfs(`{what}: {name} {offset} past the end (size {size})`) end
end

--- The window `(offset, length)` of `len` bytes, with the defaults (0, the space left).
local function window(
  what: string,
  offsetName: string,
  noun: string,
  len: number,
  offset: number?,
  length: number?
): (number, number)
  local at = if offset then nonNegative(what, offsetName, offset) else 0
  if at > len then
    check.vfs(`{what}: {offsetName} {at} past the end of the {noun} (size {len})`)
  end

  local space = len - at
  local count = if length then nonNegative(what, 'length', length) else space
  if count > space then
    check.vfs(
      `{what}: length {count} does not fit the {noun} (space {space} after {offsetName} {at})`
    )
  end

  return at, count
end

-- Reader --------------------------------------------------------------------------------------------

local ReaderMethods = {}
local ReaderMeta = {
  __index = ReaderMethods,
  __tostring = function(reader: Reader): string
    return `dream.vfs.Reader({paths.lossy(readers[reader].name)})`
  end,
}

local function openedBacking(state: ReaderState, what: string): Backing
  local backing = state.backing
  if not backing then
    check.vfs(`Reader.{what}: the reader over {paths.lossy(state.name)} is closed`)
  end

  return backing :: Backing
end

local function newReader(backing: Backing, name: string): Reader
  local reader = (setmetatable({}, ReaderMeta) :: unknown) :: Reader
  readers[reader] = { backing = backing, size = backing.size, position = 0, name = name }
  return check.handle(reader)
end

function ReaderMethods.read(self: Reader, length: number): buffer
  local state = readerState(self, 'read')
  local count = check.exact(check.method('Reader', 'read'), length, 1, 2)
  nonNegative('Reader.read', 'length', count)
  local backing = openedBacking(state, 'read')
  local out = buffer.create(math.min(count, state.size - state.position))
  state.position += copyFrom(backing, out, 0, state.position, buffer.len(out))
  return out
end

function ReaderMethods.readInto(
  self: Reader,
  target: buffer,
  bufferOffset: number?,
  length: number?
): number
  local state = readerState(self, 'readInto')
  local what = check.method('Reader', 'readInto')
  local into = check.buffer(what, target, 1, 2)
  local at, count = window(
    'Reader.readInto',
    'bufferOffset',
    'buffer',
    buffer.len(into),
    check.optionalExact(what, bufferOffset, 2, 3),
    check.optionalExact(what, length, 3, 4)
  )
  local backing = openedBacking(state, 'readInto')
  local copied = copyFrom(backing, into, at, state.position, count)
  state.position += copied
  return copied
end

function ReaderMethods.seek(self: Reader, position: number)
  local state = readerState(self, 'seek')
  local to = check.exact(check.method('Reader', 'seek'), position, 1, 2)
  nonNegative('Reader.seek', 'position', to)
  withinFile('Reader.seek', 'position', to, state.size)
  openedBacking(state, 'seek')
  state.position = to
end

function ReaderMethods.skip(self: Reader, count: number)
  local state = readerState(self, 'skip')
  local by = check.exact(check.method('Reader', 'skip'), count, 1, 2)
  nonNegative('Reader.skip', 'count', by)
  openedBacking(state, 'skip')
  if by > state.size - state.position then
    check.vfs(`Reader.skip: count {by} past the end (position {state.position}, size {state.size})`)
  end

  state.position += by
end

function ReaderMethods.tell(self: Reader): number return readerState(self, 'tell').position end

function ReaderMethods.size(self: Reader): number return readerState(self, 'size').size end

function ReaderMethods.close(self: Reader) readerState(self, 'close').backing = nil end

-- VfsFile -------------------------------------------------------------------------------------------

local Methods = {}
local FileMeta = {
  __index = Methods,
  __tostring = function(file: VfsFile): string
    return `dream.vfs.VfsFile({paths.lossy(files[file].entry.path)})`
  end,
}

local function new(entry: Entry): VfsFile
  local file = (setmetatable({}, FileMeta) :: unknown) :: VfsFile
  files[file] = { entry = entry }
  return file
end

--- The entry a file wraps, or nil for a value that isn't a VfsFile.
local function entryOf(value: unknown): Entry?
  local state = files[value :: VfsFile]
  return state and state.entry
end

function Methods.isLoose(self: VfsFile): boolean
  return not isArchive(fileState(self, 'isLoose').entry)
end

function Methods.isArchive(self: VfsFile): boolean
  return isArchive(fileState(self, 'isArchive').entry)
end

function Methods.path(self: VfsFile): string return fileState(self, 'path').entry.path end

function Methods.fileName(self: VfsFile): string?
  local entry = fileState(self, 'fileName').entry
  if isArchive(entry) then return lastComponent(entry) end

  return paths.fileName(entry.path)
end

function Methods.fileStem(self: VfsFile): string?
  local entry = fileState(self, 'fileStem').entry
  if isArchive(entry) then
    local last = lastComponent(entry)
    return if last then paths.stemOf(last) else nil
  end

  return paths.fileStem(entry.path)
end

function Methods.parentArchivePath(self: VfsFile): string?
  local ref = fileState(self, 'parentArchivePath').entry.archive
  return if ref then paths.lossy(ref.path) else nil
end

function Methods.parentArchiveName(self: VfsFile): string?
  local ref = fileState(self, 'parentArchiveName').entry.archive
  if not ref then return nil end

  local name = paths.fileName(ref.path)
  return if name then paths.lossy(name) else nil
end

function Methods.size(self: VfsFile): number return sizeOf(fileState(self, 'size')) end

--- The whole file as a string, read afresh, as the crate's `readAll` reads it.
local function readWhole(entry: Entry): string
  if isArchive(entry) then return buffer.tostring(decode(entry)) end

  return check.answer(fs.readFileString(entry.path))
end

function Methods.readAll(self: VfsFile): string return readWhole(fileState(self, 'readAll').entry) end

function Methods.readAllBuffer(self: VfsFile): buffer
  local state = fileState(self, 'readAllBuffer')
  local size = sizeOf(state)
  local out = buffer.create(size)
  copyFrom(backingOf(state), out, 0, 0, size)
  return out
end

function Methods.readInto(self: VfsFile, target: buffer, offset: number?): number
  local state = fileState(self, 'readInto')
  local what = check.method('VfsFile', 'readInto')
  local into = check.buffer(what, target, 1, 2)
  local at = check.optionalExact(what, offset, 2, 3) or 0
  if at < 0 then check.vfs 'negative buffer offset' end

  local length = buffer.len(into)
  if at > length then check.fail 'buffer access out of bounds' end

  local entry = state.entry
  if isArchive(entry) then
    local bytes = decode(entry)
    local count = math.min(buffer.len(bytes), length - at)
    buffer.copy(into, at, bytes, 0, count)
    return count
  end

  local map = check.answer(fs.open(entry.path))
  local count = check.answer(map:readAtInto(into, 0, math.min(length - at, map:size()), at))
  map:close()
  return count
end

function Methods.readAt(
  self: VfsFile,
  target: buffer,
  fileOffset: number,
  length: number?,
  bufferOffset: number?
): number
  local state = fileState(self, 'readAt')
  local what = check.method('VfsFile', 'readAt')
  local into = check.buffer(what, target, 1, 2)
  local offset = check.exact(what, fileOffset, 2, 3)
  local wanted = check.optionalExact(what, length, 3, 4)
  local at, count = window(
    'readAt',
    'bufferOffset',
    'buffer',
    buffer.len(into),
    check.optionalExact(what, bufferOffset, 4, 5),
    wanted
  )
  nonNegative('readAt', 'fileOffset', offset)
  local size = sizeOf(state)
  withinFile('readAt', 'fileOffset', offset, size)
  return copyFrom(backingOf(state), into, at, offset, count)
end

function Methods.readRange(self: VfsFile, fileOffset: number, length: number): buffer
  local state = fileState(self, 'readRange')
  local what = check.method('VfsFile', 'readRange')
  local given, wanted = check.exact(what, fileOffset, 1, 2), check.exact(what, length, 2, 3)
  local offset = nonNegative('readRange', 'fileOffset', given)
  local count = nonNegative('readRange', 'length', wanted)
  local size = sizeOf(state)
  withinFile('readRange', 'fileOffset', offset, size)
  local out = buffer.create(math.min(count, size - offset))
  copyFrom(backingOf(state), out, 0, offset, buffer.len(out))
  return out
end

function Methods.open(self: VfsFile): Reader
  local state = fileState(self, 'open')
  return newReader(backingOf(state), state.entry.path)
end

--- A reader over a host file, for the `host` table.
local function openHost(path: string): Reader
  return newReader(openBacking { path = path, source = 0 }, path)
end

table.freeze(Methods)
table.freeze(ReaderMethods)

return table.freeze {
  new = new,
  entryOf = entryOf,
  readWhole = readWhole,
  decode = decode,
  decodeEntry = decodeEntry,
  openHost = openHost,
  lastComponent = lastComponent,
  knownSize = knownSize,
}
