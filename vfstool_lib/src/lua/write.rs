// SPDX-License-Identifier: MIT OR Apache-2.0
//! Writing files: `dream.vfs.Writer`, the buffered writer `vfs:openWrite` and `host.openWrite`
//! return, and (with `lua-write`) the VFS methods that write under a VFS's write root and
//! register what they wrote as the key's winner.

use std::{
    cell::{Cell, RefCell},
    fs::{File, OpenOptions},
    io::{self, BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use l3i::{
    Error, Result,
    convert::{BytesView, Exact},
    extension::{ExtensionDescriptor, TagPolicy},
    options::Options,
    stack::{Scope, ValueView},
    userdata::{Owned, Userdata},
};

#[cfg(feature = "lua-write")]
use super::{Vfs, VfsFileHandle, host_path, with_key};
use super::{
    io::{non_negative, window},
    io_error, path_bytes,
};

/// How a file is opened for a writer: `{ append?, truncate? }`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct OpenWrite {
    pub(super) append: bool,
    /// `None` is the default: truncate unless appending.
    pub(super) truncate: Option<bool>,
}

impl OpenWrite {
    /// Reads `{ append?, truncate? }` under `context`.
    pub(super) fn read(
        scope: &impl Scope,
        options: Option<ValueView<'_>>,
        context: &str,
    ) -> Result<OpenWrite> {
        let Some(options) = options.filter(|view| !view.is_nil()) else {
            return Ok(OpenWrite::default());
        };
        Options::read(scope, options, context, |o| {
            Ok(OpenWrite {
                append: o.or("append", false)?,
                truncate: o.optional("truncate")?,
            })
        })
    }

    /// Opens `path` accordingly, creating it, and says where the position starts.
    pub(super) fn open(self, path: &Path) -> io::Result<(File, u64)> {
        let truncate = self.truncate.unwrap_or(!self.append);
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .append(self.append)
            .truncate(truncate && !self.append)
            .open(path)?;
        let position = if self.append {
            file.metadata()?.len()
        } else {
            0
        };
        Ok((file, position))
    }
}

/// How `writeFile` writes: `{ offset?, append?, create? }`.
#[derive(Clone, Copy, Debug)]
pub(super) struct WriteFile {
    pub(super) offset: Option<u64>,
    pub(super) append: bool,
    pub(super) create: bool,
}

impl Default for WriteFile {
    fn default() -> Self {
        WriteFile {
            offset: None,
            append: false,
            create: true,
        }
    }
}

impl WriteFile {
    /// Reads `{ offset?, append?, create? }` under `context`; `offset` and `append` exclude
    /// each other.
    pub(super) fn read(
        scope: &impl Scope,
        options: Option<ValueView<'_>>,
        context: &str,
    ) -> Result<WriteFile> {
        let Some(options) = options.filter(|view| !view.is_nil()) else {
            return Ok(WriteFile::default());
        };
        let options = Options::read(scope, options, context, |o| {
            let offset = match o.optional::<Exact<i64>>("offset")? {
                Some(offset) => Some(non_negative(context, "offset", offset)?),
                None => None,
            };
            Ok(WriteFile {
                offset,
                append: o.or("append", false)?,
                create: o.or("create", true)?,
            })
        })?;
        if options.offset.is_some() && options.append {
            return Err(Error::runtime(format!(
                "{context}: offset and append cannot be combined"
            )));
        }
        Ok(options)
    }

    /// Writes `data` to `path` as the options say; returns the count written.
    pub(super) fn write(self, path: &Path, data: &[u8]) -> io::Result<usize> {
        let mut open = OpenOptions::new();
        open.write(true).create(self.create);
        if self.append {
            open.append(true);
        } else if self.offset.is_none() {
            open.truncate(true);
        }
        let mut file = open.open(path)?;
        match self.offset {
            Some(offset) => write_all_at(&file, offset, data)?,
            None => file.write_all(data)?,
        }
        file.flush()?;
        Ok(data.len())
    }
}

/// Writes all of `data` at `offset` without moving the handle's own position (which
/// positional writes on Windows move; the caller re-seeks when it keeps one).
fn write_all_at(file: &File, offset: u64, data: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_all_at(data, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut written = 0;
        while written < data.len() {
            match file.seek_write(&data[written..], offset + written as u64) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => written += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

/// What a VFS-bound writer does when it closes: makes the file the key's winner.
#[cfg(feature = "lua-write")]
pub(super) struct Registration {
    pub(super) vfs: Vfs,
    pub(super) key: Vec<u8>,
    pub(super) path: PathBuf,
}

/// A buffered writer over a file (`dream.vfs.Writer`).
pub struct VfsWriter {
    file: RefCell<Option<BufWriter<File>>>,
    position: Cell<u64>,
    append: bool,
    name: Box<[u8]>,
    #[cfg(feature = "lua-write")]
    registration: Option<Registration>,
}

impl std::fmt::Debug for VfsWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VfsWriter")
            .field("name", &String::from_utf8_lossy(&self.name))
            .field("position", &self.position.get())
            .field("open", &self.file.borrow().is_some())
            .finish_non_exhaustive()
    }
}

// SAFETY: a file handle behind a buffer, a shared VFS handle, plain data; no Lua references.
// Dropping flushes the buffer and closes the handle with no Lua API.
unsafe impl Userdata for VfsWriter {
    const NAME: &'static str = "dream.vfs.Writer";
}

impl VfsWriter {
    /// A writer over `path`, opened as `options` say.
    pub(super) fn open(
        path: &Path,
        options: OpenWrite,
        #[cfg(feature = "lua-write")] registration: Option<Registration>,
    ) -> io::Result<VfsWriter> {
        let (file, position) = options.open(path)?;
        Ok(VfsWriter {
            file: RefCell::new(Some(BufWriter::new(file))),
            position: Cell::new(position),
            append: options.append,
            name: path_bytes(path).into(),
            #[cfg(feature = "lua-write")]
            registration,
        })
    }

    fn name(&self) -> String {
        String::from_utf8_lossy(&self.name).into_owned()
    }

    /// Runs `body` on the open file, or fails when the writer is closed.
    fn with_file<R>(
        &self,
        what: &str,
        body: impl FnOnce(&mut BufWriter<File>) -> io::Result<R>,
    ) -> Result<R> {
        let mut file = self.file.borrow_mut();
        let Some(file) = file.as_mut() else {
            return Err(Error::runtime(format!(
                "dream.vfs: Writer.{what}: the writer over {} is closed",
                self.name()
            )));
        };
        body(file).map_err(io_error)
    }

    /// Writes `data` at the position and advances past it.
    fn write(&self, data: &[u8]) -> Result<usize> {
        self.with_file("write", |file| file.write_all(data))?;
        self.position.set(self.position.get() + data.len() as u64);
        Ok(data.len())
    }

    /// Writes `data` at `position` without moving.
    fn write_at(&self, position: u64, data: &[u8]) -> Result<usize> {
        if self.append {
            return Err(Error::runtime(format!(
                "dream.vfs: Writer.writeAt: the writer over {} appends, so it cannot write at a position",
                self.name()
            )));
        }
        let current = self.position.get();
        self.with_file("writeAt", |file| {
            file.flush()?;
            write_all_at(file.get_ref(), position, data)?;
            file.get_mut().seek(SeekFrom::Start(current)).map(drop)
        })?;
        Ok(data.len())
    }

    fn seek(&self, position: u64) -> Result<()> {
        self.with_file("seek", |file| {
            file.seek(SeekFrom::Start(position)).map(drop)
        })?;
        self.position.set(position);
        Ok(())
    }

    fn truncate(&self, length: u64) -> Result<()> {
        let position = self.position.get().min(length);
        self.with_file("truncate", |file| {
            file.flush()?;
            file.get_ref().set_len(length)?;
            file.get_mut().seek(SeekFrom::Start(position)).map(drop)
        })?;
        self.position.set(position);
        Ok(())
    }

    /// Flushes and closes; a VFS-bound writer then registers its file as the key's winner and
    /// returns the handle. Closing twice is nothing.
    fn close(&self) -> Result<Option<Owned<VfsFileHandleOrNone>>> {
        let Some(mut file) = self.file.borrow_mut().take() else {
            return Ok(None);
        };
        file.flush().map_err(io_error)?;
        drop(file);
        #[cfg(feature = "lua-write")]
        if let Some(registration) = &self.registration {
            let file = register(
                &registration.vfs,
                "Writer.close",
                &registration.key,
                &registration.path,
            )?;
            return Ok(Some(Owned(file)));
        }
        Ok(None)
    }
}

#[cfg(feature = "lua-write")]
type VfsFileHandleOrNone = VfsFileHandle;
#[cfg(not(feature = "lua-write"))]
type VfsFileHandleOrNone = super::VfsFileHandle;

/// The slice of `data` the `offset` and `length` arguments select.
fn data_window<'d>(
    what: &str,
    data: &'d BytesView<'d>,
    offset: Option<Exact<i64>>,
    length: Option<Exact<i64>>,
) -> Result<&'d [u8]> {
    let (offset, length) = window(what, "offset", "data", data.len(), offset, length)?;
    // SAFETY: a string's bytes are immutable; a buffer's are read once here and written to a
    // file with no call into Lua in between, and this call holds the only view of it.
    Ok(&unsafe { data.bytes_unchecked() }[offset..offset + length])
}

