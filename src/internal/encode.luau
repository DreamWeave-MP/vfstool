--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- `vfstool.serialize`: Luau values as serde values. Tables whose keys are exactly `1..n` (empty
-- ones included) are arrays and other tables objects; sequence views are arrays of their rows, a
-- tree stream its nested tree, and a VfsFile its row's plain fields. And a VFS tree the way the
-- crate's own `DisplayTree` serialized: each directory's file names under `.`, then its
-- subdirectories.
local check = require './check'
local file = require './file'
local paths = require './paths'
local serialize = require './serialize'
local tree = require './tree'
local types = require './types'
local views = require './views'

type Value = serialize.Value

local MaxDepth = 128

local function utf8String(text: string, problem: string): string
  if not utf8.len(text) then check.vfs(`serialize: {problem}`) end

  return text
end

local function fileObject(entry: types.Entry): Value
  local ref = entry.archive
  local object = serialize.map()
  serialize.set(object, 'path', utf8String(entry.path, 'strings must be valid UTF-8'))
  serialize.set(object, 'isLoose', ref == nil)
  serialize.set(object, 'isArchive', ref ~= nil)
  if ref then
    serialize.set(object, 'parentArchivePath', paths.lossy(ref.path))
    local name = paths.fileName(ref.path)
    if name then serialize.set(object, 'parentArchiveName', paths.lossy(name)) end
  end

  return object
end

local function treeObject(nodes: { [string]: tree.Node }, order: { string }): Value
  local object = serialize.map()
  for _, name in order do
    local current = nodes[name]
    local files = table.create(#current.files)
    for index, entry in current.files do
      files[index] = fileObject(entry)
    end

    local node = serialize.map()
    serialize.set(node, 'files', serialize.array(files))
    serialize.set(node, 'subdirs', treeObject(current.subdirs, tree.sortedNames(current.subdirs)))
    serialize.set(object, paths.lossy(name), node)
  end

  return object
end

local function fromLuau(value: unknown, depth: number, trees: { [unknown]: tree.Tree }): Value
  if depth > MaxDepth then check.vfs 'serialize: value nests deeper than 128 levels' end

  local kind = type(value)
  if value == nil then return serialize.Null end

  if kind == 'boolean' then return value end

  if typeof(value) == 'integer' then return value end

  if kind == 'number' then
    local number = value :: number
    if number ~= number or math.abs(number) == math.huge then
      check.vfs 'serialize: a non-finite number has no representation'
    end

    if number == math.floor(number) and math.abs(number) < 9007199254740992 then
      return number + 0
    end

    return serialize.float(number)
  end

  if kind == 'string' then return utf8String(value :: string, 'strings must be valid UTF-8') end

  if kind ~= 'table' then check.vfs(`serialize: cannot serialize a {typeof(value)}`) end

  local entry = file.entryOf(value)
  if entry then return fileObject(entry) end

  local built = trees[value]
  if built then return treeObject(built.roots, built.order) end

  local rows = views.rows(value)
  if rows then
    local items = table.create(#rows)
    for index, row in rows do
      items[index] = fromLuau(row, depth + 1, trees)
    end

    return serialize.array(items)
  end

  if check.isHandle(value) then
    check.vfs 'serialize: userdata other than a VfsFile, a sequence view or a tree cannot be serialized'
  end

  local source = value :: { [unknown]: unknown }
  local count = 0
  for _ in source do
    count += 1
  end

  if count == rawlen(source) then
    local items = table.create(count)
    for index = 1, count do
      items[index] = fromLuau(source[index], depth + 1, trees)
    end

    return serialize.array(items)
  end

  local object = serialize.map()
  for key, item in source do
    local name: string
    if type(key) == 'string' then
      name = utf8String(key, 'table keys must be valid UTF-8')
    elseif typeof(key) == 'integer' then
      name = tostring(key)
    elseif type(key) == 'number' then
      name = serialize.display(key :: number)
    else
      return check.vfs(`serialize: a {typeof(key)} cannot be an object key`)
    end

    serialize.set(object, name, fromLuau(item, depth + 1, trees))
  end

  return object
end

--- A directory as the crate's `DirectoryNode` serialized: its file names under `.`, then its
--- subdirectories by name.
local function nodeValue(current: tree.Node): Value
  local node = serialize.map()
  if #current.files > 0 then
    local names = table.create(#current.files)
    for index, entry in current.files do
      names[index] = paths.lossy(tree.listedName(entry))
    end

    serialize.set(node, '.', serialize.array(names))
  end

  for _, child in tree.sortedNames(current.subdirs) do
    serialize.set(node, paths.lossy(child), nodeValue(current.subdirs[child]))
  end

  return node
end

--- The crate's `DisplayTree` as serde wrote it.
local function displayTree(built: tree.Tree): Value
  local object = serialize.map()
  for _, name in built.order do
    serialize.set(object, paths.lossy(name), nodeValue(built.roots[name]))
  end

  return object
end

return table.freeze {
  fromLuau = fromLuau,
  displayTree = displayTree,
}
