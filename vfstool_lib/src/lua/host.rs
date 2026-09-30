// SPDX-License-Identifier: MIT OR Apache-2.0
//! The module's `host` table (`lua-host`): files and directories on any host path, with no VFS
//! in between. Paths are bytes on Unix, converted where the OS insists on Unicode.

use std::{
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use l3i::{
    Error, Result,
    bind::{ArgView, Call, StackResults},
    convert::{BytesView, Exact, new_buffer},
    extension::{ExtensionDescriptor, TagPolicy},
    options::Options,
    sequence::{Sequence, SequenceItem, SequenceSource},
    stack::{Scope, ValueView},
    userdata::Owned,
    value::Table,
};

use super::{
    class_function, frozen_class_table, host_path,
    io::{Backing, VfsReader, non_negative, within_file},
    io_error, path_bytes,
    views::{Slot, push_record},
    write::{OpenWrite, VfsWriter, WriteFile},
};

/// The `host` table's type.
pub(super) const TABLE_TYPE: &str = "{ \
    readFile: (path: string) -> buffer, \
    readFileString: (path: string) -> string, \
    readAt: (path: string, offset: number, length: number) -> buffer, \
    writeFile: (path: string, data: buffer | string, options: { append: boolean? }?) -> number, \
    open: (path: string) -> dream_vfs_Reader, \
    openWrite: (path: string, options: { append: boolean?, truncate: boolean? }?) -> dream_vfs_Writer, \
    stat: (path: string) -> { size: number, isFile: boolean, isDir: boolean, modified: number?, readonly: boolean }?, \
    exists: (path: string) -> boolean, \
    list: (path: string, options: { recursive: boolean? }?) -> dream_vfs_HostEntries, \
    mkdir: (path: string, options: { recursive: boolean? }?) -> (), \
    remove: (path: string, options: { recursive: boolean? }?) -> (), \
    rename: (from: string, to: string) -> (), \
    copy: (from: string, to: string) -> number, \
    canonicalize: (path: string) -> string }";

/// One row of `host.list`.
#[derive(Debug)]
pub struct HostEntry {
    /// The entry's host path, the listed directory joined with its name.
    pub path: PathBuf,
    /// Whether it is a directory.
    pub is_dir: bool,
    /// Its size in bytes (0 for a directory).
    pub size: u64,
}

/// `dream.vfs.HostEntries`: the rows of `host.list`, sorted by path.
#[derive(Clone, Debug)]
pub struct HostEntries(pub Rc<[HostEntry]>);

impl SequenceItem for Slot<HostEntry> {
    fn push_item<S: Scope>(self, scope: &S) -> Result<()> {
        let row = self.row();
        push_record(scope, 3, |frame, table| {
            frame.push(path_bytes(&row.path))?;
            table.raw_set(frame, "path")?;
            frame.push(&row.is_dir)?;
            table.raw_set(frame, "isDir")?;
            frame.push(&(row.size as f64))?;
            table.raw_set(frame, "size")
        })
        .map(drop)
    }
}

impl SequenceSource for HostEntries {
    const NAME: &'static str = "dream.vfs.HostEntries";
    type Item = Slot<HostEntry>;
    fn len(&self) -> usize {
        self.0.len()
    }
    fn get(&self, index: usize) -> Option<Slot<HostEntry>> {
        Slot::of(&self.0, index)
    }
}

/// `{ recursive? }`.
fn recursive(scope: &impl Scope, options: Option<ValueView<'_>>, context: &str) -> Result<bool> {
    let Some(options) = options.filter(|view| !view.is_nil()) else {
        return Ok(false);
    };
    Options::read(scope, options, context, |o| o.or("recursive", false))
}

/// `{ append? }` of `host.writeFile`.
fn append_option(
    scope: &impl Scope,
    options: Option<ValueView<'_>>,
    context: &str,
) -> Result<bool> {
    let Some(options) = options.filter(|view| !view.is_nil()) else {
        return Ok(false);
    };
    Options::read(scope, options, context, |o| o.or("append", false))
}

/// Reads the whole file at `path` into a new buffer: one copy, from the file into the buffer.
fn read_file(call: &Call, path: &[u8]) -> Result<StackResults> {
    let path = host_path(path);
    let mut file = fs::File::open(&path).map_err(io_error)?;
    let len = file.metadata().map_err(io_error)?.len();
    let mut buffer = new_buffer(call, usize::try_from(len).unwrap_or(usize::MAX))?;
    // SAFETY: the buffer was created by this call and no other view of it exists; the read
    // never calls into Lua.
    let dst = unsafe { buffer.bytes_mut_unchecked() };
    std::io::Read::read_exact(&mut file, dst).map_err(|error| {
        Error::runtime(format!(
            "dream.vfs: host.readFile: {} changed while it was read ({error})",
            path.display()
        ))
    })?;
    Ok(StackResults)
}

/// `host.readAt(path, offset, length)`.
fn read_at(
    call: &Call,
    path: &[u8],
    offset: Exact<i64>,
    length: Exact<i64>,
) -> Result<StackResults> {
    let offset = non_negative("host.readAt", "offset", offset)?;
    let length = non_negative("host.readAt", "length", length)?;
    let backing = Backing::open_loose(&host_path(path)).map_err(io_error)?;
    within_file("host.readAt", "offset", offset, backing.len())?;
    let want = usize::try_from(length.min(backing.len() - offset)).unwrap_or(usize::MAX);
    let mut buffer = new_buffer(call, want)?;
    // SAFETY: as `read_file`.
    backing
        .read_at(offset, unsafe { buffer.bytes_mut_unchecked() })
        .map_err(io_error)?;
    Ok(StackResults)
}

/// `host.stat(path)`: a table, or nil when nothing is there.
fn stat(call: &Call, path: &[u8]) -> Result<StackResults> {
    let meta = match fs::metadata(host_path(path)) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            call.push(&())?;
            return Ok(StackResults);
        }
        Err(error) => return Err(io_error(error)),
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_secs_f64());
    let size = if meta.is_dir() { 0 } else { meta.len() };
    push_record(call, 5, |frame, table| {
        frame.push(&(size as f64))?;
        table.raw_set(frame, "size")?;
        frame.push(&meta.is_file())?;
        table.raw_set(frame, "isFile")?;
        frame.push(&meta.is_dir())?;
        table.raw_set(frame, "isDir")?;
        frame.push(&modified)?;
        table.raw_set(frame, "modified")?;
        frame.push(&meta.permissions().readonly())?;
        table.raw_set(frame, "readonly")
    })?;
    Ok(StackResults)
}

