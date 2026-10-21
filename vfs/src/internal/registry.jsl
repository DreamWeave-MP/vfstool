--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- Which values are VFS handles, and their state: shared by the VFS class and every report that
-- takes a VFS argument, so neither has to require the other.
local check = require './check'
local types = require './types'

type State = types.State

local states = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [unknown]: State }

local function register(vfs: unknown, state: State) states[vfs] = state end

--- The state of a VFS receiver, or the binder's receiver error.
local function receiver(value: unknown, method: string): State
  local state = states[value]
  if not state then check.receiver(check.method('VFS', method), 'dream.vfs.VFS', value) end

  return state
end

--- The state of a VFS argument at `position` of `what`, or the binder's argument error.
local function argument(value: unknown, what: string, position: number, stack: number): State
  local state = states[value]
  if not state then check.userdata(what, position, stack, 'dream.vfs.VFS', value) end

  return state
end

local function stateOf(value: unknown): State? return states[value] end

return table.freeze {
  register = register,
  receiver = receiver,
  argument = argument,
  stateOf = stateOf,
}
