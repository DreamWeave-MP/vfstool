--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- `ConflictIndex`: for each source of a load order, the files it overrides and the files that
-- override it; which sources are wholly shadowed; and what two sources have in common.
local check = require './check'
local fs = require '@dream/fs'
local layer = require './layer'
local paths = require './paths'
local types = require './types'

type Source = types.Source

export type ConflictIndex = {
  sources: (self: ConflictIndex) -> { string },
  sourcesContaining: (self: ConflictIndex, path: string) -> { number },
  conflictsReport: (
    self: ConflictIndex,
    relative: boolean?
  ) -> {
    sources: { { path: string, overrides: { string }, overriddenBy: { string } } },
  },
  shadowedReport: (
    self: ConflictIndex,
    relative: boolean?,
    listFiles: boolean?
  ) -> {
    sources: { { path: string, shadowedFiles: { string } } },
  },
  diffReport: (
    self: ConflictIndex,
    sourceA: string,
    sourceB: string
  ) -> {
    sourceA: string,
    sourceB: string,
    higherPriority: string,
    shared: { string },
    onlyInA: { string },
    onlyInB: { string },
  },
}

type Index = {
  meta: { Source },
  overrides: { { [string]: boolean } },
  overriddenBy: { { [string]: boolean } },
  counts: { number },
  toSources: { [string]: { number } },
  originals: { [number]: { [string]: string } },
}

local indexes = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [unknown]: Index }

local function indexOf(value: unknown, method: string): Index
  local index = indexes[value]
  if not index then
    check.receiver(check.method('ConflictIndex', method), 'dream.vfs.ConflictIndex', value)
  end

  return index
end

