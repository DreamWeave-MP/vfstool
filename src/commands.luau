--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- vfstool's commands, each a function of its options returning the binary's exit code. Reports go
-- where `output` names, or to standard output; messages to standard error, as the binary printed
-- them.
local cli = require './cli'
local fs = require '@dream/fs'
local paths = require '@vfs/paths'
local process = require '@dream/process'

type Options = cli.Options
type Load = cli.Load

local Exit = cli.Exit

--- The reports and what writes them, required by the first command with a report to write:
--- finding a file, extracting one, collapsing and running write none, and never compile them.
local function reports(): typeof(require './reports') return require './reports' end

--- A report written, or the failure as the binary's `vfstool: <error>` and exit 9.
local function finished(problem: string?): number
  if not problem then return Exit.ok end

  cli.err(`vfstool: {problem}`)
  return Exit.runtime
end

--- The config, or the exit code that ends the command.
local function loaded(options: Options): (Load?, number)
  return cli.load(options)
end

--- A file's or directory's name as a VFS key spells it: ASCII lowercase, `\` read as `/`.
local function component(name: string): string return paths.normalizeHost(name) end

--- What a folder holds, by the VFS key a name spells: the names that spell it, in the order the
--- folder lists them.
type Listing = { [string]: { string } }

--- The folders one command has listed, by path. A command that looks for many files walks the
--- data directories for each, and lists a folder the first time alone.
type Listings = { [string]: Listing }

local function listed(listings: Listings, dir: string): Listing
  local known = listings[dir]
  if known then return known end

  local listing: Listing = {}
  for _, name in fs.list(dir) or {} do
    local key = component(name)
    local names = listing[key]
    if names then
      table.insert(names, name)
    else
      listing[key] = { name }
    end
  end

  listings[dir] = listing
  return listing
end

--- `resolve_case_insensitive_file`: the file `components` names below `dir`, matched without
--- regard to case; of several, the greatest path.
local function resolveLoose(listings: Listings, dir: string, components: { string }): string?
  local candidates = { dir }
  for index, wanted in components do
    local last = index == #components
    local matches = {}
    for _, candidate in candidates do
      local names = listed(listings, candidate)[wanted]
      if not names then continue end

      for _, name in names do
        local path = paths.join(candidate, name)
        local stat = fs.stat(path)
        if stat and (if last then stat.isFile else stat.isDir) then table.insert(matches, path) end
      end
    end

    if #matches == 0 then return nil end

    candidates = matches
  end

  paths.sortPaths(candidates)
  return candidates[#candidates]
end

--- `find_loose_file_fast`: the loose file a VFS path names in the last data directory that has it.
local function findLoose(listings: Listings, dirs: { string }, path: string): string?
  local key = paths.safeKeyBytes(path)
  if not key then return nil end

  local components = {}
  for part in string.gmatch(key, '[^/]+') do
    table.insert(components, part)
  end

  if #components == 0 then return nil end

  local first = components[1]
  for index = #dirs, 1, -1 do
    local dir = dirs[index]
    -- A folder with nothing of the path's first name has no such file, and costs one lookup.
    if not listed(listings, dir)[first] then continue end

    local found = resolveLoose(listings, dir, components)
    if found then return found end
  end

  return nil
end

local commands = {}

export type FindFileOptions = Options & { path: string, simple: boolean?, onlyPhysical: boolean? }

local function findFileSuccess(path: string, display: string, simple: boolean?)
  if simple then
    cli.out(display)
    return
  end

  cli.out(
    `{cli.successPrefix()}Successfully found VFS File {cli.blue('stdout', path)} at path {cli.green('stdout', display)}`
  )
end

local function findFileMissing(path: string, simple: boolean?, onlyPhysical: boolean)
  if simple then return end

  local where = if onlyPhysical then 'loose files of the provided VFS' else 'the provided VFS'
  cli.err(`{cli.errPrefix()}Failed to locate {cli.blue('stderr', path)} in {where}.`)
end

--- Where a VFS path's file is: exit 1 when nothing has it, 2 when only an archive does and
--- `onlyPhysical` asks for a loose file.
function commands.findFile(options: FindFileOptions): number
  local config, code = loaded(options)
  if not config then return code end

  local loose = findLoose({}, config.dirs, options.path)
  if loose then
    findFileSuccess(options.path, loose, options.simple)
    return Exit.ok
  end

  local file = cli.build(config):getFile(options.path)
  if not file then
    findFileMissing(options.path, options.simple, false)
    return Exit.findFailed
  end

  if file:isArchive() and options.onlyPhysical then
    findFileMissing(options.path, options.simple, true)
    return Exit.notLoose
  end

  local display = if file:isArchive()
    then paths.join(file:parentArchivePath() or '', options.path)
    else file:path()
  findFileSuccess(options.path, paths.lossy(display), options.simple)
  return Exit.ok
end

export type FindOptions = Options & { path: string }

--- Every VFS path matching a regular expression, as a tree.
function commands.find(options: FindOptions): number
  local config, code = loaded(options)
  if not config then return code end

  local vfs = cli.build(config)
  return finished(reports().writeTree(options, vfs:findByRegex(options.path, options.relative == true)))
end

--- Whether `path` is one of the config's data directories, spelled any way; else the binary's
--- message and exit 8.
local function configured(config: Load, path: string): boolean
  local wanted = paths.normalizeHost(path)
  for _, dir in config.dirs do
    if paths.samePath(paths.normalizeHost(dir), wanted) then return true end
  end

  cli.err(`{cli.errPrefix()}{path} is not a configured data directory.`)
  return false
end

export type RemainingOptions = Options & { filterPath: string, replacementsOnly: boolean? }

--- What a data directory still provides, or what replaces it.
function commands.remaining(options: RemainingOptions): number
  local config, code = loaded(options)
  if not config then return code end

  if not configured(config, options.filterPath) then return Exit.invalidInput end

  local vfs = cli.build(config)
  local tree = vfs:remaining(
    options.filterPath,
    options.replacementsOnly == true,
    config.dirs,
    options.relative == true
  )
  return finished(reports().writeTree(options, tree))
end

export type CollapseOptions = Options & {
  into: string,
  allowCopying: boolean?,
  extractArchives: boolean?,
  symbolic: boolean?,
  dryRun: boolean?,
}

--- The VFS as one folder of links, or with `dryRun` the plan for it.
function commands.collapse(options: CollapseOptions): number
  local config, code = loaded(options)
  if not config then return code end

  local vfs = cli.build(config)
  local chosen = {
    allowCopying = options.allowCopying == true,
    extractArchives = options.extractArchives == true,
    useSymlinks = options.symbolic == true,
  }

  if options.dryRun then
    return finished(reports().write(options, reports().Shapes.plan(vfs:materializationPlan(options.into, chosen))))
  end

  for _, skip in vfs:collapseInto(options.into, chosen) do
    if skip.kind == 'archiveEntry' then
      cli.err(`vfstool: skipping {paths.lossy(skip.key)}, loaded from archive: {paths.lossy(skip.archive or '')}`)
    elseif skip.kind == 'archiveFile' then
      local archive = skip.archive or ''
      cli.err(`vfstool: skipping archive {paths.lossy(paths.fileName(archive) or archive)}`)
    else
      cli.err(`vfstool: skipping {paths.lossy(skip.key)}`)
    end
  end

  return Exit.ok
end

export type ExtractOptions = Options & { source: string, target: string }

--- One VFS file into a folder.
function commands.extract(options: ExtractOptions): number
  local config, code = loaded(options)
  if not config then return code end

  local vfs = cli.build(config)
  local destination = vfs:extractFile(options.source, options.target)

  if not destination then
    cli.err(
      `{cli.errPrefix()}Couldn't locate {cli.green('stderr', options.source)} in the vfs!`
    )
    return Exit.findFailed
  end

  local folder = paths.parent(destination :: string) or options.target
  cli.out(
    `{cli.successPrefix()}Successfully extracted {cli.green('stdout', options.source)} to {cli.blue('stdout', folder)}`
  )
  return Exit.ok
