--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- What every vfstool command shares: the openmw.cfg it reads, the VFS it builds from it, the
-- reports as the vfstool binary serialized its Rust structs (snake_case fields, 0-based source
-- indices, externally tagged enums), the output file or standard output, and the messages.
local fs = require '@dream/fs'
local openmwConfig = require '@openmw-config'
local paths = require '@vfs/paths'
local process = require '@dream/process'
local serialize = require '@jess/data/value'
local vfs = require '@vfs'

type Value = serialize.Value

export type Format = 'json' | 'yaml' | 'toml'

--- Commands require a config file or its directory, or OPENMW_CONFIG. The kernel does not
--- currently expose the platform facts needed for trustworthy default discovery.
export type Options = {
  config: string?,
  tokens: openmwConfig.Tokens?,
  relative: boolean?,
  format: Format?,
  output: string?,
}

--- The binary's exit codes.
local Exit = table.freeze {
  ok = 0,
  findFailed = 1,
  notLoose = 2,
  drift = 4,
  validation = 5,
  badRegex = 6,
  config = 7,
  invalidInput = 8,
  runtime = 9,
}

local Red, Green, Blue, Reset = '\27[31m', '\27[32m', '\27[34m', '\27[0m'

--- Whether `stream` gets ANSI colors: a terminal, and `NO_COLOR` unset or empty.
local function colors(stream: 'stdout' | 'stderr'): boolean
  local noColor = process.env 'NO_COLOR'
  return (noColor == nil or noColor == '') and process.isTerminal(stream)
end

local function paint(stream: 'stdout' | 'stderr', color: string, text: string): string
  if colors(stream) then return `{color}{text}{Reset}` end

  return text
end

local function out(text: string) process.write('stdout', `{text}\n`) end

local function err(text: string) process.write('stderr', `{text}\n`) end

local function errPrefix(): string return `{paint('stderr', Red, '[ ERROR ]')}: ` end

local function successPrefix(): string return `{paint('stdout', Green, '[ SUCCESS ]')}: ` end

local function blue(stream: 'stdout' | 'stderr', text: string): string
  return paint(stream, Blue, text)
end

local function green(stream: 'stdout' | 'stderr', text: string): string
  return paint(stream, Green, text)
end

--- The openmw.cfg in `dir`, found without regard to case, or nil with the message printed.
local function configIn(dir: string): string?
  local info, statError = fs.stat(dir)
  if statError then
    err(`{errPrefix()}Failed to inspect OpenMW config directory '{dir}': {statError}`)
    return nil
  end

  if not info or not info.isDir then
    err(`{errPrefix()}The requested openmw.cfg directory '{dir}' does not exist or is not a directory.`)
    return nil
  end

  local names, listError = fs.list(dir)
  if not names then
    err(`{errPrefix()}Failed to list OpenMW config directory '{dir}': {listError}`)
    return nil
  end

  for _, name in names do
    if string.lower(name) == 'openmw.cfg' then return paths.join(dir, name) end
  end

  err(`{errPrefix()}No openmw.cfg found in '{dir}'.`)
  return nil
end

--- Explicit paths only: --config or OPENMW_CONFIG, each a file or a directory holding it.
local function resolveConfig(options: Options): string?
  local selected = options.config or process.env 'OPENMW_CONFIG'
  if not selected or selected == '' then
    err(
      `{errPrefix()}An explicit OpenMW config is required: pass --config or set OPENMW_CONFIG. Default discovery is unavailable.`
    )
    return nil
  end

  local info, message = fs.stat(selected)
  if message then
    err(`{errPrefix()}Failed to inspect OpenMW config '{selected}': {message}`)
    return nil
  end

  if info and info.isFile then return selected end

  if info and info.isDir then return configIn(selected) end

  err(`{errPrefix()}The requested OpenMW config '{selected}' does not exist or is not a file or directory.`)
  return nil
end

export type Load = {
  path: string,
  config: openmwConfig.Config,
  dirs: { string },
  archives: { string },
}

--- The config `options` name, loaded; nil and the binary's exit code when it can't be.
local function load(options: Options): (Load?, number)
  local path = resolveConfig(options)
  if not path then return nil, Exit.config end

  local loaded = openmwConfig.new(path, { tokens = options.tokens })
  return {
    path = path,
    config = loaded,
    dirs = loaded:dataDirectories(),
    archives = loaded:values('fallback-archive'),
  },
    Exit.ok
end

--- The VFS the config describes: its data directories over its fallback archives.
local function build(loaded: Load): vfs.VFS
  return vfs.VFS.fromDirectories(loaded.dirs, { archives = loaded.archives })
end

local function lossy(text: string): string return paths.lossy(text) end

local function lossyList(list: { string }): Value
  local items = table.create(#list)
  for index, item in list do
    items[index] = lossy(item)
  end

  return serialize.array(items)
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
type ArchiveInfo = { sourceIndex: number, path: string, entryCount: number, winningEntryCount: number }
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
  local items = table.create(#rows)
  for index, row in rows do
    items[index] = each(row)
  end

  return serialize.array(items)
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
    out(text)
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
local function write(options: Options, value: Value): string?
  return writeText(options.output, serialize.encode(value, options.format or 'yaml'))
end

--- `write_serialized_list`: a list report; TOML has no top-level arrays, so there it is the one
--- entry of a table, under `key`.
local function writeList(options: Options, key: string, items: Value): string?
  if options.format == 'toml' then
    local wrapped = serialize.map()
    serialize.set(wrapped, key, items)
    return write(options, wrapped)
  end

  return write(options, items)
end

--- A VFS tree, as `VFS::serialize_from_tree` wrote it.
local function writeTree(options: Options, built: unknown): string?
  local encode = require '@vfs/encode'
  local internal = require '@vfs/core'
  local tree = internal.trees[built]
  return writeText(options.output, serialize.encode(encode.displayTree(tree), options.format or 'yaml'))
end

return table.freeze {
  Exit = Exit,
  out = out,
  err = err,
  errPrefix = errPrefix,
  successPrefix = successPrefix,
  blue = blue,
  green = green,
  load = load,
  build = build,
  Shapes = Shapes,
  lossy = lossy,
  write = write,
  writeList = writeList,
  writeTree = writeTree,
  writeText = writeText,
}
