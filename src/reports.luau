--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- The reports as the vfstool binary serialized its Rust structs (snake_case fields, 0-based source
-- indices, externally tagged enums), and the output file or standard output they go to. A command
-- requires this when it has a report to write: one that writes none never compiles it, nor the
-- writers under it.
local cli = require './cli'
local fs = require '@dream/fs'
local paths = require '@vfs/paths'
local serialize = require '@jess/data/value'

type Value = serialize.Value

local function lossy(text: string): string return paths.lossy(text) end

local function lossyList(list: { string }): Value
  return serialize.array([for item in list => lossy(item)])
end

local function optional(value: unknown): Value
  if value == nil then return serialize.None end

  return value
end

local Kinds: { [string]: string } = { looseDir = 'loose_dir', archive = 'archive' }

type Source = { path: string, kind: 'looseDir' | 'archive' }
type Record = {
  sourceIndex: number,
  source: Source,
  key: string,
  originalPath: string,
  resolvedPath: string,
}
type Explain = { key: string, winner: Record, overridden: { Record } }
type DuplicateEntry = { key: string, providers: { Record }, winnerIndex: number }
type Duplicates = { entries: { DuplicateEntry } }
type ArchiveInfo = cli.ArchiveInfo
type ArchiveEntry = { key: string, archivePath: string, originalPath: string, wins: boolean }
type Contribution = {
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
type PlanRow = { kind: string, key: string, source: string?, archive: string?, dest: string? }
type Plan = { actions: { PlanRow }, issues: { PlanRow } }
type Conflicts = { sources: { { path: string, overrides: { string }, overriddenBy: { string } } } }
type Shadowed = { sources: { { path: string, shadowedFiles: { string } } } }
type Diff = {
  sourceA: string,
  sourceB: string,
  higherPriority: string,
  shared: { string },
  onlyInA: { string },
  onlyInB: { string },
}
type LockEntry = {
  key: string,
  winnerSource: string,
  winnerKind: 'looseDir' | 'archive',
  winnerHashBlake3: string?,
  winnerSize: integer?,
  providerCount: number,
}
type Lock = { schemaVersion: number, entries: { LockEntry } }
type Drift = { entries: { { key: string, kind: string } }, counts: { [string]: number } }

local function sourceMeta({path, kind}: Source): Value
  return serialize.struct { { 'path', lossy(path) }, { 'kind', Kinds[kind] } }
end

--- `VfsProviderRecord`.
local function providerRecord({sourceIndex, source, key, originalPath, resolvedPath}: Record): Value
  return serialize.struct {
    { 'source_index', sourceIndex - 1 },
    { 'source', sourceMeta(source) },
    { 'key', lossy(key) },
    { 'original_path', lossy(originalPath) },
    { 'resolved_path', resolvedPath },
  }
end

local function list<T>(rows: { T }, each: (T) -> Value): Value
  return serialize.array([for row in rows => each(row)])
end

--- A camelCase name as serde's `snake_case` renames it.
local function snake(name: string): string
  return (string.gsub(name, '%u', function(letter: string): string return `_{string.lower(letter)}` end))
end

local Shapes = table.freeze {
  explain = function({key, winner, overridden}: Explain): Value
    return serialize.struct {
      { 'key', lossy(key) },
      { 'winner', providerRecord(winner) },
      { 'overridden', list(overridden, providerRecord) },
    }
  end,

  duplicates = function(report: Duplicates): Value
    return serialize.struct {
      {
        'entries',
        list(report.entries, function({key, providers, winnerIndex}: DuplicateEntry): Value
          return serialize.struct {
            { 'key', lossy(key) },
            { 'providers', list(providers, providerRecord) },
            { 'winner_index', winnerIndex - 1 },
          }
        end),
      },
    }
  end,

  archive = function({sourceIndex, path, entryCount, winningEntryCount}: ArchiveInfo): Value
    return serialize.struct {
      { 'source_index', sourceIndex - 1 },
      { 'path', lossy(path) },
      { 'entry_count', entryCount },
      { 'winning_entry_count', winningEntryCount },
    }
  end,

  archiveEntry = function({key, archivePath, originalPath, wins}: ArchiveEntry): Value
    return serialize.struct {
      { 'key', lossy(key) },
      { 'archive_path', lossy(archivePath) },
      { 'original_path', lossy(originalPath) },
      { 'wins', wins },
    }
  end,

  contributions = function(report: { sources: { Contribution } }): Value
    return serialize.struct {
      {
        'sources',
        list(report.sources, function(row: Contribution): Value
          return serialize.struct {
            { 'source_index', row.sourceIndex - 1 },
            { 'source', sourceMeta(row.source) },
            { 'winning_files', row.winningFiles },
            { 'overriding_files', row.overridingFiles },
            { 'overridden_files', row.overriddenFiles },
            { 'unique_files', row.uniqueFiles },
            { 'duplicate_files', row.duplicateFiles },
            { 'loose_files', row.looseFiles },
            { 'archive_files', row.archiveFiles },
          }
        end),
      },
    }
  end,

  plan = function(report: Plan): Value
    local function row({kind, key, source, archive, dest}: PlanRow): Value
      local fields: { { unknown } } = { { 'key', lossy(key) } }
      for _, pair in { { 'source', source }, { 'archive', archive }, { 'dest', dest } } do
        local value = pair[2]
        if value then table.insert(fields, { pair[1], lossy(value) }) end
      end

      return serialize.variant(snake(kind), serialize.struct(fields))
    end

    return serialize.struct {
      { 'actions', list(report.actions, row) },
      { 'issues', list(report.issues, row) },
    }
  end,

  conflicts = function(report: Conflicts): Value
    return serialize.struct {
      {
        'sources',
        list(report.sources, function({path, overrides, overriddenBy}): Value
          return serialize.struct {
            { 'path', lossy(path) },
            { 'overrides', lossyList(overrides) },
            { 'overridden_by', lossyList(overriddenBy) },
          }
        end),
      },
    }
  end,

  shadowed = function(report: Shadowed): Value
    return serialize.struct {
      {
        'sources',
        list(report.sources, function({path, shadowedFiles}): Value
          local fields: { { unknown } } = { { 'path', lossy(path) } }
          -- `skip_serializing_if = "Vec::is_empty"`.
          if #shadowedFiles > 0 then
            table.insert(fields, { 'shadowed_files', lossyList(shadowedFiles) })
          end

          return serialize.struct(fields)
        end),
      },
    }
  end,

  diff = function({sourceA, sourceB, higherPriority, shared, onlyInA, onlyInB}: Diff): Value
    return serialize.struct {
      { 'source_a', lossy(sourceA) },
      { 'source_b', lossy(sourceB) },
      { 'higher_priority', lossy(higherPriority) },
      { 'shared', serialize.array((shared :: unknown) :: { unknown }) },
      { 'only_in_a', serialize.array((onlyInA :: unknown) :: { unknown }) },
      { 'only_in_b', serialize.array((onlyInB :: unknown) :: { unknown }) },
    }
  end,

  lock = function({schemaVersion, entries}: Lock): Value
    local function entry(
      {key, winnerSource, winnerKind, winnerHashBlake3, winnerSize, providerCount}: LockEntry
    ): Value
      return serialize.struct {
        { 'key', lossy(key) },
        { 'winner_source', lossy(winnerSource) },
        { 'winner_kind', if winnerKind == 'looseDir' then 'loose_dir' else 'archive' },
        { 'winner_hash_blake3', optional(winnerHashBlake3) },
        { 'winner_size', optional(winnerSize) },
        { 'provider_count', providerCount },
      }
    end

    return serialize.struct { { 'schema_version', schemaVersion }, { 'entries', list(entries, entry) } }
  end,

  drift = function(report: Drift): Value
    local counts = serialize.map()
    -- A `BTreeMap<DriftKind, _>`: the variants' declaration order.
    for _, kind in
      { 'added', 'removed', 'winnerSourceChanged', 'winnerHashChanged', 'providerCountChanged' }
    do
      local count = report.counts[kind]
      if count then serialize.set(counts, snake(kind), count) end
    end

    return serialize.struct {
      {
        'entries',
        list(report.entries, function({key, kind}): Value
          return serialize.struct { { 'key', lossy(key) }, { 'kind', snake(kind) } }
        end),
      },
      { 'counts', counts },
    }
  end,
}

--- `write_serialized`: `text` to standard output with a newline, or to `path` as it is, its
--- folder made first. Nil, or the binary's message for what failed.
local function writeText(path: string?, text: string): string?
  if not path then
    cli.out(text)
    return nil
  end

  local parent = paths.parent(path)
  if parent and parent ~= '' then
    local made, message = fs.mkdir(parent, { recursive = true })
    if not made then
      return `failed to create output directory '{parent}': {string.match(message or '', ': ([^:]*)$') or message}`
    end
  end

  local written, message = fs.writeFile(path, text)
  if not written then
    return `failed to create output file '{path}': {string.match(message or '', ': ([^:]*)$') or message}`
  end

  return nil
end

--- A report in the options' format and place.
local function write(options: cli.Options, value: Value): string?
  return writeText(options.output, serialize.encode(value, options.format or 'yaml'))
end

--- `write_serialized_list`: a list report; TOML has no top-level arrays, so there it is the one
--- entry of a table, under `key`.
local function writeList(options: cli.Options, key: string, items: Value): string?
  if options.format == 'toml' then
    local wrapped = serialize.map()
    serialize.set(wrapped, key, items)
    return write(options, wrapped)
  end

  return write(options, items)
end

--- A VFS tree, as `VFS::serialize_from_tree` wrote it.
local function writeTree(options: cli.Options, built: unknown): string?
  local encode = require '@vfs/encode'
  local internal = require '@vfs/core'
  local tree = internal.trees[built]
  return writeText(options.output, serialize.encode(encode.displayTree(tree), options.format or 'yaml'))
end

return table.freeze {
  Shapes = Shapes,
  write = write,
  writeList = writeList,
  writeTree = writeTree,
}
