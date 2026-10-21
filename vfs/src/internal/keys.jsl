--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- Every VFS key as a token of one `@dream/intern` pool whose rules are dream-path's: ASCII case
-- folded, `\` read as `/`, runs of `/` made one and a leading one dropped. A path spelled any way
-- finds its key's token natively, with no normalized string made, so a lookup is that call and an
-- integer table read. Tokens are shared by every VFS; the pool only grows with new keys.
local intern = require '@dream/intern'

local Pool: dream_intern_Pool =
  intern.new { nocase = true, replace = { ['\\'] = '/' }, collapse = '/', trimStart = '/' }

--- The token of a key, normalized or not, added on first sight.
local function token(key: string): number return Pool:intern(key) end

--- The token a path's key has, or nil when no VFS has seen it.
local function find(path: string): number? return Pool:find(path) end

return table.freeze { token = token, find = find }
