--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- @vfs: OpenMW's virtual file system in Luau, a port of vfstool_lib whose `@dream/vfs`
-- binding this replaces under its own name: layered data directories and archives, lookups and
-- winners, conflict and layer indexes, provenance, lock manifests and drift, semantic conflicts,
-- materializing (dump, collapse, extract), the run workflow, serialization, and the `host` table.
local check = require '@self/internal/check'
local conflict = require '@self/internal/conflict'
local encode = require '@self/internal/encode'
local glob = require '@self/internal/glob'
local host = require '@self/internal/host'
local layer = require '@self/internal/layer'
local materialize = require '@self/internal/materialize'
local paths = require '@self/internal/paths'
local registry = require '@self/internal/registry'
local run = require '@self/internal/run'
local semantic = require '@self/internal/semantic'
local serialize = require '@self/internal/serialize'
local vfs = require '@self/internal/vfs'

export type VfsFile = typeof(vfs.VfsFile.from '')
export type VFS = vfs.VFS
export type Tree = vfs.Tree
export type DuplicateReport = vfs.DuplicateReport
export type ArchiveInfo = vfs.ArchiveInfo
export type ArchiveEntry = vfs.ArchiveEntry
export type ContributionReport = vfs.ContributionReport
export type MaterializationPlan = vfs.MaterializationPlan
export type LayerIndex = layer.LayerIndex
export type VfsLock = layer.VfsLock
export type ConflictIndex = conflict.ConflictIndex

--- The vfstool_lib release this port follows.
local Version = '1.0.1'

local function name(functionName: string): string return `dream.vfs.{functionName}` end

--- `normalize_host_path`, and its in-place form, which Luau's immutable strings make the same:
--- one `bytes.translate` call.
local normalizeHostPath, normalizeHostPathInPlace = paths.normalizeHost, paths.normalizeHost

local function pathGlobMatches(pattern: string, path: string): boolean
  local what = name 'pathGlobMatches'
  return glob.matches(check.text(what, pattern, 1, 1), check.string(what, path, 2, 2))
end

local function sourceGlobMatches(pattern: string, sourcePath: string): boolean
  local what = name 'sourceGlobMatches'
  return glob.matches(check.text(what, pattern, 1, 1), check.string(what, sourcePath, 2, 2))
end

local function analyzePair(path: string, leftBytes: string, rightBytes: string)
  local what = name 'analyzePair'
  local key = check.string(what, path, 1, 1)
  local left, right = check.string(what, leftBytes, 2, 2), check.string(what, rightBytes, 3, 3)
  local class, delta = semantic.analyze(key, left, right)
  return { assetClass = class, delta = delta }
end

local function snapshotArgument(
  what: string,
  value: unknown,
  tracked: boolean,
  position: number
): run.SnapshotData
  local class = if tracked then 'dream.vfs.MetadataSnapshot' else 'dream.vfs.Snapshot'
  return run.snapshotData(value, tracked) or check.userdata(what, position, position, class, value)
end

local function setup(tracked: boolean, functionName: string)
  return function(target: VFS, mergedDir: string, useHardlinks: boolean)
    local what = name(functionName)
    local state = registry.argument(target, what, 1, 1)
    local merged = check.string(what, mergedDir, 2, 2)
    local links = check.boolean(what, useHardlinks, 3, 3)
    return run.setup(state, merged, links, tracked)
  end
end

local function finalize(tracked: boolean, functionName: string)
  return function(mergedDir: string, outputDir: string, snapshot: run.Snapshot)
    local what = name(functionName)
    local merged = check.string(what, mergedDir, 1, 1)
    local output = check.string(what, outputDir, 2, 2)
    return run.finalize(merged, output, snapshotArgument(what, snapshot, tracked, 3))
  end
end

local function snapshotDirectory(tracked: boolean, functionName: string)
  return function(dir: string): run.Snapshot
    return run.snapshotDirectory(check.string(name(functionName), dir, 1, 1), tracked)
  end
end

local function changedFiles(tracked: boolean, functionName: string)
  return function(dir: string, snapshot: run.Snapshot): { string }
    local what = name(functionName)
    local root = check.string(what, dir, 1, 1)
    return run.changedFiles(root, snapshotArgument(what, snapshot, tracked, 2))
  end
end

local function serializeValue(value: unknown, format: string): string
  local chosen = check.text(name 'serialize', format, 2, 2)
  if chosen ~= 'json' and chosen ~= 'yaml' and chosen ~= 'toml' then
    check.vfs(`unknown serialization format '{chosen}' (expected json, yaml, or toml)`)
  end

  return serialize.encode(encode.fromLuau(value, 0, vfs.trees), chosen)
end

return table.freeze {
  version = Version,
  normalizeHostPath = normalizeHostPath,
  normalizeHostPathInPlace = normalizeHostPathInPlace,
  pathGlobMatches = pathGlobMatches,
  sourceGlobMatches = sourceGlobMatches,
  analyzePair = analyzePair,
  runSetup = setup(false, 'runSetup'),
  runSetupTracked = setup(true, 'runSetupTracked'),
  runFinalize = finalize(false, 'runFinalize'),
  runFinalizeTracked = finalize(true, 'runFinalizeTracked'),
  snapshotDirectory = snapshotDirectory(false, 'snapshotDirectory'),
  snapshotDirectoryMetadata = snapshotDirectory(true, 'snapshotDirectoryMetadata'),
  changedFiles = changedFiles(false, 'changedFiles'),
  changedFilesMetadata = changedFiles(true, 'changedFilesMetadata'),
  serialize = serializeValue,
  VFS = vfs.Class,
  VfsFile = vfs.VfsFile,
  VfsProvider = vfs.VfsProvider,
  LayerIndex = table.freeze { fromFileLists = layer.fromFileLists },
  ConflictIndex = conflict.Class,
  host = host.Table,
}
