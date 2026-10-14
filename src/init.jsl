--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- The vfstool CLI parser; reports and behavior stay in the command implementations.
local cli = require '@self/cli'
local commands = require '@self/commands'

type Options = { [string]: unknown }

local function usage(message: string): number
  cli.err(`vfstool: {message}`)
  return cli.Exit.invalidInput
end

local function value(argv: { string }, index: number, option: string): (string?, number)
  local found = argv[index + 1]
  if not found then return nil, cli.Exit.invalidInput end

  return found, index + 1
end

local function run(argv: { string }): number
  if #argv == 0 then return usage('a command is required') end

  local action = table.remove(argv, 1)
  if action == 'help' or action == '--help' or action == '-h' then
    cli.out('vfstool: use jess package test . for command and API tests')
    return cli.Exit.ok
  end

  local options: Options = { relative = false, format = 'yaml' }
  local positional: { string } = {}
  local index = 1

  while index <= #argv do
    local argument = argv[index]
    if argument == '--' then
      for commandIndex = index + 1, #argv do table.insert(positional, argv[commandIndex]) end
      break
    elseif argument == '-c' or argument == '--config' then
      local found, nextIndex = value(argv, index, argument)
      if not found then return usage(`{argument} requires a path`) end
      options.config, index = found, nextIndex
    elseif argument == '-r' or argument == '--use-relative' then
      options.relative = true
    elseif argument == '-f' or argument == '--format' then
      local found, nextIndex = value(argv, index, argument)
      if found ~= 'yaml' and found ~= 'json' and found ~= 'toml' then
        return usage(`unsupported format '{found or ''}'`)
      end
      options.format, index = found, nextIndex
    elseif argument == '-o' or argument == '--output' then
      local found, nextIndex = value(argv, index, argument)
      if not found then return usage('--output requires a path') end
      options.output, index = found, nextIndex
    elseif string.sub(argument, 1, 1) == '-' then
      local booleanOptions: { [string]: string } = {
        ['--allow-copying'] = 'allowCopying',
        ['-a'] = 'allowCopying',
        ['--extract-archives'] = 'extractArchives',
        ['-e'] = 'extractArchives',
        ['--symbolic'] = 'symbolic',
        ['--dry-run'] = 'dryRun',
        ['--only_physical'] = 'onlyPhysical',
        ['-p'] = 'onlyPhysical',
        ['--simple'] = 'simple',
        ['--replacements-only'] = 'replacementsOnly',
        ['--list-files'] = 'listFiles',
        ['-l'] = 'listFiles',
        ['--full'] = 'full',
        ['--fail-on-drift'] = 'failOnDrift',
        ['--keep-merged'] = 'keepMerged',
        ['--copy'] = 'copy',
      }
      local field = booleanOptions[argument]
      if argument == '-s' then field = if action == 'collapse' then 'symbolic' else 'simple' end

      if field then
        options[field] = true
      elseif argument == '--source-index' or argument == '--working-dir' then
        local found, nextIndex = value(argv, index, argument)
        if not found then return usage(`{argument} requires a value`) end
        options[if argument == '--source-index' then 'sourceIndex' else 'workingDir'] =
          if argument == '--source-index' then tonumber(found) else found
        index = nextIndex
      else
        return usage(`unknown option '{argument}'`)
      end
    else
      table.insert(positional, argument)
    end

    index += 1
  end

  local required = function(position: number, label: string): (string?, number?)
    local found = positional[position]
    if found then return found, nil end
    return nil, usage(`{action} requires {label}`)
  end

  if action == 'collapse' then
    local into, failure = required(1, 'a destination directory')
    if not into then return failure :: number end
    options.into = into
    return commands.collapse(options :: commands.CollapseOptions)
  elseif action == 'extract' then
    local source, failure = required(1, 'a source file')
    if not source then return failure :: number end
    local target, targetFailure = required(2, 'a destination directory')
    if not target then return targetFailure :: number end
    options.source, options.target = source, target
    return commands.extract(options :: commands.ExtractOptions)
  elseif action == 'find-file' then
    local path, failure = required(1, 'a VFS path')
    if not path then return failure :: number end
    options.path = path
    return commands.findFile(options :: commands.FindFileOptions)
  elseif action == 'find' then
    local path, failure = required(1, 'a search pattern')
    if not path then return failure :: number end
    options.path = path
    return commands.find(options :: commands.FindOptions)
  elseif action == 'remaining' then
    local path, failure = required(1, 'a data directory')
    if not path then return failure :: number end
    options.filterPath = path
    return commands.remaining(options :: commands.RemainingOptions)
  elseif action == 'duplicates' then
    options.pattern = positional[1]
    return commands.duplicates(options :: commands.DuplicatesOptions)
  elseif action == 'explain' then
    local path, failure = required(1, 'a VFS path')
    if not path then return failure :: number end
    options.path = path
    return commands.explain(options :: commands.ExplainOptions)
  elseif action == 'archive-list' then
    options.archive = positional[1]
    return commands.archiveList(options :: commands.ArchiveListOptions)
  elseif action == 'diff' then
    local sourceA, failure = required(1, 'the first data directory')
    if not sourceA then return failure :: number end
    local sourceB, secondFailure = required(2, 'the second data directory')
    if not sourceB then return secondFailure :: number end
    options.sourceA, options.sourceB = sourceA, sourceB
    return commands.diff(options :: commands.DiffOptions)
  elseif action == 'drift' then
    local lockFile, failure = required(1, 'a lock file')
    if not lockFile then return failure :: number end
    options.lockFile = lockFile
    return commands.drift(options :: commands.DriftOptions)
  elseif action == 'run' then
    local merged, failure = required(1, 'a merged directory')
    if not merged then return failure :: number end
    if #positional < 2 then return usage('run requires a command after the merged directory') end
    options.merged = merged
    local command: { string } = {}
    for commandIndex = 2, #positional do
      table.insert(command, positional[commandIndex])
    end
    options.command = command
    return commands.run(options :: commands.RunOptions)
  elseif action == 'conflicts' then
    return commands.conflicts(options :: cli.Options)
  elseif action == 'shadowed' then
    return commands.shadowed(options :: commands.ShadowedOptions)
  elseif action == 'archives' then
    return commands.archives(options :: cli.Options)
  elseif action == 'contributions' then
    return commands.contributions(options :: cli.Options)
  elseif action == 'validate' then
    return commands.validate(options :: commands.ValidateOptions)
  elseif action == 'lock' then
    return commands.lock(options :: cli.Options)
  end

  return usage(`unknown command '{action}'`)
end

return table.freeze { run = run }
