--!strict
--!native
-- What the vfstool suite uses of the VFS suites' shared support: a fresh scratch folder, and the
-- settings a runner can set before it requires the suite.
local fs = require '@dream/fs'

--- A fresh, empty folder.
local function fresh(path: string): string
  if fs.exists(path) then fs.remove(path, { recursive = true }) end

  fs.mkdir(path, { recursive = true })
  return path
end

--- What a runner sets before it requires a suite, for the cases the harness's globals set.
local settings: { [string]: unknown } = {}

return table.freeze {
  settings = settings,
  fresh = fresh,
}
