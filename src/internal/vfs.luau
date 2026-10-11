--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- `VFS`: provider stacks low to high priority and the winner of every key. Built from data
-- directories the way OpenMW builds its own (later directories win, every loose file beats every
-- archive), or by hand; queried, mutated, reported on, and written out.
local archive = require '@dream-archive'
local check = require './check'
local conflict = require './conflict'
local encode = require './encode'
local file = require './file'
local fs = require '@dream/fs'
local glob = require './glob'
local host = require './host'
local keys = require './keys'
local layer = require './layer'
local materialize = require './materialize'
local paths = require './paths'
local registry = require './registry'
local serialize = require './serialize'
local state = require './state'
local tree = require './tree'
local types = require './types'
local views = require './views'

type State = types.State
type Entry = types.Entry
type Source = types.Source
type VfsFile = file.VfsFile

export type VfsProvider = {
  source: (self: VfsProvider) -> Source,
  file: (self: VfsProvider) -> VfsFile,
}

export type EntryRow = { key: string, file: VfsFile }
export type ProviderRow = { source: Source, file: VfsFile }
export type ProviderRecord = {
  sourceIndex: number,
  source: Source,
  key: string,
  originalPath: string,
  resolvedPath: string,
}
export type TreeRow = { dir: string, file: VfsFile }
export type ArchiveInfo = {
  sourceIndex: number,
  path: string,
  entryCount: number,
  winningEntryCount: number,
}
export type ArchiveEntry = { key: string, archivePath: string, originalPath: string, wins: boolean }
export type DuplicateEntry = { key: string, providers: { ProviderRecord }, winnerIndex: number }
export type DuplicateReport = { entries: { DuplicateEntry } }
export type Contribution = {
  sourceIndex: number,
  source: Source,
  winningFiles: number,
  overridingFiles: number,
  overriddenFiles: number,
  uniqueFiles: number,
  duplicateFiles: number,
  looseFiles: number,
  archiveFiles: number,
}
export type ContributionReport = { sources: { Contribution } }
export type MaterializationPlan = materialize.Plan
export type Tree = {
  count: (self: Tree) -> number,
  toTable: (self: Tree) -> { [string]: unknown },
}

--- What `diffDirectory` finds a folder would do to the VFS: the keys it would replace, the keys it
--- would add, and what in it couldn't be read.
export type DirectoryDiff = {
  conflicts: { { key: string, incoming: VfsFile, current: VfsFile } },
  additions: { { key: string, file: VfsFile } },
  unreadable: { { path: string, error: string } },
}

--- Every method a VFS has, in the order they are defined below.
export type VFS = {
  len: (self: VFS) -> number,
  revision: (self: VFS) -> number,
  isEmpty: (self: VFS) -> boolean,
  keys: (self: VFS, limit: number?, descending: boolean?) -> views.View<string>,
  entries: (self: VFS, limit: number?, descending: boolean?) -> views.View<EntryRow>,
  getFile: (self: VFS, path: string) -> VfsFile?,
  contains: (self: VFS, path: string) -> boolean,
  findByRegex: (self: VFS, pattern: string, relative: boolean?) -> Tree,
  remaining: (self: VFS, path: string, replacementsOnly: boolean, dirs: { string }, relative: boolean?) -> Tree,
  pathsMatching: (self: VFS, substring: string) -> views.View<EntryRow>,
  pathsStartingWith: (self: VFS, prefix: string, limit: number?, descending: boolean?) -> views.View<EntryRow>,
  keysStartingWith: (self: VFS, prefix: string, limit: number?, descending: boolean?) -> views.View<string>,
  pathsWith: (self: VFS, prefix: string, limit: number?, descending: boolean?) -> views.View<EntryRow>,
  setWinnerLooseFile: (self: VFS, key: string, physicalPath: string) -> VfsFile?,
  setWinnerFile: (self: VFS, key: string, value: VfsFile) -> VfsFile?,
  pushDirectory: (self: VFS, root: string) -> (),
  pushProvider: (self: VFS, key: string, provider: VfsProvider) -> boolean,
  pushArchive: (self: VFS, path: string) -> boolean,
  removeWinner: (self: VFS, key: string) -> VfsProvider?,
  removeResolvedFile: (self: VFS, key: string) -> VfsFile?,
  removeProviderPrefix: (self: VFS, prefix: string) -> (),
  removeResolvedPrefix: (self: VFS, prefix: string) -> (),
  removeProvider: (self: VFS, key: string, sourcePath: string) -> { ProviderRow },
  removeSource: (self: VFS, sourcePath: string) -> (),
  removeResolvedMatchingGlob: (self: VFS, pattern: string) -> (),
  tree: (self: VFS, relative: boolean?) -> Tree,
  display: (self: VFS, relative: boolean?) -> string,
  dumpToDirectory: (self: VFS, dir: string, useHardlinks: boolean) -> (number, { materialize.Skip }),
  collapseInto: (self: VFS, dest: string, options: unknown?) -> { materialize.Skip },
  extractFile: (self: VFS, path: string, target: string) -> string?,
  diffDirectory: (self: VFS, dir: string) -> DirectoryDiff,
  providerRecordsFor: (self: VFS, path: string) -> views.View<ProviderRecord>,
  explain: (self: VFS, path: string) -> { key: string, winner: ProviderRecord, overridden: { ProviderRecord } }?,
  providersFor: (self: VFS, path: string) -> views.View<ProviderRow>?,
  duplicates: (self: VFS, pattern: string?) -> DuplicateReport,
  archives: (self: VFS) -> { ArchiveInfo },
  archiveEntries: (self: VFS, archive: string) -> { ArchiveEntry },
  filesFromArchive: (self: VFS, archive: string) -> views.View<string>,
  sourceContributions: (self: VFS) -> ContributionReport,
  materializationPlan: (self: VFS, dest: string, options: unknown?) -> MaterializationPlan,
  layerIndex: (self: VFS) -> layer.LayerIndex,
  serializeTree: (self: VFS, relative: boolean?, format: string) -> string,
  writeRoot: (self: VFS) -> string?,
  setWriteRoot: (self: VFS, dir: string) -> (),
  writeFile: (self: VFS, key: string, data: buffer | string, options: unknown?) -> VfsFile,
  openWrite: (self: VFS, key: string, options: unknown?) -> host.Writer,
  mkdir: (self: VFS, key: string) -> (),
  remove: (self: VFS, key: string) -> (),
  rename: (self: VFS, from: string, to: string) -> (),
}

export type VFSClass = {
  new: () -> VFS,
  fromDirectories: (dirs: { string }, options: unknown?) -> VFS,
  fromDirectoriesWithConflictIndex: (dirs: { string }, options: unknown?) -> (VFS, conflict.ConflictIndex),
  fromDirectoriesWithLayerIndex: (dirs: { string }, options: unknown?) -> (VFS, layer.LayerIndex),
}

--- A VFS's tree stream: its rows, and the tree they came from.
local trees = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [unknown]: tree.Tree }

local function method(name: string): string return check.method('VFS', name) end

local function copySource(source: Source): Source return { path = source.path, kind = source.kind } end

-- Providers -------------------------------------------------------------------------------------------

