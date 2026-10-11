--!strict
--!native
-- What the VFS suites share: a seeded generator, fixture trees written under a scratch folder,
-- and one canonical description of anything the VFS API returns, written against what both
-- `@vfs` and the native `@dream/vfs` have, so the same function describes either.
local bytes = require '@dream/bytes'
local fs = require '@dream/fs'

export type Random = (bound: number?) -> number

--- A xorshift32 generator: `random(n)` is in `0..n-1`, `random()` a u32.
local function generator(seed: number): Random
  local state = if seed == 0 then 0x9e3779b9 else seed % 0x100000000
  return function(bound: number?): number
    state = bit32.bxor(state, bit32.lshift(state, 13))
    state = bit32.bxor(state, bit32.rshift(state, 17))
    state = bit32.bxor(state, bit32.lshift(state, 5))
    return if bound then state % bound else state
  end
end

local function digest(data: string): string
  return string.format('%016x', (bytes.xxh3(data) :: unknown) :: number)
end

--- Where this file is, from its chunk name: the folder the suites are in.
local function here(): string
  local source = debug.info(1, 's')
  local path = if string.sub(source, 1, 1) == '@' or string.sub(source, 1, 1) == '='
    then string.sub(source, 2)
    else source
  return string.match(path, '^(.*)/[^/]*$') or '.'
end

--- Writes `files` (relative path to contents) below `root`, making folders as needed.
local function writeTree(root: string, files: { [string]: string })
  local names = {}
  for name in files do
    table.insert(names, name)
  end

  table.sort(names)
  for _, name in names do
    local path = `{root}/{name}`
    local parent = string.match(path, '^(.*)/[^/]*$')
    if parent then fs.mkdir(parent, { recursive = true }) end

    fs.writeFile(path, files[name])
  end
end

--- A fresh, empty folder.
local function fresh(path: string): string
  if fs.exists(path) then fs.remove(path, { recursive = true }) end

  fs.mkdir(path, { recursive = true })
  return path
end

local function quote(text: string): string return string.format('%q', text) end

--- `text` with the scratch root spelled `<root>`, so observations don't depend on where it is,
--- and a handle's type in a message spelled `table`: the native handles are userdata named after
--- their class, the port's are tables.
local function scrub(text: string, root: string?): string
  text = string.gsub(text, 'got dream%.vfs%.%a+%)', 'got table)')
  if not root or root == '' then return text end

  local out = {}
  local start = 1
  while true do
    local found = string.find(text, root, start, true)
    if not found then break end

    table.insert(out, string.sub(text, start, found - 1))
    table.insert(out, '<root>')
    start = found + #root
  end

  table.insert(out, string.sub(text, start))
  return table.concat(out)
end

local function isView(value: any): boolean
  if type(value) ~= 'table' and type(value) ~= 'userdata' then return false end

  local ok, toTable = pcall(function() return value.toTable end)
  return ok and type(toTable) == 'function'
end

local function isFile(value: any): boolean
  return (type(value) == 'table' or type(value) == 'userdata')
    and string.sub(tostring(value), 1, 18) == 'dream.vfs.VfsFile('
end

local describe: (value: any, root: string?, depth: number?) -> string

--- A VfsFile through every accessor.
local function describeFile(file: any, root: string?): string
  local function field(name: string): string
    local ok, value = pcall(function() return file[name](file) end)
    if not ok then return `{name}=!{scrub(tostring(value), root)}` end

    return `{name}={describe(value, root, 9)}`
  end

  return `file\{{table.concat({
    field 'path',
    field 'isLoose',
    field 'isArchive',
    field 'fileName',
    field 'fileStem',
    field 'parentArchivePath',
    field 'parentArchiveName',
  }, ' ')}\}`
end

--- One canonical line for any value: strings quoted with the root scrubbed, integers marked,
--- tables with sorted keys, files through their accessors, views through `toTable`.
function describe(value: any, root: string?, depth: number?): string
  local level = depth or 0
  if level > 40 then return '<deep>' end

  local kind = typeof(value)
  if kind == 'string' then return scrub(quote(value), root) end

  if kind == 'integer' then return `{tostring(value)}i` end

  if kind == 'number' then
    if value ~= value then return 'nan' end

    return if value == math.floor(value) and math.abs(value) < 2 ^ 53
      then string.format('%.0f', value)
      else string.format('%.17g', value)
  end

  if kind == 'boolean' or kind == 'nil' then return tostring(value) end

  if kind == 'buffer' then return `buffer({buffer.len(value)}:{digest(buffer.tostring(value))})` end

  if isFile(value) then return describeFile(value, root) end

  if isView(value) then
    local ok, rows = pcall(function() return value:toTable() end)
    if not ok then return `view!{scrub(tostring(rows), root)}` end

    local counted, count = pcall(function() return #value end)
    return `view#{if counted then tostring(count) else '?'}{describe(rows, root, level + 1)}`
  end

  if kind ~= 'table' then return scrub(tostring(value), root) end

  local keys = {}
  for key in value do
    table.insert(keys, key)
  end

  table.sort(keys, function(a, b)
    if type(a) == type(b) and (type(a) == 'number' or type(a) == 'string') then return a < b end

    return tostring(a) < tostring(b)
  end)
  local parts = {}
  for _, key in keys do
    table.insert(parts, `{describe(key, root, level + 1)}={describe(value[key], root, level + 1)}`)
  end

  return `\{{table.concat(parts, ',')}\}`
end

--- A call's results, or its error, as one line.
local function attempt(root: string?, call: (...any) -> ...any, ...: any): string
  local results = table.pack(pcall(call, ...))
  if not results[1] then return `error {scrub(tostring(results[2]), root)}` end

  local parts = {}
  for index = 2, results.n do
    table.insert(parts, describe(results[index], root))
  end

  return `ok {table.concat(parts, ' ')}`
end

--- Every file below `root` with its contents' digest, or a link's target: what a write left.
local function listing(root: string): string
  if not fs.exists(root) then return '<missing>' end

  local walk = fs.walk(root, { sort = true })
  local lines = {}
  for index, path in walk.paths do
    local kind = walk.kinds[index]
    local full = `{root}/{path}`
    if kind == 'file' then
      table.insert(lines, `{path} file {digest(fs.readFileString(full))}`)
    elseif kind == 'symlink' then
      table.insert(lines, `{path} link {scrub(fs.readLink(full) or '?', root)}`)
    else
      table.insert(lines, `{path} {kind}`)
    end
  end

  return table.concat(lines, '\n')
end

--- What a runner sets before it requires a suite, for the cases the harness's globals set.
local settings: { [string]: unknown } = {}

return table.freeze {
  settings = settings,
  generator = generator,
  digest = digest,
  here = here,
  writeTree = writeTree,
  fresh = fresh,
  scrub = scrub,
  describe = describe,
  attempt = attempt,
  listing = listing,
}
