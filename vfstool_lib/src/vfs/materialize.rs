// SPDX-License-Identifier: MIT OR Apache-2.0
use super::VFS;
use crate::{
    CollapseOptions, MaterializationReport, MaterializationSkip, NormalizedPath, VfsFile,
    paths::{
        is_same_file, key_to_path_buf, key_to_path_buf_bytes, key_to_string_lossy,
        normalized_safe_normalized_bytes,
    },
};
use rayon::prelude::*;
use std::{
    io,
    path::{Path, PathBuf},
};

impl VFS {
    /// Dump every file in the VFS into `dir`, preserving relative paths.
    ///
    /// When `use_hardlinks` is `true`, loose files are hardlinked; cross-device
    /// link failures fall back to a copy. All other hardlink errors propagate.
    /// Archive files are always read via [`VfsFile::open`] regardless of mode.
    /// Bethesda archive entries stream; ZIP/PK3/JPK entries are currently buffered.
    /// A loose file whose destination already is that file (the same path, a hard
    /// link to it, or a symbolic link to it) is left in place and counted, so
    /// dumping into one of the VFS's own data directories keeps its files.
    /// Directories, `dir` included, are created as the files need them.
    ///
    /// A loose winner that no longer exists, or an archive entry that cannot be opened, is left
    /// out and listed in the report's `skipped`; `written` counts the rest.
    ///
    /// # Errors
    ///
    /// Returns an error for hardlink/copy/write failures not explicitly handled
    /// as skippable cases.
    pub fn dump_to_directory(
        &self,
        dir: &Path,
        use_hardlinks: bool,
    ) -> io::Result<MaterializationReport> {
        self.dump_to_directory_impl(dir, use_hardlinks, false)
    }

    pub(crate) fn dump_to_directory_strict(
        &self,
        dir: &Path,
        use_hardlinks: bool,
    ) -> io::Result<usize> {
        self.dump_to_directory_impl(dir, use_hardlinks, true)
            .map(|report| report.written)
    }