type ProviderData = { source: Source, entry: Entry }

local providers = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [unknown]: ProviderData }

local ProviderMethods = {}
local ProviderMeta = {
  __index = ProviderMethods,
  __tostring = function(self: VfsProvider): string
    local data = providers[self]
    return `dream.vfs.VfsProvider({paths.lossy(data.entry.path)} from {paths.lossy(data.source.path)})`
  end,
}

local function providerOf(value: unknown, name: string): ProviderData
  local data = providers[value]
  if not data then
    check.receiver(check.method('VfsProvider', name), 'dream.vfs.VfsProvider', value)
  end

  return data
end

function ProviderMethods.source(self: VfsProvider): Source
  return copySource(providerOf(self, 'source').source)
end

function ProviderMethods.file(self: VfsProvider): VfsFile
  return file.new(providerOf(self, 'file').entry)
end

table.freeze(ProviderMethods)

local function newProvider(source: Source, entry: Entry): VfsProvider
  local provider = (setmetatable({}, ProviderMeta) :: unknown) :: VfsProvider
  providers[provider] = { source = source, entry = entry }
  return check.handle(provider)
end

local function providerRow(vfs: State, entry: Entry): ProviderRow
  return { source = copySource(vfs.sources[entry.source]), file = file.new(entry) }
end

-- Building ------------------------------------------------------------------------------------------

--- A copy of a file's data as an entry of source `source`.
local function entryFor(data: Entry, source: number): Entry
  return { path = data.path, archive = data.archive, index = data.index, source = source }
end

--- Every file below `root` (links followed, unreadable entries skipped), as keys and relative
--- paths; nil when the walk itself can't start and `raise` is false.
local function walkFiles(root: string, raise: boolean): ({ string }, { string })
  local walk, message =
    fs.walk(root, { followLinks = true, include = 'files', skipErrors = not raise })
  if not walk then
    if raise then check.vfs(check.walkError(message)) end

    return {}, {}
  end

  if raise and #walk.errors > 0 then check.vfs(check.walkError(walk.errors[1].message)) end

  local keys, rels = {}, {}
  for _, rel in walk.paths do
    local key = paths.safeKeyFromHost(rel)
    if key then
      table.insert(keys, key)
      table.insert(rels, rel)
    end
  end

  return keys, rels
end

--- A directory's files in the order the crate adds them: by key, then by path.
type LooseSource = {
  source: Source,
  keys: { string },
  entries: { [string]: { Entry } },
  order: { string },
}

local function looseSource(root: string): LooseSource
  local keys, rels = walkFiles(root, false)
  local byKey: { [string]: { Entry } } = {}
  local order = {}
  for index, key in keys do
    local rel = rels[index]
    local entry: Entry = { path = paths.join(root, rel), source = 0, rel = rel }
    local list = byKey[key]
    if list then
      table.insert(list, entry)
    else
      byKey[key] = { entry }
      table.insert(order, key)
    end
  end

  table.sort(order)
  for _, list in byKey do
    if #list > 1 then
      table.sort(list, function(left, right) return paths.pathLess(left.path, right.path) end)
    end
  end

  return { source = { path = root, kind = 'looseDir' }, keys = keys, entries = byKey, order = order }
end

