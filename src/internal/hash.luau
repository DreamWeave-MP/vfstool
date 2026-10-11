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
local function hostFile(path: string): (string, number)
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

--- The digest and size of a provider's file: a loose one from the host, an archive entry decoded.
local function entry(value: types.Entry): (string, number)
  if not value.archive then return hostFile(value.path) end

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
