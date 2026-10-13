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

local function sourceMeta(source: Source): Value
  return serialize.struct { { 'path', lossy(source.path) }, { 'kind', Kinds[source.kind] } }
end

--- `VfsProviderRecord`.
local function providerRecord(row: Record): Value
  return serialize.struct {
    { 'source_index', row.sourceIndex - 1 },
    { 'source', sourceMeta(row.source) },
    { 'key', lossy(row.key) },
    { 'original_path', lossy(row.originalPath) },
    { 'resolved_path', row.resolvedPath },
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
  explain = function(report: Explain): Value
    return serialize.struct {
      { 'key', lossy(report.key) },
      { 'winner', providerRecord(report.winner) },
      { 'overridden', list(report.overridden, providerRecord) },
    }
  end,

  duplicates = function(report: Duplicates): Value
    return serialize.struct {
      {
        'entries',
        list(report.entries, function(entry: DuplicateEntry): Value
          return serialize.struct {
            { 'key', lossy(entry.key) },
            { 'providers', list(entry.providers, providerRecord) },
            { 'winner_index', entry.winnerIndex - 1 },
          }
        end),
      },
    }
  end,

  archive = function(info: ArchiveInfo): Value
    return serialize.struct {
      { 'source_index', info.sourceIndex - 1 },
      { 'path', lossy(info.path) },
      { 'entry_count', info.entryCount },
      { 'winning_entry_count', info.winningEntryCount },
    }
  end,

  archiveEntry = function(entry: ArchiveEntry): Value
    return serialize.struct {
      { 'key', lossy(entry.key) },
      { 'archive_path', lossy(entry.archivePath) },
      { 'original_path', lossy(entry.originalPath) },
      { 'wins', entry.wins },
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
    local function row(entry: PlanRow): Value
      local fields: { { unknown } } = { { 'key', lossy(entry.key) } }
      for _, pair in { { 'source', entry.source }, { 'archive', entry.archive }, { 'dest', entry.dest } } do
        local value = pair[2]
        if value then table.insert(fields, { pair[1], lossy(value) }) end
      end

      return serialize.variant(snake(entry.kind), serialize.struct(fields))
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
        list(report.sources, function(source): Value
          return serialize.struct {
            { 'path', lossy(source.path) },
            { 'overrides', lossyList(source.overrides) },
            { 'overridden_by', lossyList(source.overriddenBy) },
          }
        end),
      },
    }
  end,

  shadowed = function(report: Shadowed): Value
    return serialize.struct {
      {
        'sources',
        list(report.sources, function(source): Value
          local fields: { { unknown } } = { { 'path', lossy(source.path) } }
          -- `skip_serializing_if = "Vec::is_empty"`.
          if #source.shadowedFiles > 0 then
            table.insert(fields, { 'shadowed_files', lossyList(source.shadowedFiles) })
          end

          return serialize.struct(fields)
        end),
      },
    }
  end,

  diff = function(report: Diff): Value
    return serialize.struct {
      { 'source_a', lossy(report.sourceA) },
      { 'source_b', lossy(report.sourceB) },
      { 'higher_priority', lossy(report.higherPriority) },
      { 'shared', serialize.array((report.shared :: unknown) :: { unknown }) },
      { 'only_in_a', serialize.array((report.onlyInA :: unknown) :: { unknown }) },
      { 'only_in_b', serialize.array((report.onlyInB :: unknown) :: { unknown }) },
    }
  end,

  lock = function(report: Lock): Value
    return serialize.struct {
      { 'schema_version', report.schemaVersion },
      {
        'entries',
        list(report.entries, function(entry): Value
          return serialize.struct {
            { 'key', lossy(entry.key) },
            { 'winner_source', lossy(entry.winnerSource) },
            { 'winner_kind', if entry.winnerKind == 'looseDir' then 'loose_dir' else 'archive' },
            { 'winner_hash_blake3', optional(entry.winnerHashBlake3) },
            { 'winner_size', optional(entry.winnerSize) },
            { 'provider_count', entry.providerCount },
          }
        end),
      },
    }
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
        list(report.entries, function(entry): Value
          return serialize.struct { { 'key', lossy(entry.key) }, { 'kind', snake(entry.kind) } }
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
