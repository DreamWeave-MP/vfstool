local vfsmod = require('@vfs')
local function collect(view)
    local out = {}
    for _, value in view do
        out[#out + 1] = if type(value) == 'table' and value.key ~= nil then value.key else value
    end
    return out
end
local function eq(actual, expected, label)
    if #actual ~= #expected then error(label .. ': length ' .. tostring(#actual) .. ' ~= ' .. tostring(#expected), 0) end
    for i, value in expected do
        if actual[i] ~= value then error(label .. ': [' .. tostring(i) .. '] ' .. tostring(actual[i]) .. ' ~= ' .. tostring(value), 0) end
    end
end

local vfs = vfsmod.VFS.new()
local r0 = vfs:revision()
vfs:setWinnerFile('foo/a.nif', vfsmod.VfsFile.from('/tmp/foo/a.nif'))
vfs:setWinnerFile('foo/bar.nif', vfsmod.VfsFile.from('/tmp/foo/bar.nif'))
vfs:setWinnerFile('foobar.nif', vfsmod.VfsFile.from('/tmp/foobar.nif'))
vfs:setWinnerFile('meshes/a.nif', vfsmod.VfsFile.from('/tmp/meshes/a.nif'))
vfs:setWinnerFile('meshes/b.nif', vfsmod.VfsFile.from('/tmp/meshes/b.nif'))
assert(vfs:revision() > r0, 'revision did not advance')
eq(collect(vfs:keysStartingWith('foo')), {'foo/a.nif','foo/bar.nif','foobar.nif'}, 'raw prefix')
eq(collect(vfs:pathsWith('foo')), {'foo/a.nif','foo/bar.nif'}, 'component prefix')
eq(collect(vfs:keysStartingWith('meshes/', 1, true)), {'meshes/b.nif'}, 'descending prefix page')
eq(collect(vfs:keys(2, true)), {'meshes/b.nif','meshes/a.nif'}, 'descending key page')
local before = vfs:revision()
vfs:setWinnerFile('meshes/c.nif', vfsmod.VfsFile.from('/tmp/meshes/c.nif'))
assert(vfs:revision() > before, 'revision did not invalidate after mutation')
eq(collect(vfs:keysStartingWith('meshes/', 2, true)), {'meshes/c.nif','meshes/b.nif'}, 'prefix page after mutation')
print('dreamwind vfs perf-index tests: ok')