/// `host.list(path, { recursive? })`.
fn list(call: &Call, path: &[u8], options: Option<ValueView>) -> Result<StackResults> {
    let recursive = recursive(call, options, "host.list")?;
    let root = host_path(path);
    let mut rows = Vec::new();
    if recursive {
        for entry in walkdir::WalkDir::new(&root).min_depth(1).follow_links(true) {
            let entry = entry.map_err(|error| Error::runtime(format!("dream.vfs: {error}")))?;
            let meta = entry
                .metadata()
                .map_err(|error| Error::runtime(format!("dream.vfs: {error}")))?;
            rows.push(HostEntry {
                path: entry.into_path(),
                is_dir: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
            });
        }
    } else {
        for entry in fs::read_dir(&root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let meta = fs::metadata(entry.path()).map_err(io_error)?;
            rows.push(HostEntry {
                path: entry.path(),
                is_dir: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
            });
        }
    }
    rows.sort_by(|a, b| path_bytes(&a.path).cmp(path_bytes(&b.path)));
    Sequence::push(call, HostEntries(rows.into())).map(drop)?;
    Ok(StackResults)
}

/// `host.remove(path, { recursive? })`.
fn remove(path: &Path, recursive: bool) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() {
        return fs::remove_file(path);
    }
    if recursive {
        fs::remove_dir_all(path)
    } else {
        fs::remove_dir(path)
    }
}

pub(super) fn describe(d: &mut ExtensionDescriptor) {
    d.sequence::<HostEntries>(HostEntries::NAME)
        .item_type(super::types::HOST_ENTRY)
        .tag(TagPolicy::Never)
        .doc("The entries of a host directory, sorted by path, each { path, isDir, size }.");
}

