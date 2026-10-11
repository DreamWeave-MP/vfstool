--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- `LayerIndex`: every provider of every key, low to high priority, by the path each source spells
-- it with; and the analyses over it and a VFS: provenance, the lock manifest and its drift, and
-- semantic conflicts.
local check = require './check'
local file = require './file'
local fs = require '@dream/fs'
local hash = require './hash'
local paths = require './paths'
local registry = require './registry'
local semantic = require './semantic'
local types = require './types'
local views = require './views'

type Source = types.Source
type State = types.State
type Entry = types.Entry

export type LayerProvider = {
  sourceIndex: number,
  providerIndex: number,
  source: Source,
  key: string,
  originalPath: string,
}

export type LockEntry = {
  key: string,
  winnerSource: string,
  winnerKind: types.Kind,
  winnerHashBlake3: string?,
  winnerSize: integer?,
  providerCount: number,
}

export type LockData = { schemaVersion: number, entries: { LockEntry } }

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

export type VfsLock = {
  schemaVersion: (self: VfsLock) -> number,
  entries: (self: VfsLock) -> { LockEntry },
  toTable: (self: VfsLock) -> { schemaVersion: number, entries: { LockEntry } },
}

export type ProvenanceRow = {
  source: Source,
  resolvedPath: string,
  hashBlake3: string?,
  size: integer?,
}
export type Provenance = { key: string, winner: Source, providers: { ProvenanceRow } }
export type SemanticRow = {
  source: Source,
  relation: string,
  hashBlake3: string?,
  size: integer?,
  semanticDeltaToWinner: semantic.Delta?,
}

export type SemanticEntry = {
  key: string,
  winner: Source,
  providers: { SemanticRow },
  assetClass: string,
  allIdentical: boolean,
  distinctVersions: number,
}

export type LayerIndex = {
  keys: (self: LayerIndex) -> views.View<string>,
  sources: (self: LayerIndex) -> { Source },
  sourceIdForPath: (self: LayerIndex, path: string) -> number?,
  sourceById: (self: LayerIndex, id: number) -> Source?,
  sourcesContaining: (self: LayerIndex, path: string) -> { number },
  providerOriginalPath: (self: LayerIndex, sourceIndex: number, path: string) -> string?,
  providerChain: (self: LayerIndex, path: string) -> { LayerProvider },
  duplicateKeys: (self: LayerIndex) -> views.View<string>,
  sourceContributions: (self: LayerIndex) -> ContributionReport,
  provenance: (self: LayerIndex, vfs: unknown, path: string, withHashes: boolean) -> Provenance?,
  lockManifest: (self: LayerIndex, vfs: unknown) -> VfsLock,
  diffAgainstLock: (
    self: LayerIndex,
    vfs: unknown,
    lock: VfsLock
  ) -> {
    entries: { { key: string, kind: string } },
    counts: { [string]: number },
  },
  semanticConflicts: (
    self: LayerIndex,
    vfs: unknown,
    options: {
      archiveHashMode: string?,
      includeSemanticDeltas: boolean?,
    }?
  ) -> { entries: { SemanticEntry } },
}

export type Layer = {
  sources: { Source },
  toSources: { [string]: { number } },
  originals: { [string]: { [number]: { string } } },
  keys: { string }?,
}

local SchemaVersion = 1

local layers = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [unknown]: Layer }
local locks = (setmetatable({}, { __mode = 'k' }) :: unknown) :: {
  [unknown]: { schemaVersion: number, entries: { LockEntry } },
}

local function layerOf(value: unknown, method: string): Layer
  local layer = layers[value]
  if not layer then
    check.receiver(check.method('LayerIndex', method), 'dream.vfs.LayerIndex', value)
  end

  return layer
end

local function what(method: string): string return check.method('LayerIndex', method) end

local function copySource(source: Source): Source return { path = source.path, kind = source.kind } end

