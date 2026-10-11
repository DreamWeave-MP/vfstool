--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- Paths, as vfstool_lib and dream-path spell them. VFS keys are bytes normalized the dream-path
-- way; host paths are bytes compared and split the way Rust's `std::path` does on Unix, by
-- components, so `a//b` and `a/./b` are one path and `a/b` sorts before `a.b`.
local bytes = require '@dream/bytes'

--- ASCII capitals and `\`, and what they become.
local Folded, Folds = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ\\', 'abcdefghijklmnopqrstuvwxyz/'

--- dream-path's normalization: `\` is `/`, ASCII is lowercased, leading separators go and
--- repeated ones collapse; a trailing one stays. A key already in that spelling costs one native
--- scan, which returns it as it is.
local function normalize(path: string): string
  local key = bytes.translate(path, Folded, Folds)
  if string.byte(key, 1) == 47 or string.find(key, '//', 1, true) then
    key = (string.gsub((string.gsub(key, '^/+', '')), '//+', '/'))
  end

  return key
end

local function isNormalized(path: string): boolean
  return not string.find(path, '[A-Z\\]')
    and string.byte(path, 1) ~= 47
    and not string.find(path, '//', 1, true)
end

--- vfstool's `normalize_host_path`: `\` to `/` and ASCII lowercase, nothing else, so a textual
--- comparison of host paths ignores case and separator spelling.
local function normalizeHost(path: string): string return bytes.translate(path, Folded, Folds) end

--- Whether a normalized key is one a VFS may hold: not empty, relative, no drive letter, no NUL,
--- and no `.` or `..` component.
local function isSafeNormalized(key: string): boolean
  if key == '' or string.byte(key, 1) == 47 or string.byte(key, 2) == 58 then return false end

  if string.find(key, '\0', 1, true) then return false end

  if not string.find(key, '.', 1, true) then return true end

  for component in string.gmatch(key, '[^/]+') do
    if component == '.' or component == '..' then return false end
  end

  return true
end

--- `normalized_safe_key_bytes`: a key spelled any way, or nil when it could leave the place it is
--- materialized into.
local function safeKeyBytes(raw: string): string?
  local first = string.byte(raw, 1)
  if not first or first == 47 or first == 92 or string.byte(raw, 2) == 58 then return nil end

  if string.find(raw, '\0', 1, true) then return nil end

  if string.find(raw, '.', 1, true) then
    for component in string.gmatch(raw, '[^/\\]+') do
      if component == '.' or component == '..' then return nil end
    end
  end

  local key = normalize(raw)
  if not isSafeNormalized(key) then return nil end

  return key
end

--- `normalized_safe_key`: a host path relative to its data directory as a key, by its components:
--- `.` components go, and a root, a drive letter or a `..` refuses it. The common case, a relative
--- path with neither, is one lowercase.
local function safeKeyFromHost(path: string): string?
  local folded = normalizeHost(path)
  if string.byte(folded, 2) == 58 or string.byte(folded, 1) == 47 then return nil end

  -- Only a `.` that starts a component can make a `.` or `..` one.
  if
    string.byte(folded, 1) == 46
    or string.find(folded, '/.', 1, true)
    or string.find(folded, '//', 1, true)
  then
    local parts = {}

    for component in string.gmatch(folded, '[^/]+') do
      if component == '..' then return nil end

      if component ~= '.' then table.insert(parts, component) end
    end

    folded = table.concat(parts, '/')
  end

  if folded == '' then return nil end

  return normalize(folded)
end

--- A search needle folded like a host path, with no separator collapsing.
local function foldNeedle(needle: string): string return normalizeHost(needle) end

--- Whether `key` is `prefix` or under it, on component boundaries; trailing separators on the
--- prefix name the same directory.
local function atOrUnder(key: string, prefix: string): boolean
  while string.byte(prefix, -1) == 47 do
    prefix = string.sub(prefix, 1, -2)
  end

  if key == prefix then return true end

  local length = #prefix
  return string.sub(key, 1, length) == prefix and string.byte(key, length + 1) == 47
end

--- Bytes as UTF-8 with every invalid sequence replaced, as Rust's `to_string_lossy` writes them.
local function lossy(text: string): string
  if utf8.len(text) then return text end

  return bytes.decode(text, 'utf-8')
end

local function isUtf8(text: string): boolean return utf8.len(text) ~= nil end

--- A key as the host path a report carries: the bytes, or the lossy spelling when a NUL makes them
--- no path.
local function keyPath(key: string): string
  if string.find(key, '\0', 1, true) then return lossy(key) end

  return key
end

-- Host paths, by components ---------------------------------------------------------------------

--- One component: `/` is the root, `.` the current directory at the start, `..` the parent, and
--- anything else a name.
export type Components = { string }

--- A path's components the way Rust's `Path::components` gives them on Unix.
local function components(path: string): Components
  local parts = {}
  local start = 1
  if string.byte(path, 1) == 47 then
    table.insert(parts, '/')
  elseif string.byte(path, 1) == 46 and (#path == 1 or string.byte(path, 2) == 47) then
    table.insert(parts, '.')
    start = 2
  end

  for component in string.gmatch(string.sub(path, start), '[^/]+') do
    if component ~= '.' then table.insert(parts, component) end
  end

  return parts
end

local function rank(component: string): number
  if component == '/' then return 1 end

  if component == '.' then return 2 end

  if component == '..' then return 3 end

  return 4
end

--- Rust's `Path` ordering: component by component, a root before a `.` before a `..` before a name,
--- names by their bytes.
local function comparePaths(left: string, right: string): number
  if left == right then return 0 end

  local a, b = components(left), components(right)
  for index = 1, math.min(#a, #b) do
    local x, y = a[index], b[index]
    if x ~= y then
      local rx, ry = rank(x), rank(y)
      if rx ~= ry then return if rx < ry then -1 else 1 end

      return if x < y then -1 else 1
    end
  end

  if #a == #b then return 0 end

  return if #a < #b then -1 else 1
end

local function pathLess(left: string, right: string): boolean return comparePaths(left, right) < 0 end

local function samePath(left: string, right: string): boolean
  return left == right or comparePaths(left, right) == 0
end

--- Sorts paths in Rust's `Path` order, each split into components once.
local function sortPaths(paths: { string })
  if #paths < 2 then return end

  local keys = {}

  for _, path in paths do
    if not keys[path] then
      local parts = components(path)
      local spelled = table.create(#parts)

      for index, part in parts do
        spelled[index] = string.char(rank(part)) .. part
      end

      keys[path] = table.concat(spelled, '\0')
    end
  end

  table.sort(paths, function(left, right) return keys[left] < keys[right] end)
end

--- `PathBuf::push`: an absolute path replaces the base; otherwise one separator joins them.
local function join(base: string, path: string): string
  if string.byte(path, 1) == 47 or base == '' then return path end

  if string.byte(base, -1) == 47 then return base .. path end

  return base .. '/' .. path
end

--- `Path::file_name`: the last component when it is a name.
local function fileName(path: string): string?
  local parts = components(path)
  local last = parts[#parts]
  if not last or rank(last) ~= 4 then return nil end

  return last
end

--- `Path::file_stem` of a file name.
local function stemOf(name: string): string
  if name == '..' then return name end

  local before = string.match(name, '^(.*)%.[^.]*$')
  if not before or before == '' then return name end

  return before
end

--- `Path::extension` of a file name.
local function extensionOf(name: string): string?
  if name == '..' then return nil end

  local before, after = string.match(name, '^(.*)%.([^.]*)$')
  if not before or before == '' then return nil end

  return after
end

local function fileStem(path: string): string?
  local name = fileName(path)
  return if name then stemOf(name) else nil
end

local function extension(path: string): string?
  local name = fileName(path)
  return if name then extensionOf(name) else nil
end

--- `Path::parent`: the path without its last component, spelled as it was; nil for a root or an
--- empty path.
local function parent(path: string): string?
  local trimmed = path
  while #trimmed > 1 and string.byte(trimmed, -1) == 47 do
    trimmed = string.sub(trimmed, 1, -2)
  end

  while string.sub(trimmed, -2) == '/.' do
    trimmed = string.sub(trimmed, 1, -3)
    while #trimmed > 1 and string.byte(trimmed, -1) == 47 do
      trimmed = string.sub(trimmed, 1, -2)
    end
  end

  if trimmed == '' or trimmed == '/' then return nil end

  local slash = string.find(trimmed, '/[^/]*$')
  if not slash then return '' end

  local head = string.sub(trimmed, 1, slash - 1)
  while #head > 1 and string.byte(head, -1) == 47 do
    head = string.sub(head, 1, -2)
  end

  if head == '' then return '/' end

  return head
end

--- `Path::strip_prefix`: what follows `base` in `path` when every component of `base` starts it,
--- or nil.
local function stripPrefix(path: string, base: string): string?
  local whole, head = components(path), components(base)
  if #head > #whole then return nil end

  for index, component in head do
    if whole[index] ~= component then return nil end
  end

  local rest = table.move(whole, #head + 1, #whole, 1, {})
  if rest[1] == '/' then return nil end

  return table.concat(rest, '/')
end

return table.freeze {
  normalize = normalize,
  isNormalized = isNormalized,
  normalizeHost = normalizeHost,
  isSafeNormalized = isSafeNormalized,
  safeKeyBytes = safeKeyBytes,
  safeKeyFromHost = safeKeyFromHost,
  foldNeedle = foldNeedle,
  atOrUnder = atOrUnder,
  lossy = lossy,
  isUtf8 = isUtf8,
  keyPath = keyPath,
  components = components,
  comparePaths = comparePaths,
  pathLess = pathLess,
  samePath = samePath,
  sortPaths = sortPaths,
  join = join,
  fileName = fileName,
  stemOf = stemOf,
  extensionOf = extensionOf,
  fileStem = fileStem,
  extension = extension,
  parent = parent,
  stripPrefix = stripPrefix,
}
