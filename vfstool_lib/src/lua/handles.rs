// SPDX-License-Identifier: MIT OR Apache-2.0
//! The userdata handles: the VFS itself, files, providers, indexes, locks, and snapshots.

use std::{cell::RefCell, io::Read as _};

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
    class_function, frozen_class_table, host_path, io_error, path_bytes, reports,
    source_kind_from_name,
};
use crate::{
    ConflictIndex, LayerIndex, MetadataSnapshot, Snapshot, SourceMeta, VFS, VfsFile, VfsLock,
    VfsProvider,
};

/// A VFS as scripts see it (`dream.vfs.VFS`): the VFS behind a `RefCell`, because scripts
/// mutate it through methods and Luau hands out shared references, plus the sorted key list
/// `keys()` last built, kept until the next mutation.
pub struct Vfs {
    inner: RefCell<VFS>,
    keys: RefCell<Option<KeyBlob>>,
}

// SAFETY: plain Rust data (maps, paths, archive handles), no Lua references, no Lua API in `Drop`.
unsafe impl Userdata for Vfs {
    const NAME: &'static str = "dream.vfs.VFS";
}

impl Vfs {
    /// Wraps a VFS the host built.
    #[must_use]
    pub fn new(vfs: VFS) -> Self {
        Vfs {
            inner: RefCell::new(vfs),
            keys: RefCell::new(None),
        }
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
            .inner
            .try_borrow()
            .map_err(|_| Error::runtime("dream.vfs: the VFS is being mutated"))?;
        Ok(body(&vfs))
    }

    /// The sorted keys, built on the first call after a mutation and shared afterwards.
    pub(super) fn sorted_keys(&self) -> Result<KeyBlob> {
        if let Some(keys) = self.keys.borrow().as_ref() {
            return Ok(keys.clone());
        }
        let keys = self.with(|vfs| KeyBlob::sorted(vfs.iter().map(|(key, _)| key.clone())))?;
        *self.keys.borrow_mut() = Some(keys.clone());
        Ok(keys)
    }

    /// Runs `body` with the VFS borrowed mutably.
    ///
    /// # Errors
    ///
    /// Returns an error if the VFS is otherwise in use.
    pub fn with_mut<R>(&self, body: impl FnOnce(&mut VFS) -> R) -> Result<R> {
        let mut vfs = self
            .inner
            .try_borrow_mut()
            .map_err(|_| Error::runtime("dream.vfs: the VFS is in use"))?;
        // Whatever the mutation does to the key set, the cached list is stale.
        *self.keys.borrow_mut() = None;
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
}

/// A backing file handle (`dream.vfs.VfsFile`).
#[derive(Debug, Clone)]
pub struct VfsFileHandle(pub VfsFile);

// SAFETY: a path or an `Arc` archive reference; no Lua references, no Lua API in `Drop`.
unsafe impl Userdata for VfsFileHandle {
    const NAME: &'static str = "dream.vfs.VfsFile";
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
    let mut reader = file.0.open().map_err(io_error)?;
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
    file.method("isLoose", |f: &VfsFileHandle| f.0.is_loose())
        .signature("(self): boolean");
    file.method("isArchive", |f: &VfsFileHandle| f.0.is_archive())
        .signature("(self): boolean");
    file.method("path", |f: &VfsFileHandle| f.0.path_bytes().to_vec())
        .signature("(self): string")
        .doc("The host path of a loose file, or the entry name inside its archive, byte for byte.");
    file.method("fileName", |f: &VfsFileHandle| {
        os_str_bytes(f.0.file_name())
    })
    .signature("(self): string?");
    file.method("fileStem", |f: &VfsFileHandle| {
        os_str_bytes(f.0.file_stem())
    })
    .signature("(self): string?");
    file.method("parentArchivePath", |f: &VfsFileHandle| {
        f.0.parent_archive_path()
    })
    .signature("(self): string?");
    file.method("parentArchiveName", |f: &VfsFileHandle| {
        f.0.parent_archive_name()
    })
    .signature("(self): string?");
    file.method("readAll", |f: &VfsFileHandle| -> Result<Vec<u8>> {
        let mut reader = f.0.open().map_err(io_error)?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map_err(io_error)?;
        Ok(bytes)
    })
    .signature("(self): string")
    .doc("The whole file as a string; readInto is the copy-free form.");
    file.method("readInto", read_into)
        .signature("(self, buffer: buffer, offset: number?): number")
        .doc("Reads the file into the buffer from offset (default 0), at most the space left; returns the bytes written.");
    file.metamethod("__tostring", |f: &VfsFileHandle| {
        format!("dream.vfs.VfsFile({})", f.0.path().display())
    });
}

pub(super) fn file_class_table(runtime: &l3i::Runtime) -> Result<Table> {
    frozen_class_table(runtime, |table| {
        class_function(runtime, table, "VfsFile", "from", |path: &[u8]| {
            Owned(VfsFileHandle(VfsFile::from(host_path(path))))
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
        let path: Vec<u8> = o.required("path")?;
        let kind: String = o.required("kind")?;
        Ok(SourceMeta {
            path: host_path(&path),
            kind: source_kind_from_name(&kind)?,
        })
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
            Owned(VfsFileHandle(p.0.file.clone()))
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
                    file.0.clone(),
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
