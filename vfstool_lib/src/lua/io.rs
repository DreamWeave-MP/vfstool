// SPDX-License-Identifier: MIT OR Apache-2.0
//! Positional reads: the backing a `VfsFile` handle keeps after its first positional read (a
//! memory map of a loose file, the decompressed bytes of an archive entry), the `readAt`,
//! `readRange` and `readAllBuffer` methods, and `dream.vfs.Reader`, the cursor a script parses a
//! file through. Every read is one copy from the backing into a Luau buffer; nothing is
//! allocated per call.

use std::{
    cell::{Cell, RefCell},
    fs::File,
    io,
    path::Path,
    sync::Arc,
};

use l3i::{
    Error, Result,
    bind::{Call, StackResults},
    convert::{BufferView, Exact, new_buffer},
    extension::{ExtensionDescriptor, TagPolicy},
    userdata::{Owned, Userdata},
};

use super::{VfsFileHandle, io_error};
use crate::VfsFile;

/// Where a handle's bytes come from once it has been read positionally.
#[derive(Clone, Debug)]
pub(super) enum Backing {
    /// A loose file, mapped.
    Mapped(Arc<memmap2::Mmap>),
    /// A loose file that could not be mapped, read positionally through its handle.
    File { file: Arc<File>, len: u64 },
    /// An archive entry decompressed once, or an empty file.
    Bytes(Arc<[u8]>),
}

impl Backing {
    /// Maps the loose file at `path`, or opens it for positional reads when mapping fails.
    pub(super) fn open_loose(path: &Path) -> io::Result<Backing> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        if len == 0 {
            // An empty mapping is an error on every platform; there is nothing to map.
            return Ok(Backing::Bytes(Arc::from([])));
        }
        // SAFETY: the map is private and read-only, its bytes are only ever copied into Luau
        // buffers while no call into Lua is in progress, and the data directories are the host's
        // for the runtime's life, the same assumption `readAll` and `readInto` make of them; a
        // file truncated underneath the map would fault on the copy, never corrupt Rust memory.
        match unsafe { memmap2::Mmap::map(&file) } {
            Ok(map) => Ok(Backing::Mapped(Arc::new(map))),
            Err(_) => Ok(Backing::File {
                file: Arc::new(file),
                len,
            }),
        }
    }

    /// The backing of `file`: a map of a loose file, the whole content of an archive entry.
    pub(super) fn open(file: &VfsFile) -> io::Result<Backing> {
        if file.is_loose() {
            return Backing::open_loose(file.path());
        }
        file.read_to_vec()
            .map(|bytes| Backing::Bytes(Arc::from(bytes)))
    }

    /// The size in bytes.
    pub(super) fn len(&self) -> u64 {
        match self {
            Backing::Mapped(map) => map.len() as u64,
            Backing::File { len, .. } => *len,
            Backing::Bytes(bytes) => bytes.len() as u64,
        }
    }

    /// Copies up to `dst.len()` bytes from `offset` into `dst`, short only at the end; `offset`
    /// is at most the size. Returns the count copied.
    pub(super) fn read_at(&self, offset: u64, dst: &mut [u8]) -> io::Result<usize> {
        match self {
            Backing::Mapped(map) => Ok(copy_from(map, offset, dst)),
            Backing::Bytes(bytes) => Ok(copy_from(bytes, offset, dst)),
            Backing::File { file, len } => {
                let want = usize::try_from(len.saturating_sub(offset))
                    .unwrap_or(usize::MAX)
                    .min(dst.len());
                read_file_at(file, offset, &mut dst[..want])
            }
        }
    }
}

/// One memcpy of the bytes at `offset`, as many as fit.
fn copy_from(bytes: &[u8], offset: u64, dst: &mut [u8]) -> usize {
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let count = dst.len().min(bytes.len() - start);
    dst[..count].copy_from_slice(&bytes[start..start + count]);
    count
}