end

export type ExplainOptions = Options & { path: string }

--- Every provider of one VFS path, winner first.
function commands.explain(options: ExplainOptions): number
  local config, code = loaded(options)
  if not config then return code end

  local report = cli.build(config):explain(options.path)
  if not report then
    cli.err(`{cli.errPrefix()}VFS path '{options.path}' not found.`)
    return Exit.findFailed
  end

  return finished(reports().write(options, reports().Shapes.explain(report)))
end

export type DuplicatesOptions = Options & { pattern: string? }

--- Every VFS path with more than one provider, optionally only those matching a pattern.
function commands.duplicates(options: DuplicatesOptions): number
  local config, code = loaded(options)
  if not config then return code end

  local vfs = cli.build(config)
  return finished(reports().write(options, reports().Shapes.duplicates(vfs:duplicates(options.pattern))))
end

--- The loaded archives and how many of their entries win.
function commands.archives(options: Options): number
  local config, code = loaded(options)
  if not config then return code end

  local rows = cli.build(config):archives()
  local shape = reports().Shapes.archive
  local items = table.create(#rows)
  for index, row in rows do
    items[index] = shape(row)
  end

  return finished(reports().writeList(options, 'archives', (require '@jess/data/value').array(items)))
end

export type ArchiveListOptions = Options & { archive: string?, sourceIndex: number? }

local function choices(rows: { cli.ArchiveInfo }): string
  if #rows == 0 then return '<none>' end

  local listed = table.create(#rows)
  for index, row in rows do
    listed[index] = `{row.sourceIndex - 1}={row.path}`
  end

  return table.concat(listed, ', ')
end

--- Whether `selector` names `archive`: its whole path, its file name, or a path suffix.
local function selects(selector: string, archive: string): boolean
  local wanted, path = paths.normalizeHost(selector), paths.normalizeHost(archive)
  if wanted == path then return true end

  local name = paths.fileName(archive)
  if name and paths.normalizeHost(name) == wanted then return true end

  if #wanted > #path or string.sub(path, -#wanted) ~= wanted then return false end

  local prefix = string.sub(path, 1, #path - #wanted)
  return prefix == '' or string.sub(prefix, -1) == '/'
end

--- What one archive supplies, chosen by path, file name, path suffix, or source index.
function commands.archiveList(options: ArchiveListOptions): number
  local config, code = loaded(options)
  if not config then return code end

  local vfs = cli.build(config)
  local rows = vfs:archives()
  local chosen: string
  if options.sourceIndex then
    local found
    for _, row in rows do
      if row.sourceIndex - 1 == options.sourceIndex then found = row.path end
    end

    if not found then
      cli.err(
        `{cli.errPrefix()}No loaded archive has source index {options.sourceIndex}. Available archives: {choices(rows)}`
      )
      return Exit.invalidInput
    end

    chosen = found
  else
    local selector = options.archive
    if not selector then
      cli.err(
        `{cli.errPrefix()}archive-list needs an archive selector or --source-index. Available archives: {choices(rows)}`
      )
      return Exit.invalidInput
    end

    local matches = {}
    for _, row in rows do
      if selects(selector, row.path) then table.insert(matches, row) end
    end

    if #matches == 0 then
      cli.err(
        `{cli.errPrefix()}No loaded archive matches '{selector}'. Available archives: {choices(rows)}`
      )
      return Exit.invalidInput
    end

    if #matches > 1 then
      cli.err(
        `{cli.errPrefix()}Archive selector '{selector}' is ambiguous. Use --source-index or a longer path suffix. Matches: {choices(matches)}`
      )
      return Exit.invalidInput
    end

    chosen = matches[1].path
  end

  local entries = vfs:archiveEntries(chosen)
  local shape = reports().Shapes.archiveEntry
  local items = table.create(#entries)
  for index, entry in entries do
    items[index] = shape(entry)
  end

  return finished(reports().writeList(options, 'entries', (require '@jess/data/value').array(items)))
end

--- What each source provides, wins, overrides, and loses.
function commands.contributions(options: Options): number
  local config, code = loaded(options)
  if not config then return code end

  return finished(reports().write(options, reports().Shapes.contributions(cli.build(config):sourceContributions())))
end

export type ValidateOptions = Options & { full: boolean? }

--- Missing data directories, archives, content and groundcover files: exit 5 when any is.
function commands.validate(options: ValidateOptions): number
  local config, code = loaded(options)
  if not config then return code end

  local serialize = require '@jess/data/value'
  local issues = {}
  local function issue(kind: string, fields: { { unknown } })
    table.insert(issues, serialize.variant(kind, serialize.struct(fields)))
  end

  for _, dir in config.dirs do
    local stat = fs.stat(dir)
    if not stat then
      issue('MissingDataDirectory', { { 'path', paths.lossy(dir) } })
    elseif not stat.isDir then
      issue('DataPathNotDirectory', { { 'path', paths.lossy(dir) } })
    end
  end

  local vfs = if options.full then cli.build(config) else nil
  local listings: Listings = {}
  local has: (name: string) -> boolean = if vfs
    then function(name: string): boolean return vfs:getFile(name) ~= nil end
    else function(name: string): boolean return findLoose(listings, config.dirs, name) ~= nil end

  local loadedArchives: { [string]: boolean } = {}
  if vfs then
    for _, row in vfs:archives() do
      loadedArchives[row.path] = true
    end
  end

  for _, name in config.archives do
    if not has(name) then
      issue('MissingFallbackArchive', { { 'name', name } })
      continue
    end

    local file = if vfs then vfs:getFile(name) else nil
    if file then
      local path = file:path()
      if not loadedArchives[path] then
        issue('UnreadableFallbackArchive', { { 'name', name }, { 'path', paths.lossy(path) } })
      end
    end
  end

  for _, name in config.config:values('content') do
    if not has(name) then issue('MissingContentFile', { { 'name', name } }) end
  end

  for _, name in config.config:values('groundcover') do
    if not has(name) then issue('MissingGroundcoverFile', { { 'name', name } }) end
  end

  local problem = reports().write(options, serialize.struct { { 'issues', serialize.array(issues) } })
  if problem then return finished(problem) end

  return if #issues > 0 then Exit.validation else Exit.ok
end

--- The conflict index of the config's load order.
local function conflictIndex(config: Load): cli.ConflictIndex
  local _, index =
    cli.library().VFS.fromDirectoriesWithConflictIndex(config.dirs, { archives = config.archives })
  return index
end

--- Each source's overrides and what overrides it.
function commands.conflicts(options: Options): number
  local config, code = loaded(options)
  if not config then return code end

  local report = conflictIndex(config):conflictsReport(options.relative == true)
  return finished(reports().write(options, reports().Shapes.conflicts(report)))
end

export type ShadowedOptions = Options & { listFiles: boolean? }

--- The sources every file of which a later source overrides.
function commands.shadowed(options: ShadowedOptions): number
  local config, code = loaded(options)
  if not config then return code end

  local report = conflictIndex(config):shadowedReport(options.relative == true, options.listFiles == true)
  cli.err(`{#report.sources} sources are fully shadowed`)
  return finished(reports().write(options, reports().Shapes.shadowed(report)))
end

export type DiffOptions = Options & { sourceA: string, sourceB: string }

--- What two data directories share and what only one of them has.
function commands.diff(options: DiffOptions): number
  local config, code = loaded(options)
  if not config then return code end

  if not configured(config, options.sourceA) or not configured(config, options.sourceB) then
    return Exit.invalidInput
  end

  local report = conflictIndex(config):diffReport(options.sourceA, options.sourceB)
  return finished(reports().write(options, reports().Shapes.diff(report)))
end

--- The VFS and its layer index.
local function layered(config: Load): (cli.VFS, cli.LayerIndex)
  return cli.library().VFS.fromDirectoriesWithLayerIndex(config.dirs, { archives = config.archives })
end

--- A deterministic manifest of every winner.
function commands.lock(options: Options): number
  local config, code = loaded(options)
  if not config then return code end

  local vfs, index = layered(config)
  local lock = index:lockManifest(vfs)

  return finished(reports().write(options, reports().Shapes.lock(lock:toTable())))
end

export type DriftOptions = Options & { lockFile: string, failOnDrift: boolean? }

--- The VFS against a lock manifest; exit 4 for drift when `failOnDrift`. The lock reader and the
--- decoders it reads with are required here: no other command compiles them.
function commands.drift(options: DriftOptions): number
  local expected, problem = (require './lock').read(options.lockFile)
  if not expected then return finished(problem) end

  local config, code = loaded(options)
  if not config then return code end

  local vfs = cli.build(config)
  local internal = require '@vfs/core'
  local state = internal.stateOf(vfs)
  if not state then return finished('could not access VFS state') end

  local report = (require '@vfs/layer').drift(internal.layerOf(state), state, expected)

  local written = reports().write(options, reports().Shapes.drift(report))
  if written then return finished(written) end

  return if #report.entries > 0 and options.failOnDrift then Exit.drift else Exit.ok
end

export type RunOptions = Options & {
  merged: string,
  command: { string },
  keepMerged: boolean?,
  copy: boolean?,
  workingDir: string?,
}

--- Whether `path` can be `run`'s merged folder: nothing there yet, or an empty directory.
local function freshDirectory(path: string): (boolean?, string?)
  local stat, message = fs.lstat(path)
  if message then return nil, message end

  if not stat then return true, nil end

  if not stat.isDir or stat.isSymlink then return false, nil end

  local names, listed = fs.list(path)
  if not names then return nil, listed end

  return #names == 0, nil
end

--- How a child ended, as Rust's `ExitStatus` prints it.
local function status(result: dream_process_Result): string
  if result.code then return `exit status: {result.code}` end

  return `signal: {result.signal}`
end

--- Dumps the VFS into a merged folder, runs a command on it (`{}` in its arguments is the
--- folder), and copies what it changed into data-local or `output`; the command's exit code.
--- Needs `process.spawn`.
function commands.run(options: RunOptions): number
  local config, code = loaded(options)
  if not config then return code end

  local destination = options.output or config.config:path('data-local')
  if not destination then
    cli.err(`{cli.errPrefix()}No data-local set in openmw.cfg; use --output to specify a destination.`)
    return Exit.invalidInput
  end

  local merged = options.merged
  local fresh, problem = freshDirectory(merged)
  if problem then return finished(problem) end

  if not fresh then
    cli.err(
      `{cli.errPrefix()}merged directory {merged} already exists and is not an empty directory; choose an empty scratch directory.`
    )
    return Exit.invalidInput
  end

  local vfs = cli.build(config)
  local function cleanup(): string?
    if options.keepMerged then return nil end

    local removed, message = fs.remove(merged, { recursive = true })
    if removed then return nil end

    return `failed to remove merged dir '{merged}': {string.match(message or '', ': ([^:]*)$') or message}`
  end

  cli.err(`Dumping VFS to {merged}...`)
  local count, baseline = cli.library().runSetupTracked(vfs, merged, not options.copy)

  cli.err(`Dumped {count} files.`)
  local absolute = fs.absolute(merged) or merged
  local argv = (table.create(#options.command) :: unknown) :: { string }
  for index, argument in options.command do
    argv[index] = if argument == '{}' then absolute else argument
  end

  local program = table.remove(argv, 1) :: string
  local result, failure = process.run(program, argv, { cwd = options.workingDir })
  if not result then
    cleanup()
    return finished(`failed to start '{program}': {string.match(failure or '', ': ([^:]*)$') or failure}`)
  end

  local exitCode = result.code or Exit.runtime
  if not result.success then
    cli.err(`vfstool: subprocess exited with {status(result)}, not capturing files.`)
    local failed = cleanup()
    if failed then cli.err(`vfstool: {failed}`) end

    return exitCode
  end

  local copied = cli.library().runFinalizeTracked(merged, destination, baseline)

  local rows = copied
  if #rows == 0 then
    cli.err 'No files changed.'
  else
    cli.err(`Capturing {#rows} changed file(s) to {destination}...`)
    for _, row in rows do
      cli.out(`{row.relativePath} -> {row.destinationPath}`)
    end
  end

  local failed = cleanup()
  if failed then return finished(failed) end

  return exitCode
end

local _ = process
return table.freeze(commands)