pub(super) fn describe_writer(d: &mut ExtensionDescriptor) {
    let mut writer = d.userdata::<VfsWriter>(VfsWriter::NAME);
    writer.tag(TagPolicy::Never).doc(
        "A buffered writer over a file; close registers a VFS writer's file as its key's winner.",
    );
    writer
        .method(
            "write",
            |w: &VfsWriter, data: BytesView, offset: Option<Exact<i64>>, length: Option<Exact<i64>>| {
                let bytes = data_window("Writer.write", &data, offset, length)?;
                w.write(bytes).map(|count| count as f64)
            },
        )
        .signature("(self, data: buffer | string, offset: number?, length: number?): number")
        .doc("Writes data (a buffer or a string), or its slice from offset of length bytes, at the position and advances past it; returns the count.");
    writer
        .method(
            "writeAt",
            |w: &VfsWriter, position: Exact<i64>, data: BytesView, offset: Option<Exact<i64>>, length: Option<Exact<i64>>| {
                let position = non_negative("Writer.writeAt", "position", position)?;
                let bytes = data_window("Writer.writeAt", &data, offset, length)?;
                w.write_at(position, bytes).map(|count| count as f64)
            },
        )
        .signature("(self, position: number, data: buffer | string, offset: number?, length: number?): number")
        .doc("Writes data at position without moving; an error on an append writer.");
    writer
        .method("seek", |w: &VfsWriter, position: Exact<i64>| {
            w.seek(non_negative("Writer.seek", "position", position)?)
        })
        .signature("(self, position: number)")
        .doc("Moves to position; past the end, the next write extends the file.");
    writer
        .method("tell", |w: &VfsWriter| w.position.get() as f64)
        .signature("(self): number");
    writer
        .method("flush", |w: &VfsWriter| w.with_file("flush", Write::flush))
        .signature("(self)");
    writer
        .method("truncate", |w: &VfsWriter, length: Exact<i64>| {
            w.truncate(non_negative("Writer.truncate", "length", length)?)
        })
        .signature("(self, length: number)")
        .doc("Cuts or extends the file to length bytes; a position past it moves to it.");
    writer
        .method("close", VfsWriter::close)
        .signature("(self): dream_vfs_VfsFile?")
        .doc("Flushes and closes. A writer from vfs:openWrite then makes its file the key's winner and returns it; closing again does nothing.");
    writer.metamethod("__tostring", |w: &VfsWriter| {
        format!("dream.vfs.Writer({})", w.name())
    });
}