    fn dump_to_directory_impl(
        &self,
        dir: &Path,
        use_hardlinks: bool,
        strict: bool,
    ) -> io::Result<MaterializationReport> {
        self.validate_materialization_paths()?;

        let outcomes = self
            .file_map
            .par_iter()
            .map(
                |(relative_path, file)| -> io::Result<Option<MaterializationSkip>> {
                    let relative_path_buf = Self::materialization_path(relative_path)?;
                    let dest = dir.join(&relative_path_buf);
                    Self::ensure_output_parent_safe(dir, &dest)?;
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    if file.is_loose() {
                        if !file.path().exists() {
                            if strict {
                                return Err(io::Error::new(
                                    io::ErrorKind::NotFound,
                                    format!(
                                        "source for VFS key '{}' no longer exists at {}",
                                        key_to_string_lossy(relative_path),
                                        file.path().display()
                                    ),
                                ));
                            }
                            return Ok(Some(MaterializationSkip::MissingLooseSource {
                                key: key_to_path_buf(relative_path),
                                source: file.path().to_path_buf(),
                            }));
                        }
                        if is_same_file(file.path(), &dest)? {
                            return Ok(None);
                        }
                        if use_hardlinks {
                            if dest.exists() {
                                std::fs::remove_file(&dest)?;
                            }
                            match std::fs::hard_link(file.path(), &dest) {
                                Ok(()) => {}
                                Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
                                    std::fs::copy(file.path(), &dest)?;
                                }
                                Err(e) => return Err(e),
                            }
                        } else {
                            Self::remove_existing_output_file(&dest)?;
                            std::fs::copy(file.path(), &dest)?;
                        }
                    } else {
                        match file.open() {
                            Ok(mut reader) => {
                                Self::remove_existing_output_file(&dest)?;
                                let mut out = std::fs::File::create(&dest)?;
                                std::io::copy(&mut reader, &mut out)?;
                            }
                            Err(e) => {
                                if strict {
                                    return Err(e);
                                }
                                return Ok(Some(MaterializationSkip::UnreadableArchiveEntry {
                                    key: key_to_path_buf(relative_path),
                                    archive: PathBuf::from(
                                        file.parent_archive_path().unwrap_or_default(),
                                    ),
                                    error: e.to_string(),
                                }));
                            }
                        }
                    }
                    Ok(None)
                },
            )
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self::materialization_report(outcomes))
    }

    /// One outcome per winner, `None` for one that was written, as a report.
    fn materialization_report(outcomes: Vec<Option<MaterializationSkip>>) -> MaterializationReport {
        let written = outcomes.iter().filter(|outcome| outcome.is_none()).count();
        let mut skipped: Vec<MaterializationSkip> = outcomes.into_iter().flatten().collect();
        skipped.sort_by(|a, b| {
            a.key()
                .as_os_str()
                .as_encoded_bytes()
                .cmp(b.key().as_os_str().as_encoded_bytes())
        });
        MaterializationReport { written, skipped }
    }

    /// Collapse the entire VFS into `dest`, creating hardlinks, symlinks, or copies.
    ///
    /// A file already at a destination is replaced, unless it already is the loose file being
    /// written there (the same path, a hard link to it, or a symbolic link to it): that one is left
    /// in place, so collapsing into one of the VFS's own data directories keeps its files. Symbolic
    /// links point at the file's absolute path, whether or not the VFS was built from relative
    /// directories.
    ///
    /// Without `extract_archives`, winners inside archives are left out; with it, archive files
    /// themselves are. Either way the report's `skipped` lists them, and `written` counts the
    /// rest.
    ///
    /// # Errors
    ///
    /// Returns an error if creating the destination root directory fails or any selected file cannot
    /// be materialized.
    pub fn collapse_into(
        &self,
        dest: &Path,
        opts: &CollapseOptions,
    ) -> io::Result<MaterializationReport> {
        self.validate_materialization_paths()?;
        std::fs::create_dir_all(dest)?;

        let outcomes = self
            .file_map
            .par_iter()
            .map(
                |(relative_path, file)| -> io::Result<Option<MaterializationSkip>> {
                    let relative_path_buf = Self::materialization_path(relative_path)?;
                    let merged_path = dest.join(&relative_path_buf);
                    Self::ensure_output_parent_safe(dest, &merged_path)?;
                    let Some(merged_dir) = merged_path.parent() else {
                        return Err(io::Error::other(format!(
                            "failed to resolve parent dir for {}",
                            merged_path.display()
                        )));
                    };

                    std::fs::create_dir_all(merged_dir).map_err(|e| {
                        io::Error::new(
                            e.kind(),
                            format!("failed to create directory {}: {e}", merged_dir.display()),
                        )
                    })?;

                    if file.is_loose() {
                        Self::collapse_loose_file(relative_path, file, &merged_path, opts)
                    } else if opts.extract_archives {
                        Self::collapse_archive_file(file, relative_path, &merged_path)
                            .map(|()| None)
                    } else {
                        Ok(Some(MaterializationSkip::ArchiveEntry {
                            key: key_to_path_buf(relative_path),
                            archive: PathBuf::from(file.parent_archive_path().unwrap_or_default()),
                        }))
                    }
                },
            )
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self::materialization_report(outcomes))
    }

    fn collapse_loose_file(
        key: &NormalizedPath,
        file: &VfsFile,
        merged_path: &Path,
        opts: &CollapseOptions,
    ) -> io::Result<Option<MaterializationSkip>> {
        if !file.path().exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "source file no longer exists for {} at {}",
                    merged_path.display(),
                    file.path().display()
                ),
            ));
        }

        if Self::is_archive_file(file) && opts.extract_archives {
            return Ok(Some(MaterializationSkip::ArchiveFile {
                key: key_to_path_buf(key),
                archive: file.path().to_path_buf(),
            }));
        }

        if is_same_file(file.path(), merged_path)? {
            return Ok(None);
        }

        match std::fs::remove_file(merged_path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(io::Error::new(
                    e.kind(),
                    format!(
                        "failed to remove existing file at {}: {e}",
                        merged_path.display()
                    ),
                ));
            }
        }

        let link_result = if opts.use_symlinks {
            // A relative target resolves from the link's own folder, not from where the VFS
            // was built, so a link to a relative data directory's file would dangle.
            std::path::absolute(file.path()).and_then(|target| Self::symlink(&target, merged_path))
        } else {
            std::fs::hard_link(file.path(), merged_path)
        };

        match link_result {
            Ok(()) => Ok(None),
            Err(_) if opts.allow_copying => Self::copy_replacing_output(file.path(), merged_path)
                .map(|_| None)
                .map_err(|copy_err| {
                    io::Error::new(
                        copy_err.kind(),
                        format!(
                            "link failed for {}; fallback copy to {} also failed: {copy_err}",
                            file.path().display(),
                            merged_path.display()
                        ),
                    )
                }),
            Err(e) => Err(io::Error::new(
                e.kind(),
                format!("link failed for {}: {e}", file.path().display()),
            )),
        }
    }

    fn collapse_archive_file(
        file: &VfsFile,
        relative_path: &NormalizedPath,
        merged_path: &Path,
    ) -> io::Result<()> {
        match file.open() {
            Ok(mut data) => (|| -> io::Result<()> {
                Self::remove_existing_output_file(merged_path)?;
                let mut out = std::fs::File::create(merged_path)?;
                std::io::copy(&mut data, &mut out)?;
                Ok(())
            })()
            .map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!(
                        "failed to extract {} to {}: {e}",
                        key_to_string_lossy(relative_path),
                        merged_path.display()
                    ),
                )
            }),
            Err(e) => Err(io::Error::new(
                e.kind(),
                format!(
                    "failed to open archived file {}: {e}",
                    key_to_string_lossy(relative_path)
                ),
            )),
        }
    }

    pub(super) fn is_archive_file(file: &VfsFile) -> bool {
        let Some(ext) = file.path().extension() else {
            return false;
        };
        let ext = ext.to_ascii_lowercase();
        let name = file.file_name().unwrap_or_default().to_ascii_lowercase();
        let is_bethesda_archive = cfg!(feature = "beth-archives")
            && matches!(ext.to_str(), Some("bsa" | "ba2"))
            && name != "archiveinvalidationinvalidated!.bsa";
        let is_zip_archive =
            cfg!(feature = "zip") && matches!(ext.to_str(), Some("zip" | "pk3" | "jpk"));
        is_bethesda_archive || is_zip_archive
    }

    #[cfg(unix)]
    fn symlink(src: &Path, dst: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(src, dst)
    }

    #[cfg(windows)]
    fn symlink(src: &Path, dst: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_file(src, dst)
    }

    #[cfg(not(any(unix, windows)))]
    fn symlink(_src: &Path, _dst: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symlinks are not supported on this platform",
        ))
    }

    /// Extract a single VFS file into `dest_dir`.
    ///
    /// Returns the path of the extracted file on success. Returns `None` if
    /// `vfs_path` is not found in the VFS. A file already at the destination is
    /// replaced, unless it already is the loose file being extracted, as when
    /// `dest_dir` is the folder it lives in: that one is left as it is.
    ///
    /// # Errors
    ///
    /// Returns an error if destination creation, source reading, or destination writing fails.
    pub fn extract_file(&self, vfs_path: &Path, dest_dir: &Path) -> io::Result<Option<PathBuf>> {
        let normalized_key = crate::VfsKeyInput::to_vfs_key(vfs_path);
        let Some(file) = self.file_map.get(&normalized_key) else {
            return Ok(None);
        };

        std::fs::create_dir_all(dest_dir)?;

        let normalized_path = Self::materialization_path(&normalized_key)?;
        let file_name = normalized_path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "vfs_path has no file name")
        })?;

        let dest = dest_dir.join(file_name);
        Self::ensure_output_parent_safe(dest_dir, &dest)?;

        if file.is_loose() {
            if !is_same_file(file.path(), &dest)? {
                Self::copy_replacing_output(file.path(), &dest)?;
            }
        } else {
            Self::remove_existing_output_file(&dest)?;
            let mut reader = file.open()?;
            let mut out = std::fs::File::create(&dest)?;
            std::io::copy(&mut reader, &mut out)?;
        }

        Ok(Some(dest))
    }

    fn copy_replacing_output(src: &Path, dest: &Path) -> io::Result<u64> {
        Self::remove_existing_output_file(dest)?;
        std::fs::copy(src, dest)
    }

    fn remove_existing_output_file(path: &Path) -> io::Result<()> {
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("destination is a directory: {}", path.display()),
            )),
            Ok(_) => std::fs::remove_file(path),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }

    fn validate_materialization_paths(&self) -> io::Result<()> {
        for key in self.file_map.keys() {
            if !normalized_safe_normalized_bytes(key.as_bytes()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "VFS key '{}' cannot be safely materialized",
                        key_to_string_lossy(key)
                    ),
                ));
            }
        }
        Ok(())
    }

    fn materialization_path(key: &NormalizedPath) -> io::Result<PathBuf> {
        key_to_path_buf_bytes(key).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "VFS key '{}' cannot be represented as a path on this platform",
                    key_to_string_lossy(key)
                ),
            )
        })
    }

    pub(super) fn ensure_output_parent_safe(root: &Path, output: &Path) -> io::Result<()> {
        let relative = output
            .strip_prefix(root)
            .map_err(|_| io::Error::other("output path should be under root"))?;
        if std::fs::symlink_metadata(root).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("output root is a symlink: {}", root.display()),
            ));
        }

        let mut current = root.to_path_buf();
        let mut components = relative.components().peekable();
        while let Some(component) = components.next() {
            if components.peek().is_none() {
                break;
            }
            current.push(component.as_os_str());
            match std::fs::symlink_metadata(&current) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("output parent is a symlink: {}", current.display()),
                    ));
                }
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => break,
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }
}
