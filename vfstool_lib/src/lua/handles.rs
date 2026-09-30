// SPDX-License-Identifier: MIT OR Apache-2.0
//! The userdata handles: the VFS itself, files, providers, indexes, locks, and snapshots.

use std::{cell::RefCell, io::Read as _, rc::Rc};

use l3i::{
    Error, Result,
    bind::Call,
    convert::{BufferView, Exact},
    extension::{ExtensionDescriptor, TagPolicy},
    options::Options,
    stack::{Scope, ValueView},
    userdata::{Owned, Userdata},
    value::Table,
};

use super::views::KeyBlob;
use super::{
    class_function, frozen_class_table, host_path, io, io_error, path_bytes, reports,
    source_kind_from_name,
};
use crate::{
    ConflictIndex, LayerIndex, MetadataSnapshot, Snapshot, SourceMeta, VFS, VfsFile, VfsLock,
    VfsProvider,
};

/// A VFS as scripts see it (`dream.vfs.VFS`): the VFS behind a `RefCell`, because scripts
/// mutate it through methods and Luau hands out shared references, plus the sorted key list
/// `keys()` last built, kept until the next mutation, and (with `lua-write`) the write root.
///
/// The handle is shared: a clone is the same VFS, so the host keeps one for a
/// [`VfsRequireNavigator`](crate::lua::VfsRequireNavigator) or a callback and pushes another to
/// scripts with `l3i::userdata::push_owned(scope, handle.clone())`, and every change a script
/// makes is visible through both.
#[derive(Clone)]
pub struct Vfs(Rc<VfsState>);

/// The state every clone of a [`Vfs`] shares.
struct VfsState {
    inner: RefCell<VFS>,
    keys: RefCell<Option<KeyBlob>>,
    #[cfg(feature = "lua-write")]
    write_root: RefCell<Option<std::path::PathBuf>>,
    #[cfg(feature = "lua-write")]
    write_root_grant: std::cell::Cell<WriteRootGrant>,
}

/// Who may give a [`Vfs`] its write root (`lua-write`). The module's constructors record
/// whether the runtime's policy grants the `filesystem.write` capability
/// ([`WRITE_CAPABILITY`](crate::lua::WRITE_CAPABILITY)); a VFS the host makes in Rust is the
/// host's to configure.
#[cfg(feature = "lua-write")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteRootGrant {
    /// Scripts may set it: the runtime grants `filesystem.write`.
    Scripts,
    /// The runtime does not grant `filesystem.write`: the `writeRoot` option and
    /// `setWriteRoot` raise, and the VFS never has a write root.
    Refused,
    /// The host sets it from Rust ([`Vfs::set_write_root`]); `setWriteRoot` raises. What
    /// [`Vfs::new`] starts with.
    Host,
}

// SAFETY: plain Rust data (maps, paths, archive handles) behind an `Rc` that only the VM's
// thread touches, no Lua references, no Lua API in `Drop`.
unsafe impl Userdata for Vfs {
    const NAME: &'static str = "dream.vfs.VFS";
}

impl Vfs {
    /// Wraps a VFS the host built.
    #[must_use]
    pub fn new(vfs: VFS) -> Self {
        Vfs(Rc::new(VfsState {
            inner: RefCell::new(vfs),
            keys: RefCell::new(None),
            #[cfg(feature = "lua-write")]
            write_root: RefCell::new(None),
            #[cfg(feature = "lua-write")]
            write_root_grant: std::cell::Cell::new(WriteRootGrant::Host),
        }))
    }