// ---------------------------------------------------------------------------------------------
// The VFS methods (lua-write)
// ---------------------------------------------------------------------------------------------

/// The write root of `v`, or the error a VFS without one raises.
#[cfg(feature = "lua-write")]
fn write_root(what: &str, v: &Vfs) -> Result<PathBuf> {
    v.write_root().ok_or_else(|| {
        Error::runtime(format!(
            "dream.vfs: {what}: this VFS has no write root (build it with writeRoot, or call vfs:setWriteRoot)"
        ))
    })
}

/// The host path of `key` under the write root, refused before anything touches the disk when
/// the key could leave it: lexically (`..`, a root, a drive letter, NUL), or on disk, through a
/// symbolic link between the root and the target.
#[cfg(feature = "lua-write")]
fn target(what: &str, v: &Vfs, key: &[u8]) -> Result<PathBuf> {
    let root = write_root(what, v)?;
    if crate::paths::normalized_safe_key_bytes(key).is_none() {
        return Err(Error::runtime(format!(
            "dream.vfs: {what}: key '{}' escapes the write root (a key is relative, with no '..', root, drive letter or NUL)",
            String::from_utf8_lossy(key)
        )));
    }
    let relative: Vec<u8> = key
        .iter()
        .map(|&byte| if byte == b'\\' { b'/' } else { byte })
        .collect();
    contain(what, &root, &relative)?;
    Ok(root.join(host_path(&relative)))
}

