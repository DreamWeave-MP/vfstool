--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- The lists a script walks: `#items`, `items[i]` (1-based, nil past the end), `for i, item in
-- items`, and `items:toTable()` for a plain copy. Keys are a frozen array; every other view makes
-- the row a script reads when it reads it, as the native views did, so listing a hundred thousand
-- entries makes no table until one is touched.

export type View<T> = { [number]: T, toTable: (self: View<T>) -> { T } }

--- Every value that is a view, for `serialize`, which writes a view as its rows.
local views = (setmetatable({}, { __mode = 'k' }) :: unknown) :: { [unknown]: () -> { unknown } }

local function copy<T>(items: { T }): { T } return table.move(items, 1, #items, 1, {}) end

local ArrayMeta = table.freeze {
  __index = table.freeze {
    toTable = function<T>(self: { T }): { T } return copy(self) end,
  },
}

--- A frozen array as a view.
local function array<T>(items: { T }): View<T>
  local view = (table.freeze(setmetatable(items, ArrayMeta)) :: unknown) :: View<T>
  views[view] = function(): { unknown } return (copy(items) :: unknown) :: { unknown } end
  return view
end

--- A view of `count` rows that `row(i)` makes when one is read.
local function lazy<T>(count: number, row: (index: number) -> T): View<T>
  local function toTable(): { T }
    local out = table.create(count)
    for index = 1, count do
      out[index] = row(index)
    end

    return out
  end

  local meta = {
    __len = function(): number return count end,
    __index = function(_: unknown, key: unknown): unknown
      if key == 'toTable' then return toTable end

      if type(key) ~= 'number' or key ~= math.floor(key) or key < 1 or key > count then
        return nil
      end

      return row(key)
    end,

    __iter = function(): () -> (number?, T?)
      local index = 0
      return function(): (number?, T?)
        if index >= count then return nil, nil end

        index += 1
        return index, row(index)
      end
    end,
    __newindex = function() error('attempt to modify a readonly table', 2) end,
  }
  local view = (setmetatable({}, meta) :: unknown) :: View<T>
  views[view] = function(): { unknown } return (toTable() :: unknown) :: { unknown } end
  return view
end

--- The rows of a view, or nil for a value that isn't one.
local function rows(value: unknown): { unknown }?
  local list = views[value]
  return if list then list() else nil
end

return table.freeze {
  array = array,
  lazy = lazy,
  rows = rows,
}