--- `ConflictIndex::from_layer_index`: each key's sources, one per source spelling (kind and folded
--- path), and every conflict between them.
local function fromLayerData(data: layer.Layer): Index
  local sources = data.sources
  local index: Index = {
    meta = sources,
    overrides = {},
    overriddenBy = {},
    counts = table.create(#sources, 0),
    toSources = {},
    originals = {},
  }

  for position = 1, #sources do
    index.overrides[position] = {}
    index.overriddenBy[position] = {}
    index.originals[position] = {}
  end

  for _, key in layer.sortedKeys(data) do
    local unique, seen = {}, {}
    for _, sourceIndex in data.toSources[key] do
      local source = sources[sourceIndex]
      local identity = `{source.kind}\0{paths.normalizeHost(source.path)}`
      if not seen[identity] then
        seen[identity] = true
        table.insert(unique, sourceIndex)
      end
    end

    for _, sourceIndex in unique do
      index.counts[sourceIndex] += 1
    end

    if #unique > 1 then
      for _, sourceIndex in unique do
      local bySource = data.originals[key]
      local spellings = if bySource then bySource[sourceIndex] else nil
      local original = if spellings then spellings[1] else nil
      if original and original ~= key then index.originals[sourceIndex][key] = original end
      end
    end

    if #unique > 0 then index.toSources[key] = unique end
  end

  for key, unique in index.toSources do
    if #unique < 2 then continue end

    local path = paths.keyPath(key)
    for position, sourceIndex in unique do
      if position > 1 then index.overrides[sourceIndex][path] = true end

      if position < #unique then index.overriddenBy[sourceIndex][path] = true end
    end
  end

  return index
end

local Methods = {}
local IndexMeta = {
  __index = Methods,
  __tostring = function(self: ConflictIndex): string
    return `dream.vfs.ConflictIndex({#indexes[self].meta} sources)`
  end,
}

local function new(index: Index): ConflictIndex
  local handle = (setmetatable({}, IndexMeta) :: unknown) :: ConflictIndex
  indexes[handle] = index
  return check.handle(handle)
end

local function fromLayer(data: layer.Layer): ConflictIndex return new(fromLayerData(data)) end

function Methods.sources(self: ConflictIndex): { string }
  local out = {}
  for position, source in indexOf(self, 'sources').meta do
    out[position] = source.path
  end

  return out
end

function Methods.sourcesContaining(self: ConflictIndex, path: string): { number }
  local index = indexOf(self, 'sourcesContaining')
  local key = paths.normalize(check.str('dream.vfs.ConflictIndex.sourcesContaining', path, 1, 2))
  local sources = index.toSources[key]
  if not sources or #sources < 2 then return {} end

  return table.clone(sources)
end

--- `report_path`: a key as a report shows it, relative or below its source.
local function reportPath(index: Index, sourceIndex: number, key: string, relative: boolean): string
  if relative then return key end

  local source = index.meta[sourceIndex]
  if source.kind == 'archive' then return `{paths.lossy(source.path)}::{paths.lossy(key)}` end

  local original = index.originals[sourceIndex][paths.normalize(key)] or key
  return paths.join(source.path, original)
end

local function reportList(
  index: Index,
  sourceIndex: number,
  set: { [string]: boolean },
  relative: boolean
): { string }
  local out = {}
  for key in set do
    table.insert(out, reportPath(index, sourceIndex, key, relative))
  end

  paths.sortPaths(out)
  return out
end

function Methods.conflictsReport(self: ConflictIndex, relative: boolean?)
  local index = indexOf(self, 'conflictsReport')
  local rel = check.optionalBoolean('dream.vfs.ConflictIndex.conflictsReport', relative, 1, 2)
    ~= false
  local sources = {}
  for position, source in index.meta do
    sources[position] = {
      path = source.path,
      overrides = reportList(index, position, index.overrides[position], rel),
      overriddenBy = reportList(index, position, index.overriddenBy[position], rel),
    }
  end

  return { sources = sources }
end

function Methods.shadowedReport(self: ConflictIndex, relative: boolean?, listFiles: boolean?)
  local index = indexOf(self, 'shadowedReport')
  local what = 'dream.vfs.ConflictIndex.shadowedReport'
  local rel = check.optionalBoolean(what, relative, 1, 2) ~= false
  local list = check.optionalBoolean(what, listFiles, 2, 3) ~= false
  local sources = {}
  for position, source in index.meta do
    local count = index.counts[position]
    local overridden = 0
    for _ in index.overriddenBy[position] do
      overridden += 1
    end

    if count == 0 or overridden ~= count then continue end

    table.insert(sources, {
      path = source.path,
      shadowedFiles = if list
        then reportList(index, position, index.overriddenBy[position], rel)
        else {},
    })
  end

  return { sources = sources }
end

local function positions(index: Index, path: string): { number }
  local exact = {}
  for position, source in index.meta do
    if paths.samePath(source.path, path) then table.insert(exact, position) end
  end

  if #exact > 0 then return exact end

  local folded = paths.normalizeHost(path)
  local found = {}
  for position, source in index.meta do
    if paths.samePath(paths.normalizeHost(source.path), folded) then
      table.insert(found, position)
    end
  end

  return found
end

local function keysOf(index: Index, found: { number }, path: string): { [string]: boolean }
  local keys = {}
  if #found == 0 then
    -- With skipErrors, what the walk can't read is left out, as the crate's walk left it.
    local walk = fs.walk(path, { followLinks = true, include = 'files', skipErrors = true })
    for _, rel in if walk then walk.paths else {} do
      local key = paths.safeKeyFromHost(rel)
      if key then keys[paths.lossy(key)] = true end
    end

    return keys
  end

  local wanted = {}
  for _, position in found do
    wanted[position] = true
  end

  for key, sources in index.toSources do
    for _, sourceIndex in sources do
      if wanted[sourceIndex] then
        keys[paths.lossy(key)] = true
        break
      end
    end
  end

  return keys
end

local function highest(found: { number }): number?
  local best
  for _, position in found do
    if not best or position > best then best = position end
  end

  return best
end

function Methods.diffReport(self: ConflictIndex, sourceA: string, sourceB: string)
  local index = indexOf(self, 'diffReport')
  local what = 'dream.vfs.ConflictIndex.diffReport'
  local a, b = check.str(what, sourceA, 1, 2), check.str(what, sourceB, 2, 3)
  local foundA, foundB = positions(index, a), positions(index, b)
  local keysA, keysB = keysOf(index, foundA, a), keysOf(index, foundB, b)
  local shared, onlyA, onlyB = {}, {}, {}
  for key in keysA do
    table.insert(if keysB[key] then shared else onlyA, key)
  end

  for key in keysB do
    if not keysA[key] then table.insert(onlyB, key) end
  end

  table.sort(shared)
  table.sort(onlyA)
  table.sort(onlyB)
  local topA, topB = highest(foundA), highest(foundB)
  local spelledA = if topA then index.meta[topA].path else a
  local spelledB = if topB then index.meta[topB].path else b
  return {
    sourceA = spelledA,
    sourceB = spelledB,
    higherPriority = if topA and topB and topA > topB then spelledA else spelledB,
    shared = shared,
    onlyInA = onlyA,
    onlyInB = onlyB,
  }
end

table.freeze(Methods)

--- `walk_dir`: a directory's files by relative path, skipping ones no key can name.
local function walkDir(dir: string): { string }
  local walk = fs.walk(dir, { followLinks = true, include = 'files', skipErrors = true })
  if not walk then return {} end

  local files = {}
  for _, rel in walk.paths do
    local key = paths.safeKeyFromHost(rel)
    if key and not string.find(key, '\0', 1, true) then table.insert(files, rel) end
  end

  return files
end

local function fromDirectories(dirs: unknown): ConflictIndex
  if type(dirs) ~= 'table' then check.tableArgument('dirs', dirs, 1) end

  local rows = {}
  for position, dir in check.stringArray('dirs', dirs :: { unknown }, check.vfs) do
    rows[position] =
      { source = { path = dir, kind = 'looseDir' :: types.Kind }, files = walkDir(dir) }
  end

  return fromLayer(layer.build(rows))
end

local function fromFileLists(rows: unknown): ConflictIndex
  if type(rows) ~= 'table' then
    check.argument(
      'dream.vfs.ConflictIndex.fromFileLists',
      1,
      1,
      'table',
      `expected table, got {typeof(rows)}`
    )
  end

  local parsed = {}
  for position, row in rows :: { unknown } do
    local context = `ConflictIndex.fromFileLists[{position}]`
    local given = check.options(context, row) :: { [string]: unknown }
    local source = check.requiredString(context, given, 'source')
    local files = given.files
    if files == nil then check.fail(`{context}: missing required option 'files'`) end

    if type(files) ~= 'table' then
      check.fail(`{context}.files: expected table, got {typeof(files)}`)
    end

    parsed[position] = {
      source = { path = source, kind = 'looseDir' :: types.Kind },
      files = check.stringArray(`{context}.files`, files :: { unknown }),
    }
    check.finish(context, given, { 'source', 'files' })
  end

  return fromLayer(layer.build(parsed))
end

local function fromLayerIndex(value: unknown): ConflictIndex
  local data = layer.layerData(value)
  if not data then
    local what = 'dream.vfs.ConflictIndex.fromLayerIndex'
    check.userdata(what, 1, 1, 'dream.vfs.LayerIndex', value)
  end

  return fromLayer(data :: layer.Layer)
end

return table.freeze {
  fromLayer = fromLayer,
  Class = table.freeze {
    fromDirectories = fromDirectories,
    fromFileLists = fromFileLists,
    fromLayerIndex = fromLayerIndex,
  },
}
