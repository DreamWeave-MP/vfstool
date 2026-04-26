// SPDX-License-Identifier: GPL-3.0-only
#[cfg(any(feature = "bsa", feature = "zip"))]
use crate::archives;
use crate::{SourceKind, SourceMeta, VFS, VfsFile, normalize_path, paths::normalized_safe_key};
use ahash::AHashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// One provider for a normalized VFS key.
#[derive(Debug, Clone)]
pub struct VfsProvider {
    /// Source metadata for the provider.
    pub source: SourceMeta,
    /// Backing file for this provider.
    pub file: VfsFile,
}

/// Provider-aware mutable VFS.
///
/// Providers for each key are stored low-to-high priority. Removing the current winner reveals the
/// next lower-priority provider when one exists.
///
/// Source paths are matched lexically. Pass the same source path representation to removal methods
/// that was used when providers were inserted.
#[derive(Debug, Default)]
pub struct MutableVfs {
    providers: AHashMap<PathBuf, Vec<VfsProvider>>,
}

impl MutableVfs {
    /// Create an empty provider-aware VFS.
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers: AHashMap::new(),
        }
    }

    /// Build a provider-aware VFS from ordered loose directories.
    ///
    /// Later directories have higher priority, matching `OpenMW` `data=` semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if a directory traversal entry cannot be read.
    pub fn from_directories(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> std::io::Result<Self> {
        let mut mutable = Self::new();
        for dir in search_dirs {
            mutable.push_directory(dir)?;
        }
        Ok(mutable)
    }

    /// Build a provider-aware VFS from ordered loose directories and archive names.
    ///
    /// Archives are resolved through the loose directory files, matching `OpenMW`'s archive list
    /// behavior. Archive providers are inserted at lower priority than every loose directory;
    /// later loose directories still override earlier loose directories.
    ///
    /// # Errors
    ///
    /// Returns an error if a directory traversal entry cannot be read.
    #[cfg(any(feature = "bsa", feature = "zip"))]
    pub fn from_directories_with_archives(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path>>,
        archive_list: &[&str],
    ) -> std::io::Result<Self> {
        let mut loose_lookup = AHashMap::new();
        let mut directory_entries = Vec::new();

        for dir in search_dirs {
            let dir_entries = Self::directory_providers(dir)?;
            loose_lookup.extend(
                dir_entries
                    .iter()
                    .map(|(key, provider)| (key.clone(), provider.file.clone())),
            );
            directory_entries.push(dir_entries);
        }

        let mut mutable = Self::new();
        let archive_handles = archives::from_set(&loose_lookup, archive_list);
        for archive in &archive_handles {
            let source = SourceMeta {
                path: archive.path().to_path_buf(),
                kind: SourceKind::Archive,
            };
            for (key, file) in archives::file_map(&vec![std::sync::Arc::clone(archive)]) {
                mutable.push_provider(
                    key,
                    VfsProvider {
                        source: source.clone(),
                        file,
                    },
                );
            }
        }

        for entries in directory_entries {
            for (key, provider) in entries {
                mutable.push_provider(key, provider);
            }
        }

        Ok(mutable)
    }

    /// Insert every file from one archive as a higher-priority provider.
    ///
    /// This is useful for manual provider stacks. For `OpenMW`-style priority, prefer
    /// [`Self::from_directories_with_archives`], which puts archive providers below loose files.
    ///
    /// Returns `false` and leaves the VFS unchanged when `archive_path` cannot be opened as a
    /// supported archive.
    #[cfg(any(feature = "bsa", feature = "zip"))]
    pub fn push_archive<P: AsRef<Path>>(&mut self, archive_path: P) -> bool {
        let Some(archive) = archives::open_archive(archive_path.as_ref()) else {
            return false;
        };

        let source = SourceMeta {
            path: archive.path().to_path_buf(),
            kind: SourceKind::Archive,
        };
        for (key, file) in archives::file_map(&vec![archive]) {
            self.push_provider(
                key,
                VfsProvider {
                    source: source.clone(),
                    file,
                },
            );
        }
        true
    }

    /// Insert a provider at the highest priority for `key`.
    ///
    /// Returns `false` and leaves the VFS unchanged when `key` is not a safe relative VFS path.
    pub fn push_provider<P: AsRef<Path>>(&mut self, key: P, provider: VfsProvider) -> bool {
        let Some(key) = normalized_safe_key(key.as_ref()) else {
            return false;
        };
        self.providers.entry(key).or_default().push(provider);
        true
    }

    /// Insert every loose file under `root` as a higher-priority provider.
    ///
    /// # Errors
    ///
    /// Returns an error if directory traversal fails.
    pub fn push_directory<P: AsRef<Path>>(&mut self, root: P) -> std::io::Result<()> {
        for (key, provider) in Self::directory_providers(root)? {
            self.push_provider(key, provider);
        }

        Ok(())
    }

    fn directory_providers<P: AsRef<Path>>(
        root: P,
    ) -> std::io::Result<Vec<(PathBuf, VfsProvider)>> {
        let root = root.as_ref();
        let source = SourceMeta {
            path: root.to_path_buf(),
            kind: SourceKind::LooseDir,
        };
        let mut providers = Vec::new();

        for entry in WalkDir::new(root).follow_links(true) {
            let entry = entry.map_err(std::io::Error::other)?;
            if !entry.file_type().is_file() {
                continue;
            }

            let key = entry
                .path()
                .strip_prefix(root)
                .map_or_else(|_| entry.path().to_path_buf(), PathBuf::from);
            let Some(key) = normalized_safe_key(&key) else {
                eprintln!(
                    "vfstool: skipping unsafe VFS path '{}' from {}",
                    key.display(),
                    entry.path().display()
                );
                continue;
            };

            providers.push((
                key,
                VfsProvider {
                    source: source.clone(),
                    file: VfsFile::from(entry.path()),
                },
            ));
        }

        Ok(providers)
    }

    /// Materialize current winners into a normal [`VFS`].
    #[must_use]
    pub fn to_vfs(&self) -> VFS {
        let mut vfs = VFS::new();
        for (key, providers) in &self.providers {
            if let Some(provider) = providers.last() {
                vfs.insert_file(key, provider.file.clone());
            }
        }
        vfs
    }

    /// Return providers for `key`, ordered low-to-high priority.
    #[must_use]
    pub fn providers_for<P: AsRef<Path>>(&self, key: P) -> Option<&[VfsProvider]> {
        let key = normalize_path(key.as_ref()).into_owned();
        self.providers.get(&key).map(Vec::as_slice)
    }

    /// Remove the current winner for `key`, revealing the next lower-priority provider if present.
    pub fn remove_winner<P: AsRef<Path>>(&mut self, key: P) -> Option<VfsProvider> {
        let key = normalize_path(key.as_ref()).into_owned();
        let providers = self.providers.get_mut(&key)?;
        let removed = providers.pop();
        if providers.is_empty() {
            self.providers.remove(&key);
        }
        removed
    }

    /// Remove all providers for `key` whose source path matches `source`.
    pub fn remove_provider<P: AsRef<Path>>(&mut self, key: P, source: &Path) -> Vec<VfsProvider> {
        let key = normalize_path(key.as_ref()).into_owned();
        let Some(providers) = self.providers.get_mut(&key) else {
            return Vec::new();
        };

        let mut removed = Vec::new();
        let mut i = 0;
        while i < providers.len() {
            if providers[i].source.path == source {
                removed.push(providers.remove(i));
            } else {
                i += 1;
            }
        }
        if providers.is_empty() {
            self.providers.remove(&key);
        }
        removed
    }

    /// Remove every provider from `source`.
    pub fn remove_source(&mut self, source: &Path) -> Vec<(PathBuf, VfsProvider)> {
        self.remove_matching_provider(|_, provider| provider.source.path == source)
    }

    /// Remove providers under `prefix` regardless of source.
    pub fn remove_prefix<P: AsRef<Path>>(&mut self, prefix: P) -> Vec<(PathBuf, VfsProvider)> {
        let prefix = normalize_path(prefix.as_ref()).into_owned();
        self.remove_matching_provider(|key, _| key.starts_with(&prefix))
    }

    /// Remove providers accepted by `matcher`.
    pub fn remove_matching_provider(
        &mut self,
        mut matcher: impl FnMut(&Path, &VfsProvider) -> bool,
    ) -> Vec<(PathBuf, VfsProvider)> {
        let keys = self.providers.keys().cloned().collect::<Vec<_>>();
        let mut removed = Vec::new();

        for key in keys {
            let Some(providers) = self.providers.get_mut(&key) else {
                continue;
            };
            let mut i = 0;
            while i < providers.len() {
                if matcher(&key, &providers[i]) {
                    removed.push((key.clone(), providers.remove(i)));
                } else {
                    i += 1;
                }
            }
            if providers.is_empty() {
                self.providers.remove(&key);
            }
        }

        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(feature = "zip")]
    use std::io::Write;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "{name}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn new_in_current_dir(name: &str) -> Self {
            let dir = std::env::current_dir().unwrap().join(format!(
                "{name}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
            let target = self.0.join(rel);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, data).unwrap();
            target
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn remove_winner_reveals_lower_priority_provider() {
        let low = TempDir::new("mutable_vfs_low");
        let high = TempDir::new("mutable_vfs_high");
        let low_file = low.write("textures/foo.dds", b"low");
        let high_file = high.write("textures/foo.dds", b"high");
        let mut mutable = MutableVfs::from_directories([low.path(), high.path()]).unwrap();

        assert_eq!(
            mutable
                .to_vfs()
                .get_file("textures/foo.dds")
                .unwrap()
                .path(),
            high_file
        );
        let removed = mutable
            .remove_winner("Textures\\Foo.dds")
            .expect("winner should be removed");

        assert_eq!(removed.file.path(), high_file);
        assert_eq!(
            mutable
                .to_vfs()
                .get_file("textures/foo.dds")
                .unwrap()
                .path(),
            low_file
        );
    }

    #[test]
    fn remove_source_reveals_remaining_source() {
        let low = TempDir::new("mutable_vfs_remove_source_low");
        let high = TempDir::new("mutable_vfs_remove_source_high");
        let low_file = low.write("shared.txt", b"low");
        high.write("shared.txt", b"high");
        high.write("only_high.txt", b"high");
        let mut mutable = MutableVfs::from_directories([low.path(), high.path()]).unwrap();

        let removed = mutable.remove_source(high.path());

        assert_eq!(removed.len(), 2);
        let vfs = mutable.to_vfs();
        assert_eq!(vfs.get_file("shared.txt").unwrap().path(), low_file);
        assert!(vfs.get_file("only_high.txt").is_none());
    }

    #[test]
    fn remove_prefix_removes_providers_under_prefix() {
        let data = TempDir::new("mutable_vfs_remove_prefix");
        data.write("textures/foo.dds", b"tex");
        data.write("meshes/foo.nif", b"mesh");
        let mut mutable = MutableVfs::from_directories([data.path()]).unwrap();

        let removed = mutable.remove_prefix("Textures");

        assert_eq!(removed.len(), 1);
        let vfs = mutable.to_vfs();
        assert!(vfs.get_file("textures/foo.dds").is_none());
        assert!(vfs.get_file("meshes/foo.nif").is_some());
    }

    #[test]
    fn remove_source_uses_lexical_source_paths() {
        let data = TempDir::new_in_current_dir("mutable_vfs_lexical_source");
        data.write("file.txt", b"data");
        let relative = PathBuf::from(data.path().file_name().unwrap());
        let mut mutable = MutableVfs::from_directories([relative.as_path()]).unwrap();

        assert!(mutable.remove_source(data.path()).is_empty());
        assert!(mutable.to_vfs().get_file("file.txt").is_some());
        assert_eq!(mutable.remove_source(&relative).len(), 1);
    }

    #[test]
    fn push_provider_rejects_unsafe_keys() {
        let data = TempDir::new("mutable_vfs_unsafe_provider");
        let file = data.write("source.txt", b"data");
        let mut mutable = MutableVfs::new();

        let inserted = mutable.push_provider(
            "../escape.txt",
            VfsProvider {
                source: SourceMeta {
                    path: data.path().to_path_buf(),
                    kind: SourceKind::LooseDir,
                },
                file: VfsFile::from(file),
            },
        );

        assert!(!inserted);
        assert!(mutable.providers_for("../escape.txt").is_none());
        assert_eq!(mutable.to_vfs().iter().count(), 0);
    }

    #[test]
    #[cfg(unix)]
    fn push_directory_skips_filenames_that_normalize_to_unsafe_keys() {
        let data = TempDir::new("mutable_vfs_scan_unsafe_keys");
        data.write("..\\outside.txt", b"escape");
        data.write("safe.txt", b"safe");

        let mutable = MutableVfs::from_directories([data.path()]).unwrap();

        assert!(mutable.providers_for("safe.txt").is_some());
        assert!(mutable.providers_for("../outside.txt").is_none());
        assert_eq!(mutable.to_vfs().iter().count(), 1);
    }

    #[test]
    fn remove_middle_provider_preserves_current_winner() {
        let low = TempDir::new("mutable_vfs_middle_low");
        let middle = TempDir::new("mutable_vfs_middle_mid");
        let high = TempDir::new("mutable_vfs_middle_high");
        low.write("shared.txt", b"low");
        middle.write("shared.txt", b"middle");
        let high_file = high.write("shared.txt", b"high");
        let mut mutable =
            MutableVfs::from_directories([low.path(), middle.path(), high.path()]).unwrap();

        let removed = mutable.remove_provider("shared.txt", middle.path());

        assert_eq!(removed.len(), 1);
        assert_eq!(
            mutable.to_vfs().get_file("shared.txt").unwrap().path(),
            high_file
        );
        assert_eq!(mutable.providers_for("shared.txt").unwrap().len(), 2);
    }

    #[test]
    #[cfg(feature = "zip")]
    fn archives_are_lower_priority_than_loose_files() {
        let data = TempDir::new("mutable_vfs_archives_lower_priority");
        let archive_path = data.path().join("base.zip");
        let archive_file = fs::File::create(&archive_path).unwrap();
        let mut writer = zip::ZipWriter::new(archive_file);
        writer
            .start_file("textures/foo.dds", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"archive").unwrap();
        writer.finish().unwrap();
        let loose_file = data.write("textures/foo.dds", b"loose");

        let mut mutable =
            MutableVfs::from_directories_with_archives([data.path()], &["base.zip"]).unwrap();

        let providers = mutable.providers_for("textures/foo.dds").unwrap();
        assert_eq!(providers.len(), 2);
        assert_eq!(providers[0].source.kind, SourceKind::Archive);
        assert_eq!(providers[1].source.kind, SourceKind::LooseDir);
        assert_eq!(
            mutable
                .to_vfs()
                .get_file("textures/foo.dds")
                .unwrap()
                .path(),
            loose_file
        );

        let removed = mutable.remove_winner("textures/foo.dds").unwrap();
        assert_eq!(removed.source.kind, SourceKind::LooseDir);
        assert!(
            mutable
                .to_vfs()
                .get_file("textures/foo.dds")
                .unwrap()
                .is_archive()
        );
    }
}