    /// Whether `other` is a clone of this handle, sharing its VFS.
    #[must_use]
    pub fn shares(&self, other: &Vfs) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// Pushes a VFS handle onto `scope` (the `dream.vfs` extension must be installed).
    ///
    /// # Errors
    ///
    /// Returns an error if the type is not registered in the scope's runtime.
    pub fn push(scope: &impl Scope, vfs: VFS) -> Result<ValueView<'_>> {
        l3i::userdata::push_owned(scope, Vfs::new(vfs))
    }

    /// Runs `body` with the VFS borrowed.
    ///
    /// # Errors
    ///
    /// Returns an error if a mutation of this VFS is in progress.
    pub fn with<R>(&self, body: impl FnOnce(&VFS) -> R) -> Result<R> {
        let vfs = self
            .0
            .inner
            .try_borrow()
            .map_err(|_| Error::runtime("dream.vfs: the VFS is being mutated"))?;
        Ok(body(&vfs))
    }

    /// The sorted keys, built on the first call after a mutation and shared afterwards.
    pub(super) fn sorted_keys(&self) -> Result<KeyBlob> {
        if let Some(keys) = self.0.keys.borrow().as_ref() {
            return Ok(keys.clone());
        }
        let keys = self.with(|vfs| KeyBlob::sorted(vfs.iter().map(|(key, _)| key.clone())))?;
        *self.0.keys.borrow_mut() = Some(keys.clone());
        Ok(keys)
    }

    /// Runs `body` with the VFS borrowed mutably.
    ///
    /// # Errors
    ///
    /// Returns an error if the VFS is otherwise in use.
    pub fn with_mut<R>(&self, body: impl FnOnce(&mut VFS) -> R) -> Result<R> {
        let mut vfs = self
            .0
            .inner
            .try_borrow_mut()
            .map_err(|_| Error::runtime("dream.vfs: the VFS is in use"))?;
        // Whatever the mutation does to the key set, the cached list is stale.
        *self.0.keys.borrow_mut() = None;
        Ok(body(&mut vfs))
    }

    /// Takes the VFS out, leaving an empty one behind (for hosts reclaiming a script's VFS).
    ///
    /// # Errors
    ///
    /// Returns an error if the VFS is in use.
    pub fn take(&self) -> Result<VFS> {
        self.with_mut(std::mem::take)
    }

    /// The directory `writeFile`, `openWrite`, `mkdir`, `remove` and `rename` work under, in
    /// canonical form, or `None` when the VFS refuses writes.
    #[cfg(feature = "lua-write")]
    #[must_use]
    pub fn write_root(&self) -> Option<std::path::PathBuf> {
        self.0.write_root.borrow().clone()
    }

    /// Makes `root` the write root, creating the directory when it is absent and keeping its
    /// canonical path, so that a key resolved under it can be checked against it; `None`
    /// clears the root.
    ///
    /// # Errors
    ///
    /// Returns the error of creating or canonicalizing the directory; the root is then
    /// unchanged.
    #[cfg(feature = "lua-write")]
    pub fn set_write_root(&self, root: Option<std::path::PathBuf>) -> std::io::Result<()> {
        let root = match root {
            Some(root) => {
                std::fs::create_dir_all(&root)?;
                Some(super::canonical(&root)?)
            }
            None => None,
        };
        *self.0.write_root.borrow_mut() = root;
        Ok(())
    }

    /// Who may give this VFS its write root from a script.
    #[cfg(feature = "lua-write")]
    #[must_use]
    pub fn write_root_grant(&self) -> WriteRootGrant {
        self.0.write_root_grant.get()
    }

    /// Decides who may give this VFS its write root from a script: how a host lets scripts of a
    /// runtime that grants `filesystem.write` move the root of a VFS it pushed from Rust
    /// (`WriteRootGrant::Scripts`), or withdraws that.
    #[cfg(feature = "lua-write")]
    pub fn set_write_root_grant(&self, grant: WriteRootGrant) {
        self.0.write_root_grant.set(grant);
    }

    /// The error `setWriteRoot` and the `writeRoot` option raise when `what` may not set the
    /// root, or `Ok` when scripts may.
    #[cfg(feature = "lua-write")]
    pub(super) fn check_write_root_grant(&self, what: &str) -> Result<()> {
        match self.write_root_grant() {
            WriteRootGrant::Scripts => Ok(()),
            WriteRootGrant::Refused => Err(Error::permission(format!(
                "dream.vfs: {what} requires the '{}' capability, which this runtime does not grant",
                super::WRITE_CAPABILITY
            ))),
            WriteRootGrant::Host => Err(Error::permission(format!(
                "dream.vfs: {what}: the host made this VFS in Rust and keeps its write root; only the host sets it"
            ))),
        }
    }
}

/// A backing file handle (`dream.vfs.VfsFile`): the file, and after its first positional read
/// its backing, a memory map of a loose file or the bytes of an archive entry, kept for every
/// later `readAt`, `readRange`, `readAllBuffer` and reader.
#[derive(Debug, Clone)]
pub struct VfsFileHandle {
    /// The file.
    pub file: VfsFile,
    backing: RefCell<Option<io::Backing>>,
}