/// Refuses `<root>/<relative>` when a symbolic link sits at any existing component from the
/// root down to it, or when the deepest existing part of it resolves outside the canonical
/// `root`. `relative` is a checked key with `/` separators, so its segments are plain names.
///
/// The walk reads the disk once per component and the write follows; a writer with its own
/// access to the root can change things in between, which the sandbox does not defend against.
#[cfg(feature = "lua-write")]
fn contain(what: &str, root: &Path, relative: &[u8]) -> Result<()> {
    let mut deepest = root.to_path_buf();
    let mut end = 0;
    for segment in relative.split(|&byte| byte == b'/') {
        end += segment.len();
        let walked = &relative[..end];
        end += 1;
        let current = root.join(host_path(walked));
        let Ok(meta) = std::fs::symlink_metadata(&current) else {
            // Nothing there (or nothing a directory can hold): the write creates the rest, or
            // reports its own error.
            break;
        };
        if meta.file_type().is_symlink() {
            return Err(Error::runtime(format!(
                "dream.vfs: {what}: '{}' is a symbolic link; the write root does not follow links",
                String::from_utf8_lossy(walked)
            )));
        }
        deepest = current;
    }
    let resolved = std::fs::canonicalize(&deepest).map_err(io_error)?;
    if !resolved.starts_with(root) {
        return Err(Error::runtime(format!(
            "dream.vfs: {what}: key '{}' escapes the write root ({} resolves to {})",
            String::from_utf8_lossy(relative),
            deepest.display(),
            resolved.display()
        )));
    }
    Ok(())
}

/// [`target`] for a key that will become a file: also refused, before the disk is touched, when
/// the VFS could not take it because a directory or a file of that name is in the way.
#[cfg(feature = "lua-write")]
fn file_target(what: &str, v: &Vfs, key: &[u8]) -> Result<PathBuf> {
    let path = target(what, v, key)?;
    let normalized = crate::paths::normalized_safe_key_bytes(key).expect("target checked the key");
    let conflicts = v.with(|vfs| {
        !vfs.contains_normalized(normalized.as_bytes())
            && vfs.key_has_materialization_conflict(&normalized)
    })?;
    if conflicts {
        return Err(Error::runtime(format!(
            "dream.vfs: {what}: the VFS refused key '{}' (a file and a directory cannot share a name)",
            String::from_utf8_lossy(key)
        )));
    }
    Ok(path)
}