/// The names of the `host` table's functions, in the order of [`TABLE_TYPE`].
const FUNCTIONS: [&str; 14] = [
    "readFile",
    "readFileString",
    "readAt",
    "writeFile",
    "open",
    "openWrite",
    "stat",
    "exists",
    "list",
    "mkdir",
    "remove",
    "rename",
    "copy",
    "canonicalize",
];

/// The frozen `host` table: the functions when the runtime grants `filesystem.host`, else a
/// table of the same shape whose every function raises.
pub(super) fn table(runtime: &l3i::Runtime, granted: bool) -> Result<Table> {
    if granted {
        return granted_table(runtime);
    }
    frozen_class_table(runtime, |table| {
        for name in FUNCTIONS {
            let message = format!(
                "dream.vfs: host.{name} requires the '{}' capability, which this runtime does not grant",
                super::HOST_CAPABILITY
            );
            // The stub takes whatever the real function would have, and raises.
            class_function(runtime, table, "host", name, move |_: ArgView| {
                Err::<(), Error>(Error::permission(message.clone()))
            })?;
        }
        Ok(())
    })
}

/// The `host` table of a runtime that grants `filesystem.host`.
fn granted_table(runtime: &l3i::Runtime) -> Result<Table> {
    frozen_class_table(runtime, |table| {
        class_function(runtime, table, "host", "readFile", read_file)?;
        class_function(runtime, table, "host", "readFileString", |path: &[u8]| {
            fs::read(host_path(path)).map_err(io_error)
        })?;
        class_function(runtime, table, "host", "readAt", read_at)?;
        class_function(
            runtime,
            table,
            "host",
            "writeFile",
            |call: &Call, path: &[u8], data: BytesView, options: Option<ValueView>| {
                let append = append_option(call, options, "host.writeFile")?;
                let options = WriteFile {
                    append,
                    ..WriteFile::default()
                };
                // SAFETY: the bytes go straight to the file, with no call into Lua while the
                // slice lives, through this call's only view of them.
                options
                    .write(&host_path(path), unsafe { data.bytes_unchecked() })
                    .map(|count| count as f64)
                    .map_err(io_error)
            },
        )?;
        class_function(runtime, table, "host", "open", |path: &[u8]| {
            let path = host_path(path);
            let backing = Backing::open_loose(&path).map_err(io_error)?;
            Ok::<_, Error>(Owned(VfsReader::new(backing, path_bytes(&path))))
        })?;
        class_function(
            runtime,
            table,
            "host",
            "openWrite",
            |call: &Call, path: &[u8], options: Option<ValueView>| {
                let options = OpenWrite::read(call, options, "host.openWrite")?;
                VfsWriter::open(
                    &host_path(path),
                    options,
                    #[cfg(feature = "lua-write")]
                    None,
                )
                .map(Owned)
                .map_err(io_error)
            },
        )?;
        class_function(runtime, table, "host", "stat", stat)?;
        class_function(runtime, table, "host", "exists", |path: &[u8]| {
            fs::exists(host_path(path)).map_err(io_error)
        })?;
        class_function(runtime, table, "host", "list", list)?;
        class_function(
            runtime,
            table,
            "host",
            "mkdir",
            |call: &Call, path: &[u8], options: Option<ValueView>| {
                let path = host_path(path);
                if recursive(call, options, "host.mkdir")? {
                    fs::create_dir_all(path).map_err(io_error)
                } else {
                    fs::create_dir(path).map_err(io_error)
                }
            },
        )?;
        class_function(
            runtime,
            table,
            "host",
            "remove",
            |call: &Call, path: &[u8], options: Option<ValueView>| {
                let recursive = recursive(call, options, "host.remove")?;
                remove(&host_path(path), recursive).map_err(io_error)
            },
        )?;
        class_function(
            runtime,
            table,
            "host",
            "rename",
            |from: &[u8], to: &[u8]| fs::rename(host_path(from), host_path(to)).map_err(io_error),
        )?;
        class_function(runtime, table, "host", "copy", |from: &[u8], to: &[u8]| {
            fs::copy(host_path(from), host_path(to))
                .map(|count| count as f64)
                .map_err(io_error)
        })?;
        class_function(runtime, table, "host", "canonicalize", |path: &[u8]| {
            super::canonical(&host_path(path))
                .map(|path| path_bytes(&path).to_vec())
                .map_err(io_error)
        })
    })
}