// SAFETY: a path or an `Arc` archive reference, and a map, file handle or bytes behind `Arc`s;
// no Lua references, no Lua API in `Drop`.
unsafe impl Userdata for VfsFileHandle {
    const NAME: &'static str = "dream.vfs.VfsFile";
}

impl VfsFileHandle {
    /// A handle to `file`, with nothing read yet.
    #[must_use]
    pub fn new(file: VfsFile) -> Self {
        VfsFileHandle {
            file,
            backing: RefCell::new(None),
        }
    }

    /// The backing, opened on the first call and kept.
    pub(super) fn backing(&self) -> Result<io::Backing> {
        if let Some(backing) = self.backing.borrow().as_ref() {
            return Ok(backing.clone());
        }
        let backing = io::Backing::open(&self.file).map_err(io_error)?;
        *self.backing.borrow_mut() = Some(backing.clone());
        Ok(backing)
    }

    /// The size in bytes: the backing's when it is open, the index's when the archive has one,
    /// otherwise the content's, which opens the backing.
    pub(super) fn size(&self) -> Result<u64> {
        if let Some(backing) = self.backing.borrow().as_ref() {
            return Ok(backing.len());
        }
        match self.file.known_size().map_err(io_error)? {
            Some(size) => Ok(size),
            None => self.backing().map(|backing| backing.len()),
        }
    }

    /// Copies up to `dst.len()` bytes from `offset` (at most the size) into `dst`, short only at
    /// the end: from the open backing, straight from the archive for a stored entry, otherwise
    /// through the backing opened now.
    pub(super) fn read_at(&self, offset: u64, dst: &mut [u8]) -> Result<usize> {
        if let Some(backing) = self.backing.borrow().as_ref() {
            return backing.read_at(offset, dst).map_err(io_error);
        }
        if self.file.is_archive()
            && let Some(count) = self.file.read_stored_at(offset, dst).map_err(io_error)?
        {
            return Ok(count);
        }
        self.backing()?.read_at(offset, dst).map_err(io_error)
    }
}

/// One provider for a key (`dream.vfs.VfsProvider`).
#[derive(Debug, Clone)]
pub struct VfsProviderHandle(pub VfsProvider);

// SAFETY: as `VfsFileHandle`.
unsafe impl Userdata for VfsProviderHandle {
    const NAME: &'static str = "dream.vfs.VfsProvider";
}

/// The canonical provider-occurrence index (`dream.vfs.LayerIndex`).
#[derive(Debug, Clone)]
pub struct LayerIndexHandle(pub LayerIndex);

// SAFETY: maps and paths only.
unsafe impl Userdata for LayerIndexHandle {
    const NAME: &'static str = "dream.vfs.LayerIndex";
}

/// The derived source-level conflict projection (`dream.vfs.ConflictIndex`).
pub struct ConflictIndexHandle(pub ConflictIndex);

// SAFETY: maps and paths only.
unsafe impl Userdata for ConflictIndexHandle {
    const NAME: &'static str = "dream.vfs.ConflictIndex";
}

/// A deterministic lock manifest (`dream.vfs.VfsLock`).
#[derive(Debug, Clone)]
pub struct VfsLockHandle(pub VfsLock);

// SAFETY: plain data.
unsafe impl Userdata for VfsLockHandle {
    const NAME: &'static str = "dream.vfs.VfsLock";
}

/// A content-hash baseline (`dream.vfs.Snapshot`), opaque to scripts.
#[derive(Debug)]
pub struct SnapshotHandle(pub Snapshot);

// SAFETY: a map of paths to digests.
unsafe impl Userdata for SnapshotHandle {
    const NAME: &'static str = "dream.vfs.Snapshot";
}

/// A hash-plus-metadata baseline (`dream.vfs.MetadataSnapshot`), opaque to scripts.
#[derive(Debug)]
pub struct MetadataSnapshotHandle(pub MetadataSnapshot);

// SAFETY: a map of paths to digests and metadata.
unsafe impl Userdata for MetadataSnapshotHandle {
    const NAME: &'static str = "dream.vfs.MetadataSnapshot";
}

