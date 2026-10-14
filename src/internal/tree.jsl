--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- The VFS as a sorted directory tree: under `Data Files` with keys for paths (relative), or under
-- `/` with each file's own path; an archive's files under the archive. As a stream of
-- `{ dir, file }` rows, as nested tables, as the crate's text listing, and as the value its
-- serializer writes.
local file = require './file'
local paths = require './paths'
local types = require './types'

type Entry = types.Entry
type State = types.State

export type Node = {
  files: { Entry },
  subdirs: { [string]: Node },
}

--- A tree: its roots (one) by name.
export type Tree = { roots: { [string]: Node }, order: { string } }

local function node(): Node return { files = {}, subdirs = {} } end

--- `VfsFile::file_name`, for sorting: an archive entry's last component, a loose file's name.
local function fileName(entry: Entry): string?
  if entry.archive then return file.lastComponent(entry) end

  return paths.fileName(entry.path)
end

--- `listed_name`: the file name, or the whole path when it has none.
local function listedName(entry: Entry): string
  return fileName(entry) or (if entry.archive then paths.lossy(entry.path) else entry.path)
end

--- A directory's files by file name, a file without one first, and files of one name in the
--- order they came in: the crate's stable `sort_by` on `Option<&OsStr>`. That order is a total
--- one, so the names sort as plain strings and each brings its files along.
local function sortFiles(files: { Entry }): { Entry }
  local sorted: { Entry }, names: { string } = {}, {}
  local named: { [string]: Entry } = {}
  local repeated: { [string]: { Entry } } = {}

  for _, entry in files do
    local name = fileName(entry)
    if not name then
      table.insert(sorted, entry)
      continue
    end

    if not named[name] then
      named[name] = entry
      table.insert(names, name)
      continue
    end

    local others = repeated[name]
    if others then
      table.insert(others, entry)
    else
      repeated[name] = { entry }
    end
  end

  table.sort(names)

  for _, name in names do
    table.insert(sorted, named[name])
    local others = repeated[name]
    if others then table.move(others, 1, #others, #sorted + 1, sorted) end
  end

  return sorted
end

local function sortNode(target: Node)
  if #target.files > 1 then target.files = sortFiles(target.files) end

  for _, child in target.subdirs do
    sortNode(child)
  end
end

--- `build_tree`: every winner `accept` takes, under its directory.
local function build(
  state: State,
  relative: boolean,
  accept: ((key: string, entry: Entry) -> boolean)?
): Tree
  local rootName = if relative then 'Data Files' else '/'
  local root = node()

  --- What an archive's files are listed under, and the directory every parent path walks to:
  --- each is worked out for the first file that needs it.
  local listedUnder: { [types.ArchiveRef]: string } = {}
  local directories: { [string]: Node } = {}

  for key, entry in state.files do
    local ref = entry.archive
    local path: string
    if ref then
      local under = listedUnder[ref]
      if not under then
        local parent = if relative then paths.fileName(ref.path) else ref.path
        under = paths.lossy(parent or '')
        listedUnder[ref] = under
      end

      path = paths.join(under, paths.keyPath(key))
    else
      path = if relative then paths.keyPath(key) else entry.path
    end

    if accept and not accept(key, entry) then continue end

    local parent = paths.parent(path)
    if not parent or parent == '' then parent = rootName end

    local target = directories[parent :: string]
    if not target then
      local current = ''
      target = root

      for _, component in paths.components(parent :: string) do
        current = if current == '' then component else paths.join(current, component)
        if paths.samePath(current, rootName) then continue end

        local child = target.subdirs[component]
        if not child then
          child = node()
          target.subdirs[component] = child
        end

        target = child
      end

      directories[parent :: string] = target
    end

    table.insert(target.files, entry)
  end

  sortNode(root)
  return { roots = { [rootName] = root }, order = { rootName } }
end

local function sortedNames(subdirs: { [string]: Node }): { string }
  local names = {}
  for name in subdirs do
    table.insert(names, name)
  end

  table.sort(names)
  return names
end

--- Every `{ dir, entry }` row, depth first: a directory's files, then its subdirectories.
local function rows(tree: Tree): { { dir: string, entry: Entry } }
  local out = {}

  local function walk(dir: string, current: Node)
    local display = paths.lossy(dir)
    for _, entry in current.files do
      table.insert(out, { dir = display, entry = entry })
    end

    for _, name in sortedNames(current.subdirs) do
      walk(paths.join(dir, name), current.subdirs[name])
    end
  end

  for _, name in tree.order do
    walk(name, tree.roots[name])
  end

  return out
end

--- The text listing: `├── name/` lines indented one `│   ` per level, files before
--- subdirectories; an empty directory is left out.
local function display(tree: Tree): string
  local out = {}

  local function write(current: Node, name: string, depth: number)
    if #current.files == 0 and next(current.subdirs) == nil then return end

    local label = paths.lossy(name)
    local last = string.byte(label, -1)
    local slash = if last == 47 or last == 92 then '' else '/'
    table.insert(out, `{string.rep('│   ', depth)}├── {label}{slash}\n`)
    local inner = string.rep('│   ', depth + 1)
    for _, entry in current.files do
      table.insert(out, `{inner}├── {paths.lossy(listedName(entry))}\n`)
    end

    for _, child in sortedNames(current.subdirs) do
      write(current.subdirs[child], child, depth + 1)
    end
  end

  for _, name in tree.order do
    write(tree.roots[name], name, 0)
  end

  return table.concat(out)
end

--- A file's plain row: what `tree:toTable()` and `serialize` show of it.
local function fileRow(entry: Entry): { [string]: unknown }
  local ref = entry.archive
  local name = ref and paths.fileName(ref.path)
  return {
    path = entry.path,
    isLoose = ref == nil,
    isArchive = ref ~= nil,
    parentArchivePath = if ref then paths.lossy(ref.path) else nil,
    parentArchiveName = if name then paths.lossy(name) else nil,
    file = file.new(entry),
  }
end

--- The nested `{ [root] = { files, subdirs } }` shape.
local function toTable(built: Tree): { [string]: unknown }
  local function nodeTable(current: Node): { [string]: unknown }
    local files = [for entry in current.files => fileRow(entry)]
    local subdirs = {}

    for name, child in current.subdirs do
      subdirs[paths.lossy(name)] = nodeTable(child)
    end

    return { files = files, subdirs = subdirs }
  end

  local out = {}
  for _, name in built.order do
    out[paths.lossy(name)] = nodeTable(built.roots[name])
  end

  return out
end

return table.freeze {
  build = build,
  toTable = toTable,
  fileRow = fileRow,
  rows = rows,
  display = display,
  sortedNames = sortedNames,
  listedName = listedName,
  fileName = fileName,
}