/// Fills `dst` from `offset` of `file` with positional reads, stopping at the end.
fn read_file_at(file: &File, offset: u64, dst: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < dst.len() {
        let position = offset + filled as u64;
        #[cfg(unix)]
        let read = {
            use std::os::unix::fs::FileExt;
            file.read_at(&mut dst[filled..], position)
        };
        #[cfg(windows)]
        let read = {
            use std::os::windows::fs::FileExt;
            file.seek_read(&mut dst[filled..], position)
        };
        match read {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

// ---------------------------------------------------------------------------------------------
// Argument checks, each naming the argument it refuses
// ---------------------------------------------------------------------------------------------

/// A count or offset argument that must not be negative.
pub(super) fn non_negative(what: &str, name: &str, value: Exact<i64>) -> Result<u64> {
    u64::try_from(value.0)
        .map_err(|_| Error::runtime(format!("dream.vfs: {what}: {name} {} is negative", value.0)))
}

/// An offset into a file of `size` bytes: at most `size`, so that a read at the end is empty.
pub(super) fn within_file(what: &str, name: &str, offset: u64, size: u64) -> Result<()> {
    if offset > size {
        return Err(Error::runtime(format!(
            "dream.vfs: {what}: {name} {offset} past the end (size {size})"
        )));
    }
    Ok(())
}

/// The window `(offset, length)` of `len` bytes of a buffer or string, with the defaults (0,
/// the space left) and each argument checked against it. `offset_name` is the offset argument's
/// name (`bufferOffset`, `offset`) and `noun` what the bytes are (`buffer`, `data`) in errors.
pub(super) fn window(
    what: &str,
    offset_name: &str,
    noun: &str,
    len: usize,
    offset: Option<Exact<i64>>,
    length: Option<Exact<i64>>,
) -> Result<(usize, usize)> {
    let offset = match offset {
        Some(offset) => {
            usize::try_from(non_negative(what, offset_name, offset)?).unwrap_or(usize::MAX)
        }
        None => 0,
    };
    if offset > len {
        return Err(Error::runtime(format!(
            "dream.vfs: {what}: {offset_name} {offset} past the end of the {noun} (size {len})"
        )));
    }
    let space = len - offset;
    let length = match length {
        Some(length) => {
            usize::try_from(non_negative(what, "length", length)?).unwrap_or(usize::MAX)
        }
        None => space,
    };
    if length > space {
        return Err(Error::runtime(format!(
            "dream.vfs: {what}: length {length} does not fit the {noun} (space {space} after {offset_name} {offset})"
        )));
    }
    Ok((offset, length))
}

/// [`window`] over a buffer argument named `buffer`, with `bufferOffset` and `length`.
pub(super) fn buffer_window(
    what: &str,
    buffer_len: usize,
    buffer_offset: Option<Exact<i64>>,
    length: Option<Exact<i64>>,
) -> Result<(usize, usize)> {
    window(
        what,
        "bufferOffset",
        "buffer",
        buffer_len,
        buffer_offset,
        length,
    )
}

/// The count a read of `length` bytes at `offset` of a file of `size` bytes copies.
fn clamp(offset: u64, length: u64, size: u64) -> usize {
    usize::try_from(length.min(size - offset)).unwrap_or(usize::MAX)
}

// ---------------------------------------------------------------------------------------------
// VfsFile methods
// ---------------------------------------------------------------------------------------------

/// `file:readAt(buffer, fileOffset, length?, bufferOffset?)`.
pub(super) fn read_at(
    file: &VfsFileHandle,
    mut buffer: BufferView<'_>,
    file_offset: Exact<i64>,
    length: Option<Exact<i64>>,
    buffer_offset: Option<Exact<i64>>,
) -> Result<f64> {
    let (buffer_offset, length) = buffer_window("readAt", buffer.len(), buffer_offset, length)?;
    let file_offset = non_negative("readAt", "fileOffset", file_offset)?;
    let size = file.size()?;
    within_file("readAt", "fileOffset", file_offset, size)?;
    let want = clamp(file_offset, length as u64, size);
    // SAFETY: the window was checked against the buffer, no other view of the buffer is held by
    // this call, and the copy runs from this crate's own map, bytes or file handle, which never
    // call into Lua, so nothing can write the buffer while the slice lives.
    let dst = unsafe { &mut buffer.bytes_mut_unchecked()[buffer_offset..buffer_offset + want] };
    file.read_at(file_offset, dst).map(|count| count as f64)
}

/// `file:readRange(fileOffset, length)`: a new buffer of the bytes there.
pub(super) fn read_range(
    file: &VfsFileHandle,
    call: &Call,
    file_offset: Exact<i64>,
    length: Exact<i64>,
) -> Result<StackResults> {
    let file_offset = non_negative("readRange", "fileOffset", file_offset)?;
    let length = non_negative("readRange", "length", length)?;
    let size = file.size()?;
    within_file("readRange", "fileOffset", file_offset, size)?;
    let want = clamp(file_offset, length, size);
    let mut buffer = new_buffer(call, want)?;
    // SAFETY: the buffer was created by this call and no other view of it exists; the copy
    // never calls into Lua.
    file.read_at(file_offset, unsafe { buffer.bytes_mut_unchecked() })?;
    Ok(StackResults)
}

/// `file:readAllBuffer()`: the whole file as a new buffer.
pub(super) fn read_all_buffer(file: &VfsFileHandle, call: &Call) -> Result<StackResults> {
    let size = file.size()?;
    let mut buffer = new_buffer(call, usize::try_from(size).unwrap_or(usize::MAX))?;
    // SAFETY: as `read_range`.
    file.read_at(0, unsafe { buffer.bytes_mut_unchecked() })?;
    Ok(StackResults)
}

/// `file:open()`: a reader over the handle's backing.
pub(super) fn open_reader(file: &VfsFileHandle) -> Result<Owned<VfsReader>> {
    let backing = file.backing()?;
    Ok(Owned(VfsReader::new(backing, file.file.path_bytes())))
}

// ---------------------------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------------------------

/// A sequential reader over a file's backing (`dream.vfs.Reader`): a position, and the map or
/// bytes the file handle it came from keeps, so reading allocates nothing per call.
#[derive(Debug)]
pub struct VfsReader {
    backing: RefCell<Option<Backing>>,
    size: u64,
    position: Cell<u64>,
    name: Box<[u8]>,
}

// SAFETY: a map, a file handle or bytes behind `Arc`s and two cells; no Lua references, and
// dropping closes the handle without any Lua API.
unsafe impl Userdata for VfsReader {
    const NAME: &'static str = "dream.vfs.Reader";
}

impl VfsReader {
    /// A reader at position 0 over `backing`; `name` names the file in errors.
    pub(super) fn new(backing: Backing, name: &[u8]) -> Self {
        VfsReader {
            size: backing.len(),
            backing: RefCell::new(Some(backing)),
            position: Cell::new(0),
            name: name.into(),
        }
    }

    /// The reader's name in messages.
    fn name(&self) -> String {
        String::from_utf8_lossy(&self.name).into_owned()
    }

    /// Runs `body` on the backing, or fails when the reader is closed.
    fn with_backing<R>(&self, what: &str, body: impl FnOnce(&Backing) -> Result<R>) -> Result<R> {
        let backing = self.backing.borrow();
        let Some(backing) = backing.as_ref() else {
            return Err(Error::runtime(format!(
                "dream.vfs: Reader.{what}: the reader over {} is closed",
                self.name()
            )));
        };
        body(backing)
    }

    /// Copies up to `dst.len()` bytes at the position and advances past them.
    fn read_next(&self, what: &str, dst: &mut [u8]) -> Result<usize> {
        let position = self.position.get();
        let count = self.with_backing(what, |backing| {
            backing.read_at(position, dst).map_err(io_error)
        })?;
        self.position.set(position + count as u64);
        Ok(count)
    }

    /// The bytes left before the end.
    fn remaining(&self) -> u64 {
        self.size.saturating_sub(self.position.get())
    }
}

/// `reader:read(length)`.
fn reader_read(reader: &VfsReader, call: &Call, length: Exact<i64>) -> Result<StackResults> {
    let length = non_negative("Reader.read", "length", length)?;
    let want = usize::try_from(length.min(reader.remaining())).unwrap_or(usize::MAX);
    let mut buffer = new_buffer(call, want)?;
    // SAFETY: the buffer was created by this call and no other view of it exists; the copy
    // never calls into Lua.
    reader.read_next("read", unsafe { buffer.bytes_mut_unchecked() })?;
    Ok(StackResults)
}

/// `reader:readInto(buffer, bufferOffset?, length?)`.
fn reader_read_into(
    reader: &VfsReader,
    mut buffer: BufferView<'_>,
    buffer_offset: Option<Exact<i64>>,
    length: Option<Exact<i64>>,
) -> Result<f64> {
    let (buffer_offset, length) =
        buffer_window("Reader.readInto", buffer.len(), buffer_offset, length)?;
    let want = usize::try_from((length as u64).min(reader.remaining())).unwrap_or(usize::MAX);
    // SAFETY: as `read_at`: the window is inside the buffer, this call holds the only view, and
    // the copy never calls into Lua.
    let dst = unsafe { &mut buffer.bytes_mut_unchecked()[buffer_offset..buffer_offset + want] };
    reader.read_next("readInto", dst).map(|count| count as f64)
}

/// `reader:seek(position)`.
fn reader_seek(reader: &VfsReader, position: Exact<i64>) -> Result<()> {
    let position = non_negative("Reader.seek", "position", position)?;
    within_file("Reader.seek", "position", position, reader.size)?;
    reader.with_backing("seek", |_| Ok(()))?;
    reader.position.set(position);
    Ok(())
}

/// `reader:skip(count)`.
fn reader_skip(reader: &VfsReader, count: Exact<i64>) -> Result<()> {
    let count = non_negative("Reader.skip", "count", count)?;
    reader.with_backing("skip", |_| Ok(()))?;
    let remaining = reader.remaining();
    if count > remaining {
        return Err(Error::runtime(format!(
            "dream.vfs: Reader.skip: count {count} past the end (position {}, size {})",
            reader.position.get(),
            reader.size
        )));
    }
    reader.position.set(reader.position.get() + count);
    Ok(())
}

pub(super) fn describe_reader(d: &mut ExtensionDescriptor) {
    let mut reader = d.userdata::<VfsReader>(VfsReader::NAME);
    reader.tag(TagPolicy::Preferred).doc(
        "A sequential reader over a file: a position over the map or bytes its VfsFile keeps.",
    );
    reader
        .method("read", reader_read)
        .signature("(self, length: number): buffer")
        .doc("The next length bytes as a new buffer, fewer at the end; advances past them.");
    reader
        .method("readInto", reader_read_into)
        .signature("(self, buffer: buffer, bufferOffset: number?, length: number?): number")
        .doc("Copies the next bytes into the buffer at bufferOffset (default 0), at most length (default the space left); returns the count and advances past them.");
    reader
        .method("seek", reader_seek)
        .signature("(self, position: number)")
        .doc("Moves to position, 0 to size inclusive.");
    reader
        .method("skip", reader_skip)
        .signature("(self, count: number)")
        .doc("Moves count bytes forward, at most to the end.");
    reader
        .method("tell", |r: &VfsReader| r.position.get() as f64)
        .signature("(self): number");
    reader
        .method("size", |r: &VfsReader| r.size as f64)
        .signature("(self): number");
    reader
        .method("close", |r: &VfsReader| {
            r.backing.borrow_mut().take();
        })
        .signature("(self)")
        .doc("Releases the backing; every later read on this reader is an error.");
    reader.metamethod("__tostring", |r: &VfsReader| {
        format!("dream.vfs.Reader({})", r.name())
    });
}
