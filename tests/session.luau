--!strict
-- One process's worth of every vfstool command, for `differential.luau` to run against two
-- builds of the package and compare stream for stream: the steps the reports suite records, then
-- what a suite on every platform can't hold: `run` with real children (through `sh`), and a
-- data folder whose folders and files differ only by case. `jess run --package <build>
-- --package-path jpk --capability filesystem.read --capability filesystem.write --capability
-- process.environment --capability process.spawn tests/session.luau <scratch> <transcript>`
-- writes the steps and their exit codes to `<transcript>`; what the commands print goes where
-- they print it.
local fs = require '@dream/fs'
local transcript = require './transcript'
local vfstool = require '@vfstool'

local scratch: string, written: string = ...

if fs.exists(scratch) then fs.remove(scratch, { recursive = true }) end

fs.mkdir(scratch, { recursive = true })

local root = fs.canonicalize(scratch)
local parts = { transcript.render(transcript.record(root)) }

--- Every file and folder below `dir`, a file with what it holds.
local function listing(dir: string): string
  if not fs.exists(dir) then return '<missing>' end

  local walk = fs.walk(dir, { sort = true })
  local lines = {}
  for index, path in walk.paths do
    local kind = walk.kinds[index]
    table.insert(lines, if kind == 'file' then `{path}: {fs.readFileString(`{dir}/{path}`)}` else `{path}: <{kind}>`)
  end

  return table.concat(lines, '\n')
end

--- Runs a command, with a line on standard output to say where its own begins.
local function run(name: string, argv: { string })
  print(`--- {name}`)
  table.insert(parts, `### {name}\nexit {vfstool.run(argv)}\n`)
end

local merged = `{root}/merged`
local Change = 'echo changed > "$0/textures/rock.dds"; echo new > "$0/new.txt"; mkdir "$0/deep"; echo deeper > "$0/deep/file.txt"'

run('run, capturing to data-local', { 'run', '--config', root, merged, '--', 'sh', '-c', Change, '{}' })
table.insert(parts, `local: {listing(`{root}/Local`)}\nmerged: {listing(merged)}\n`)
run('run --copy --keep-merged --output', {
  'run',
  '--config',
  root,
  '--copy',
  '--keep-merged',
  '--output',
  `{root}/captured`,
  merged,
  '--',
  'sh',
  '-c',
  Change,
  '{}',
})
table.insert(parts, `captured: {listing(`{root}/captured`)}\nmerged kept: {tostring(fs.exists(merged))}\n`)
fs.remove(merged, { recursive = true })
run('run, nothing changed', { 'run', '--config', root, merged, 'true' })
run('run, the child fails', { 'run', '--config', root, merged, '--', 'sh', '-c', 'exit 3' })
run('run, the child is killed', { 'run', '--config', root, merged, '--', 'sh', '-c', 'kill -9 $$' })
run('run, no such program', { 'run', '--config', root, merged, 'no-such-program-anywhere', '{}' })
run('run --working-dir', {
  'run',
  '--config',
  root,
  '--working-dir',
  `{root}/Local`,
  merged,
  '--',
  'sh',
  '-c',
  'pwd > "$0/where.txt"',
  '{}',
})
table.insert(parts, `local after: {listing(`{root}/Local`)}\n`)
run('run, {} alone and in a word', {
  'run',
  '--config',
  root,
  merged,
  '--',
  'sh',
  '-c',
  'test "$0" = "$1" && test "$2" = "x{}"',
  '{}',
  '{}',
  'x{}',
})

-- The driver sets OPENMW_CONFIG to the scratch folder.
run('find-file -s, the configuration from the environment', { 'find-file', '-s', 'Morrowind.esm' })

local twins = `{root}/twins`
local Files = {
  { 'Data/Textures/a.dds', '1' },
  { 'Data/textures/a.dds', '2' },
  { 'Data/TEXTURES/A.DDS', '3' },
  { 'Data/textures/b.dds', '4' },
  { 'Data/Textures/c.DDS', '5' },
  { 'Data/Meshes/x', 'a file, not a folder' },
  { 'Data/meshes/x/d.nif', '6' },
  { 'Data/Plugin.esp', '7' },
  { 'Data/plugin.ESP', '8' },
  { 'Over/Textures/b.dds', '9' },
  { 'Over/textures/sub/e.dds', '10' },
  { 'Over/Textures/Sub', 'a file' },
}

for _, file in Files do
  local path = `{twins}/{file[1]}`
  fs.mkdir(string.match(path, '^(.*)/[^/]*$') :: string, { recursive = true })
  fs.writeFile(path, file[2])
end

local Settings = {
  `data="{twins}/Data"`,
  `data="{twins}/Over"`,
  `data="{twins}/Absent"`,
  'content=plugin.esp',
  'content=PLUGIN.ESP',
  'content=textures/a.dds',
  'content=textures/sub/e.dds',
  'content=meshes/x/d.nif',
  'content=meshes/x',
  'content=textures/sub',
  'content=textures',
  'content=absent.esp',
  'groundcover=Textures\\C.dds',
  'groundcover=/textures//b.dds',
  'groundcover=textures/b.dds/',
  'groundcover=./plugin.esp',
  'fallback-archive=meshes/X/D.NIF',
}

fs.writeFile(`{twins}/openmw.cfg`, table.concat(Settings, '\n') .. '\n')

local Wanted = {
  'textures/a.dds',
  'TEXTURES\\A.DDS',
  'textures/b.dds',
  'textures/c.dds',
  'meshes/x/d.nif',
  'meshes/x',
  'plugin.esp',
  'textures/sub/e.dds',
  'textures/sub',
  'textures',
  'textures/',
  '/textures/a.dds',
  'textures//a.dds',
  './plugin.esp',
  'absent/a.dds',
  '',
}

for _, path in Wanted do
  run(`twins: find-file -s {path}`, { 'find-file', '-s', '--config', twins, path })
  run(`twins: find-file -p {path}`, { 'find-file', '-p', '--config', twins, path })
end

for _, format in { 'yaml', 'json', 'toml' } do
  run(`twins: validate, {format}`, { 'validate', '--config', twins, '--format', format })
  run(`twins: validate --full, {format}`, { 'validate', '--full', '--config', twins, '--format', format })
end

fs.writeFile(written, table.concat(parts))
