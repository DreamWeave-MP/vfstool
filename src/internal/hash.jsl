--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- BLAKE3 over a file's contents, as the lock manifest, provenance, and the run workflow's
-- snapshots take it: a loose file read from the host in pieces, an archive entry decoded.
local bytes = require '@dream/bytes'
local check = require './check'
local file = require './file'
local fs = require '@dream/fs'
local types = require './types'

--- Files up to this size are read whole; larger ones are hashed a piece at a time.
local Whole = 4 * 1024 * 1024

--- The hex BLAKE3 digest and size of the host file at `path`; raises the crate's I/O errors.
--- A regular file short enough to read whole is read with one call once the host has said how
--- long it is (`known`, from a caller that already asked about a regular file), which costs less
--- than a map made and dropped for it. Anything else goes through a reader, and so does a file
--- that can't be described or read, or whose bytes turn out not to be that many, so what is
--- answered and what is raised stay the reader's.
local function hostFile(path: string, known: number?): (string, number)
  local length = known
  if not length then
    local stat = fs.stat(path)
    if stat and stat.isFile then length = stat.size end
  end

  if length and length <= Whole then
    local data = fs.readFile(path)
    if data and buffer.len(data) == length then return bytes.blake3(data), length end
  end

  local reader = check.answer(fs.open(path))
  local size = reader:size()
  if size <= Whole then
    local digest = bytes.blake3(check.answer(reader:readAt(0, size)))
    reader:close()
    return digest, size
  end

  local hasher = bytes.hasher 'blake3'
  local chunk = buffer.create(Whole)
  local offset = 0
  while offset < size do
    local count = check.answer(reader:readAtInto(chunk, offset, math.min(Whole, size - offset)))
    if count < Whole then
      hasher:update(buffer.readstring(chunk, 0, count))
    else
      hasher:update(chunk)
    end

    offset += count
  end

  reader:close()
  return hasher:finish(), size
end

--- The digest and size of a provider's file: a loose one from the host (a regular file `known`
--- bytes long, when the caller has asked), an archive entry decoded.
local function entry(value: types.Entry, known: number?): (string, number)
  if not value.archive then return hostFile(value.path, known) end

  local decoded = file.decode(value)
  return bytes.blake3(decoded), buffer.len(decoded)
end

--- The hex digest of bytes already read.
local function ofBytes(data: buffer): string return bytes.blake3(data) end

--- The raw 32 digest bytes of a host file, as the snapshots compare them.
local function raw(path: string): string
  local digest = hostFile(path)
  return digest
end

return table.freeze {
  hostFile = hostFile,
  entry = entry,
  bytes = ofBytes,
  raw = raw,
}
