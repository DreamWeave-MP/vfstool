--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- The run workflow: dump a VFS into a scratch folder, snapshot it, let a tool work on it, and
-- capture what the tool added or changed. Snapshots hash every file's contents, because a tool
-- that rewrites a file in place can keep its size and timestamp.
local check = require './check'
local fs = require '@dream/fs'
local hash = require './hash'
local materialize = require './materialize'
local paths = require './paths'
local types = require './types'

type State = types.State

export type SnapshotEntry = { hash: string, size: number, modified: number? }

--- A baseline: each file's digest by its path below the folder, and, for a tracked one, its size
--- and modification time.
export type Snapshot = {
  len: (self: Snapshot) -> number,
}

export type SnapshotData = { tracked: boolean, files: { [string]: SnapshotEntry }, count: number }

local snapshots = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [unknown]: SnapshotData }

local function snapshotOf(value: unknown, class: string, method: string): SnapshotData
  local data = snapshots[value]
  local expected = if class == 'MetadataSnapshot' then true else false
  if not data or data.tracked ~= expected then
    check.receiver(check.method(class, method), `dream.vfs.{class}`, value)
  end

  return data
end

local SnapshotMeta = {
  __index = table.freeze {
    len = function(self: Snapshot): number
      local data = snapshots[self]
      return snapshotOf(
        self,
        if data and data.tracked then 'MetadataSnapshot' else 'Snapshot',
        'len'
      ).count
    end,
  },
}

local function newSnapshot(data: SnapshotData): Snapshot
  local snapshot = (setmetatable({}, SnapshotMeta) :: unknown) :: Snapshot
  snapshots[snapshot] = data
  return check.handle(snapshot)
end

--- Every file below `dir` by its relative path, not following links; a traversal error stops it.
local function filesBelow(dir: string): { string }
  local walk, message = fs.walk(dir, { include = 'files' })
  if not walk then return check.vfs(check.walkError(message)) end

  return walk.paths
end

local function snapshotDirectory(dir: string, tracked: boolean): Snapshot
  local files, count = {}, 0
  for _, rel in filesBelow(dir) do
    local path = paths.join(dir, rel)
    local digest, size = hash.hostFile(path)
    local modified
    if tracked then
      local stat = fs.stat(path)
      modified = stat and stat.modified
    end

    files[rel] = { hash = digest, size = size, modified = modified }
    count += 1
  end

  return newSnapshot { tracked = tracked, files = files, count = count }
end

--- The files below `dir` that the baseline lacks or whose contents differ, sorted.
local function changedFiles(dir: string, baseline: SnapshotData): { string }
  local changed = {}
  for _, rel in filesBelow(dir) do
    local before = baseline.files[rel]
    if not before or hash.hostFile(paths.join(dir, rel)) ~= before.hash then
      table.insert(changed, rel)
    end
  end

  paths.sortPaths(changed)
  return changed
end

--- Copies each changed file into `output`, keeping its relative path.
type Copied = { { relativePath: string, destinationPath: string } }

local function capture(merged: string, changed: { string }, output: string): Copied
  local copied: Copied = {}
  for _, rel in changed do
    local source, destination = paths.join(merged, rel), paths.join(output, rel)
    local parent = paths.parent(destination)
    if parent and parent ~= '' then check.answer(fs.mkdir(parent, { recursive = true })) end

    if not check.answer(fs.sameFile(source, destination)) then
      check.answer(fs.copy(source, destination))
    end

    table.insert(copied, { relativePath = rel, destinationPath = destination })
  end

  return copied
end

--- `run_setup`: `merged` emptied (whatever it held), the VFS dumped into it, and a baseline.
local function setup(
  vfs: State,
  merged: string,
  hardlinks: boolean,
  tracked: boolean
): (number, Snapshot)
  local present, message = fs.lstat(merged)
  if message then check.io(message) end

  if present and not present.isDir and not present.isSymlink then
    -- `remove_dir_all` refuses a file; listing it answers the same ENOTDIR.
    check.answer(fs.list(merged))
  end

  if present and fs.exists(merged) then check.answer(fs.remove(merged, { recursive = true })) end

  check.answer(fs.mkdir(merged, { recursive = true }))
  local count = materialize.dump(vfs, merged, hardlinks, true)
  return count, snapshotDirectory(merged, tracked)
end

local function finalize(merged: string, output: string, baseline: SnapshotData)
  local changed = changedFiles(merged, baseline)
  return capture(merged, changed, output)
end

local function snapshotData(value: unknown, tracked: boolean): SnapshotData?
  local data = snapshots[value]
  if not data or data.tracked ~= tracked then return nil end

  return data
end

return table.freeze {
  snapshotDirectory = snapshotDirectory,
  changedFiles = changedFiles,
  setup = setup,
  finalize = finalize,
  snapshotData = snapshotData,
}
