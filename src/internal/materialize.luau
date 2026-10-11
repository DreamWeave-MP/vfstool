--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- Writing a VFS out: every winner dumped into a folder (linked or copied, archive entries
-- extracted), collapsed into one folder (hard or symbolic links, archives skipped or extracted),
-- one file extracted, and the plan a collapse would follow. Every output path is checked first:
-- a key that could leave the folder, or a symbolic link where a parent folder should be, stops it.
local check = require './check'
local file = require './file'
local fs = require '@dream/fs'
local paths = require './paths'
local types = require './types'

type Entry = types.Entry
type State = types.State

export type Options = { allowCopying: boolean, extractArchives: boolean, useSymlinks: boolean }

export type Skip = {
  kind: string,
  key: string,
  archive: string?,
  source: string?,
  error: string?,
}
export type PlanRow = { kind: string, key: string, source: string?, archive: string?, dest: string? }
export type Plan = { actions: { PlanRow }, issues: { PlanRow } }

--- What a failed step answers: the crate's `io::Error` text, without the `dream.vfs: ` prefix the
--- binding adds once, when the public function raises it.
type Problem = string?

local function raise(problem: string): never return check.vfs(problem) end

--- `lstat`, or the OS's reason it couldn't ask.
local function lstat(path: string): (dream_fs_Stat?, Problem)
  local stat, message = fs.lstat(path)
  if message then return nil, check.osError(message) end

  return stat, nil
end

--- Whether `path` exists; a path the OS can't ask about counts as nothing, as `Path::exists`
--- counted it.
local function exists(path: string): boolean return fs.exists(path) == true end

--- Whether `destination` already is the file `source` (a hard link to it included).
local function sameFile(source: string, destination: string): (boolean, Problem)
  local same, message = fs.sameFile(source, destination)
  if same == nil then return false, check.osError(message) end

  return same, nil
end

--- The reason a `@dream/fs` call that answered `ok, message` failed, or nil when it didn't.
local function refused(ok: unknown, message: string?): Problem
  if ok == nil then return check.osError(message) end

  return nil
end

local function createDirAll(path: string): Problem
  return refused(fs.mkdir(path, { recursive = true }))
end

--- `remove_existing_output_file`: a file or link at `path` goes; a directory is refused.
local function removeExistingOutput(path: string): Problem
  local stat, problem = lstat(path)
  if problem then return problem end

  if not stat then return nil end

  if stat.isDir and not stat.isSymlink then
    return `destination is a directory: {paths.lossy(path)}`
  end

  return refused(fs.remove(path))
end

local function copyReplacing(source: string, destination: string): Problem
  local problem = removeExistingOutput(destination)
  if problem then return problem end

  return refused(fs.copy(source, destination))
end

--- `ensure_output_parent_safe`: `output` is under `root`, and neither `root` nor any existing
--- folder between them is a symbolic link.
local function ensureParentSafe(root: string, output: string): Problem
  local relative = paths.stripPrefix(output, root)
  if not relative then return 'output path should be under root' end

  local rootStat, problem = lstat(root)
  if problem then return problem end

  if rootStat and rootStat.isSymlink then return `output root is a symlink: {paths.lossy(root)}` end

  local components = paths.components(relative)
  local current = root
  for index = 1, #components - 1 do
    current = paths.join(current, components[index])
    local stat, failed = lstat(current)
    if failed then return failed end

    if not stat then break end

    if stat.isSymlink then return `output parent is a symlink: {paths.lossy(current)}` end
  end

  return nil
end

--- `validate_materialization_paths`: every key is one that can be written below a folder.
local function validateKeys(vfs: State)
  for key in vfs.files do
    if not paths.isSafeNormalized(key) then
      raise(`VFS key '{paths.lossy(key)}' cannot be safely materialized`)
    end
  end
end

--- `materialization_path`: the key as a relative host path.
local function keyPath(key: string): string
  if string.find(key, '\0', 1, true) then
    raise(`VFS key '{paths.lossy(key)}' cannot be represented as a path on this platform`)
  end

  return key
end

--- `is_archive_file`: a loose `.bsa` or `.ba2` the VFS would read as an archive.
local function isArchiveFile(entry: Entry): boolean
  local extension = paths.extension(entry.path)
  if not extension then return false end

  local lowered = string.lower(extension)
  local name = string.lower(paths.fileName(entry.path) or '')
  return (lowered == 'bsa' or lowered == 'ba2') and name ~= 'archiveinvalidationinvalidated!.bsa'
end

local function archivePath(entry: Entry): string
  local ref = entry.archive
  return if ref then paths.lossy(ref.path) else ''