--- An archive's entries as keys, each with the name it is stored under: a TES4 entry without a
--- full path is its folder and name joined with `\`.
local function archiveEntries(ref: types.ArchiveRef): { { key: string, entry: Entry } }
  local out = {}
  local handle = ref.handle
  local tes4 = handle:format() == 'bsaTes4'
  for index = 1, handle:len() do
    local described = handle:entry(index)
    if not described then continue end

    local name = described.path
    if not name and tes4 then
      local folder, base = described.folder, described.name
      if folder and base then
        name = if folder ~= '' and base ~= '' then `{folder}\\{base}` else folder .. base
      end
    end

    if not name then continue end

    local key = paths.safeKeyBytes(name)
    if key then
      table.insert(
        out,
        { key = key, entry = { path = name, archive = ref, index = index, source = 0 } }
      )
    end
  end

  return out
end

--- Opens an archive the way the crate's `open_archive` does: any failure is no archive.
local function openArchive(path: string): types.ArchiveRef?
  -- @dream-archive's openPath raises for a file it can't read or doesn't recognize.
  local ok, handle = pcall(archive.openPath, path)
  if not ok then return nil end

  return { path = path, handle = handle :: archive.Archive }
end

--- Appends a source's entries in order, skipping keys a file and a directory would share.
local function appendSource(vfs: State, source: Source, list: { { key: string, entry: Entry } })
  local index = state.pushSource(vfs, source)
  for _, item in list do
    item.entry.source = index
    state.insert(vfs, item.key, item.entry)
  end
end

local function appendLoose(vfs: State, loose: LooseSource)
  local index = state.pushSource(vfs, loose.source)
  for _, key in loose.order do
    for _, entry in loose.entries[key] do
      entry.source = index
      state.insert(vfs, key, entry)
    end
  end
end

--- `VFS::from_directories`: the archives the list names (found as loose files of the data
--- directories, the last directory's copy winning) below every loose file, later directories above
--- earlier ones.
local function fromDirectories(dirs: { string }, archives: { string }?): State
  local vfs = state.new()
  local loose = (table.create(#dirs) :: unknown) :: { LooseSource }
  for index, dir in dirs do
    loose[index] = looseSource(dir)
  end

  if archives then
    local found: { [string]: string } = {}
    local wanted: { [string]: boolean } = {}
    for _, name in archives do
      wanted[paths.normalize(name)] = true
    end

    for _, source in loose do
      for key, list in source.entries do
        if wanted[key] then found[key] = list[#list].path end
      end
    end

    for _, name in archives do
      local path = found[paths.normalize(name)]
      local ref = path and openArchive(path)
      if ref then appendSource(vfs, { path = ref.path, kind = 'archive' }, archiveEntries(ref)) end
    end
  end

  for _, source in loose do
    appendLoose(vfs, source)
  end

  return vfs
end

--- `push_provider_batch`: one new source with every entry it can take; none taken, no source.
local function pushBatch(
  vfs: State,
  source: Source,
  list: { { key: string, entry: Entry } }
): number
  local kept = {}
  for _, item in list do
    if paths.isSafeNormalized(item.key) then table.insert(kept, item) end
  end

  if #kept == 0 then return 0 end

  local index = state.pushSource(vfs, source)
  local inserted = 0
  for _, item in kept do
    item.entry.source = index
    if state.insert(vfs, item.key, item.entry) then
      inserted += 1
    end
  end

  if inserted == 0 then state.popSource(vfs) end

  return inserted
end

-- Rows -------------------------------------------------------------------------------------------------

local function sortedKeys(vfs: State): { string }
  local cached = vfs.keys
  if cached then return cached end

  local keys = (table.create(vfs.count) :: unknown) :: { string }
  for key in vfs.files do
    table.insert(keys, key)
  end

  table.sort(keys)
  vfs.keys = keys
  return keys
end

local function entriesView(keys: { string }, entries: { Entry }): views.View<EntryRow>
  return views.lazy(
    #keys,
    function(index: number): EntryRow return { key = keys[index], file = file.new(entries[index]) } end
  )
end

--- The entries `accept` takes, sorted by key.
local function entriesWhere(vfs: State, accept: (key: string) -> boolean): views.View<EntryRow>
  local keys: { string } = {}
  for key in vfs.files do
    if accept(key) then table.insert(keys, key) end
  end

  table.sort(keys)
  local entries = (table.create(#keys) :: unknown) :: { Entry }
  for index, key in keys do
    entries[index] = vfs.files[key]
  end

  return entriesView(keys, entries)
end

--- `provider_record_from_entry`.
local function record(vfs: State, key: string, entry: Entry): ProviderRecord
  local source = vfs.sources[entry.source]
  local original = state.originalPath(vfs, key, entry)
  local resolved = if source.kind == 'archive'
    then `{paths.lossy(source.path)}::{paths.lossy(original)}`
    else paths.lossy(paths.join(source.path, original))
  return {
    sourceIndex = entry.source,
    source = copySource(source),
    key = paths.keyPath(key),
    originalPath = original,
    resolvedPath = resolved,
  }
end

local function records(vfs: State, key: string): { ProviderRecord }
  local out = {}
  for index, entry in vfs.providers[key] or {} do
    out[index] = record(vfs, key, entry)
  end

  return out
end

local function treeStream(built: tree.Tree): Tree
  local rows = tree.rows(built)
  local count = #rows
  local stream = (
    setmetatable({}, {
      __iter = function(): () -> (number?, TreeRow?)
        local index = 0
        return function(): (number?, TreeRow?)
          if index >= count then return nil, nil end

          index += 1
          local row = rows[index]
          return index, { dir = row.dir, file = file.new(row.entry) }
        end
      end,

      __index = table.freeze {
        count = function(): number return count end,
        toTable = function(): { [string]: unknown } return tree.toTable(built) end,
      },
      __tostring = function(): string return 'dream.vfs.Tree' end,
      -- A stream has no length, as the native stream had none: `tree:count()` is the row count.
      __len = function(): number return error('attempt to get length of a dream.vfs.Tree value', 2) end,
    }) :: unknown
  ) :: Tree
  trees[stream] = built
  return stream
end

local function regex(pattern: string): Regex
  -- `jess.regex.new` is a Rust binding that raises for a pattern it cannot compile.
  local ok, compiled = pcall(jess.regex.new, pattern, { caseInsensitive = true } :: RegexOptions)
  if not ok then
    local message = string.gsub(tostring(compiled), '^jess%.regex%.new: ', '')
    check.vfs(`invalid regex: {message}`)
  end

  return compiled :: Regex
end

-- The class --------------------------------------------------------------------------------------------

local Methods = {}
local VfsMeta = {
  __index = Methods,
  __tostring = function(self: VFS): string
    local vfs = registry.stateOf(self)
    return `dream.vfs.VFS({if vfs then vfs.count else 0} files)`
  end,
}

local function wrap(vfs: State): VFS
  local handle = (setmetatable({}, VfsMeta) :: unknown) :: VFS
  registry.register(handle, vfs)
  return check.handle(handle)
end

local function receiver(self: unknown, name: string): State return registry.receiver(self, name) end

local function keyArgument(name: string, value: unknown, position: number?): string
  local at = position or 1
  return check.string(method(name), value, at, at + 1)
end

function Methods.len(self: VFS): number return receiver(self, 'len').count end

function Methods.revision(self: VFS): number return receiver(self, 'revision').revision end

function Methods.isEmpty(self: VFS): boolean return receiver(self, 'isEmpty').count == 0 end

function Methods.keys(self: VFS, limit: number?, descending: boolean?): views.View<string>
  local vfs = receiver(self, 'keys')
  local keys = sortedKeys(vfs)
  if not limit then return views.array(table.clone(keys)) end

  local take = math.max(0, math.min(#keys, math.floor(limit)))
  local selected = (table.create(take) :: unknown) :: { string }
  if descending == true then
    for index = 1, take do selected[index] = keys[#keys - index + 1] end
  else
    for index = 1, take do selected[index] = keys[index] end
  end
  return views.array(selected)
end

function Methods.entries(self: VFS, limit: number?, descending: boolean?): views.View<EntryRow>
  local vfs = receiver(self, 'entries')
  local keys = sortedKeys(vfs)
  if limit then
    local take = math.max(0, math.min(#keys, math.floor(limit)))
    local selectedKeys = (table.create(take) :: unknown) :: { string }
    local entries = (table.create(take) :: unknown) :: { Entry }
    if descending == true then
      for index = 1, take do
        local key = keys[#keys - index + 1]
        selectedKeys[index] = key
        entries[index] = vfs.files[key]
      end
    else
      for index = 1, take do
        local key = keys[index]
        selectedKeys[index] = key
        entries[index] = vfs.files[key]
      end
    end
    return entriesView(selectedKeys, entries)
  end
  local entries = (table.create(#keys) :: unknown) :: { Entry }
  for index, key in keys do
    entries[index] = vfs.files[key]
  end

  return entriesView(table.clone(keys), entries)
end

function Methods.getFile(self: VFS, path: string): VfsFile?
  local vfs = receiver(self, 'getFile')
  local token = keys.find(keyArgument('getFile', path))
  local entry = if token then vfs.winners[token] else nil
  return if entry then file.new(entry) else nil
end

function Methods.contains(self: VFS, path: string): boolean
  local vfs = receiver(self, 'contains')
  local token = keys.find(keyArgument('contains', path))
  return token ~= nil and vfs.winners[token] ~= nil
end

function Methods.findByRegex(self: VFS, pattern: string, relative: boolean?): Tree
  local vfs = receiver(self, 'findByRegex')
  local name = method 'findByRegex'
  local compiled = regex(check.text(name, pattern, 1, 2))
  local rel = check.optionalBoolean(name, relative, 2, 3)
  return treeStream(
    tree.build(
      vfs,
      rel ~= false,
      function(key: string): boolean return compiled:isMatch(paths.lossy(key)) end
    )
  )
end

local function atOrUnderPath(path: string, root: string): boolean
  return paths.samePath(path, root) or paths.stripPrefix(path, root) ~= nil
end

function Methods.remaining(
  self: VFS,
  filterPath: string,
  replacementsOnly: boolean,
  allDirs: { string },
  relative: boolean?
): Tree
  local vfs = receiver(self, 'remaining')
  local name = method 'remaining'
  local filter = check.string(name, filterPath, 1, 2)
  local replacements = check.boolean(name, replacementsOnly, 2, 3)
  if type(allDirs) ~= 'table' then check.tableArgument('allDirs', allDirs, 4) end

  local dirs = check.stringArray('allDirs', allDirs :: { unknown })
  local rel = check.optionalBoolean(name, relative, 4, 5)
  local normalized = paths.normalizeHost(filter)
  local chosen = {}
  for _, dir in dirs do
    if paths.samePath(paths.normalizeHost(dir), normalized) then table.insert(chosen, dir) end
  end

  local filtered = fromDirectories(chosen, nil)
  return treeStream(tree.build(vfs, rel ~= false, function(key: string, entry: Entry): boolean
    local filePath = paths.normalizeHost(entry.path)
    if replacements then
      return filtered.files[key] ~= nil and not atOrUnderPath(filePath, normalized)
    end

    return atOrUnderPath(filePath, normalized)
  end))
end

function Methods.pathsMatching(self: VFS, substring: string): views.View<EntryRow>
  local vfs = receiver(self, 'pathsMatching')
  local needle = paths.foldNeedle(keyArgument('pathsMatching', substring))
  return entriesWhere(
    vfs,
    function(key: string): boolean return string.find(key, needle, 1, true) ~= nil end
  )
end

local function lowerBound(items: { string }, needle: string): number
  local low, high = 1, #items + 1
  while low < high do
    local mid = math.floor((low + high) / 2)
    if items[mid] < needle then low = mid + 1 else high = mid end
  end
  return low
end

local function prefixSuccessor(prefix: string): string?
  for index = #prefix, 1, -1 do
    local byte = string.byte(prefix, index)
    if byte < 255 then return string.sub(prefix, 1, index - 1) .. string.char(byte + 1) end
  end
  return nil
end

--- Winners whose normalized key starts with the normalized byte prefix. Unlike `pathsWith`, this
--- is not component-aware: `foo` also matches `foobar`. The optional page arguments let callers
--- consume a tiny ordered slice without materializing the whole range.
function Methods.pathsStartingWith(
  self: VFS,
  prefix: string,
  limit: number?,
  descending: boolean?
): views.View<EntryRow>
  local vfs = receiver(self, 'pathsStartingWith')
  local normalized = paths.normalize(keyArgument('pathsStartingWith', prefix))
  local sorted = sortedKeys(vfs)
  local first = lowerBound(sorted, normalized)
  local successor = prefixSuccessor(normalized)
  local last = if successor then lowerBound(sorted, successor) - 1 else #sorted
  local take = if limit then math.max(0, math.floor(limit)) else nil
  local keys = ((if take then table.create(take) else {}) :: unknown) :: { string }
  local entries = ((if take then table.create(take) else {}) :: unknown) :: { Entry }

  if descending == true then
    for index = last, first, -1 do
      local key = sorted[index]
      if not key or string.sub(key, 1, #normalized) ~= normalized then continue end

      keys[#keys + 1] = key
      entries[#entries + 1] = vfs.files[key]
      if take and #keys >= take then break end
    end
  else
    for index = first, last do
      local key = sorted[index]
      if not key or string.sub(key, 1, #normalized) ~= normalized then continue end

      keys[#keys + 1] = key
      entries[#entries + 1] = vfs.files[key]
      if take and #keys >= take then break end
    end
  end

  return entriesView(keys, entries)
end

function Methods.keysStartingWith(
  self: VFS,
  prefix: string,
  limit: number?,
  descending: boolean?
): views.View<string>
  local vfs = receiver(self, 'keysStartingWith')
  local normalized = paths.normalize(keyArgument('keysStartingWith', prefix))
  local sorted = sortedKeys(vfs)
  local first = lowerBound(sorted, normalized)
  local successor = prefixSuccessor(normalized)
  local last = if successor then lowerBound(sorted, successor) - 1 else #sorted
  local count = math.max(0, last - first + 1)
  if limit then count = math.min(count, math.max(0, math.floor(limit))) end

  if descending == true then
    return views.lazy(count, function(index: number): string return sorted[last - index + 1] end)
  end

  return views.lazy(count, function(index: number): string return sorted[first + index - 1] end)
end

function Methods.pathsWith(self: VFS, prefix: string, limit: number?, descending: boolean?): views.View<EntryRow>
  local vfs = receiver(self, 'pathsWith')
  local normalized = paths.normalize(keyArgument('pathsWith', prefix))
  while string.byte(normalized, -1) == 47 do normalized = string.sub(normalized, 1, -2) end
  if normalized == '' then return entriesView({}, {}) end

  local sorted = sortedKeys(vfs)
  local first = lowerBound(sorted, normalized)
  local last = lowerBound(sorted, normalized .. '0') - 1
  local take = if limit then math.max(0, math.floor(limit)) else nil
  local keys = ((if take then table.create(take) else {}) :: unknown) :: { string }
  local entries = ((if take then table.create(take) else {}) :: unknown) :: { Entry }

  if descending == true then
    for index = last, first, -1 do
      local key = sorted[index]
      if not key or not paths.atOrUnder(key, normalized) then continue end

      keys[#keys + 1] = key
      entries[#entries + 1] = vfs.files[key]
      if take and #keys >= take then break end
    end
  else
    for index = first, last do
      local key = sorted[index]
      if not key or not paths.atOrUnder(key, normalized) then continue end

      keys[#keys + 1] = key
      entries[#entries + 1] = vfs.files[key]
      if take and #keys >= take then break end
    end
  end

  return entriesView(keys, entries)
end

-- Mutation ---------------------------------------------------------------------------------------------

--- `provider_source`: an archive entry's archive, or a loose file's folder.
local function sourceOf(entry: Entry): Source
  local ref = entry.archive
  if ref then return { path = paths.lossy(ref.path), kind = 'archive' } end

  return { path = paths.parent(entry.path) or '', kind = 'looseDir' }
end

--- `set_winner_file`: the whole stack of `key` replaced by one winner; the previous winner.
local function setWinner(vfs: State, key: string, data: Entry): VfsFile?
  local normalized = paths.safeKeyBytes(key)
  if not normalized then return nil end

  if not vfs.files[normalized] and state.conflicts(vfs, normalized) then return nil end

  local previous = vfs.files[normalized]
  local index = state.pushSource(vfs, sourceOf(data))
  state.dropStack(vfs, normalized)
  state.pushEntry(vfs, normalized, entryFor(data, index))
  state.refresh(vfs, normalized)
  state.touched(vfs)
  return if previous then file.new(previous) else nil
end

function Methods.setWinnerLooseFile(self: VFS, key: string, physicalPath: string): VfsFile?
  local vfs = receiver(self, 'setWinnerLooseFile')
  local name = method 'setWinnerLooseFile'
  local target = check.string(name, key, 1, 2)
  local path = check.string(name, physicalPath, 2, 3)
  return setWinner(vfs, target, { path = path, source = 0 })
end

function Methods.setWinnerFile(self: VFS, key: string, value: VfsFile): VfsFile?
  local vfs = receiver(self, 'setWinnerFile')
  local name = method 'setWinnerFile'
  local target = check.string(name, key, 1, 2)
  local data = file.entryOf(value)
  if not data then check.userdata(name, 2, 3, 'dream.vfs.VfsFile', value) end

  return setWinner(vfs, target, data :: Entry)
end

function Methods.pushDirectory(self: VFS, root: string)
  local vfs = receiver(self, 'pushDirectory')
  local dir = keyArgument('pushDirectory', root)
  local keys, rels = walkFiles(dir, true)
  local list = (table.create(#keys) :: unknown) :: { { key: string, entry: Entry } }
  for index, key in keys do
    local rel = rels[index]
    list[index] = { key = key, entry = { path = paths.join(dir, rel), source = 0, rel = rel } }
  end

  if #list == 0 then return end

  table.sort(
    list,
    function(left: { key: string, entry: Entry }, right: { key: string, entry: Entry }): boolean
      if left.key ~= right.key then return left.key < right.key end

      return paths.pathLess(left.entry.path, right.entry.path)
    end
  )
  pushBatch(vfs, { path = dir, kind = 'looseDir' }, list)
  state.touched(vfs)
end

function Methods.pushProvider(self: VFS, key: string, provider: VfsProvider): boolean
  local vfs = receiver(self, 'pushProvider')
  local name = method 'pushProvider'
  local target = check.string(name, key, 1, 2)
  local data = providers[provider]
  if not data then check.userdata(name, 2, 3, 'dream.vfs.VfsProvider', provider) end

  local normalized = paths.safeKeyBytes(target)
  if not normalized then return false end

  if not vfs.files[normalized] and state.conflicts(vfs, normalized) then return false end

  local index = state.pushSource(vfs, copySource(data.source))
  state.pushEntry(vfs, normalized, entryFor(data.entry, index))
  state.refresh(vfs, normalized)
  state.touched(vfs)
  return true
end

function Methods.pushArchive(self: VFS, path: string): boolean
  local vfs = receiver(self, 'pushArchive')
  local target = keyArgument('pushArchive', path)
  local ref = openArchive(target)
  if not ref then return false end

  pushBatch(vfs, { path = ref.path, kind = 'archive' }, archiveEntries(ref))
  state.touched(vfs)
  return true
end

function Methods.removeWinner(self: VFS, key: string): VfsProvider?
  local vfs = receiver(self, 'removeWinner')
  local normalized = paths.normalize(keyArgument('removeWinner', key))
  local stack = vfs.providers[normalized]
  if not stack then return nil end

  local removed = table.remove(stack)
  state.refresh(vfs, normalized)
  state.touched(vfs)
  if not removed then return nil end

  return newProvider(copySource(vfs.sources[removed.source]), removed)
end

--- `discard_stack`: the key and its whole stack gone; its winner, if it had one.
local function discard(vfs: State, key: string): Entry?
  state.dropStack(vfs, key)
  local removed = vfs.files[key]
  state.refresh(vfs, key)
  return removed
end

function Methods.removeResolvedFile(self: VFS, key: string): VfsFile?
  local vfs = receiver(self, 'removeResolvedFile')
  local removed = discard(vfs, paths.normalize(keyArgument('removeResolvedFile', key)))
  if removed then state.touched(vfs) end

  return if removed then file.new(removed) else nil
end

--- `remove_matching_provider`: every provider `matches` takes, from every stack.
local function removeMatching(
  vfs: State,
  matches: (key: string, entry: Entry) -> boolean
): { { key: string, provider: ProviderRow } }
  local keys = {}
  for key in vfs.providers do
    table.insert(keys, key)
  end

  local removed = {}
  for _, key in keys do
    local stack = vfs.providers[key]
    if not stack then continue end

    local index = 1
    while index <= #stack do
      local entry = stack[index]
      if matches(key, entry) then
        table.remove(stack, index)
        table.insert(removed, { key = key, provider = providerRow(vfs, entry) })
      else
        index += 1
      end
    end

    state.refresh(vfs, key)
  end

  state.touched(vfs)
  return removed
end

function Methods.removeProviderPrefix(self: VFS, prefix: string)
  local vfs = receiver(self, 'removeProviderPrefix')
  local normalized = paths.normalize(keyArgument('removeProviderPrefix', prefix))
  return removeMatching(
    vfs,
    function(key: string): boolean return paths.atOrUnder(key, normalized) end
  )
end

--- `remove_resolved_keys`.
local function removeResolved(vfs: State, keys: { string }): { { key: string, file: VfsFile } }
  local removed = {}
  for _, key in keys do
    local entry = discard(vfs, key)
    if entry then table.insert(removed, { key = key, file = file.new(entry) }) end
  end

  if #removed > 0 then state.touched(vfs) end

  return removed
end

function Methods.removeResolvedPrefix(self: VFS, prefix: string)
  local vfs = receiver(self, 'removeResolvedPrefix')
  local normalized = paths.normalize(keyArgument('removeResolvedPrefix', prefix))
  local keys = {}
  for key in vfs.providers do
    if paths.atOrUnder(key, normalized) then table.insert(keys, key) end
  end

  return removeResolved(vfs, keys)
end

function Methods.removeProvider(self: VFS, key: string, sourcePath: string): { ProviderRow }
  local vfs = receiver(self, 'removeProvider')
  local name = method 'removeProvider'
  local normalized = paths.normalize(check.string(name, key, 1, 2))
  local source = check.string(name, sourcePath, 2, 3)
  local stack = vfs.providers[normalized]
  if not stack then return {} end

  local removed = {}
  local index = 1
  while index <= #stack do
    local entry = stack[index]
    if paths.samePath(vfs.sources[entry.source].path, source) then
      table.remove(stack, index)
      table.insert(removed, providerRow(vfs, entry))
    else
      index += 1
    end
  end

  state.refresh(vfs, normalized)
  state.touched(vfs)
  return removed
end

function Methods.removeSource(self: VFS, sourcePath: string)
  local vfs = receiver(self, 'removeSource')
  local source = keyArgument('removeSource', sourcePath)
  return removeMatching(
    vfs,
    function(_: string, entry: Entry): boolean
      return paths.samePath(vfs.sources[entry.source].path, source)
    end
  )
end

function Methods.removeResolvedMatchingGlob(self: VFS, pattern: string)
  local vfs = receiver(self, 'removeResolvedMatchingGlob')
  local matcher = glob.compile(check.text(method 'removeResolvedMatchingGlob', pattern, 1, 2))
  local keys = {}
  if matcher then
    for key in vfs.files do
      if matcher(paths.keyPath(key)) then table.insert(keys, key) end
    end
  end

  return removeResolved(vfs, keys)
end

-- Materialization -----------------------------------------------------------------------------------

function Methods.tree(self: VFS, relative: boolean?): Tree
  local vfs = receiver(self, 'tree')
  local rel = check.optionalBoolean(method 'tree', relative, 1, 2)
  return treeStream(tree.build(vfs, rel ~= false))
end

function Methods.display(self: VFS, relative: boolean?): string
  local vfs = receiver(self, 'display')
  local rel = check.optionalBoolean(method 'display', relative, 1, 2)
  return tree.display(tree.build(vfs, rel ~= false))
end

local function skipRows(skipped: { materialize.Skip }): { materialize.Skip }
  local rows = (table.create(#skipped) :: unknown) :: { materialize.Skip }
  for index, skip in skipped do
    rows[index] = {
      kind = skip.kind,
      key = skip.key,
      archive = skip.archive,
      source = skip.source,
      error = skip.error,
    }
  end

  return rows
end

function Methods.dumpToDirectory(
  self: VFS,
  dir: string,
  useHardlinks: boolean
): (number, { materialize.Skip })
  local vfs = receiver(self, 'dumpToDirectory')
  local name = method 'dumpToDirectory'
  local target = check.string(name, dir, 1, 2)
  local links = check.boolean(name, useHardlinks, 2, 3)
  local written, skipped = materialize.dump(vfs, target, links, false)
  return written, skipRows(skipped)
end

local function collapseOptions(context: string, value: unknown): materialize.Options
  local given = check.options(context, value)
  local options = {
    allowCopying = check.option(context, given, 'allowCopying', 'boolean') == true,
    extractArchives = check.option(context, given, 'extractArchives', 'boolean') == true,
    useSymlinks = check.option(context, given, 'useSymlinks', 'boolean') == true,
  }
  check.finish(context, given, { 'allowCopying', 'extractArchives', 'useSymlinks' })
  return options
end

function Methods.collapseInto(self: VFS, dest: string, options: unknown?): { materialize.Skip }
  local vfs = receiver(self, 'collapseInto')
  local target = keyArgument('collapseInto', dest)
  local chosen = collapseOptions('vfs:collapseInto', options)
  return skipRows(materialize.collapse(vfs, target, chosen))
end

function Methods.extractFile(self: VFS, vfsPath: string, destDir: string): string?
  local vfs = receiver(self, 'extractFile')
  local name = method 'extractFile'
  local key = check.string(name, vfsPath, 1, 2)
  local target = check.string(name, destDir, 2, 3)
  return materialize.extractFile(vfs, key, target)
end

function Methods.diffDirectory(self: VFS, dir: string): DirectoryDiff
  local vfs = receiver(self, 'diffDirectory')
  local root = keyArgument('diffDirectory', dir)
  local walk, message = fs.walk(root, { followLinks = true, include = 'files', skipErrors = true })
  local conflicts: { { key: string, incoming: VfsFile, current: VfsFile } } = {}
  local additions: { { key: string, file: VfsFile } } = {}
  local unreadable: { { path: string, error: string } } = {}
  if not walk then
    table.insert(unreadable, { path = root, error = check.walkError(message) })
  else
    for _, rel in walk.paths do
      local key = paths.safeKeyFromHost(rel)
      if not key then continue end

      local incoming = file.new { path = paths.join(root, rel), source = 0 }
      local current = vfs.files[key]
      if current then
        table.insert(
          conflicts,
          { key = paths.keyPath(key), incoming = incoming, current = file.new(current) }
        )
      else
        table.insert(additions, { key = paths.keyPath(key), file = incoming })
      end
    end

    for _, problem in walk.errors do
      table.insert(unreadable, { path = problem.path, error = problem.message })
    end

    table.sort(
      unreadable,
      function(left, right) return paths.pathLess(left.path, right.path) end
    )
  end

  return { conflicts = conflicts, additions = additions, unreadable = unreadable }
end

-- Reports ----------------------------------------------------------------------------------------------

function Methods.providerRecordsFor(self: VFS, path: string): views.View<ProviderRecord>
  local vfs = receiver(self, 'providerRecordsFor')
  local rows = records(vfs, paths.normalize(keyArgument('providerRecordsFor', path)))
  return views.lazy(#rows, function(index: number): ProviderRecord
    local row = rows[index]
    return {
      sourceIndex = row.sourceIndex,
      source = copySource(row.source),
      key = row.key,
      originalPath = row.originalPath,
      resolvedPath = row.resolvedPath,
    }
  end)
end

function Methods.explain(
  self: VFS,
  path: string
): { key: string, winner: ProviderRecord, overridden: { ProviderRecord } }?
  local vfs = receiver(self, 'explain')
  local key = paths.normalize(keyArgument('explain', path))
  local rows = records(vfs, key)
  local winner = table.remove(rows)
  if not winner then return nil end

  return { key = paths.keyPath(key), winner = winner, overridden = rows }
end

function Methods.providersFor(self: VFS, key: string): views.View<ProviderRow>?
  local vfs = receiver(self, 'providersFor')
  local token = keys.find(keyArgument('providersFor', key))
  local stack = if token then vfs.stacks[token] else nil
  if not stack then return nil end

  local entries = table.clone(stack)
  return views.lazy(
    #entries,
    function(index: number): ProviderRow return providerRow(vfs, entries[index]) end
  )
end

function Methods.duplicates(self: VFS, pattern: string?): DuplicateReport
  local vfs = receiver(self, 'duplicates')
  local wanted = check.optionalText(method 'duplicates', pattern, 1, 2)
  local compiled = if wanted then regex(wanted) else nil
  local keys = {}
  for key, stack in vfs.providers do
    if #stack > 1 and (not compiled or compiled:isMatch(paths.lossy(key))) then
      table.insert(keys, paths.keyPath(key))
    end
  end

  paths.sortPaths(keys)
  local entries = (table.create(#keys) :: unknown) :: { DuplicateEntry }
  for index, key in keys do
    local rows = records(vfs, paths.normalize(key))
    entries[index] = { key = key, providers = rows, winnerIndex = #rows }
  end

  return { entries = entries }
end

function Methods.archives(self: VFS): { ArchiveInfo }
  local vfs = receiver(self, 'archives')
  local counts: { [number]: { number } } = {}
  for _, stack in vfs.providers do
    local top = #stack
    for position, entry in stack do
      if vfs.sources[entry.source].kind ~= 'archive' then continue end

      local count = counts[entry.source]
      if not count then
        count = { 0, 0 }
        counts[entry.source] = count
      end

      count[1] += 1
      if position == top then
        count[2] += 1
      end
    end
  end

  local rows: { ArchiveInfo } = {}
  for index, source in vfs.sources do
    if source.kind ~= 'archive' then continue end

    local count = counts[index] or { 0, 0 }
    table.insert(rows, {
      sourceIndex = index,
      path = source.path,
      entryCount = count[1],
      winningEntryCount = count[2],
    })
  end

  -- Stable, by path: the crate's `sort_by`.
  local position: { [ArchiveInfo]: number } = {}
  for index, row in rows do
    position[row] = index
  end

  table.sort(rows, function(left: ArchiveInfo, right: ArchiveInfo): boolean
    local order = paths.comparePaths(left.path, right.path)
    if order ~= 0 then return order < 0 end

    return position[left] < position[right]
  end)
  return rows
end

--- `archive_entries`: what every source at `path` (spelled any way) provides, and whether it wins.
local function archiveRows(vfs: State, path: string): { ArchiveEntry }
  local wanted = paths.normalizeHost(path)
  local matching: { [number]: boolean } = {}
  for index, source in vfs.sources do
    if source.kind == 'archive' and paths.samePath(paths.normalizeHost(source.path), wanted) then
      matching[index] = true
    end
  end

  local rows: { ArchiveEntry } = {}
  for key, stack in vfs.providers do
    local top = #stack
    for position, entry in stack do
      if not matching[entry.source] then continue end

      table.insert(rows, {
        key = paths.keyPath(key),
        archivePath = vfs.sources[entry.source].path,
        originalPath = state.originalPath(vfs, key, entry),
        wins = position == top,
      })
    end
  end

  table.sort(rows, function(left: ArchiveEntry, right: ArchiveEntry): boolean
    local order = paths.comparePaths(left.key, right.key)
    if order ~= 0 then return order < 0 end

    order = paths.comparePaths(left.archivePath, right.archivePath)
    if order ~= 0 then return order < 0 end

    order = paths.comparePaths(left.originalPath, right.originalPath)
    if order ~= 0 then return order < 0 end

    return not left.wins and right.wins
  end)
  return rows
end

function Methods.archiveEntries(self: VFS, archivePath: string): { ArchiveEntry }
  local vfs = receiver(self, 'archiveEntries')
  return archiveRows(vfs, keyArgument('archiveEntries', archivePath))
end

function Methods.filesFromArchive(self: VFS, archivePath: string): views.View<string>
  local vfs = receiver(self, 'filesFromArchive')
  local keys: { string } = {}
  for index, row in archiveRows(vfs, keyArgument('filesFromArchive', archivePath)) do
    keys[index] = row.key
  end

  return views.array(keys)
end

--- The layer index of a VFS, built when first asked for and kept until it changes.
local function layerOf(vfs: State): layer.Layer
  local cached = vfs.layer
  if cached then return cached :: layer.Layer end

  local sources, files = state.layerRows(vfs)
  local rows = (table.create(#sources) :: unknown) :: { { source: Source, files: { string } } }
  for index, source in sources do
    rows[index] = { source = source, files = files[index] }
  end

  local built = layer.build(rows)
  vfs.layer = built
  return built
end

function Methods.sourceContributions(self: VFS): ContributionReport
  return layer.contributions(layerOf(receiver(self, 'sourceContributions')))
end

function Methods.materializationPlan(self: VFS, dest: string, options: unknown?): MaterializationPlan
  local vfs = receiver(self, 'materializationPlan')
  local target = keyArgument('materializationPlan', dest)
  local chosen = collapseOptions('vfs:materializationPlan', options)
  return materialize.plan(vfs, target, chosen)
end

function Methods.layerIndex(self: VFS): layer.LayerIndex
  return layer.new(layerOf(receiver(self, 'layerIndex')))
end

function Methods.serializeTree(self: VFS, relative: boolean?, format: string): string
  local vfs = receiver(self, 'serializeTree')
  local name = method 'serializeTree'
  local rel = check.optionalBoolean(name, relative, 1, 2)
  local chosen = check.text(name, format, 2, 3)
  return serialize.encode(encode.displayTree(tree.build(vfs, rel ~= false)), chosen)
end

-- Writes under the write root ------------------------------------------------------------------------

local function writeRootOf(vfs: State, what: string): string
  local root = vfs.writeRoot
  if not root then
    check.vfs(
      `{what}: this VFS has no write root (build it with writeRoot, or call vfs:setWriteRoot)`
    )
  end

  return root :: string
end

local function canonicalRoot(dir: string): string
  check.answer(fs.mkdir(dir, { recursive = true }))
  return check.answer(fs.canonicalize(dir))
end

local function checkGrant(vfs: State, what: string)
  if vfs.grant == 'host' then
    check.fail(
      `dream.vfs: {what}: the host made this VFS in Rust and keeps its write root; only the host sets it`
    )
  end
end

--- `target`: the host path of `key` below the write root, refused before the disk is touched when
--- the key could leave it, or when a symbolic link sits on the way.
local function target(vfs: State, what: string, key: string): string
  local root = writeRootOf(vfs, what)
  if not paths.safeKeyBytes(key) then
    check.vfs(
      `{what}: key '{paths.lossy(key)}' escapes the write root (a key is relative, with no '..', root, drive letter or NUL)`
    )
  end

  local relative = string.gsub(key, '\\', '/')
  local deepest = root
  local walked = ''
  for segment in string.gmatch(relative, '[^/]+') do
    walked = if walked == '' then segment else `{walked}/{segment}`
    local current = paths.join(root, walked)
    -- A component the OS can't ask about ends the check, as the crate's `symlink_metadata` loop did.
    local stat = fs.lstat(current)
    if not stat then break end

    if stat.isSymlink then
      check.vfs(
        `{what}: '{paths.lossy(walked)}' is a symbolic link; the write root does not follow links`
      )
    end

    deepest = current
  end

  local resolved = check.answer(fs.canonicalize(deepest))
  if not atOrUnderPath(resolved, root) then
    check.vfs(
      `{what}: key '{paths.lossy(relative)}' escapes the write root ({paths.lossy(deepest)} resolves to {paths.lossy(
        resolved
      )})`
    )
  end

  return paths.join(root, relative)
end

local function fileTarget(vfs: State, what: string, key: string): string
  local path = target(vfs, what, key)
  local normalized = paths.safeKeyBytes(key) :: string
  if not vfs.files[normalized] and state.conflicts(vfs, normalized) then
    check.vfs(
      `{what}: the VFS refused key '{paths.lossy(key)}' (a file and a directory cannot share a name)`
    )
  end

  return path
end

--- `register`: the file at `path` made `key`'s winner, checked to have taken.
local function register(vfs: State, what: string, key: string, path: string): VfsFile
  setWinner(vfs, key, { path = path, source = 0 })
  local winner = vfs.files[paths.normalize(key)]
  if not winner or winner.archive or not paths.samePath(winner.path, path) then
    check.vfs(
      `{what}: the VFS refused key '{paths.lossy(key)}' (a file and a directory cannot share a name)`
    )
  end

  return file.new(winner :: Entry)
end

local function ensureParent(path: string)
  local parent = paths.parent(path)
  if parent and parent ~= '' then check.answer(fs.mkdir(parent, { recursive = true })) end
end

function Methods.writeRoot(self: VFS): string? return receiver(self, 'writeRoot').writeRoot end

function Methods.setWriteRoot(self: VFS, dir: string)
  local vfs = receiver(self, 'setWriteRoot')
  local root = keyArgument('setWriteRoot', dir)
  checkGrant(vfs, 'setWriteRoot')
  vfs.writeRoot = canonicalRoot(root)
end

function Methods.writeFile(
  self: VFS,
  key: string,
  data: buffer | string,
  options: unknown?
): VfsFile
  local vfs = receiver(self, 'writeFile')
  local name = method 'writeFile'
  local wanted = check.string(name, key, 1, 2)
  if type(data) ~= 'string' and type(data) ~= 'buffer' then
    check.argument(name, 2, 3, 'string or buffer', `expected buffer, got {typeof(data)}`)
  end

  local context = 'vfs:writeFile'
  local given = check.options(context, options)
  local offset = check.option(context, given, 'offset', 'integer') :: number?
  if offset and offset < 0 then check.vfs(`{context}: offset {offset} is negative`) end

  local append = check.option(context, given, 'append', 'boolean') == true
  local create = check.option(context, given, 'create', 'boolean') ~= false
  check.finish(context, given, { 'offset', 'append', 'create' })
  if offset and append then check.fail(`{context}: offset and append cannot be combined`) end

  local path = fileTarget(vfs, 'writeFile', wanted)
  ensureParent(path)
  check.answer(fs.writeFile(path, data, { offset = offset, append = append, create = create }))
  return register(vfs, 'writeFile', wanted, path)
end

function Methods.openWrite(self: VFS, key: string, options: unknown?): host.Writer
  local vfs = receiver(self, 'openWrite')
  local wanted = keyArgument('openWrite', key)
  local chosen = host.openOptions('vfs:openWrite', options)
  local path = fileTarget(vfs, 'openWrite', wanted)
  ensureParent(path)
  return host.newWriter(
    path,
    chosen,
    function(): VfsFile return register(vfs, 'Writer.close', wanted, path) end
  )
end

function Methods.mkdir(self: VFS, key: string)
  local vfs = receiver(self, 'mkdir')
  local path = target(vfs, 'mkdir', keyArgument('mkdir', key))
  check.answer(fs.mkdir(path, { recursive = true }))
end

--- `winners_under`: the keys under `prefix` whose winner is a loose file under `dir`.
local function winnersUnder(
  vfs: State,
  prefix: string,
  dir: string
): { { key: string, path: string } }
  local normalized = paths.normalize(prefix)
  local out = {}
  for key, entry in vfs.files do
    if
      paths.atOrUnder(key, normalized)
      and not entry.archive
      and paths.stripPrefix(entry.path, dir)
    then
      table.insert(out, { key = key, path = entry.path })
    end
  end

  return out
end

local function winsWith(vfs: State, key: string, path: string): boolean
  local winner = vfs.files[paths.normalize(key)]
  return winner ~= nil and not winner.archive and paths.samePath(winner.path, path)
end

local function popWinner(vfs: State, key: string)
  local normalized = paths.normalize(key)
  local stack = vfs.providers[normalized]
  if not stack then return end

  table.remove(stack)
  state.refresh(vfs, normalized)
  state.touched(vfs)
end

function Methods.remove(self: VFS, key: string)
  local vfs = receiver(self, 'remove')
  local wanted = keyArgument('remove', key)
  local path = target(vfs, 'remove', wanted)
  local stat = fs.lstat(path)
  if not stat then check.vfs(`remove: '{paths.lossy(wanted)}' is not under the write root`) end

  if (stat :: dream_fs_Stat).isDir then
    check.answer(fs.remove(path, { recursive = true }))
    for _, winner in winnersUnder(vfs, wanted, path) do
      popWinner(vfs, winner.key)
    end

    return
  end

  check.answer(fs.remove(path))
  if winsWith(vfs, wanted, path) then popWinner(vfs, wanted) end
end

function Methods.rename(self: VFS, from: string, to: string)
  local vfs = receiver(self, 'rename')
  local name = method 'rename'
  local source = check.string(name, from, 1, 2)
  local destination = check.string(name, to, 2, 3)
  local sourcePath = target(vfs, 'rename', source)
  local destinationPath = target(vfs, 'rename', destination)
  local stat = fs.lstat(sourcePath)
  if not stat then check.vfs(`rename: '{paths.lossy(source)}' is not under the write root`) end

  ensureParent(destinationPath)
  check.answer(fs.rename(sourcePath, destinationPath))
  if (stat :: dream_fs_Stat).isDir then
    local fromPrefix, toPrefix = paths.normalize(source), paths.normalize(destination)
    for _, winner in winnersUnder(vfs, source, sourcePath) do
      local relative = paths.stripPrefix(winner.path, sourcePath) :: string
      popWinner(vfs, winner.key)
      local newKey = toPrefix .. string.sub(winner.key, #fromPrefix + 1)
      setWinner(vfs, newKey, { path = paths.join(destinationPath, relative), source = 0 })
    end
  elseif winsWith(vfs, source, sourcePath) then
    popWinner(vfs, source)
    register(vfs, 'rename', destination, destinationPath)
  end
end

table.freeze(Methods)

-- Constructors ---------------------------------------------------------------------------------------

local ConstructorOptions = { 'archives', 'writeRoot' }

local function directories(value: unknown): { string }
  if type(value) ~= 'table' then check.tableArgument('dirs', value, 1) end

  return check.stringArray('dirs', value :: { unknown }, check.vfs)
end

local function constructorOptions(context: string, value: unknown): ({ string }?, string?)
  local given = check.options(context, value)
  local archives: { string }?
  local list = given and given.archives
  if list ~= nil then
    if type(list) ~= 'table' then
      check.fail(`{context}.archives: expected table, got {typeof(list)}`)
    end

    archives = check.stringArray(`{context}.archives`, list :: { unknown })
  end

  local root = check.option(context, given, 'writeRoot', 'string') :: string?
  check.finish(context, given, ConstructorOptions)
  return archives, root
end

local function construct(vfs: State, root: string?): VFS
  vfs.grant = 'scripts'
  if root then vfs.writeRoot = canonicalRoot(root) end

  return wrap(vfs)
end

local function new(): VFS return construct(state.new(), nil) end

local function fromDirectoriesFunction(dirs: { string }, options: unknown?): VFS
  local list = directories(dirs)
  local archives, root = constructorOptions('VFS.fromDirectories', options)
  return construct(fromDirectories(list, archives), root)
end

local function withConflictIndex(dirs: { string }, options: unknown?)
  local list = directories(dirs)
  local archives, root = constructorOptions('VFS.fromDirectoriesWithConflictIndex', options)
  local vfs = fromDirectories(list, archives)
  local handle = construct(vfs, root)
  return handle, conflict.fromLayer(layerOf(vfs))
end

local function withLayerIndex(dirs: { string }, options: unknown?)
  local list = directories(dirs)
  local archives, root = constructorOptions('VFS.fromDirectoriesWithLayerIndex', options)
  local vfs = fromDirectories(list, archives)
  local handle = construct(vfs, root)
  return handle, layer.new(layerOf(vfs))
end

local Class = table.freeze({
  new = new,
  fromDirectories = fromDirectoriesFunction,
  fromDirectoriesWithConflictIndex = withConflictIndex,
  fromDirectoriesWithLayerIndex = withLayerIndex,
}) :: VFSClass

local function fileFrom(path: string): VfsFile
  return file.new { path = check.string('dream.vfs.VfsFile.from', path, 1, 1), source = 0 }
end

local function providerNew(source: unknown, value: VfsFile): VfsProvider
  local context = 'VfsProvider.new'
  if type(source) ~= 'table' then
    check.fail(`{context}: options must be a table, got {typeof(source)}`)
  end

  local given = check.options(context, source) :: { [string]: unknown }
  local path = check.requiredString(context, given, 'path')
  local kind = check.requiredString(context, given, 'kind')
  if kind ~= 'looseDir' and kind ~= 'archive' then
    check.fail(`{context}.kind: unknown source kind '{kind}' (expected 'looseDir' or 'archive')`)
  end

  check.finish(context, given, { 'path', 'kind' })
  local data = file.entryOf(value)
  if not data then
    local name = 'dream.vfs.VfsProvider.new'
    check.userdata(name, 2, 2, 'dream.vfs.VfsFile', value)
  end

  return newProvider({ path = path, kind = kind :: types.Kind }, data :: Entry)
end

--- A VFS over loose directories and archives, for hosts and tools that build one themselves.
local function build(dirs: { string }, archives: { string }?): VFS
  return construct(fromDirectories(dirs, archives), nil)
end

return table.freeze {
  Class = Class,
  VfsFile = table.freeze { from = fileFrom },
  VfsProvider = table.freeze { new = providerNew },
  build = build,
  fromDirectories = fromDirectories,
  layerOf = layerOf,
  stateOf = registry.stateOf,
  trees = trees,
  wrap = wrap,
}
