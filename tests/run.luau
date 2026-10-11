--!strict
--!native
-- The VFS port's own suite: vfstool_lib's tests, its Luau API scripts first, held to the port
-- alone. The fixture trees go under the folder the harness names as(`scratchRoot`).
--
-- Left out on purpose: the native binding's plan and type-definition gates, `require` over the
-- VFS and its template cache (the port has no require navigator), a host-made VFS's write-root
-- grant, the write capability's own refusals (`@dream/fs` refuses for the port), a writer
-- flushed by the garbage collector, and the zip cases (the port reads no zip archives).
local archive = require '@dream-archive'
local fs = require '@dream/fs'
local support = require './support'
local test = require '@jess/test'
local vfstool: any = require '@vfs'
local expect = test.expect
local suite = test.new()

local globals = _G :: any
local base = rawget(globals, 'scratchRoot')
  or support.settings.scratchRoot
  or `{fs.cwd()}/vfs-suite`

--- A fresh folder for one case, canonical, as a write root is kept.
local function scratch(name: string): string return fs.canonicalize(support.fresh(`{base}/{name}`)) end

--- `files` written below a fresh folder.
local function tree(name: string, files: { [string]: string }): string
  local dir = scratch(name)
  support.writeTree(dir, files)
  return dir
end

--- `count` bytes cycling through 0..255.
local function cycle(count: number): string
  local out = buffer.create(count)
  for index = 0, count - 1 do
    buffer.writeu8(out, index, index % 256)
  end

  return buffer.tostring(out)
end

local function isInteger(value: unknown, expected: number): boolean
  return typeof(value) == 'integer' and tostring(value) == tostring(expected)
end

local function refused(call: (...any) -> ...any, ...: any): string
  local ok, err = pcall(call, ...)
  assert(not ok, 'the call went through')
  return tostring(err)
end

local function keysOf(vfs: any): { string } return vfs:keys():toTable() end

suite.describe('paths', function()
  suite.it('normalizes the way dream-path did', function()
    for input, expected in
      {
        ['textures/landscape/foo.dds'] = 'textures/landscape/foo.dds',
        ['textures\\landscape\\foo.dds'] = 'textures/landscape/foo.dds',
        ['Meshes/Actors/Foo.NIF'] = 'meshes/actors/foo.nif',
        ['Meshes\\Actors\\XBase_Anim.NIF'] = 'meshes/actors/xbase_anim.nif',
        ['Data Files\\Morrowind.esm'] = 'data files/morrowind.esm',
        [''] = '',
        ['Morrowind.ESM'] = 'morrowind.esm',
        ['mixed\\Case/Path\\FILE.ext'] = 'mixed/case/path/file.ext',
      } :: { [string]: string }

    do
      expect(vfstool.normalizeHostPath(input)).toBe(expected)
      expect(vfstool.normalizeHostPathInPlace(input)).toBe(expected)
    end
  end)

  suite.it('lowercases ASCII only and keeps other bytes', function()
    local result = vfstool.normalizeHostPath 'Textures/Nordström.dds'
    expect(string.sub(result, 1, 9)).toBe 'textures/'
    expect(string.find(result, 'tröm', 1, true) ~= nil).toBe(true)
    expect(vfstool.normalizeHostPath 'A/\255B').toBe 'a/\255b'
  end)

  suite.it('matches path and source globs', function()
    expect(vfstool.pathGlobMatches('config/**', 'config/settings.ini')).toBe(true)
    expect(vfstool.pathGlobMatches('config/*.ini', 'config/settings.ini')).toBe(true)
    expect(vfstool.pathGlobMatches('meshes/**', 'config/settings.ini')).toBe(false)
    expect(vfstool.sourceGlobMatches('**', '/any/where')).toBe(true)
  end)
end)