/// Makes the file at `path` the winner of `key`, and checks that the VFS took it.
#[cfg(feature = "lua-write")]
fn register(v: &Vfs, what: &str, key: &[u8], path: &Path) -> Result<VfsFileHandle> {
    v.with_mut(|vfs| {
        vfs.set_winner_loose_file(key, path);
    })?;
    let winner = v.with(|vfs| with_key(key, |key| vfs.get_file_normalized(key).cloned()))?;
    match winner {
        Some(file) if file.is_loose() && file.path() == path => Ok(VfsFileHandle::new(file)),
        _ => Err(Error::runtime(format!(
            "dream.vfs: {what}: the VFS refused key '{}' (a file and a directory cannot share a name)",
            String::from_utf8_lossy(key)
        ))),
    }
}

/// Creates the directory `path` will be written into.
#[cfg(feature = "lua-write")]
fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    Ok(())
}

/// The keys under `prefix` whose winner is a loose file under the host directory `dir`, with
/// those files' paths.
#[cfg(feature = "lua-write")]
fn winners_under(v: &Vfs, prefix: &[u8], dir: &Path) -> Result<Vec<(Vec<u8>, PathBuf)>> {
    v.with(|vfs| {
        vfs.paths_with(prefix)
            .filter(|(_, file)| file.is_loose() && file.path().starts_with(dir))
            .map(|(key, file)| (key.as_bytes().to_vec(), file.path().to_path_buf()))
            .collect()
    })
}

/// Whether `key`'s winner is the loose file at `path`.
#[cfg(feature = "lua-write")]
fn wins_with(v: &Vfs, key: &[u8], path: &Path) -> Result<bool> {
    v.with(|vfs| {
        with_key(key, |key| {
            vfs.get_file_normalized(key)
                .is_some_and(|file| file.is_loose() && file.path() == path)
        })
    })
}

#[cfg(feature = "lua-write")]
pub(super) fn describe_vfs_writes(vfs: &mut l3i::extension::UserdataBuilder<'_, Vfs>) {
    use l3i::bind::Call;
    vfs.method("writeRoot", |v: &Vfs| {
        v.write_root().map(|root| path_bytes(&root).to_vec())
    })
    .signature("(self): string?")
    .doc("The directory writes go under, as the canonical path, or nil when this VFS refuses writes.");
    vfs.method("setWriteRoot", |v: &Vfs, dir: &[u8]| {
        v.check_write_root_grant("setWriteRoot")?;
        v.set_write_root(Some(host_path(dir))).map_err(io_error)
    })
    .signature("(self, dir: string)")
    .doc("Makes dir the write root, created if absent and kept as its canonical path. Needs the filesystem.write capability.");
    vfs.method(
        "writeFile",
        |v: &Vfs, call: &Call, key: &[u8], data: BytesView, options: Option<ValueView>| {
            let options = WriteFile::read(call, options, "vfs:writeFile")?;
            let path = file_target("writeFile", v, key)?;
            ensure_parent(&path)?;
            // SAFETY: as `data_window`: the bytes go straight to the file, with no call into
            // Lua while the slice lives, through this call's only view of them.
            options
                .write(&path, unsafe { data.bytes_unchecked() })
                .map_err(io_error)?;
            register(v, "writeFile", key, &path).map(Owned)
        },
    )
    .signature("(self, key: string, data: buffer | string, options: { offset: number?, append: boolean?, create: boolean? }?): dream_vfs_VfsFile")
    .doc("Writes data to <writeRoot>/<key>, creating directories, then makes that file the key's winner and returns it. offset writes in place without truncating; append adds at the end; create = false refuses a file that does not exist.");
    vfs.method(
        "openWrite",
        |v: &Vfs, call: &Call, key: &[u8], options: Option<ValueView>| {
            let options = OpenWrite::read(call, options, "vfs:openWrite")?;
            let path = file_target("openWrite", v, key)?;
            ensure_parent(&path)?;
            let registration = Registration {
                vfs: v.clone(),
                key: key.to_vec(),
                path: path.clone(),
            };
            VfsWriter::open(&path, options, Some(registration))
                .map(Owned)
                .map_err(io_error)
        },
    )
    .signature("(self, key: string, options: { append: boolean?, truncate: boolean? }?): dream_vfs_Writer")
    .doc("A writer over <writeRoot>/<key>, truncated unless append or truncate = false; close makes the file the key's winner.");
    describe_vfs_directory_writes(vfs);
}

