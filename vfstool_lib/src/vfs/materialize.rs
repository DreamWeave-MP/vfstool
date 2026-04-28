// SPDX-License-Identifier: GPL-3.0-only
use super::VFS;
use crate::{
    CollapseOptions, NormalizedPath, VfsFile,
    paths::{key_to_path_buf_bytes, key_to_string_lossy, normalized_safe_normalized_bytes},
};
use rayon::prelude::*;
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
};

impl VFS {
    /// Dump every file in the VFS into `dir`, preserving relative paths.
    ///
    /// When `use_hardlinks` is `true`, loose files are hardlinked; cross-device
    /// link failures fall back to a copy. All other hardlink errors propagate.
    /// Archive files are always read via [`VfsFile::open`] regardless of mode.
    /// Bethesda archive entries stream; ZIP/PK3 entries are currently buffered.
    /// The destination directory must already exist. Returns the number of
    /// files successfully written.
    ///
    /// # Errors
    ///
    /// Returns an error for hardlink/copy/write failures not explicitly handled
    /// as skippable cases.
    pub fn dump_to_directory(&self, dir: &Path, use_hardlinks: bool) -> std::io::Result<usize> {
        self.validate_materialization_paths()?;

        let written: std::io::Result<Vec<bool>> = self
            .file_map
            .par_iter()
            .map(|(relative_path, file)| -> std::io::Result<bool> {
                let relative_path_buf = key_to_path_buf_bytes(relative_path);
                let dest = dir.join(&relative_path_buf);
                Self::ensure_output_parent_safe(dir, &dest)?;
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                if file.is_loose() {
                    if !file.path().exists() {
                        eprintln!(
                            "vfstool: skipping {}: source no longer exists at {}",
                            key_to_string_lossy(relative_path),
                            file.path().display()
                        );
                        return Ok(false);
                    }
                    if use_hardlinks {
                        if dest.exists() {
                            std::fs::remove_file(&dest)?;
                        }
                        match std::fs::hard_link(file.path(), &dest) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
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
                            eprintln!(
                                "vfstool: skipping {}: {e}",
                                key_to_string_lossy(relative_path)
                            );
                            return Ok(false);
                        }
                    }
                }
                Ok(true)
            })
            .collect();
        Ok(written?.into_iter().filter(|&ok| ok).count())
    }

    /// Collapse the entire VFS into `dest`, creating hardlinks, symlinks, or copies.
    ///
    /// Per-file errors are reported via `eprintln!` rather than aborting -
    /// this matches the original CLI behavior of continuing past individual
    /// link/copy failures.
    ///
    /// # Errors
    ///
    /// Returns an error if creating the destination root directory fails.
    pub fn collapse_into(&self, dest: &Path, opts: &CollapseOptions) -> io::Result<()> {
        self.validate_materialization_paths()?;
        std::fs::create_dir_all(dest)?;

        self.file_map
            .par_iter()
            .map(|(relative_path, file)| -> io::Result<()> {
                let relative_path_buf = key_to_path_buf_bytes(relative_path);
                let merged_path = dest.join(&relative_path_buf);
                Self::ensure_output_parent_safe(dest, &merged_path)?;
                let Some(merged_dir) = merged_path.parent() else {
                    eprintln!(
                        "vfstool: failed to resolve parent dir for {}",
                        merged_path.display()
                    );
                    return Ok(());
                };

                if let Err(e) = std::fs::create_dir_all(merged_dir) {
                    eprintln!(
                        "vfstool: failed to create directory {}: {}",
                        merged_dir.display(),
                        e
                    );
                    return Ok(());
                }

                if file.is_loose() {
                    Self::collapse_loose_file(file, &merged_path, opts);
                } else if opts.extract_archives {
                    Self::collapse_archive_file(file, relative_path, &merged_path);
                } else {
                    eprintln!(
                        "vfstool: skipping {}, loaded from archive: {}",
                        key_to_string_lossy(relative_path),
                        file.parent_archive_path().unwrap_or_default()
                    );
                }

                Ok(())
            })
            .collect()
    }

    fn collapse_loose_file(file: &VfsFile, merged_path: &Path, opts: &CollapseOptions) {
        if !file.path().exists() {
            eprintln!(
                "vfstool: skipping {}: source file no longer exists at {}",
                merged_path.display(),
                file.path().display()
            );
            return;
        }

        if Self::is_archive_file(file) && opts.extract_archives {
            eprintln!(
                "vfstool: skipping archive {}",
                file.file_name().unwrap_or_default().to_string_lossy()
            );
            return;
        }

        if let Err(e) = std::fs::remove_file(merged_path)
            && e.kind() != io::ErrorKind::NotFound
        {
            eprintln!(
                "vfstool: failed to remove existing file at {}: {}",
                merged_path.display(),
                e
            );
            return;
        }

        let link_result = if opts.use_symlinks {
            Self::symlink(file.path(), merged_path)
        } else {
            std::fs::hard_link(file.path(), merged_path)
        };

        if let Err(e) = link_result {
            eprintln!("vfstool: link failed for {}: {}", file.path().display(), e);
            if opts.allow_copying
                && let Err(copy_err) = Self::copy_replacing_output(file.path(), merged_path)
            {
                eprintln!(
                    "vfstool: fallback copy of {} to {} failed: {}",
                    file.path().display(),
                    merged_path.display(),
                    copy_err
                );
            }
        }
    }

    fn collapse_archive_file(file: &VfsFile, relative_path: &NormalizedPath, merged_path: &Path) {
        match file.open() {
            Ok(mut data) => {
                let result = (|| -> io::Result<()> {
                    Self::remove_existing_output_file(merged_path)?;
                    let mut out = std::fs::File::create(merged_path)?;
                    std::io::copy(&mut data, &mut out)?;
                    Ok(())
                })();
                if let Err(e) = result {
                    eprintln!(
                        "vfstool: failed to extract {} to {}: {}",
                        key_to_string_lossy(relative_path),
                        merged_path.display(),
                        e
                    );
                }
            }
            Err(e) => eprintln!(
                "vfstool: failed to open archived file {}: {}",
                key_to_string_lossy(relative_path),
                e
            ),
        }
    }

    pub(super) fn is_archive_file(file: &VfsFile) -> bool {
        let Some(ext) = file.path().extension() else {
            return false;
        };
        let ext = ext.to_ascii_lowercase();
        let name = file.file_name().unwrap_or_default().to_ascii_lowercase();
        matches!(ext.to_str(), Some("bsa" | "ba2" | "zip" | "pk3"))
            && name != "archiveinvalidationinvalidated!.bsa"
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
    /// `vfs_path` is not found in the VFS.
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

        let normalized_path = key_to_path_buf_bytes(&normalized_key);
        let file_name = normalized_path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "vfs_path has no file name")
        })?;

        let dest = dest_dir.join(file_name);
        Self::ensure_output_parent_safe(dest_dir, &dest)?;

        if file.is_loose() {
            Self::copy_replacing_output(file.path(), &dest)?;
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
        let keys = self.file_map.keys().cloned().collect::<BTreeSet<_>>();
        for key in &keys {
            if !normalized_safe_normalized_bytes(key.as_bytes()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "VFS key '{}' cannot be safely materialized",
                        key_to_string_lossy(key)
                    ),
                ));
            }

            let mut prefix = Vec::new();
            for component in key.as_bytes().split(|&byte| byte == b'/') {
                if !prefix.is_empty() {
                    prefix.push(b'/');
                }
                prefix.extend_from_slice(component);
                if prefix.as_slice() == key.as_bytes() {
                    break;
                }
                let prefix_key = NormalizedPath::from_normalized_bytes_unchecked(prefix.clone());
                if keys.contains(&prefix_key) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "VFS keys '{}' and '{}' cannot both be materialized as filesystem paths",
                            String::from_utf8_lossy(&prefix),
                            key_to_string_lossy(key)
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    fn ensure_output_parent_safe(root: &Path, output: &Path) -> io::Result<()> {
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