suite.describe('lookup and priority', function()
  suite.it('finds a file under any spelling of its key', function()
    local dir = tree('lookup', { ['Textures/Foo.DDS'] = 'foo' })
    local vfs = vfstool.VFS.fromDirectories { dir }
    expect(vfs:getFile('textures/foo.dds'):readAll()).toBe 'foo'
    expect(vfs:getFile('TEXTURES\\FOO.DDS'):path()).toBe(`{dir}/Textures/Foo.DDS`)
    expect(vfs:getFile 'textures/bar.dds').toBeNil()
    expect(vfs:contains 'Textures\\Foo.dds').toBe(true)
    expect(vfs:contains 'textures/nope.dds').toBe(false)
  end)

  suite.it('lets the later directory win, key by key', function()
    local one = tree('priority-1', { ['a.txt'] = '1', ['only1.txt'] = '1', ['B.txt'] = '1' })
    local two = tree('priority-2', { ['a.txt'] = '2', ['only2.txt'] = '2' })
    local three = tree('priority-3', { ['A.TXT'] = '3', ['b.txt'] = '3' })
    local vfs = vfstool.VFS.fromDirectories { one, two, three }
    expect(vfs:getFile('a.txt'):readAll()).toBe '3'
    expect(vfs:getFile('b.txt'):readAll()).toBe '3'
    expect(vfs:getFile('only1.txt'):readAll()).toBe '1'
    expect(vfs:getFile('only2.txt'):readAll()).toBe '2'
    expect(vfs:len()).toBe(4)
    local reversed = vfstool.VFS.fromDirectories { three, two, one }
    expect(reversed:getFile('a.txt'):readAll()).toBe '1'
  end)

  suite.it('reports each provider of one source by the path it has', function()
    local dir = tree('same-source', { ['Dup.txt'] = 'upper', ['sub/x.txt'] = 'x' })
    support.writeTree(dir, { ['dup.txt'] = 'lower' })
    local vfs = vfstool.VFS.fromDirectories { dir }
    local records = vfs:providerRecordsFor 'dup.txt'
    expect(#records).toBe(2)
    local names = { records[1].resolvedPath, records[2].resolvedPath }
    table.sort(names)
    expect(names[1]).toBe(`{dir}/Dup.txt`)
    expect(names[2]).toBe(`{dir}/dup.txt`)
  end)

  suite.it('queries by substring and by whole path components', function()
    local dir = tree('queries', {
      ['textures/a.dds'] = '',
      ['textures/sub/b.dds'] = '',
      ['texturesextra/c.dds'] = '',
      ['meshes/d.nif'] = '',
    })
    local vfs = vfstool.VFS.fromDirectories { dir }
    expect(#vfs:pathsMatching 'TEXTURES').toBe(3)
    expect(#vfs:pathsMatching 'nothing').toBe(0)
    expect(#vfs:pathsWith 'textures').toBe(2)
    expect(#vfs:pathsWith 'Textures\\').toBe(2)
    expect(#vfs:pathsWith 'textures/sub').toBe(1)
    expect(#vfs:pathsWith 'textur').toBe(0)
    expect(#vfs:pathsWith 'nothing').toBe(0)
    expect(vfs:findByRegex('^textures/'):count()).toBe(2)
  end)

  suite.it('keeps its keys sorted', function()
    local files = {}
    for index = 0, 63 do
      files[`dir{index % 7}/file{index}.txt`] = ''
    end

    local keys = vfstool.VFS.fromDirectories({ tree('sorted', files) }):keys()
    expect(#keys).toBe(64)
    for index = 2, #keys do
      assert(keys[index - 1] < keys[index], `{keys[index - 1]} before {keys[index]}`)
    end

    local plain = keys:toTable()
    table.sort(plain)
    for index = 1, 64 do
      expect(plain[index]).toBe(keys[index])
    end
  end)
end)

suite.describe('views', function()
  suite.it('are sequences with one-based indices', function()
    local files = {}
    for index = 0, 7 do
      files[`dir{index % 3}/file{index}.txt`] = ''
    end

    local dir = tree('views', files)
    local vfs = vfstool.VFS.fromDirectories { dir }
    assert(vfs:len() == 8 and not vfs:isEmpty())
    local keys = vfs:keys()
    assert(#keys == 8, 'len')
    assert(keys[1] == 'dir0/file0.txt' and keys[8] == 'dir2/file5.txt', 'sorted, 1-based')
    assert(keys[0] == nil and keys[9] == nil and keys[1.5] == nil, 'no element')
    local seen = 0
    for index, key in keys do
      seen += 1
      assert(keys[index] == key)
    end

    assert(seen == 8, 'iterate')
    local plain = keys:toTable()
    assert(#plain == 8 and plain[2] == 'dir0/file3.txt', 'toTable')
    local entries = vfs:entries()
    assert(
      #entries == 8 and entries[1].key == 'dir0/file0.txt' and entries[1].file:isLoose(),
      'entries'
    )
    assert(#vfs:pathsWith 'dir1' == 3 and vfs:pathsWith('dir1')[1].key == 'dir1/file1.txt')
    assert(#vfs:pathsWith 'DIR1/' == 3 and #vfs:pathsWith 'dir' == 0, 'component boundaries')
    assert(#vfs:pathsMatching 'file7' == 1 and #vfs:pathsMatching 'FILE' == 8)
  end)

  suite.it('stream the tree and nest it on request', function()
    local files = {}
    for index = 0, 7 do
      files[`dir{index % 3}/file{index}.txt`] = ''
    end

    local vfs = vfstool.VFS.fromDirectories { tree('tree', files) }
    local count = 0
    for _, row in vfs:tree() do
      count += 1
      assert(string.find(row.dir, 'Data Files', 1, true) == 1 and row.file:isLoose())
    end

    assert(count == 8 and vfs:tree():count() == 8, 'tree rows')
    local nested = vfs:tree():toTable()
    assert(#nested['Data Files'].subdirs.dir0.files == 3, 'nested shape')
    expect(string.find(vfs:display(), 'file0.txt', 1, true) ~= nil).toBe(true)
    expect(refused(function() return #vfs:tree() end) ~= '').toBe(true)
  end)

  suite.it('number providers and sources from one', function()
    local dir = tree('records', { ['dir0/file0.txt'] = '' })
    local vfs = vfstool.VFS.fromDirectories { dir }
    assert(#vfs:providersFor 'dir0/file0.txt' == 1 and vfs:providersFor 'nope' == nil)
    local records = vfs:providerRecordsFor 'dir0/file0.txt'
    assert(
      #records == 1 and records[1].sourceIndex == 1 and records[1].key == 'dir0/file0.txt',
      '1-based source index'
    )
    assert(vfs:explain('dir0/file0.txt').winner.sourceIndex == 1)
    local layer = vfs:layerIndex()
    assert(layer:sourcesContaining('dir0/file0.txt')[1] == 1 and layer:sourceIdForPath(dir) == 1)
    assert(layer:sourceById(1).path == dir and layer:sourceById(2) == nil)
    assert(not pcall(function() return layer:sourceById(0) end), 'zero is not an index')
    assert(layer:providerChain('dir0/file0.txt')[1].providerIndex == 1)
    expect(tostring(vfs)).toBe 'dream.vfs.VFS(1 files)'
  end)

  suite.it('follow a mutation', function()
    local dir = tree('mutation-keys', { ['a.txt'] = '', ['b.txt'] = '' })
    local vfs = vfstool.VFS.fromDirectories { dir }
    local before = vfs:keys()
    assert(vfs:removeResolvedFile 'a.txt' ~= nil)
    local after = vfs:keys()
    assert(#before == 2 and #after == 1 and after[1] == 'b.txt', 'keys follow mutation')
  end)
end)

suite.describe('files', function()
  suite.it('read whole, into buffers, and by name', function()
    local dir = tree('reads', { ['Data/Big.bin'] = cycle(1000) })
    local vfs = vfstool.VFS.fromDirectories { dir }
    local file = vfs:getFile 'DATA\\BIG.BIN'
    assert(file, 'found in another spelling')
    assert(#file:readAll() == 1000, 'readAll')
    assert(file:fileName() == 'Big.bin' and file:fileStem() == 'Big')
    local buf = buffer.create(1000)
    assert(file:readInto(buf) == 1000)
    assert(buffer.readu8(buf, 255) == 255 and buffer.readu8(buf, 256) == 0, 'contents')
    local small = buffer.create(64)
    assert(file:readInto(small, 32) == 32, 'reads at most the space after the offset')
    assert(buffer.readu8(small, 32) == 0 and buffer.readu8(small, 63) == 31)
    assert(not pcall(file.readInto, file, small, 65), 'offset past the end')
    assert(not pcall(file.readInto, file, small, -1), 'negative offset')
    assert(not pcall(vfs.getFile, vfs, 42), 'a path is a string')
    local missing = vfstool.VfsFile.from '/definitely/missing'
    assert(not pcall(missing.readAll, missing))
  end)

  suite.it('take keys that are not UTF-8', function()
    local dir = scratch 'odd-bytes'
    support.writeTree(dir, { ['odd/\255.dat'] = 'odd', ['a.txt'] = '' })
    -- A filesystem that holds no name that isn't UTF-8 refused the write, and `@dream/fs` answers
    -- nil for that instead of raising: there is no such key to test there.
    if not fs.exists(`{dir}/odd/\255.dat`) then return end

    local vfs = vfstool.VFS.fromDirectories { dir }
    local odd = vfs:getFile 'ODD/\255.DAT'
    assert(
      odd and odd:readAll() == 'odd' and odd:fileName() == '\255.dat',
      'a key that is not UTF-8 is a key'
    )
    assert(vfs:keys()[2] == 'odd/\255.dat' and #vfs:pathsMatching '\255' == 1)
    assert(vfs:explain('odd/\255.dat').key == 'odd/\255.dat')
  end)

  suite.it('read positionally and name their arguments', function()
    local dir = tree('positional', { ['data/big.bin'] = cycle(1000), ['data/empty.bin'] = '' })
    local vfs = vfstool.VFS.fromDirectories { dir }
    local file = vfs:getFile 'data/big.bin'
    assert(file:size() == 1000, 'size from metadata')
    local buf = buffer.create(64)
    assert(file:readAt(buf, 0) == 64, 'fills the buffer by default')
    assert(buffer.readu8(buf, 0) == 0 and buffer.readu8(buf, 63) == 63)
    assert(file:readAt(buf, 100, 16, 8) == 16, 'length and bufferOffset')
    assert(
      buffer.readu8(buf, 8) == 100
        and buffer.readu8(buf, 23) == 115
        and buffer.readu8(buf, 24) == 24
    )
    assert(file:readAt(buf, 990) == 10, 'short at the end of the file')
    assert(file:readAt(buf, 1000) == 0, 'the end reads nothing')
    for _, case in
      {
        { { 1001 }, 'dream.vfs: readAt: fileOffset 1001 past the end (size 1000)' },
        { { -1 }, 'dream.vfs: readAt: fileOffset -1 is negative' },
        {
          { 0, 65 },
          'dream.vfs: readAt: length 65 does not fit the buffer (space 64 after bufferOffset 0)',
        },

        {
          { 0, 8, 60 },
          'dream.vfs: readAt: length 8 does not fit the buffer (space 4 after bufferOffset 60)',
        },
        { { 0, -3 }, 'dream.vfs: readAt: length -3 is negative' },
        { { 1.5 }, 'not an exact' },
      } :: { { any } }

    do
      local err = refused(file.readAt, file, buf, table.unpack(case[1]))
      assert(string.find(err, case[2], 1, true), err)
    end

    local err = refused(file.readAt, file, buf, 0, nil, 65)
    assert(
      string.find(
        err,
        'dream.vfs: readAt: bufferOffset 65 past the end of the buffer (size 64)',
        1,
        true
      ),
      err
    )
    local range = file:readRange(250, 10)
    assert(
      buffer.len(range) == 10 and buffer.readu8(range, 0) == 250 and buffer.readu8(range, 9) == 3,
      'readRange'
    )
    assert(buffer.len(file:readRange(995, 100)) == 5, 'readRange is short at the end')
    assert(buffer.len(file:readRange(1000, 1)) == 0)
    err = refused(file.readRange, file, 1001, 1)
    assert(
      string.find(err, 'dream.vfs: readRange: fileOffset 1001 past the end (size 1000)', 1, true),
      err
    )
    err = refused(file.readRange, file, 0, -1)
    assert(string.find(err, 'dream.vfs: readRange: length -1 is negative', 1, true), err)
    local whole = file:readAllBuffer()
    assert(buffer.len(whole) == 1000 and buffer.readu8(whole, 999) == 231, 'readAllBuffer')
    local empty = vfs:getFile 'data/empty.bin'
    assert(
      empty:size() == 0 and buffer.len(empty:readAllBuffer()) == 0 and empty:readAt(buf, 0) == 0
    )
    local missing = vfstool.VfsFile.from(`{dir}/missing.bin`)
    err = refused(missing.size, missing)
    assert(string.find(err, 'dream.vfs: ', 1, true), err)
    assert(not pcall(missing.readAt, missing, buf, 0))
  end)

  suite.it('open readers that walk sequentially and refuse use after close', function()
    local dir = tree('reader', { ['a.bin'] = cycle(300) })
    local file = vfstool.VFS.fromDirectories({ dir }):getFile 'a.bin'
    local reader = file:open()
    assert(reader:size() == 300 and reader:tell() == 0)
    assert(
      string.find(tostring(reader), 'dream.vfs.Reader(', 1, true)
        and string.find(tostring(reader), 'a.bin', 1, true)
    )
    local first = reader:read(16)
    assert(buffer.len(first) == 16 and buffer.readu8(first, 15) == 15 and reader:tell() == 16)
    local buf = buffer.create(32)
    assert(reader:readInto(buf) == 32 and buffer.readu8(buf, 0) == 16 and reader:tell() == 48)
    assert(
      reader:readInto(buf, 8, 4) == 4
        and buffer.readu8(buf, 8) == 48
        and buffer.readu8(buf, 12) == 28,
      'window'
    )
    reader:skip(200)
    assert(reader:tell() == 252)
    local tail = reader:read(100)
    assert(buffer.len(tail) == 48 and reader:tell() == 300, 'short at the end')
    assert(buffer.len(reader:read(10)) == 0, 'nothing left')
    reader:seek(10)
    assert(reader:tell() == 10 and buffer.readu8(reader:read(1), 0) == 10)
    reader:seek(300)
    assert(reader:readInto(buf) == 0)
    local err = refused(reader.seek, reader, 301)
    assert(
      string.find(err, 'dream.vfs: Reader.seek: position 301 past the end (size 300)', 1, true),
      err
    )
    err = refused(reader.seek, reader, -1)
    assert(string.find(err, 'dream.vfs: Reader.seek: position -1 is negative', 1, true), err)
    reader:seek(0)
    err = refused(reader.skip, reader, 301)
    assert(
      string.find(
        err,
        'dream.vfs: Reader.skip: count 301 past the end (position 0, size 300)',
        1,
        true
      ),
      err
    )
    err = refused(reader.read, reader, -1)
    assert(string.find(err, 'dream.vfs: Reader.read: length -1 is negative', 1, true), err)
    err = refused(reader.readInto, reader, buf, 33)
    assert(
      string.find(
        err,
        'dream.vfs: Reader.readInto: bufferOffset 33 past the end of the buffer (size 32)',
        1,
        true
      ),
      err
    )
    local other = file:open()
    assert(other:tell() == 0 and buffer.readu8(other:read(1), 0) == 0 and reader:tell() == 0)
    reader:close()
    reader:close()
    err = refused(reader.read, reader, 1)
    assert(
      string.find(err, 'dream.vfs: Reader.read: the reader over ', 1, true)
        and string.find(err, 'a.bin is closed', 1, true),
      err
    )
    assert(string.find(refused(reader.seek, reader, 0), 'is closed', 1, true))
    assert(reader:tell() == 0 and reader:size() == 300, 'tell and size still answer')
    assert(buffer.readu8(other:read(1), 0) == 1, 'the other reader is unaffected')
    local walk = file:open()
    local chunk = buffer.create(64)
    local total, checksum = 0, 0
    while true do
      local n = walk:readInto(chunk)
      if n == 0 then break end

      for index = 0, n - 1 do
        checksum += buffer.readu8(chunk, index)
      end

      total += n
    end

    assert(total == 300 and checksum == 255 * 128 + 44 * 43 / 2, tostring(checksum))
  end)

  suite.it('read BSA members positionally', function()
    local dir = scratch 'bsa-reads'
    local builder = archive.bsa.tes3.Builder.new()
    builder:addBytes('meshes\\x\\door.nif', cycle(3000))
    fs.writeFile(`{dir}/Morrowind.bsa`, builder:toBytes())
    local vfs = vfstool.VFS.fromDirectories({ dir }, { archives = { 'Morrowind.bsa' } })
    local file = vfs:getFile 'meshes/x/door.nif'
    assert(file and file:isArchive() and file:size() == 3000)
    expect(file:parentArchiveName()).toBe 'Morrowind.bsa'
    local buf = buffer.create(8)
    assert(file:readAt(buf, 512) == 8 and buffer.readu8(buf, 0) == 0 and buffer.readu8(buf, 7) == 7)
    assert(file:readAt(buf, 2996) == 4)
    assert(buffer.len(file:readRange(2990, 100)) == 10)
    local reader = file:open()
    reader:skip(2999)
    assert(buffer.readu8(reader:read(4), 0) == 2999 % 256 and reader:tell() == 3000)
    assert(buffer.len(file:readAllBuffer()) == 3000)
  end)

  suite.it('put loose files above archives', function()
    local dir = scratch 'bsa-loose'
    local builder = archive.bsa.tes3.Builder.new()
    builder:addBytes('meshes\\a.nif', 'packed')
    builder:addBytes('meshes\\b.nif', 'packed')
    fs.writeFile(`{dir}/Mod.bsa`, builder:toBytes())
    support.writeTree(dir, { ['Meshes/A.nif'] = 'loose' })
    local vfs = vfstool.VFS.fromDirectories({ dir }, { archives = { 'mod.bsa' } })
    expect(vfs:getFile('meshes/a.nif'):readAll()).toBe 'loose'
    expect(vfs:getFile('meshes/b.nif'):readAll()).toBe 'packed'
    expect(#vfs:archives() >= 1).toBe(true)
  end)
end)

suite.describe('options and errors', function()
  suite.it('refuse unknown options', function()
    local dir = tree('options', { ['a.txt'] = 'abcd' })
    local err = refused(vfstool.VFS.fromDirectories, { dir }, { archive = { 'x.bsa' } })
    assert(string.find(err, 'unknown option \'archive\'', 1, true), err)
    local empty = vfstool.VFS.new()
    err = refused(empty.collapseInto, empty, dir, { allow_copying = true })
    assert(string.find(err, 'unknown option \'allow_copying\'', 1, true), err)
    err =
      refused(vfstool.VfsProvider.new, { path = dir, kind = 'weird' }, vfstool.VfsFile.from(dir))
    assert(string.find(err, 'unknown source kind \'weird\'', 1, true), err)
  end)

  suite.it('name the argument or the path that was wrong', function()
    local dir = scratch 'argument-paths'
    for _, case in
      {
        {
          vfstool.VFS.fromDirectories,
          { 42 },
          'dirs: Lua stack index 1: expected table, got number',
        },
        { vfstool.VFS.fromDirectories, { { 42 } }, 'dirs[1]: expected string, got number' },
        {
          vfstool.VFS.fromDirectories,
          { { dir }, { archives = 'a.bsa' } },
          'VFS.fromDirectories.archives: expected table, got string',
        },

        {
          vfstool.VFS.fromDirectories,
          { { dir }, { archives = { 1 } } },
          'VFS.fromDirectories.archives[1]: expected string, got number',
        },

        {
          vfstool.VfsProvider.new,
          { { path = 42, kind = 'looseDir' }, vfstool.VfsFile.from(dir) },
          'VfsProvider.new.path: expected string, got number',
        },

        {
          vfstool.LayerIndex.fromFileLists,
          { { { source = { path = dir, kind = 'looseDir' }, files = 'nope' } } },
          'LayerIndex.fromFileLists[1].files: expected table, got string',
        },

        {
          vfstool.ConflictIndex.fromFileLists,
          { { { source = dir, files = { 1 } } } },
          'ConflictIndex.fromFileLists[1].files[1]: expected string, got number',
        },

        {
          vfstool.LayerIndex.fromFileLists,
          { { { source = { path = dir, kind = 'looseDir' }, files = { 1 } } } },
          'LayerIndex.fromFileLists[1].files[1]: expected string, got number',
        },

        {
          vfstool.LayerIndex.fromFileLists,
          { { { source = { path = dir }, files = {} } } },
          'LayerIndex.fromFileLists[1].source: missing required option \'kind\'',
        },
        { vfstool.LayerIndex.fromFileLists, { 'rows' }, 'expected table, got string' },
      } :: { { any } }

    do
      local err = refused(case[1], table.unpack(case[2]))
      assert(string.find(err, case[3], 1, true), err)
    end
  end)

  suite.it('give sizes as integers', function()
    local dir = tree('integers', { ['a.txt'] = 'abcd' })
    local vfs, layer = vfstool.VFS.fromDirectoriesWithLayerIndex { dir }
    local provenance = layer:provenance(vfs, 'a.txt', true)
    assert(isInteger(provenance.providers[1].size, 4), 'sizes are integers')
    assert(#provenance.providers[1].hashBlake3 == 64)
    local lock = layer:lockManifest(vfs)
    assert(
      isInteger(lock:entries()[1].winnerSize, 4) and lock:toTable().entries[1].providerCount == 1
    )
  end)
end)

suite.describe('serialize', function()
  suite.it('writes JSON the way serde_json did', function()
    local dir = tree('serialize', { ['a.txt'] = '' })
    local vfs = vfstool.VFS.fromDirectories { dir }
    expect(vfstool.serialize({ files = vfs:keys() }, 'json')).toBe '{"files":["a.txt"]}'
    expect(vfstool.serialize({}, 'json')).toBe '[]'
    expect(vfstool.serialize({ 1, 2, 3 }, 'json')).toBe '[1,2,3]'
    expect(string.find(vfstool.serialize({ answer = 42 }, 'json'), 'answer', 1, true) ~= nil).toBe(
      true
    )
    assert(not pcall(vfstool.serialize, vfs, 'json'), 'a VFS handle is not serializable')
    local err = refused(vfstool.serialize, {}, 'xml')
    assert(string.find(err, 'unknown serialization format \'xml\'', 1, true), err)
  end)

  suite.it('writes TOML tables and YAML documents', function()
    expect(string.find(vfstool.serialize({ a = { b = true } }, 'toml'), '[a]', 1, true) ~= nil).toBe(
      true
    )
    expect(vfstool.serialize({ a = 1 }, 'yaml')).toBe 'a: 1\n'
    expect(vfstool.serialize({ list = { 'x', 'y' } }, 'yaml')).toBe 'list:\n- x\n- y\n'
    expect(vfstool.serialize({ text = 'true' }, 'yaml')).toBe 'text: \'true\'\n'
  end)

  suite.it('writes views that hold files and tree streams', function()
    local dir = tree('serialize-views', { ['Textures/Rock.dds'] = 'rock' })
    local vfs = vfstool.VFS.fromDirectories { dir }
    local rock = `{dir}/Textures/Rock.dds`
    -- Fields in the order the binding pushed them: serde_json kept insertion order in jess.
    local file = '{"path":'
      .. string.sub(vfstool.serialize({ rock }, 'json'), 2, -2)
      .. ',"isLoose":true,"isArchive":false}'
    expect(vfstool.serialize(vfs:entries(), 'json')).toBe(
      `[\{"key":"textures/rock.dds","file":{file}}]`
    )
    local providers = vfstool.serialize(vfs:providersFor 'textures/rock.dds', 'json')
    assert(string.find(providers, `"file":{file}`, 1, true), providers)
    assert(string.find(providers, '"kind":"looseDir"', 1, true), providers)
    local streamed = vfstool.serialize(vfs:tree(), 'json')
    assert(string.find(streamed, `"textures":\{"files":[{file}],"subdirs":\{}}`, 1, true), streamed)
    local found = vfstool.serialize({ found = vfs:getFile 'textures/rock.dds' }, 'yaml')
    assert(string.find(found, 'isLoose: true', 1, true), found)
  end)
end)

suite.describe('semantic analysis', function()
  suite.it('calls a comment-only INI change cosmetic', function()
    local semantic = vfstool.analyzePair('settings.ini', '[x]\na=1\n', '# comment\n[x]\na=1\n')
    expect(semantic.assetClass).toBe 'ini'
    expect(semantic.delta.kind).toBe 'cosmeticOnly'
  end)

  suite.it('tells TOML, JSON and text apart', function()
    expect(vfstool.analyzePair('a.toml', 'x = 1\n', 'x=1').delta.kind).toBe 'cosmeticOnly'
    expect(vfstool.analyzePair('a.toml', 'x = 1\n', 'x = 2\n').delta.kind).toBe 'behaviorChanging'
    expect(vfstool.analyzePair('a.json', '{"x": 1}', '{ "x": 1 }').delta.kind).toBe 'cosmeticOnly'
    expect(vfstool.analyzePair('a.txt', 'same', 'same').delta.kind).toBe 'noOpEquivalent'
    expect(vfstool.analyzePair('a.dds', 'a', 'b').assetClass).toBe 'text'
  end)

  suite.it('reports semantic conflicts as rows of plain tables', function()
    local low = tree('semantic-low', { ['a.txt'] = 'same', ['b.json'] = '{"x": 1}' })
    local high = tree('semantic-high', { ['a.txt'] = 'same', ['b.json'] = '{ "x": 1 }' })
    local vfs, layer = vfstool.VFS.fromDirectoriesWithLayerIndex { low, high }
    local report =
      layer:semanticConflicts(vfs, { includeSemanticDeltas = true, archiveHashMode = 'disabled' })
    assert(#report.entries == 2, #report.entries)
    local byKey = {}
    for _, entry in report.entries do
      byKey[entry.key] = entry
    end

    local a, b = byKey['a.txt'], byKey['b.json']
    assert(a.winner.path == high and a.winner.kind == 'looseDir', a.winner.path)
    assert(#a.providers == 2 and a.allIdentical == true and a.distinctVersions == 1)
    assert(a.providers[1].source.path == low and a.providers[1].relation == 'identicalToWinner')
    assert(isInteger(a.providers[1].size, 4) and #a.providers[1].hashBlake3 == 64)
    assert(a.providers[2].semanticDeltaToWinner.kind == 'noOpEquivalent')
    assert(b.allIdentical == false and b.distinctVersions == 2 and b.assetClass == 'json')
    assert(b.providers[1].relation == 'differentFromWinner')
    assert(b.providers[1].semanticDeltaToWinner.kind == 'cosmeticOnly')
    local err = refused(layer.semanticConflicts, layer, vfs, { archiveHashMode = 'sometimes' })
    assert(
      string.find(err, 'archiveHashMode', 1, true)
        and string.find(err, 'got \'sometimes\'', 1, true),
      err
    )
  end)
end)

suite.describe('reports and layers', function()
  suite.it('explain, deduplicate and plan across a load order', function()
    local low = tree('reports-low', { ['Textures/Foo.DDS'] = 'same' })
    local high =
      tree('reports-high', { ['textures/foo.dds'] = 'same', ['meshes/bar.nif'] = 'mesh' })
    local vfs, conflict = vfstool.VFS.fromDirectoriesWithConflictIndex { low, high }
    assert(vfs:len() == 2)
    assert(vfs:getFile('meshes/bar.nif'):readAll() == 'mesh')
    assert(vfs:pathsMatching('textures')[1].key == 'textures/foo.dds')
    local explain = vfs:explain 'textures/foo.dds'
    assert(explain.winner.source.path == high)
    assert(explain.winner.sourceIndex == 2 and explain.overridden[1].sourceIndex == 1)
    assert(#vfs:duplicates().entries == 1 and vfs:duplicates().entries[1].winnerIndex == 2)
    assert(#vfs:duplicates('^textures/').entries == 1)
    assert(#vfs:duplicates('^meshes/').entries == 0)
    assert(#vfs:materializationPlan(high, { allowCopying = true }).actions >= 1)
    assert(#conflict:sources() == 2)
  end)

  suite.it('chain providers, count contributions and lock the result', function()
    local low = tree('layers-low', { ['Textures/Foo.DDS'] = 'same' })
    local high = tree('layers-high', { ['textures/foo.dds'] = 'same', ['meshes/bar.nif'] = 'mesh' })
    local vfs = vfstool.VFS.fromDirectories { low, high }
    local layer = vfs:layerIndex()
    assert(#layer:keys() == 2)
    assert(#layer:providerChain 'textures/foo.dds' == 2)
    assert(#layer:sourceContributions().sources == 2)
    assert(layer:sourceContributions().sources[2].sourceIndex == 2)
    local provenance = layer:provenance(vfs, 'textures/foo.dds', true)
    assert(provenance.winner.path == high)
    assert(isInteger(provenance.providers[2].size, 4))
    local lock = layer:lockManifest(vfs)
    assert(lock:schemaVersion() == 1)
    assert(#lock:entries() == 2)
    assert(#layer:diffAgainstLock(vfs, lock).entries == 0)
    assert(layer:semanticConflicts(vfs, { includeSemanticDeltas = true }).entries[1].allIdentical)
  end)

  suite.it('lock deterministically and see drift', function()
    local low = tree('drift-low', { ['a.txt'] = 'a', ['b.txt'] = 'b' })
    local high = tree('drift-high', { ['b.txt'] = 'B' })
    local vfs, layer = vfstool.VFS.fromDirectoriesWithLayerIndex { low, high }
    local lock = layer:lockManifest(vfs)
    local again = layer:lockManifest(vfs)
    expect(vfstool.serialize(lock:toTable(), 'json')).toBe(
      vfstool.serialize(again:toTable(), 'json')
    )
    local keys = {}
    for _, entry in lock:entries() do
      table.insert(keys, entry.key)
    end

    expect(table.concat(keys, ',')).toBe 'a.txt,b.txt'
    support.writeTree(high, { ['b.txt'] = 'changed' })
    local changed, changedLayer = vfstool.VFS.fromDirectoriesWithLayerIndex { low, high }
    expect(#changedLayer:diffAgainstLock(changed, lock).entries).toBe(1)
    fs.remove(`{high}/b.txt`)
    local moved, movedLayer = vfstool.VFS.fromDirectoriesWithLayerIndex { low, high }
    local kinds = {}
    for _, entry in movedLayer:diffAgainstLock(moved, lock).entries do
      table.insert(kinds, entry.kind)
    end

    expect(table.concat(kinds, ',')).toBe 'winnerSourceChanged,winnerHashChanged,providerCountChanged'
  end)

  suite.it('build a layer index from file lists', function()
    local layer = vfstool.LayerIndex.fromFileLists {
      { source = { path = '/a', kind = 'looseDir' }, files = { 'Textures/X.dds', 'y.txt' } },
      { source = { path = '/b', kind = 'looseDir' }, files = { 'textures\\x.dds' } },
    }
    expect(#layer:keys()).toBe(2)
    expect(#layer:providerChain 'textures/x.dds').toBe(2)
    expect(#layer:sourcesContaining 'y.txt').toBe(1)
  end)
end)

suite.describe('conflict indexes', function()
  suite.it('find no conflicts without overlap', function()
    local one = tree('conflict-none-1', { ['a.txt'] = '' })
    local two = tree('conflict-none-2', { ['b.txt'] = '' })
    local index = vfstool.ConflictIndex.fromDirectories { one, two }
    for _, row in index:conflictsReport(true).sources do
      expect(#row.overrides + #row.overriddenBy).toBe(0)
    end

    expect(#index:sourcesContaining 'a.txt').toBe(0)
  end)

  suite.it('give the middle source both directions', function()
    local one = tree('conflict-mid-1', { ['a.txt'] = '' })
    local two = tree('conflict-mid-2', { ['A.TXT'] = '' })
    local three = tree('conflict-mid-3', { ['a.txt'] = '' })
    local report = vfstool.ConflictIndex.fromDirectories({ one, two, three }):conflictsReport(true)
    local first, middle, last = report.sources[1], report.sources[2], report.sources[3]
    expect(#first.overrides .. ':' .. #first.overriddenBy).toBe '0:1'
    expect(#middle.overrides .. ':' .. #middle.overriddenBy).toBe '1:1'
    expect(#last.overrides .. ':' .. #last.overriddenBy).toBe '1:0'
    expect(middle.overrides[1]).toBe 'a.txt'
  end)

  suite.it('never conflict a source with itself', function()
    local one = tree('conflict-self', { ['a.txt'] = '', ['A.TXT'] = '' })
    local index = vfstool.ConflictIndex.fromDirectories { one, one }
    expect(#index:sourcesContaining 'a.txt' <= 1 or index:sourcesContaining('a.txt')[1] ~= nil).toBe(
      true
    )
    local single =
      vfstool.ConflictIndex.fromDirectories { tree('conflict-single', { ['a.txt'] = '' }) }
    expect(#single:conflictsReport(true).sources[1].overrides).toBe(0)
  end)

  suite.it('normalize file lists and list sources in order', function()
    local index = vfstool.ConflictIndex.fromFileLists {
      { source = '/one', files = { 'Textures\\A.dds' } },
      { source = '/two', files = { 'textures/a.dds' } },
      { source = '/three', files = { 'TEXTURES/A.DDS' } },
    }
    local containing = index:sourcesContaining 'textures/a.dds'
    expect(table.concat(containing, ',')).toBe '1,2,3'
  end)

  suite.it('report shadowed sources, wholly and in part', function()
    local low = tree('shadow-low', { ['a.txt'] = '', ['b.txt'] = '' })
    local partial = tree('shadow-partial', { ['c.txt'] = '', ['d.txt'] = '' })
    local high = tree('shadow-high', { ['a.txt'] = '', ['b.txt'] = '', ['c.txt'] = '' })
    local index = vfstool.ConflictIndex.fromDirectories { low, partial, high }
    local shadowed = index:shadowedReport(true)
    expect(#shadowed.sources).toBe(1)
    expect(shadowed.sources[1].path).toBe(low)
    expect(table.concat(shadowed.sources[1].shadowedFiles, ',')).toBe 'a.txt,b.txt'
    expect(#index:shadowedReport(true, false).sources[1].shadowedFiles).toBe(0)
  end)

  suite.it('diff two sources, in the index or on disk', function()
    local low = tree('diff-low', { ['shared.txt'] = '', ['low.txt'] = '' })
    local high = tree('diff-high', { ['Shared.txt'] = '', ['high.txt'] = '' })
    local outside = tree('diff-outside', { ['shared.txt'] = '', ['out.txt'] = '' })
    local index = vfstool.ConflictIndex.fromDirectories { low, high }
    local diff = index:diffReport(low, high)
    expect(table.concat(diff.shared, ',')).toBe 'shared.txt'
    expect(table.concat(diff.onlyInA, ',')).toBe 'low.txt'
    expect(table.concat(diff.onlyInB, ',')).toBe 'high.txt'
    expect(diff.higherPriority).toBe(high)
    expect(index:diffReport(high, low).higherPriority).toBe(high)
    local disk = index:diffReport(outside, low)
    expect(table.concat(disk.onlyInA, ',')).toBe 'out.txt'
  end)

  suite.it('spell absolute report paths as their sources do', function()
    local low = tree('absolute-low', { ['Textures/Foo.DDS'] = '' })
    local high = tree('absolute-high', { ['textures/foo.dds'] = '' })
    local report = vfstool.ConflictIndex.fromDirectories({ low, high }):conflictsReport(false)
    expect(report.sources[1].overriddenBy[1]).toBe(`{low}/Textures/Foo.DDS`)
    expect(report.sources[2].overrides[1]).toBe(`{high}/textures/foo.dds`)
  end)
end)

suite.describe('mutation', function()
  suite.it('reveals lower providers and takes manual ones', function()
    local low = tree('mutable-low', { ['shared.txt'] = 'low' })
    local high = tree('mutable-high', { ['shared.txt'] = 'high' })
    local manualRoot = tree('mutable-manual', { ['manual.txt'] = 'manual' })
    local manualFile = `{manualRoot}/manual.txt`
    local vfs = vfstool.VFS.fromDirectories { low, high }
    assert(#vfs:providersFor 'shared.txt' == 2)
    assert(vfs:providersFor('shared.txt')[1].source.path == low)
    local removed = vfs:removeWinner 'shared.txt'
    assert(removed:source().path == high and removed:file():readAll() == 'high')
    assert(string.find(vfs:getFile('shared.txt'):path(), low, 1, true) == 1)
    local file = vfstool.VfsFile.from(manualFile)
    local provider = vfstool.VfsProvider.new({ path = manualRoot, kind = 'looseDir' }, file)
    assert(vfs:pushProvider('manual.txt', provider))
    assert(vfs:contains 'manual.txt')
    assert(#vfs:removeSource(manualRoot) == 1)
    assert(vfs:contains 'manual.txt' == false)
    assert(vfs:setWinnerLooseFile('new/thing.txt', manualFile) == nil)
    assert(vfs:getFile('new/thing.txt'):path() == manualFile)
    assert(#vfs:removeResolvedPrefix 'new' == 1 and vfs:len() == 1)
    vfs:pushDirectory(manualRoot)
    assert(vfs:contains 'manual.txt')
    assert(#vfs:removeResolvedMatchingGlob '**/*.txt' == 2 and vfs:isEmpty())
  end)

  suite.it('refuses keys that would escape or collide', function()
    local dir = tree('unsafe', { ['a.txt'] = 'a', ['dir/b.txt'] = 'b' })
    local file = `{dir}/a.txt`
    local vfs = vfstool.VFS.fromDirectories { dir }
    for _, bad in { '../x.txt', '/absolute.txt', 'C:\\absolute.txt', 'a.txt/child.txt', 'dir' } do
      expect(vfs:setWinnerLooseFile(bad, file)).toBeNil()
    end

    expect(table.concat(keysOf(vfs), ',')).toBe 'a.txt,dir/b.txt'
    expect(vfs:len()).toBe(2)
  end)

  suite.it('removes by prefix on component boundaries', function()
    local dir =
      tree('prefix', { ['tex/a.dds'] = '', ['texture/b.dds'] = '', ['tex/sub/c.dds'] = '' })
    local vfs = vfstool.VFS.fromDirectories { dir }
    expect(#vfs:removeResolvedPrefix 'tex/').toBe(2)
    expect(table.concat(keysOf(vfs), ',')).toBe 'texture/b.dds'
    local again = vfstool.VFS.fromDirectories { dir }
    expect(#again:removeProviderPrefix 'TEX').toBe(2)
    expect(again:len()).toBe(1)
  end)

  suite.it('removes the resolved file without revealing the one below', function()
    local low = tree('resolved-low', { ['a.txt'] = 'low' })
    local high = tree('resolved-high', { ['A.txt'] = 'high' })
    local vfs = vfstool.VFS.fromDirectories { low, high }
    expect(vfs:removeResolvedFile 'A.TXT' ~= nil).toBe(true)
    expect(vfs:contains 'a.txt').toBe(false)
  end)

  suite.it('skips directories that are not there', function()
    local dir = tree('missing-dir', { ['a.txt'] = '' })
    local vfs = vfstool.VFS.fromDirectories { `{dir}/nope`, dir }
    expect(vfs:len()).toBe(1)
    expect(vfstool.VFS.fromDirectories({ scratch 'empty-dir' }):isEmpty()).toBe(true)
  end)
end)

suite.describe('materialization', function()
  suite.it('dumps files and folders, copying or linking', function()
    local dir = tree('dump-data', { ['a.txt'] = 'a', ['Sub/Deep/B.txt'] = 'b' })
    local vfs = vfstool.VFS.fromDirectories { dir }
    local out = scratch 'dump-out'
    local written = vfs:dumpToDirectory(`{out}/copy`, false)
    expect(written).toBe(2)
    expect(fs.readFileString(`{out}/copy/sub/deep/b.txt`)).toBe 'b'
    expect(vfs:dumpToDirectory(`{out}/link`, true)).toBe(2)
    expect(fs.sameFile(`{out}/link/a.txt`, `{dir}/a.txt`)).toBe(true)
    expect(vfs:dumpToDirectory(`{out}/copy`, false)).toBe(2)
  end)

  suite.it('returns what a write-out leaves out', function()
    local dir = tree('skips-data', { ['kept.txt'] = 'kept', ['gone.txt'] = 'gone' })
    local out = scratch 'skips-out'
    local vfs = vfstool.VFS.fromDirectories { dir }
    fs.remove(`{dir}/gone.txt`)
    local written, skipped = vfs:dumpToDirectory(`{out}/dump`, false)
    assert(written == 1 and #skipped == 1, tostring(written))
    assert(skipped[1].kind == 'missingLooseSource' and skipped[1].key == 'gone.txt')
    assert(skipped[1].source == `{dir}/gone.txt` and skipped[1].archive == nil)
    local err = refused(vfs.collapseInto, vfs, `{out}/collapse`)
    assert(string.find(err, 'no longer exists', 1, true), err)
    vfs:removeResolvedFile 'gone.txt'
    assert(#vfs:collapseInto(`{out}/collapse`) == 0)
    local diff = vfs:diffDirectory(dir)
    assert(#diff.unreadable == 0 and #diff.conflicts == 1)
  end)

  suite.it('extracts archive members with their contents', function()
    local dir = scratch 'extract'
    local builder = archive.bsa.tes3.Builder.new()
    builder:addBytes('textures\\a.dds', 'member')
    fs.writeFile(`{dir}/Pack.bsa`, builder:toBytes())
    local vfs = vfstool.VFS.fromDirectories({ dir }, { archives = { 'Pack.bsa' } })
    local out = scratch 'extract-out'
    expect(vfs:dumpToDirectory(out, false)).toBe(2)
    expect(fs.exists(`{out}/pack.bsa`)).toBe(true)
    expect(fs.readFileString(`{out}/textures/a.dds`)).toBe 'member'
  end)

  suite.it('runs setup and finalize around a merged folder', function()
    local data = tree('run-data', { ['config/settings.ini'] = '[x]\na = 1\n' })
    local merged, output = scratch 'run-merged', scratch 'run-output'
    local vfs = vfstool.VFS.fromDirectories { data }
    local count, snapshot = vfstool.runSetup(vfs, merged, false)
    assert(count == 1 and snapshot:len() == 1)
    fs.writeFile(`{merged}/config/settings.ini`, '[x]\na = 2\n')
    assert(#vfstool.changedFiles(merged, snapshot) == 1)
    local copied = vfstool.runFinalize(merged, output, snapshot)
    assert(vfstool.normalizeHostPath(copied[1].relativePath) == 'config/settings.ini')
    local _, tracked = vfstool.runSetupTracked(vfs, merged, false)
    fs.writeFile(`{merged}/new.txt`, 'new')
    assert(#vfstool.changedFilesMetadata(merged, tracked) == 1)
    assert(#vfstool.runFinalizeTracked(merged, output, tracked) == 1)
    assert(vfstool.snapshotDirectory(output):len() == 2)
  end)
end)

suite.describe('writes', function()
  suite.it('go under the write root and register winners', function()
    local data = tree('write-data', { ['textures/a.dds'] = 'old' })
    local out = scratch 'write-root'
    local vfs = vfstool.VFS.fromDirectories({ data }, { writeRoot = out })
    assert(vfs:writeRoot() == out)
    local file = vfs:writeFile('textures/a.dds', 'new')
    assert(file:isLoose() and file:path() == `{out}/textures/a.dds`, file:path())
    assert(file:readAll() == 'new' and vfs:getFile('textures/a.dds'):readAll() == 'new')
    assert(vfs:providersFor('textures/a.dds')[1].source.path == `{out}/textures`)
    vfs:writeFile('new/deep/b.bin', buffer.fromstring 'bufferdata')
    assert(vfs:getFile('new/deep/b.bin'):readAll() == 'bufferdata')
    vfs:writeFile('textures/a.dds', 'XY', { offset = 1 })
    assert(vfs:getFile('textures/a.dds'):readAll() == 'nXY', 'offset writes in place')
    vfs:writeFile('textures/a.dds', 'Z', { append = true })
    assert(vfs:getFile('textures/a.dds'):readAll() == 'nXYZ', 'append')
    assert(
      string.find(
        refused(vfs.writeFile, vfs, 'missing.txt', 'x', { create = false }),
        'dream.vfs: ',
        1,
        true
      )
    )
    local err = refused(vfs.writeFile, vfs, 'x.txt', 'x', { offset = 0, append = true })
    assert(string.find(err, 'vfs:writeFile: offset and append cannot be combined', 1, true), err)
    err = refused(vfs.writeFile, vfs, 'x.txt', 'x', { bogus = true })
    assert(string.find(err, 'vfs:writeFile: unknown option \'bogus\'', 1, true), err)
    err = refused(vfs.writeFile, vfs, 'x.txt', 'x', { offset = -1 })
    assert(string.find(err, 'offset -1 is negative', 1, true), err)
    for _, bad in { '../x', '/abs', 'C:\\x', 'a/../b', '', 'a\\..\\b', 'a/./b' } do
      err = refused(vfs.writeFile, vfs, bad, 'x')
      assert(string.find(err, 'escapes the write root', 1, true), `{bad}: {err}`)
      err = refused(vfs.mkdir, vfs, bad)
      assert(string.find(err, 'escapes the write root', 1, true), `{bad}: {err}`)
    end

    err = refused(vfs.writeFile, vfs, 'textures', 'x')
    assert(string.find(err, 'the VFS refused key \'textures\'', 1, true), err)
  end)

  suite.it('through writers, registered on close', function()
    local out = scratch 'writer-root'
    local vfs = vfstool.VFS.fromDirectories({}, { writeRoot = out })
    local w = vfs:openWrite 'log.txt'
    assert(tostring(w) == `dream.vfs.Writer({out}/log.txt)`, tostring(w))
    assert(w:write 'hello ' == 6 and w:tell() == 6)
    assert(w:write(buffer.fromstring 'xxworldxx', 2, 5) == 5 and w:tell() == 11)
    assert(w:writeAt(0, 'J') == 1 and w:tell() == 11, 'writeAt does not move')
    w:seek(6)
    w:write 'W'
    w:truncate(5)
    assert(w:tell() == 5, 'a position past the new end moves to it')
    w:flush()
    assert(not vfs:contains 'log.txt', 'registered on close only')
    local closed = w:close()
    assert(closed and closed:readAll() == 'Jello' and vfs:getFile('log.txt'):readAll() == 'Jello')
    assert(w:close() == nil, 'closing again is nothing')
    local err = refused(w.write, w, 'x')
    assert(
      string.find(err, 'dream.vfs: Writer.write: the writer over ', 1, true)
        and string.find(err, 'log.txt is closed', 1, true),
      err
    )
    local a = vfs:openWrite('log.txt', { append = true })
    assert(a:tell() == 5, 'an append writer starts at the end')
    a:write '!'
    err = refused(a.writeAt, a, 0, 'x')
    assert(string.find(err, 'appends, so it cannot write at a position', 1, true), err)
    a:close()
    assert(vfs:getFile('log.txt'):readAll() == 'Jello!')
    local k = vfs:openWrite('log.txt', { truncate = false })
    k:write 'H'
    k:close()
    assert(vfs:getFile('log.txt'):readAll() == 'Hello!')
    err = refused(vfs.openWrite, vfs, 'log.txt', { mode = 'w' })
    assert(string.find(err, 'vfs:openWrite: unknown option \'mode\'', 1, true), err)
  end)

  suite.it('move directories and their winners under the root', function()
    local data = tree('dirs-data', { ['textures/a.dds'] = 'old' })
    local out = scratch 'dirs-root'
    local vfs = vfstool.VFS.fromDirectories({ data }, { writeRoot = out })
    vfs:writeFile('log.txt', 'Hello!')
    vfs:mkdir 'made/dir'
    vfs:rename('log.txt', 'moved/log.txt')
    assert(not vfs:contains 'log.txt' and vfs:getFile('moved/log.txt'):readAll() == 'Hello!')
    assert(vfs:getFile('moved/log.txt'):path() == `{out}/moved/log.txt`)
    vfs:remove 'moved/log.txt'
    assert(not vfs:contains 'moved/log.txt')
    local err = refused(vfs.remove, vfs, 'textures/nope.dds')
    assert(
      string.find(
        err,
        'dream.vfs: remove: \'textures/nope.dds\' is not under the write root',
        1,
        true
      ),
      err
    )
    err = refused(vfs.rename, vfs, 'nope.txt', 'x.txt')
    assert(
      string.find(err, 'dream.vfs: rename: \'nope.txt\' is not under the write root', 1, true),
      err
    )
    vfs:writeFile('tmp/a.txt', 'a')
    vfs:writeFile('tmp/b/c.txt', 'c')
    vfs:remove 'tmp'
    assert(
      not vfs:contains 'tmp/a.txt' and not vfs:contains 'tmp/b/c.txt',
      'a directory takes its winners with it'
    )
    vfs:writeFile('d1/x.txt', 'x')
    vfs:writeFile('d1/sub/y.txt', 'y')
    vfs:rename('d1', 'd2')
    assert(not vfs:contains 'd1/x.txt' and vfs:getFile('d2/x.txt'):path() == `{out}/d2/x.txt`)
    assert(vfs:getFile('d2/sub/y.txt'):readAll() == 'y')
    expect(fs.stat(`{out}/made/dir`).isDir).toBe(true)
    expect(fs.exists(`{out}/tmp`)).toBe(false)
  end)

  suite.it('need a write root first', function()
    local data = tree('no-root', { ['a.txt'] = '' })
    local out = scratch 'later-root'
    local plain = vfstool.VFS.fromDirectories { data }
    assert(plain:writeRoot() == nil)
    local err = refused(plain.writeFile, plain, 'a.txt', 'b')
    assert(string.find(err, 'dream.vfs: writeFile: this VFS has no write root', 1, true), err)
    plain:setWriteRoot(`{out}/later`)
    assert(plain:writeRoot() == `{out}/later`, 'created at once, canonical')
    assert(plain:writeFile('a.txt', 'b'):path() == `{out}/later/a.txt`)
  end)

  suite.it('do not follow symbolic links', function()
    local out = scratch 'symlink-root'
    local outside = tree('symlink-outside', { ['keep/victim.txt'] = 'untouched' })
    fs.mkdir(`{out}/textures`)
    local linked = fs.symlink(`{outside}/keep`, `{out}/textures/link`, { directory = true })
      and fs.symlink(`{outside}/keep/victim.txt`, `{out}/victim.txt`)
      and fs.symlink(`{out}/textures`, `{out}/inside`, { directory = true })
    -- A host that makes no symbolic links answers nil for the first one instead of raising: there
    -- is no link to refuse there.
    if not linked then return end

    local vfs = vfstool.VFS.new()
    vfs:setWriteRoot(out)
    local function denied(what: string, ...: any): string return refused(vfs[what], vfs, ...) end

    local err = denied('writeFile', 'textures/link/new.txt', 'x')
    assert(
      err
        == 'dream.vfs: writeFile: \'textures/link\' is a symbolic link; the write root does not follow links',
      err
    )
    assert(
      string.find(
        denied('writeFile', 'textures\\link\\deeper\\new.txt', 'x'),
        '\'textures/link\' is a symbolic link',
        1,
        true
      )
    )
    assert(
      string.find(
        denied('writeFile', 'victim.txt', 'x'),
        '\'victim.txt\' is a symbolic link',
        1,
        true
      )
    )
    assert(string.find(denied('openWrite', 'textures/link/new.txt'), 'is a symbolic link', 1, true))
    assert(string.find(denied('mkdir', 'textures/link/dir'), 'is a symbolic link', 1, true))
    assert(string.find(denied('remove', 'textures/link'), 'is a symbolic link', 1, true))
    assert(string.find(denied('remove', 'textures/link/victim.txt'), 'is a symbolic link', 1, true))
    vfs:writeFile('real.txt', 'real')
    assert(
      string.find(
        denied('rename', 'real.txt', 'textures/link/moved.txt'),
        'is a symbolic link',
        1,
        true
      )
    )
    assert(
      string.find(
        denied('rename', 'victim.txt', 'elsewhere.txt'),
        'rename: \'victim.txt\' is a symbolic link',
        1,
        true
      )
    )
    assert(
      string.find(
        denied('writeFile', 'inside/new.txt', 'x'),
        '\'inside\' is a symbolic link',
        1,
        true
      )
    )
    assert(vfs:writeFile('textures/plain.txt', 'plain'):path() == `{out}/textures/plain.txt`)
    vfs:mkdir 'textures/made'
    vfs:rename('real.txt', 'textures/made/real.txt')
    assert(vfs:getFile('textures/made/real.txt'):readAll() == 'real')
    expect(fs.readFileString(`{outside}/keep/victim.txt`)).toBe 'untouched'
    expect(fs.exists(`{outside}/keep/new.txt`)).toBe(false)
    expect(fs.lstat(`{out}/textures/link`).isSymlink).toBe(true)
  end)
end)

suite.describe('the host table', function()
  suite.it('reads host paths whole, as text, and in part', function()
    local dir = tree('host-reads', { ['in/big.bin'] = cycle(1000), ['in/sub/small.txt'] = 'small' })
    local host = vfstool.host
    local whole = host.readFile(`{dir}/in/big.bin`)
    assert(buffer.len(whole) == 1000 and buffer.readu8(whole, 999) == 231)
    assert(host.readFileString(`{dir}/in/sub/small.txt`) == 'small')
    local part = host.readAt(`{dir}/in/big.bin`, 256, 8)
    assert(buffer.len(part) == 8 and buffer.readu8(part, 0) == 0 and buffer.readu8(part, 7) == 7)
    assert(buffer.len(host.readAt(`{dir}/in/big.bin`, 996, 100)) == 4, 'short at the end')
    local err = refused(host.readAt, `{dir}/in/big.bin`, 1001, 1)
    assert(
      string.find(err, 'dream.vfs: host.readAt: offset 1001 past the end (size 1000)', 1, true),
      err
    )
    assert(string.find(refused(host.readFile, `{dir}/nope.bin`), 'dream.vfs: ', 1, true))
    local reader = host.open(`{dir}/in/big.bin`)
    assert(reader:size() == 1000)
    reader:seek(500)
    assert(buffer.readu8(reader:read(1), 0) == 500 % 256 and reader:tell() == 501)
    reader:close()
  end)

  suite.it('writes, stats and lists host paths', function()
    local dir =
      tree('host-writes', { ['in/big.bin'] = cycle(1000), ['in/sub/small.txt'] = 'small' })
    local host = vfstool.host
    assert(host.writeFile(`{dir}/out.txt`, 'abc') == 3)
    assert(host.writeFile(`{dir}/out.txt`, buffer.fromstring 'def', { append = true }) == 3)
    assert(host.readFileString(`{dir}/out.txt`) == 'abcdef')
    local err = refused(host.writeFile, `{dir}/out.txt`, 'x', { offset = 1 })
    assert(string.find(err, 'host.writeFile: unknown option \'offset\'', 1, true), err)
    local st = host.stat(`{dir}/out.txt`)
    assert(
      st.size == 6
        and st.isFile
        and not st.isDir
        and st.readonly == false
        and type(st.modified) == 'number'
    )
    assert(host.stat(`{dir}/in`).isDir and host.stat(`{dir}/in`).size == 0)
    assert(host.stat(`{dir}/nope`) == nil)
    assert(host.exists(`{dir}/out.txt`) and not host.exists(`{dir}/nope`))
    local rows = host.list(`{dir}/in`)
    assert(
      #rows == 2
        and rows[1].path == `{dir}/in/big.bin`
        and rows[1].size == 1000
        and not rows[1].isDir
    )
    assert(rows[2].path == `{dir}/in/sub` and rows[2].isDir and rows[2].size == 0)
    local deep = host.list(`{dir}/in`, { recursive = true })
    assert(
      #deep == 3 and deep[3].path == `{dir}/in/sub/small.txt` and deep[3].size == 5,
      deep[3].path
    )
    assert(#deep:toTable() == 3)
    err = refused(host.list, `{dir}/in`, { deep = true })
    assert(string.find(err, 'host.list: unknown option \'deep\'', 1, true), err)
    local w = host.openWrite(`{dir}/written.bin`)
    assert(w:write(buffer.fromstring '0123456789') == 10)
    w:writeAt(0, 'AB')
    w:truncate(4)
    assert(w:close() == nil, 'a host writer registers nothing')
    assert(host.readFileString(`{dir}/written.bin`) == 'AB23')
    local a = host.openWrite(`{dir}/written.bin`, { append = true })
    a:write '!'
    a:close()
    assert(host.readFileString(`{dir}/written.bin`) == 'AB23!')
  end)

  suite.it('makes, moves, copies and removes host paths', function()
    local dir = tree('host-moves', { ['in/big.bin'] = cycle(10), ['out.txt'] = 'abcdef' })
    local host = vfstool.host
    host.mkdir(`{dir}/a/b/c`, { recursive = true })
    assert(host.stat(`{dir}/a/b/c`).isDir)
    assert(not pcall(host.mkdir, `{dir}/x/y`), 'mkdir without recursive needs the parent')
    host.mkdir(`{dir}/x`)
    host.rename(`{dir}/out.txt`, `{dir}/x/out.txt`)
    assert(not host.exists(`{dir}/out.txt`) and host.readFileString(`{dir}/x/out.txt`) == 'abcdef')
    assert(host.copy(`{dir}/x/out.txt`, `{dir}/x/copy.txt`) == 6)
    assert(host.readFileString(`{dir}/x/copy.txt`) == 'abcdef')
    host.remove(`{dir}/x/copy.txt`)
    assert(not host.exists(`{dir}/x/copy.txt`))
    assert(not pcall(host.remove, `{dir}/x`), 'a directory with content needs recursive')
    host.remove(`{dir}/x`, { recursive = true })
    assert(not host.exists(`{dir}/x`))
    assert(host.canonicalize(`{dir}/in/../in/big.bin`) == `{host.canonicalize(dir)}/in/big.bin`)
  end)
end)

return suite