end

--- Writes an archive entry's decoded bytes to `destination`, replacing a file there. Answers why
--- the entry couldn't be read (`unreadable`), or why it couldn't be written (`problem`).
local function extractEntry(entry: Entry, destination: string): (string?, Problem)
  local bytes, unreadable = file.decodeEntry(entry)
  if not bytes then return string.gsub(unreadable :: string, '^dream%.vfs: ', ''), nil end

  local problem = removeExistingOutput(destination)
  if problem then return nil, problem end

  return nil, refused(fs.writeFile(destination, bytes))
end

--- The winners' keys, sorted, so a run that stops stops at the same key every time.
local function sortedKeys(vfs: State): { string }
  local keys: { string } = {}
  for key in vfs.files do
    table.insert(keys, key)
  end

  table.sort(keys)
  return keys
end

local function sortSkips(skipped: { Skip })
  table.sort(skipped, function(a: Skip, b: Skip): boolean return a.key < b.key end)
end

--- `dump_to_directory`: every winner into `dir`, hard linked or copied, archive entries
--- extracted; `strict` stops at a missing or unreadable file instead of skipping it.
local function dump(
  vfs: State,
  dir: string,
  hardlinks: boolean,
  strict: boolean
): (number, { Skip })
  validateKeys(vfs)
  local written, skipped = 0, {}

  for _, key in sortedKeys(vfs) do
    local entry = vfs.files[key]
    local destination = paths.join(dir, keyPath(key))
    local unsafe = ensureParentSafe(dir, destination)
    if unsafe then raise(unsafe) end

    local parent = paths.parent(destination)
    local made = parent and parent ~= '' and createDirAll(parent)
    if made then raise(made) end

    if entry.archive then
      local unreadable, problem = extractEntry(entry, destination)
      if problem then raise(problem) end

      if not unreadable then
        written += 1
        continue
      end

      if strict then raise(unreadable) end

      table.insert(skipped, {
        kind = 'unreadableArchiveEntry',
        key = paths.keyPath(key),
        archive = archivePath(entry),
        error = unreadable,
      })
      continue
    end

    if not exists(entry.path) then
      if strict then
        raise(
          `source for VFS key '{paths.lossy(key)}' no longer exists at {paths.lossy(entry.path)}`
        )
      end

      table.insert(
        skipped,
        { kind = 'missingLooseSource', key = paths.keyPath(key), source = entry.path }
      )
      continue
    end

    written += 1
    local same, compared = sameFile(entry.path, destination)
    if compared then raise(compared) end

    if same then continue end

    if not hardlinks then
      local problem = copyReplacing(entry.path, destination)
      if problem then raise(problem) end

      continue
    end

    if exists(destination) then
      local problem = refused(fs.remove(destination))
      if problem then raise(problem) end
    end

    local linked, message, kind = fs.hardLink(entry.path, destination)
    if linked then continue end

    -- A hard link can't cross file systems, so the file is copied instead.
    if kind ~= 'crossesDevices' then raise(check.osError(message)) end

    local problem = refused(fs.copy(entry.path, destination))
    if problem then raise(problem) end
  end

  sortSkips(skipped)
  return written, skipped
end

--- A link from `merged` to `source`: symbolic (to its absolute path) or hard.
local function link(source: string, merged: string, symbolic: boolean): Problem
  if not symbolic then return refused(fs.hardLink(source, merged)) end

  local target, message = fs.absolute(source)
  if not target then return check.osError(message) end

  return refused(fs.symlink(target, merged))
end

local function collapseLoose(key: string, entry: Entry, merged: string, options: Options): Skip?
  if not exists(entry.path) then
    raise(`source file no longer exists for {paths.lossy(merged)} at {paths.lossy(entry.path)}`)
  end

  if isArchiveFile(entry) and options.extractArchives then
    return { kind = 'archiveFile', key = paths.keyPath(key), archive = entry.path }
  end

  local same, compared = sameFile(entry.path, merged)
  if compared then raise(compared) end

  if same then return nil end

  local removed, message, kind = fs.remove(merged)
  if not removed and kind ~= 'notFound' then
    raise(`failed to remove existing file at {paths.lossy(merged)}: {check.osError(message)}`)
  end

  local reason = link(entry.path, merged, options.useSymlinks)
  if not reason then return nil end

  if not options.allowCopying then raise(`link failed for {paths.lossy(entry.path)}: {reason}`) end

  local problem = copyReplacing(entry.path, merged)
  if problem then
    raise(
      `link failed for {paths.lossy(entry.path)}; fallback copy to {paths.lossy(merged)} also failed: {problem}`
    )
  end

  return nil
