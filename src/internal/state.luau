--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- A VFS's state and the rules every mutation follows: provider stacks low to high priority, the
-- winner of each key cached in `files`, and the directories the winners sit in counted, so a key
-- that would need a file and a directory of one name is refused before it is inserted.
local bytes = require '@dream/bytes'
local keys = require './keys'
local paths = require './paths'
local types = require './types'

type State = types.State
type Entry = types.Entry
type Source = types.Source

local function new(): State
  return {
    files = {},
    count = 0,
    revision = 1,
    providers = {},
    winners = {},
    stacks = {},
    sources = {},
    provided = {},
    directories = {},
    keys = nil,
    layer = nil,
    writeRoot = nil,
    grant = 'host',
  }
end

--- Drops what a mutation makes stale: the sorted key list and the layer index.
local function touched(state: State)
  state.revision += 1
  state.keys = nil
  state.layer = nil
end

local function pushSource(state: State, source: Source): number
  table.insert(state.sources, source)
  table.insert(state.provided, false)
  return #state.sources
end

local function popSource(state: State)
  table.remove(state.sources)
  table.remove(state.provided)
end

--- Puts `entry` on top of `key`'s stack; the winner is not refreshed. A stack made for it is
--- filed under the key's token, which is then answered for whoever needs it next.
local function pushEntry(state: State, key: string, entry: Entry): number?
  state.provided[entry.source] = true
  local stack = state.providers[key]
  if stack then
    table.insert(stack, entry)
    return nil
  end

  local created = { entry }
  local token = keys.token(key)
  state.providers[key] = created
  state.stacks[token] = created
  return token
end

--- Drops `key`'s whole stack; its winner goes with the next `refresh`.
local function dropStack(state: State, key: string)
  state.providers[key] = nil
  state.stacks[keys.token(key)] = nil
end

--- The directory a key, or a directory, sits in; nil at the top.
local function parentOf(path: string): string?
  local slash = bytes.rfind(path, '/')
  return if slash then string.sub(path, 1, slash) else nil
end

-- `directories` counts what sits directly in each directory that holds a winner somewhere below
-- it: its winners, and its directories that hold one. A winner in a directory already counted
-- touches that directory alone, and one that opens a directory counts it in the directory above.

local function addDirectories(state: State, key: string)
  local directories = state.directories
  local parent = parentOf(key)
  while parent do
    local count = directories[parent]
    if count then
      directories[parent] = count + 1
      return
    end

    directories[parent] = 1
    parent = parentOf(parent)
  end
end

local function removeDirectories(state: State, key: string)
  local directories = state.directories
  local parent = parentOf(key)
  while parent do
    local count = directories[parent]
    if not count then return end

    if count > 1 then
      directories[parent] = count - 1
      return
    end

    directories[parent] = nil
    parent = parentOf(parent)
  end
end

--- `refresh`, by a caller that may already hold the key's token.
local function settle(state: State, key: string, token: number?)
  local stack = state.providers[key]
  local top = stack and stack[#stack]
  if top then
    if not state.files[key] then
      addDirectories(state, key)
      state.count += 1
    end

    state.files[key] = top
    state.winners[token or keys.token(key)] = top
    return
  end

  if state.files[key] then
    state.files[key] = nil
    state.winners[token or keys.token(key)] = nil
    state.count -= 1
    removeDirectories(state, key)
  end

  dropStack(state, key)
end

--- Makes the top of `key`'s stack its winner, or removes the key when the stack is empty.
local function refresh(state: State, key: string) settle(state, key, nil) end

--- Whether `key` needs a directory where a winner is a file, or a file where winners sit in a
--- directory of its name. No path is both a winner and a directory, and what a directory sits in
--- are directories, so the search for a winner above the key ends at the first directory.
local function conflicts(state: State, key: string): boolean
  local directories = state.directories
  if directories[key] then return true end

  local files = state.files
  local parent = parentOf(key)
  while parent and not directories[parent] do
    if files[parent] then return true end

    parent = parentOf(parent)
  end

  return false
end

--- Inserts `entry` for `key` unless the key would conflict with a file or directory already there;
--- returns whether it went in. The winner is refreshed at once, so the next insert sees it.
local function insert(state: State, key: string, entry: Entry): boolean
  if not state.files[key] and conflicts(state, key) then return false end

  settle(state, key, pushEntry(state, key, entry))
  return true
end

--- `provider_original_path`: a loose provider's path below its source, else the key.
local function originalPath(state: State, key: string, entry: Entry): string
  local source = state.sources[entry.source]
  if source and source.kind == 'looseDir' then
    local rel = entry.rel
    if rel then return rel end

    return paths.stripPrefix(entry.path, source.path) or paths.keyPath(key)
  end

  return entry.path
end

--- The rows a layer index is built from: every source that provides a key, or never provided
--- one, with the original path of each file it provides, in source order.
local function layerRows(state: State): ({ Source }, { { string } })
  local used: { boolean } = table.create(#state.sources, false)
  for index, provided in state.provided do
    used[index] = not provided
  end

  for _, stack in state.providers do
    for _, entry in stack do
      used[entry.source] = true
    end
  end

  local remap: { number } = table.create(#state.sources, 0)
  local sources: { Source } = {}
  for index, source in state.sources do
    if used[index] then
      table.insert(sources, source)
      remap[index] = #sources
    end
  end

  local files = (table.create(#sources) :: unknown) :: { { string } }
  for index = 1, #sources do
    files[index] = {}
  end

  for key, stack in state.providers do
    for _, entry in stack do
      local source = state.sources[entry.source]
      local path = if source.kind == 'archive'
        then paths.keyPath(key)
        else originalPath(state, key, entry)
      table.insert(files[remap[entry.source]], path)
    end
  end

  return sources, files
end

return table.freeze {
  new = new,
  touched = touched,
  pushSource = pushSource,
  popSource = popSource,
  pushEntry = pushEntry,
  dropStack = dropStack,
  refresh = refresh,
  conflicts = conflicts,
  insert = insert,
  originalPath = originalPath,
  layerRows = layerRows,
}
