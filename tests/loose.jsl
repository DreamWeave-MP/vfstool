--!strict
--!native
-- What `validate` finds among loose files, and `find-file` with it: every name is looked for in
-- the data folders from the last to the first, component by component and without regard to case,
-- and a command lists a folder once however many names it looks for there. The reports suite pins
-- one small load order; this one is for what a wrong listing gets wrong: many names over the same
-- folders, a file where a folder is wanted and a folder where a file is, and, where the file
-- system tells them apart, several names in one folder that spell the same key. Everything it
-- writes stays under the test's scratch root.
local fs = require '@dream/fs'
local json = require '@jess/json'
local support = require './support'
local test = require '@jess/test'
local vfstool = require '@vfstool'

local expect = test.expect
local suite = test.new()
local globals = _G :: { [string]: unknown }
local scratchRoot = globals.scratchRoot or support.settings.scratchRoot or `{fs.cwd()}/vfs-suite`

--- Writes `files` (relative path, contents) below `root`, in the order given.
local function writeTree(root: string, files: { { string } })
  for _, file in files do
    local path = `{root}/{file[1]}`
    local parent = string.match(path, '^(.*)/[^/]*$')
    if parent then fs.mkdir(parent, { recursive = true }) end

    fs.writeFile(path, file[2])
  end
end

--- An openmw.cfg under `root` of the folders named, then of `settings` as they are.
local function configure(root: string, folders: { string }, settings: { string })
  local lines = [for folder in folders => `data="{root}/{folder}"`]
  for _, setting in settings do
    table.insert(lines, setting)
  end

  fs.writeFile(`{root}/openmw.cfg`, table.concat(lines, '\n') .. '\n')
end

--- What `validate` reports over the configuration under `root`: its exit code, and each issue as
--- its kind and its name, or its path below `root`.
local function validated(root: string): (number, { string })
  local path = `{root}/validate.json`
  local code = vfstool.run { 'validate', '--config', root, '--format', 'json', '--output', path }
  local report = json.decode(assert(fs.readFileString(path), 'no report')) :: { issues: { { [string]: { [string]: string } } } }
  local issues = {}

  for _, issue in report.issues do
    for kind, fields in issue do
      local where = fields.path
      table.insert(issues, `{kind} {fields.name or string.sub(where, #root + 2)}`)
    end
  end

  return code, issues
end

--- The exit code of `find-file -s` for `name`: 0 for a file something has, 1 for one nothing has.
local function found(root: string, name: string): number
  return vfstool.run { 'find-file', '-s', '--config', root, name }
end