end

local function collapseArchive(key: string, entry: Entry, merged: string)
  local unreadable, problem = extractEntry(entry, merged)
  if unreadable then raise(`failed to open archived file {paths.lossy(key)}: {unreadable}`) end

  if problem then
    raise(`failed to extract {paths.lossy(key)} to {paths.lossy(merged)}: {problem}`)
  end
end

--- `collapse_into`: the VFS as one folder; returns the winners left out.
local function collapse(vfs: State, dest: string, options: Options): { Skip }
  validateKeys(vfs)
  local made = createDirAll(dest)
  if made then raise(made) end

  local skipped: { Skip } = {}

  for _, key in sortedKeys(vfs) do
    local entry = vfs.files[key]
    local merged = paths.join(dest, keyPath(key))
    local unsafe = ensureParentSafe(dest, merged)
    if unsafe then raise(unsafe) end

    local folder = paths.parent(merged)
    if not folder then return raise(`failed to resolve parent dir for {paths.lossy(merged)}`) end

    local problem = createDirAll(folder)
    if problem then raise(`failed to create directory {paths.lossy(folder)}: {problem}`) end

    if entry.archive then
      if options.extractArchives then
        collapseArchive(key, entry, merged)
      else
        table.insert(
          skipped,
          { kind = 'archiveEntry', key = paths.keyPath(key), archive = archivePath(entry) }
        )
      end

      continue
    end

    local skip = collapseLoose(key, entry, merged, options)
    if skip then table.insert(skipped, skip) end
  end

  sortSkips(skipped)
  return skipped
end

--- `extract_file`: one key's winner into `destDir` under its file name; nil for an unknown key.
local function extractFile(vfs: State, vfsPath: string, destDir: string): string?
  local key = paths.normalize(vfsPath)
  local entry = vfs.files[key]
  if not entry then return nil end

  local made = createDirAll(destDir)
  if made then raise(made) end

  local name = paths.fileName(keyPath(key))
  if not name then return raise 'vfs_path has no file name' end

  local destination = paths.join(destDir, name)
  local unsafe = ensureParentSafe(destDir, destination)
  if unsafe then raise(unsafe) end

  if entry.archive then
    local unreadable, problem = extractEntry(entry, destination)
    if unreadable or problem then raise((unreadable or problem) :: string) end

    return destination
  end

  local same, compared = sameFile(entry.path, destination)
  if compared then raise(compared) end

  if not same then
    local problem = copyReplacing(entry.path, destination)
    if problem then raise(problem) end
  end

  return destination
end

--- One action or issue row, its fields in the order (and table size) the binding gave them.
local function planRow(
  kind: string,
  key: string,
  source: string?,
  archive: string?,
  dest: string?
): PlanRow
  return { kind = kind, key = key, source = source, archive = archive, dest = dest }
end

--- `materialization_plan`: what `collapse` would do with each winner, and what stops it.
local function plan(vfs: State, dest: string, options: Options): Plan
  local actions: { PlanRow }, issues: { PlanRow } = {}, {}

  for _, key in sortedKeys(vfs) do
    local entry = vfs.files[key]
    if string.find(key, '\0', 1, true) then
      local display = paths.lossy(key)
      table.insert(
        issues,
        planRow('unsafeDestination', display, nil, nil, paths.join(dest, display))
      )
      continue
    end

    local target = paths.join(dest, key)
    if ensureParentSafe(dest, target) then
      table.insert(issues, planRow('unsafeDestination', key, nil, nil, target))
      continue
    end

    if entry.archive then
      if options.extractArchives then
        table.insert(actions, planRow('extractArchive', key, nil, archivePath(entry), target))
      else
        table.insert(actions, planRow('skipArchiveFile', key, nil, archivePath(entry), nil))
      end

      continue
    end

    if not exists(entry.path) then
      table.insert(issues, planRow('missingLooseSource', key, entry.path, nil, nil))
    elseif options.extractArchives and isArchiveFile(entry) then
      table.insert(actions, planRow('skipArchiveFile', key, nil, entry.path, nil))
    elseif options.useSymlinks then
      table.insert(actions, planRow('symlink', key, entry.path, nil, target))
    else
      table.insert(actions, planRow('hardlink', key, entry.path, nil, target))
    end
  end

  return { actions = actions, issues = issues }
end

return table.freeze {
  dump = dump,
  collapse = collapse,
  extractFile = extractFile,
  plan = plan,
  isArchiveFile = isArchiveFile,
  ensureParentSafe = ensureParentSafe,
  refused = refused,
  raise = raise,
}