/// `mkdir`, `remove` and `rename` under the write root.
#[cfg(feature = "lua-write")]
fn describe_vfs_directory_writes(vfs: &mut l3i::extension::UserdataBuilder<'_, Vfs>) {
    vfs.method("mkdir", |v: &Vfs, key: &[u8]| {
        let path = target("mkdir", v, key)?;
        std::fs::create_dir_all(path).map_err(io_error)
    })
    .signature("(self, key: string)")
    .doc("Creates the directory <writeRoot>/<key> and its parents.");
    vfs.method("remove", |v: &Vfs, key: &[u8]| {
        let path = target("remove", v, key)?;
        let meta = std::fs::symlink_metadata(&path).map_err(|_| {
            Error::runtime(format!(
                "dream.vfs: remove: '{}' is not under the write root",
                String::from_utf8_lossy(key)
            ))
        })?;
        if meta.is_dir() {
            std::fs::remove_dir_all(&path).map_err(io_error)?;
            for (key, _) in winners_under(v, key, &path)? {
                v.with_mut(|vfs| vfs.remove_winner(&key[..]))?;
            }
        } else {
            std::fs::remove_file(&path).map_err(io_error)?;
            if wins_with(v, key, &path)? {
                v.with_mut(|vfs| vfs.remove_winner(key))?;
            }
        }
        Ok::<_, Error>(())
    })
    .signature("(self, key: string)")
    .doc("Deletes <writeRoot>/<key>, a file or a whole directory, and drops the winners it held; a key with nothing under the write root is an error.");
    vfs.method("rename", |v: &Vfs, from: &[u8], to: &[u8]| {
        let source = target("rename", v, from)?;
        let dest = target("rename", v, to)?;
        let meta = std::fs::symlink_metadata(&source).map_err(|_| {
            Error::runtime(format!(
                "dream.vfs: rename: '{}' is not under the write root",
                String::from_utf8_lossy(from)
            ))
        })?;
        ensure_parent(&dest)?;
        std::fs::rename(&source, &dest).map_err(io_error)?;
        if meta.is_dir() {
            let from_prefix = dream_path::normalize_path(from);
            let to_prefix = dream_path::normalize_path(to);
            for (key, path) in winners_under(v, from, &source)? {
                let relative = path.strip_prefix(&source).map_err(|_| Error::runtime("dream.vfs: rename: a winner left its directory"))?;
                let mut new_key = to_prefix.clone();
                new_key.extend_from_slice(&key[from_prefix.len()..]);
                v.with_mut(|vfs| {
                    vfs.remove_winner(&key[..]);
                    vfs.set_winner_loose_file(&new_key[..], dest.join(relative));
                })?;
            }
        } else if wins_with(v, from, &source)? {
            v.with_mut(|vfs| vfs.remove_winner(from))?;
            register(v, "rename", to, &dest)?;
        }
        Ok::<_, Error>(())
    })
    .signature("(self, from: string, to: string)")
    .doc("Moves <writeRoot>/<from> to <writeRoot>/<to>, a file or a directory, and moves the winners with it.");
}