suite.describe('vfstool loose files', function()
  suite.it('finds each name in the last folder that has it, a folder listed once for them all', function()
    local root = fs.canonicalize(support.fresh(`{scratchRoot}/vfstool-loose`))

    writeTree(`{root}/First`, {
      { 'Plugin.esp', 'first plugin' },
      { 'Meshes/X/Chair.nif', 'first chair' },
      { 'Textures/Deep/Down/Rock.dds', 'first rock' },
      { 'Shared/one.txt', 'one' },
    })

    -- `meshes/x` is a file here and a folder below, and `Other.esp` a folder.
    writeTree(`{root}/Second`, {
      { 'meshes/x', 'a file where the first folder has a folder' },
      { 'SHARED/two.txt', 'two' },
      { 'Other.esp/inner.txt', 'inside a folder named as a plugin is' },
    })

    fs.mkdir(`{root}/Third`, { recursive = true })

    configure(root, { 'First', 'Second', 'Third', 'Absent', 'First/Plugin.esp' }, {
      'fallback-archive=plugin.esp',
      'fallback-archive=Missing.bsa',
      'content=plugin.esp',
      'content=PLUGIN.ESP',
      'content=meshes/x/chair.nif',
      'content=Meshes\\X\\CHAIR.NIF',
      'content=meshes/x',
      'content=meshes',
      'content=textures/deep/down/rock.dds',
      'content=textures/deep/down',
      'content=textures/deep/down/rock.dds/more',
      'content=shared/one.txt',
      'content=shared/two.txt',
      'content=shared/three.txt',
      'content=other.esp',
      'content=other.esp/inner.txt',
      'content=absent.esp',
      'content=../openmw.cfg',
      'groundcover=Shared/ONE.txt',
      'groundcover=textures/deep/down/rock.dds/',
      'groundcover=absent/one.txt',
      'groundcover=third',
    })

    local code, issues = validated(root)

    expect(code).toBe(5)
    expect(issues).toEqual {
      'MissingDataDirectory Absent',
      'DataPathNotDirectory First/Plugin.esp',
      'MissingFallbackArchive Missing.bsa',
      'MissingContentFile meshes',
      'MissingContentFile textures/deep/down',
      'MissingContentFile textures/deep/down/rock.dds/more',
      'MissingContentFile shared/three.txt',
      'MissingContentFile other.esp',
      'MissingContentFile absent.esp',
      'MissingContentFile ../openmw.cfg',
      'MissingGroundcoverFile absent/one.txt',
      'MissingGroundcoverFile third',
    }

    -- The same walk for one name. A folder is no file to `find-file` either.
    expect(found(root, 'MESHES/X/chair.nif')).toBe(0)
    expect(found(root, 'shared\\TWO.txt')).toBe(0)
    expect(found(root, 'other.esp/inner.txt')).toBe(0)
    expect(found(root, 'shared/three.txt')).toBe(1)
    expect(found(root, 'textures/deep/down')).toBe(1)
    expect(found(root, 'other.esp')).toBe(1)

    -- Nothing is kept from one command to the next: what is written since is found, and what is
    -- removed since is not.
    writeTree(`{root}/Third`, { { 'shared/THREE.txt', 'three' }, { 'Absent.esp', 'no longer' } })
    fs.remove(`{root}/Second/SHARED/two.txt`)

    local after, left = validated(root)

    expect(after).toBe(5)
    expect(left).toEqual {
      'MissingDataDirectory Absent',
      'DataPathNotDirectory First/Plugin.esp',
      'MissingFallbackArchive Missing.bsa',
      'MissingContentFile meshes',
      'MissingContentFile textures/deep/down',
      'MissingContentFile textures/deep/down/rock.dds/more',
      'MissingContentFile shared/two.txt',
      'MissingContentFile other.esp',
      'MissingContentFile ../openmw.cfg',
      'MissingGroundcoverFile absent/one.txt',
      'MissingGroundcoverFile third',
    }
  end)

  suite.it('walks every name that spells a key, where the file system tells them apart', function()
    local root = fs.canonicalize(support.fresh(`{scratchRoot}/vfstool-twins`))

    fs.writeFile(`{root}/probe`, 'lower')

    -- One name for both: nothing here can be written, and the case above is the whole of it.
    if fs.exists(`{root}/PROBE`) then return end

    -- Each key is spelled more than one way in its folder, and what is looked for is under one
    -- spelling alone: the first written, the last, or one between. A file takes a folder's name
    -- under another spelling, both ways round.
    writeTree(`{root}/Data`, {
      { 'Meshes/x', 'a file' },
      { 'meshes/x/d.nif', 'under the second spelling' },
      { 'textures/y/e.dds', 'under the first spelling' },
      { 'Textures/y', 'a file' },
      { 'Icons/a.tga', 'first' },
      { 'ICONS/b.tga', 'second' },
      { 'icons/c.tga', 'third' },
      { 'Sound/Fx/one.wav', 'one' },
      { 'sound/FX/two.wav', 'two' },
      { 'SOUND/fx/three.wav', 'three' },
      { 'Plugin.esp', 'one' },
      { 'plugin.ESP', 'another' },
    })

    configure(root, { 'Data' }, {
      'content=meshes/x/d.nif',
      'content=meshes/x',
      'content=textures/y/e.dds',
      'content=textures/y',
      'content=icons/a.tga',
      'content=icons/b.tga',
      'content=icons/c.tga',
      'content=icons/d.tga',
      'content=sound/fx/one.wav',
      'content=sound/fx/two.wav',
      'content=sound/fx/three.wav',
      'content=sound/fx/four.wav',
      'content=sound/fx',
      'content=PLUGIN.esp',
    })

    local code, issues = validated(root)

    expect(code).toBe(5)
    expect(issues).toEqual {
      'MissingContentFile icons/d.tga',
      'MissingContentFile sound/fx/four.wav',
      'MissingContentFile sound/fx',
    }

    expect(found(root, 'meshes/x/d.nif')).toBe(0)
    expect(found(root, 'textures/y/e.dds')).toBe(0)
    expect(found(root, 'sound/fx/three.wav')).toBe(0)
    expect(found(root, 'sound/fx/four.wav')).toBe(1)
  end)
end)

return suite