// ---------------------------------------------------------------------------------------------
// VfsFile
// ---------------------------------------------------------------------------------------------

fn os_str_bytes(value: Option<&std::ffi::OsStr>) -> Option<Vec<u8>> {
    value.map(|value| value.as_encoded_bytes().to_vec())
}

/// `file:readInto(buffer, offset?)`: streams the file into the buffer from `offset` and returns
/// the number of bytes written, at most the space after `offset`.
fn read_into(
    file: &VfsFileHandle,
    mut buffer: BufferView<'_>,
    offset: Option<Exact<i64>>,
) -> Result<f64> {
    let offset = usize::try_from(offset.map_or(0, |o| o.0))
        .map_err(|_| Error::runtime("dream.vfs: negative buffer offset"))?;
    if offset > buffer.len() {
        return Err(Error::runtime("buffer access out of bounds"));
    }
    let mut reader = file.file.open().map_err(io_error)?;
    // SAFETY: the reader is this crate's own file or archive reader, which holds no Lua handle
    // and never calls into Lua, and no other view of the buffer exists in this call, so the
    // slice is the only access to the buffer while it lives.
    let dest = unsafe { &mut buffer.bytes_mut_unchecked()[offset..] };
    let mut written = 0;
    while written < dest.len() {
        match reader.read(&mut dest[written..]) {
            Ok(0) => break,
            Ok(count) => written += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(written as f64)
}

pub(super) fn describe_file(d: &mut ExtensionDescriptor) {
    let mut file = d.userdata::<VfsFileHandle>(VfsFileHandle::NAME);
    file.tag(TagPolicy::Preferred)
        .doc("A loose file on disk or an entry inside an archive.");
    file.method("isLoose", |f: &VfsFileHandle| f.file.is_loose())
        .signature("(self): boolean");
    file.method("isArchive", |f: &VfsFileHandle| f.file.is_archive())
        .signature("(self): boolean");
    file.method("path", |f: &VfsFileHandle| f.file.path_bytes().to_vec())
        .signature("(self): string")
        .doc("The host path of a loose file, or the entry name inside its archive, byte for byte.");
    file.method("fileName", |f: &VfsFileHandle| {
        os_str_bytes(f.file.file_name())
    })
    .signature("(self): string?");
    file.method("fileStem", |f: &VfsFileHandle| {
        os_str_bytes(f.file.file_stem())
    })
    .signature("(self): string?");
    file.method("parentArchivePath", |f: &VfsFileHandle| {
        f.file.parent_archive_path()
    })
    .signature("(self): string?");
    file.method("parentArchiveName", |f: &VfsFileHandle| {
        f.file.parent_archive_name()
    })
    .signature("(self): string?");
    file.method("size", |f: &VfsFileHandle| f.size().map(|size| size as f64))
        .signature("(self): number")
        .doc("The size in bytes: a loose file's metadata, or an archive entry's uncompressed size, from the archive's index when it has one.");
    file.method("readAll", |f: &VfsFileHandle| -> Result<Vec<u8>> {
        let mut reader = f.file.open().map_err(io_error)?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map_err(io_error)?;
        Ok(bytes)
    })
    .signature("(self): string")
    .doc("The whole file as a string; readInto is the copy-free form.");
    file.method("readAllBuffer", io::read_all_buffer)
        .signature("(self): buffer")
        .doc("The whole file as a new buffer, one copy from the file's map or bytes.");
    file.method("readInto", read_into)
        .signature("(self, buffer: buffer, offset: number?): number")
        .doc("Reads the file into the buffer from offset (default 0), at most the space left; returns the bytes written.");
    file.method("readAt", io::read_at)
        .signature("(self, buffer: buffer, fileOffset: number, length: number?, bufferOffset: number?): number")
        .doc("Copies length bytes (default the space left) from fileOffset into the buffer at bufferOffset (default 0); returns the count, fewer only at the end of the file.");
    file.method("readRange", io::read_range)
        .signature("(self, fileOffset: number, length: number): buffer")
        .doc("A new buffer of length bytes from fileOffset, shorter only at the end of the file.");
    file.method("open", io::open_reader)
        .signature("(self): dream_vfs_Reader")
        .doc("A sequential reader over the file's map or bytes.");
    file.metamethod("__tostring", |f: &VfsFileHandle| {
        format!("dream.vfs.VfsFile({})", f.file.path().display())
    });
}

pub(super) fn file_class_table(runtime: &l3i::Runtime) -> Result<Table> {
    frozen_class_table(runtime, |table| {
        class_function(runtime, table, "VfsFile", "from", |path: &[u8]| {
            Owned(VfsFileHandle::new(VfsFile::from(host_path(path))))
        })
    })
}

// ---------------------------------------------------------------------------------------------
// VfsProvider
// ---------------------------------------------------------------------------------------------

/// Reads a `{ path = string, kind = "looseDir" | "archive" }` table.
pub(super) fn source_meta(
    scope: &impl Scope,
    view: ValueView<'_>,
    context: &str,
) -> Result<SourceMeta> {
    Options::read(scope, view, context, |o| {
        let path = o.required_bytes("path", |path| Ok(host_path(path)))?;
        let kind = o.required_str("kind", source_kind_from_name)?;
        Ok(SourceMeta { path, kind })
    })
}

pub(super) fn describe_provider(d: &mut ExtensionDescriptor) {
    let mut provider = d.userdata::<VfsProviderHandle>(VfsProviderHandle::NAME);
    provider
        .tag(TagPolicy::Never)
        .doc("One provider for a key: its source and backing file.");
    provider
        .method("source", |p: &VfsProviderHandle, call: &Call| {
            reports::source_meta(call, &p.0.source)
        })
        .signature(format!("(self): {}", super::types::SOURCE));
    provider
        .method("file", |p: &VfsProviderHandle| {
            Owned(VfsFileHandle::new(p.0.file.clone()))
        })
        .signature("(self): dream_vfs_VfsFile");
    provider.metamethod("__tostring", |p: &VfsProviderHandle| {
        format!(
            "dream.vfs.VfsProvider({} from {})",
            p.0.file.path().display(),
            p.0.source.path.display()
        )
    });
}

pub(super) fn provider_class_table(runtime: &l3i::Runtime) -> Result<Table> {
    frozen_class_table(runtime, |table| {
        class_function(
            runtime,
            table,
            "VfsProvider",
            "new",
            |call: &Call, source: ValueView, file: &VfsFileHandle| {
                let source = source_meta(call, source, "VfsProvider.new")?;
                Ok::<_, Error>(Owned(VfsProviderHandle(VfsProvider::new(
                    source,
                    file.file.clone(),
                ))))
            },
        )
    })
}

// ---------------------------------------------------------------------------------------------
// Snapshots and locks
// ---------------------------------------------------------------------------------------------

pub(super) fn describe_snapshots(d: &mut ExtensionDescriptor) {
    d.userdata::<SnapshotHandle>(SnapshotHandle::NAME)
        .tag(TagPolicy::Never)
        .doc("A directory content-hash baseline for runFinalize and changedFiles.")
        .method("len", |s: &SnapshotHandle| s.0.len() as f64)
        .signature("(self): number");
    d.userdata::<MetadataSnapshotHandle>(MetadataSnapshotHandle::NAME)
        .tag(TagPolicy::Never)
        .doc("A directory hash-and-metadata baseline for runFinalizeTracked and changedFilesMetadata.")
        .method("len", |s: &MetadataSnapshotHandle| s.0.len() as f64)
        .signature("(self): number");
    let mut lock = d.userdata::<VfsLockHandle>(VfsLockHandle::NAME);
    lock.tag(TagPolicy::Never)
        .doc("A deterministic lock manifest of the current winners.");
    lock.method("schemaVersion", |l: &VfsLockHandle| {
        f64::from(l.0.schema_version)
    })
    .signature("(self): number");
    lock.method("entries", |l: &VfsLockHandle, call: &Call| {
        reports::lock_entries(call, &l.0.entries)
    })
    .signature(format!("(self): {}", super::types::LOCK_ENTRIES));
    lock.method("toTable", |l: &VfsLockHandle, call: &Call| {
        reports::lock(call, &l.0)
    })
    .signature(format!(
        "(self): {{ schemaVersion: number, entries: {} }}",
        super::types::LOCK_ENTRIES
    ));
}

/// `path` pushed as bytes, for the class tables' string results.
pub(super) fn path_string(path: &std::path::Path) -> Vec<u8> {
    path_bytes(path).to_vec()
}