--- `LayerIndex::from_file_lists`: each source's files by the key their path normalizes to.
local function build(rows: { { source: Source, files: { string } } }): Layer
  local layer: Layer = { sources = {}, toSources = {}, originals = {} }

  for index, row in rows do
    layer.sources[index] = row.source

    for _, path in row.files do
      local key = paths.safeKeyFromHost(path)
      if not key then continue end

      local bySource = layer.originals[key]
      if not bySource then
        bySource = {}
        layer.originals[key] = bySource
      end

      local spellings = bySource[index]
      if spellings then
        table.insert(spellings, path)
      else
        bySource[index] = { path }
      end

      local sources = layer.toSources[key]
      if sources then
        table.insert(sources, index)
      else
        layer.toSources[key] = { index }
      end
    end
  end

  return layer
end

local function sortedKeys(layer: Layer): { string }
  local cached = layer.keys
  if cached then return cached end

  local keys = {}
  for key in layer.toSources do
    table.insert(keys, key)
  end

  table.sort(keys)
  layer.keys = keys
  return keys
end

--- `provider_chain`: every provider of the key `path` normalizes to, low to high priority.
local function chain(layer: Layer, path: string): { LayerProvider }
  local key = paths.normalize(path)
  local sources = layer.toSources[key]
  if not sources then return {} end

  local seen: { [number]: number } = {}
  local rows = (table.create(#sources) :: unknown) :: { LayerProvider }

  for position, sourceIndex in sources do
    local source = layer.sources[sourceIndex]
    if not source then continue end

    local occurrence = (seen[sourceIndex] or 0) + 1
    seen[sourceIndex] = occurrence
    local bySource = layer.originals[key]
    local spellings: { string }? = if bySource then bySource[sourceIndex] else nil
    table.insert(rows, {
      sourceIndex = sourceIndex,
      providerIndex = position,
      source = copySource(source),
      key = paths.keyPath(key),
      originalPath = (if spellings then spellings[occurrence] else nil) or paths.keyPath(key),
    })
  end

  return rows
end

-- Provider I/O ------------------------------------------------------------------------------------

--- How archive providers are hashed: not at all, only a key's winner, or every one.
type HashMode = 'disabled' | 'winnerOnly' | 'allProviders'

--- The VFS file a layer provider names, by its position in the key's stack.
local function providerEntry(state: State, provider: LayerProvider): Entry?
  local stack = state.providers[paths.normalize(provider.key)]
  return stack and stack[provider.providerIndex]
end

local function isWinner(state: State, provider: LayerProvider): boolean
  local stack = state.providers[paths.normalize(provider.key)]
  return stack ~= nil and #stack == provider.providerIndex
end

--- Whether something is at `path`; a path the OS can't ask about counts as nothing, as the
--- crate's `Path::exists` counted it.
local function exists(path: string): boolean return fs.exists(path) == true end

--- `fingerprint_for_provider`: the digest and size of a provider's file, or nil when it has none
--- this mode reads.
local function fingerprint(
  layer: Layer,
  state: State,
  provider: LayerProvider,
  mode: HashMode
): (string?, number?)
  local entry = providerEntry(state, provider)
  if not entry then return nil, nil end

  if layer.sources[provider.sourceIndex].kind == 'looseDir' then
    if not exists(entry.path) then return nil, nil end

    return hash.entry(entry)
  end

  if mode == 'disabled' or (mode == 'winnerOnly' and not isWinner(state, provider)) then
    return nil, nil
  end

  return hash.entry(entry)
end

--- `read_provider_bytes`: a provider's whole contents, or nil when this mode reads none.
local function contents(
  layer: Layer,
  state: State,
  provider: LayerProvider,
  mode: HashMode
): buffer?
  local entry = providerEntry(state, provider)
  if not entry then return nil end

  if layer.sources[provider.sourceIndex].kind == 'looseDir' then
    if not exists(entry.path) then return nil end
  elseif mode == 'disabled' or (mode == 'winnerOnly' and not isWinner(state, provider)) then
    return nil
  end

  if entry.archive then return file.decode(entry) end

  return check.answer(fs.readFile(entry.path))
end

local function size(value: number?): integer? return if value then integer.create(value) else nil end

-- Lock manifests ----------------------------------------------------------------------------------

local LockMethods = {}
local LockMeta = { __index = LockMethods }

local function lockOf(
  value: unknown,
  method: string
): { schemaVersion: number, entries: { LockEntry } }
  local lock = locks[value]
  if not lock then check.receiver(check.method('VfsLock', method), 'dream.vfs.VfsLock', value) end

  return lock
end

local function copyLockEntry(entry: LockEntry): LockEntry
  return {
    key = entry.key,
    winnerSource = entry.winnerSource,
    winnerKind = entry.winnerKind,
    winnerHashBlake3 = entry.winnerHashBlake3,
    winnerSize = entry.winnerSize,
    providerCount = entry.providerCount,
  }
end

function LockMethods.schemaVersion(self: VfsLock): number
  return lockOf(self, 'schemaVersion').schemaVersion
end

function LockMethods.entries(self: VfsLock): { LockEntry }
  local out = {}
  for index, entry in lockOf(self, 'entries').entries do
    out[index] = copyLockEntry(entry)
  end

  return out
end

function LockMethods.toTable(self: VfsLock): { schemaVersion: number, entries: { LockEntry } }
  local lock = lockOf(self, 'toTable')
  return { schemaVersion = lock.schemaVersion, entries = self:entries() }
end

local function newLock(schemaVersion: number, entries: { LockEntry }): VfsLock
  local lock = (setmetatable({}, LockMeta) :: unknown) :: VfsLock
  locks[lock] = { schemaVersion = schemaVersion, entries = entries }
  return check.handle(lock)
end

--- The schema version and rows of a lock handle, or nil for a value that isn't one.
local function lockData(value: unknown): { schemaVersion: number, entries: { LockEntry } }?
  return locks[value]
end

local function lockManifest(layer: Layer, state: State): { LockEntry }
  local entries: { LockEntry } = {}

  for _, key in sortedKeys(layer) do
    local providers = chain(layer, paths.keyPath(key))
    local winner = providers[#providers]
    if not winner or not isWinner(state, winner) then continue end

    local digest, bytes = fingerprint(layer, state, winner, 'winnerOnly')
    table.insert(entries, {
      key = paths.keyPath(key),
      winnerSource = winner.source.path,
      winnerKind = winner.source.kind,
      winnerHashBlake3 = digest,
      winnerSize = size(bytes),
      providerCount = #providers,
    })
  end

  local order: { string } = {}
  for index, entry in entries do
    order[index] = entry.key
  end

  paths.sortPaths(order)
  local byKey: { [string]: LockEntry } = {}
  for _, entry in entries do
    byKey[entry.key] = entry
  end

  local sorted = (table.create(#entries) :: unknown) :: { LockEntry }
  for index, key in order do
    sorted[index] = byKey[key]
  end

  return sorted
end

local DriftKinds = table.freeze {
  'added',
  'removed',
  'winnerSourceChanged',
  'winnerHashChanged',
  'providerCountChanged',
}

local function drift(
  layer: Layer,
  state: State,
  expected: LockData
): { entries: { { key: string, kind: string } }, counts: { [string]: number } }
  if expected.schemaVersion ~= SchemaVersion then
    check.vfs(
      `unsupported VFS lock schema_version {expected.schemaVersion}; expected {SchemaVersion}`
    )
  end

  local current = lockManifest(layer, state)
  local expectedMap, currentMap = {}, {}
  for _, row in expected.entries do
    expectedMap[paths.lossy(row.key)] = row
  end

  for _, row in current do
    currentMap[paths.lossy(row.key)] = row
  end

  local found: { { key: string, kind: number } } = {}
  for key, row in currentMap do
    if not expectedMap[key] then table.insert(found, { key = row.key, kind = 1 }) end
  end

  for key, row in expectedMap do
    if not currentMap[key] then table.insert(found, { key = row.key, kind = 2 }) end
  end

  for key, row in expectedMap do
    local now = currentMap[key]
    if not now then continue end

    if paths.lossy(row.winnerSource) ~= paths.lossy(now.winnerSource) then
      table.insert(found, { key = now.key, kind = 3 })
    end

    if row.winnerHashBlake3 ~= now.winnerHashBlake3 then
      table.insert(found, { key = now.key, kind = 4 })
    end

    if row.providerCount ~= now.providerCount then
      table.insert(found, { key = now.key, kind = 5 })
    end
  end

  table.sort(
    found,
    function(a: { key: string, kind: number }, b: { key: string, kind: number }): boolean
      local order = paths.comparePaths(a.key, b.key)
      if order ~= 0 then return order < 0 end

      return a.kind < b.kind
    end
  )

  local entries = (table.create(#found) :: unknown) :: { { key: string, kind: string } }
  local counts: { [string]: number } = {}
  for index, row in found do
    local kind = DriftKinds[row.kind]
    entries[index] = { key = row.key, kind = kind }
    counts[kind] = (counts[kind] or 0) + 1
  end

  return { entries = entries, counts = counts }
end

-- The class -----------------------------------------------------------------------------------------

local Methods = {}
local LayerMeta = {
  __index = Methods,
  __tostring = function(self: LayerIndex): string
    return `dream.vfs.LayerIndex({#layers[self].sources} sources)`
  end,
}

local function new(layer: Layer): LayerIndex
  local handle = (setmetatable({}, LayerMeta) :: unknown) :: LayerIndex
  layers[handle] = layer
  return check.handle(handle)
end

--- A 1-based index argument, as the binding read one.
local function oneBased(name: string, value: number): number
  if value < 1 then check.vfs(`{name} must be a 1-based index, got {value}`) end

  return value
end

function Methods.keys(self: LayerIndex): views.View<string>
  return views.array(table.clone(sortedKeys(layerOf(self, 'keys'))))
end

function Methods.sources(self: LayerIndex): { Source }
  local out = {}
  for index, source in layerOf(self, 'sources').sources do
    out[index] = copySource(source)
  end

  return out
end

function Methods.sourceIdForPath(self: LayerIndex, path: string): number?
  local layer = layerOf(self, 'sourceIdForPath')
  local wanted = check.str(what 'sourceIdForPath', path, 1, 2)
  for index, source in layer.sources do
    if paths.samePath(source.path, wanted) then return index end
  end

  return nil
end

function Methods.sourceById(self: LayerIndex, id: number): Source?
  local layer = layerOf(self, 'sourceById')
  local index = oneBased('source id', check.exact(what 'sourceById', id, 1, 2))
  local source = layer.sources[index]
  return if source then copySource(source) else nil
end

function Methods.sourcesContaining(self: LayerIndex, path: string): { number }
  local layer = layerOf(self, 'sourcesContaining')
  local key = paths.normalize(check.str(what 'sourcesContaining', path, 1, 2))
  local sources = layer.toSources[key]
  return if sources then table.clone(sources) else {}
end

function Methods.providerOriginalPath(self: LayerIndex, sourceIndex: number, path: string): string?
  local layer = layerOf(self, 'providerOriginalPath')
  local index = check.exact(what 'providerOriginalPath', sourceIndex, 1, 2)
  local key = paths.normalize(check.str(what 'providerOriginalPath', path, 2, 3))
  oneBased('source index', index)
  local bySource = layer.originals[key]
  local spellings = if bySource then bySource[index] else nil
  return if spellings then spellings[1] else nil
end

function Methods.providerChain(self: LayerIndex, path: string): { LayerProvider }
  local layer = layerOf(self, 'providerChain')
  return chain(layer, check.str(what 'providerChain', path, 1, 2))
end

function Methods.duplicateKeys(self: LayerIndex): views.View<string>
  local layer = layerOf(self, 'duplicateKeys')
  local keys: { string } = {}
  for _, key in sortedKeys(layer) do
    if #layer.toSources[key] > 1 then table.insert(keys, key) end
  end

  return views.array(keys)
end

--- `source_contributions`: what each source provides, wins, overrides, and loses.
local function contributions(layer: Layer): ContributionReport
  local rows: { Contribution } = {}
  for index, source in layer.sources do
    rows[index] = {
      sourceIndex = index,
      source = copySource(source),
      winningFiles = 0,
      overridingFiles = 0,
      overriddenFiles = 0,
      uniqueFiles = 0,
      duplicateFiles = 0,
      looseFiles = 0,
      archiveFiles = 0,
    }
  end

  for _, sources in layer.toSources do
    local count = #sources
    for position, sourceIndex in sources do
      local row = rows[sourceIndex]
      if not row then continue end

      local counts = row :: { [string]: number }
      if layer.sources[sourceIndex].kind == 'looseDir' then
        counts.looseFiles += 1
      else
        counts.archiveFiles += 1
      end

      if count == 1 then
        counts.uniqueFiles += 1
      else
        counts.duplicateFiles += 1
      end

      if position == count then
        counts.winningFiles += 1
      else
        for later = position + 1, count do
          if sources[later] ~= sourceIndex then
            counts.overriddenFiles += 1
            break
          end
        end
      end

      for earlier = 1, position - 1 do
        if sources[earlier] ~= sourceIndex then
          counts.overridingFiles += 1
          break
        end
      end
    end
  end

  return { sources = rows }
end

function Methods.sourceContributions(self: LayerIndex): ContributionReport
  return contributions(layerOf(self, 'sourceContributions'))
end

function Methods.provenance(
  self: LayerIndex,
  vfs: unknown,
  path: string,
  withHashes: boolean
): Provenance?
  local layer = layerOf(self, 'provenance')
  local name = what 'provenance'
  local state = registry.argument(vfs, name, 1, 2)
  local wanted = check.str(name, path, 2, 3)
  local hashes = check.boolean(name, withHashes, 3, 4)
  local providers = chain(layer, wanted)
  local winner = providers[#providers]
  if not winner or not isWinner(state, winner) then return nil end

  local key = paths.normalize(wanted)
  local rows = (table.create(#providers) :: unknown) :: { ProvenanceRow }
  for index, provider in providers do
    local digest, bytes
    if hashes then
      digest, bytes = fingerprint(layer, state, provider, 'allProviders')
    end

    local source = provider.source
    local resolved
    if source.kind == 'looseDir' then
      local entry = providerEntry(state, provider)
      resolved =
        paths.lossy(if entry then entry.path else paths.join(source.path, provider.originalPath))
    else
      resolved = `{paths.lossy(source.path)}::{paths.lossy(provider.originalPath)}`
    end

    rows[index] = {
      source = copySource(source),
      resolvedPath = resolved,
      hashBlake3 = digest,
      size = size(bytes),
    }
  end

  return { key = paths.keyPath(key), winner = copySource(winner.source), providers = rows }
end

function Methods.lockManifest(self: LayerIndex, vfs: unknown): VfsLock
  local layer = layerOf(self, 'lockManifest')
  local state = registry.argument(vfs, what 'lockManifest', 1, 2)
  return newLock(SchemaVersion, lockManifest(layer, state))
end

function Methods.diffAgainstLock(
  self: LayerIndex,
  vfs: unknown,
  lock: VfsLock
): {
  entries: { { key: string, kind: string } },
  counts: { [string]: number },
}
  local layer = layerOf(self, 'diffAgainstLock')
  local name = what 'diffAgainstLock'
  local state = registry.argument(vfs, name, 1, 2)
  local expected = locks[lock]
  if not expected then return check.userdata(name, 2, 3, 'dream.vfs.VfsLock', lock) end

  return drift(layer, state, expected)
end

local HashModes: { [string]: boolean } = { disabled = true, winnerOnly = true, allProviders = true }

function Methods.semanticConflicts(
  self: LayerIndex,
  vfs: unknown,
  options: unknown
): { entries: { SemanticEntry } }
  local layer = layerOf(self, 'semanticConflicts')
  local state = registry.argument(vfs, what 'semanticConflicts', 1, 2)
  local context = 'layer:semanticConflicts'
  local given = check.options(context, options)
  local mode = check.option(context, given, 'archiveHashMode', 'string') :: string?
  if mode and not HashModes[mode] then
    check.fail(
      `{context}.archiveHashMode: expected 'disabled', 'winnerOnly', or 'allProviders', got '{mode}'`
    )
  end

  local deltas = check.option(context, given, 'includeSemanticDeltas', 'boolean') == true
  check.finish(context, given, { 'archiveHashMode', 'includeSemanticDeltas' })
  local hashMode: HashMode = (mode or 'winnerOnly') :: HashMode

  local keys: { string } = {}
  for key, sources in layer.toSources do
    if #sources > 1 then table.insert(keys, paths.keyPath(key)) end
  end

  paths.sortPaths(keys)

  local entries: { SemanticEntry } = {}
  for _, key in keys do
    local providers = chain(layer, key)
    local winner = providers[#providers]
    if #providers < 2 or not isWinner(state, winner) then continue end

    local winnerBytes = if deltas then contents(layer, state, winner, hashMode) else nil
    local winnerDigest, winnerSize
    if winnerBytes then
      winnerDigest, winnerSize = hash.bytes(winnerBytes), buffer.len(winnerBytes)
    else
      winnerDigest, winnerSize = fingerprint(layer, state, winner, hashMode)
    end

    local seen: { [string]: boolean } = {}
    local distinct, hashed, class = 0, 0, 'unknown'
    local rows = (table.create(#providers) :: unknown) :: { SemanticRow }

    for index, provider in providers do
      local current: buffer?
      if deltas then
        current = if provider == winner
          then winnerBytes
          else contents(layer, state, provider, hashMode)
      end

      local digest, bytes
      if provider == winner then
        digest, bytes = winnerDigest, winnerSize
      elseif current then
        digest, bytes = hash.bytes(current), buffer.len(current)
      else
        digest, bytes = fingerprint(layer, state, provider, hashMode)
      end

      local delta: semantic.Delta?
      if deltas and winnerBytes and current then
        local pairClass, pairDelta = semantic.analyze(key, current, winnerBytes)
        class = pairClass
        delta = pairDelta
      end

      local relation = 'unknown'
      if digest then
        hashed += 1
        if winnerDigest then
          relation = if winnerDigest == digest then 'identicalToWinner' else 'differentFromWinner'
        end

        if not seen[digest] then
          seen[digest] = true
          distinct += 1
        end
      end

      rows[index] = {
        source = copySource(provider.source),
        relation = relation,
        hashBlake3 = digest,
        size = size(bytes),
        semanticDeltaToWinner = delta,
      }
    end

    table.insert(entries, {
      key = key,
      winner = copySource(winner.source),
      providers = rows,
      assetClass = class,
      allIdentical = winnerDigest ~= nil and hashed == #providers and distinct == 1,
      distinctVersions = distinct,
    })
  end

  return { entries = entries }
end

table.freeze(Methods)
table.freeze(LockMethods)

--- The class table: `LayerIndex.fromFileLists`.
local function fromFileLists(rows: unknown): LayerIndex
  local name = 'dream.vfs.LayerIndex.fromFileLists'
  if type(rows) ~= 'table' then
    check.argument(name, 1, 1, 'table', `expected table, got {typeof(rows)}`)
  end

  local parsed = {}
  for index, row in rows :: { unknown } do
    local context = `LayerIndex.fromFileLists[{index}]`
    local given = check.options(context, row) :: { [string]: unknown }

    local sourceValue = given.source
    if sourceValue == nil then check.fail(`{context}: missing required option 'source'`) end

    if type(sourceValue) ~= 'table' then
      check.fail(`{context}.source: expected table, got {typeof(sourceValue)}`)
    end

    local sourceContext = `{context}.source`
    local source = check.options(sourceContext, sourceValue) :: { [string]: unknown }
    local sourcePath = check.requiredString(sourceContext, source, 'path')
    local kind = check.requiredString(sourceContext, source, 'kind')
    if kind ~= 'looseDir' and kind ~= 'archive' then
      check.fail(
        `{sourceContext}.kind: unknown source kind '{kind}' (expected 'looseDir' or 'archive')`
      )
    end

    check.finish(sourceContext, source, { 'path', 'kind' })

    local files = given.files
    if files == nil then check.fail(`{context}: missing required option 'files'`) end

    if type(files) ~= 'table' then
      check.fail(`{context}.files: expected table, got {typeof(files)}`)
    end

    parsed[index] = {
      source = { path = sourcePath, kind = kind :: types.Kind },
      files = check.stringArray(`{context}.files`, files :: { unknown }),
    }
    check.finish(context, given, { 'source', 'files' })
  end

  return new(build(parsed))
end

local function layerData(value: unknown): Layer? return layers[value] end

return table.freeze {
  build = build,
  new = new,
  chain = chain,
  sortedKeys = sortedKeys,
  contributions = contributions,
  lockManifest = lockManifest,
  drift = drift,
  newLock = newLock,
  lockData = lockData,
  layerData = layerData,
  fromFileLists = fromFileLists,
  SchemaVersion = SchemaVersion,
}
