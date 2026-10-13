--!strict
-- Whole vfstool processes, timed from outside: `jess package run` of each package folder given,
-- for commands over a load order of three folders, where starting up is most of what a run
-- costs. Run by `jess run --capability filesystem.read --capability filesystem.write
-- --capability process.spawn jpk/vfstool/bench/process.luau <jess> <package path> <scratch>
-- <rounds> <package folder>...`: the jess to start (a release build), the folder the packages'
-- dependencies are found in, a scratch folder, how many times each command runs, and one or more
-- folders of this package (two builds of it, to compare them). The folders take turns, round by
-- round, so what else the machine is doing falls on each alike. Prints one line per folder and
-- command: the folder, the command, a tab, and the median wall-clock seconds of its runs. Every
-- run's exit code is checked, and the report of one that writes a report.
local fs = require '@dream/fs'
local process = require '@dream/process'

local jessPath: string?, packagePath: string?, scratch: string?, roundCount: string? = ...
local packages: { string } = { select(5, ...) }
local rounds = tonumber(roundCount)

if not jessPath or not packagePath or not scratch or not rounds or #packages == 0 then
  error('vfstool bench: usage: process.luau <jess> <package path> <scratch> <rounds> <package folder>...', 0)
end

assert(fs.mkdir(scratch, { recursive = true }))

local root = `{assert(fs.canonicalize(scratch))}/vfstool-process`

if fs.exists(root) then assert(fs.remove(root, { recursive = true })) end

local Folders, PerFolder = 3, 8
local lines = {}

for folder = 1, Folders do
  local dir = `{root}/Mod {folder}`
  assert(fs.mkdir(`{dir}/Textures`, { recursive = true }))

  -- Half of a folder's files are the next folder's too.
  for file = 1, PerFolder do
    assert(fs.writeFile(`{dir}/Textures/Tx_Rock_{folder * PerFolder // 2 + file}.dds`, `{folder}:{file}`))
  end

  assert(fs.writeFile(`{dir}/Mod {folder}.esp`, `plugin {folder}`))
  table.insert(lines, `data="{dir}"`)
  table.insert(lines, `content=Mod {folder}.esp`)
end

assert(fs.writeFile(`{root}/openmw.cfg`, table.concat(lines, '\n') .. '\n'))

local keys = Folders * (PerFolder // 2 + 1) + PerFolder // 2
local report = `{root}/report.json`

--- A lock for drift to read, written by the first folder's build.
local kept = `{root}/kept.json`

type Command = { name: string, argv: { string }, code: number, check: ((written: string) -> boolean)? }

local Commands: { Command } = {
  { name = 'help', argv = { 'help' }, code = 0 },
  { name = 'find-file -s, a loose file', argv = { 'find-file', '-s', '--config', root, 'mod 1.ESP' }, code = 0 },
  {
    name = 'find-file -s, a file nothing has',
    argv = { 'find-file', '-s', '--config', root, 'textures/absent.dds' },
    code = 1,
  },
  {
    name = 'validate',
    argv = { 'validate', '--config', root, '--format', 'json', '--output', report },
    code = 0,
    check = function(written: string): boolean return written == '{"issues":[]}' end,
  },
  {
    name = 'lock',
    argv = { 'lock', '--config', root, '--format', 'json', '--output', report },
    code = 0,
    check = function(written: string): boolean
      return select(2, string.gsub(written, '"provider_count":', '')) == keys
    end,
  },
  {
    name = 'drift',
    argv = { 'drift', kept, '--config', root, '--format', 'json', '--output', report },
    code = 0,
    check = function(written: string): boolean return written == '{"entries":[],"counts":{}}' end,
  },
}

--- One run of a command by the package in `folder`: its wall-clock seconds, checked.
local function run(folder: string, command: Command, output: string?): number
  local argv = { 'package', 'run', folder, '--package-path', packagePath, '--trusted', '--' }
  for _, argument in command.argv do
    table.insert(argv, if output and argument == report then output else argument)
  end

  if fs.exists(report) then assert(fs.remove(report)) end

  local started = os.clock()
  local result, message = process.run(jessPath, argv, { stdout = 'null', stderr = 'null' })
  local seconds = os.clock() - started

  if not result then error(`vfstool bench: {jessPath} did not start: {message}`, 0) end

  if result.code ~= command.code then
    error(`vfstool bench: {folder}: {command.name} exited with {result.code}, not {command.code}`, 0)
  end

  local check = command.check
  if check and not output and not check(fs.readFileString(report) or '') then
    error(`vfstool bench: {folder}: {command.name} wrote a report that fails its check`, 0)
  end

  return seconds
end

run(packages[1], Commands[5], kept)

local samples: { [string]: { number } } = {}

for _ = 0, rounds do
  for _, folder in packages do
    for _, command in Commands do
      local name = `{folder}: {command.name}`
      local list = samples[name]
      if not list then
        -- The round before the first is every command's warm-up, and is not kept.
        samples[name] = {}
        run(folder, command)
        continue
      end

      table.insert(list, run(folder, command))
    end
  end
end

assert(fs.remove(root, { recursive = true }))

for _, folder in packages do
  for _, command in Commands do
    local list = samples[`{folder}: {command.name}`]
    table.sort(list)
    print(`{folder}: {command.name}\t{string.format('%.9f', list[(#list + 1) // 2])}`)
  end
end
