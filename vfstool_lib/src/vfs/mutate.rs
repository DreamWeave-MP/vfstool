// SPDX-License-Identifier: GPL-3.0-only
use super::{ProviderEntry, VFS, VfsProvider};
#[cfg(any(feature = "beth-archives", feature = "zip"))]
use crate::archives;
use crate::{
    NormalizedPath, SourceKind, SourceMeta, VfsFile, VfsKeyInput, path_glob_matches,
    paths::{
        key_is_at_or_under_prefix, key_to_path_buf_lossy, normalized_safe_key,
        normalized_safe_normalized_bytes,
    },
};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

impl VFS {
    /// Replace the provider stack for `key` with a single resolved winner.
    ///
    /// This is intentionally winner-only mutation: it discards lower-priority providers for `key`.
    /// Use [`Self::push_provider`] to add a provider without discarding the stack.
    pub fn set_winner_file<K: VfsKeyInput + ?Sized>(
        &mut self,
        key: &K,
        file: VfsFile,
    ) -> Option<VfsFile> {
        let normalized = key.to_safe_vfs_key()?;
        let previous = self.file_map.get(&normalized).cloned();
        let source = provider_source(&file);
        let source_index = self.push_source(source.clone());
        self.providers.insert(
            normalized.clone(),
            vec![ProviderEntry {
                source_index,
                provider: VfsProvider { source, file },
            }],
        );
        self.file_map.insert(
            normalized.clone(),
            self.providers[&normalized][0].provider.file.clone(),
        );
        self.rebuild_layer_index();
        previous
    }

    /// Replace the provider stack for `key` with one loose-file winner.
    pub fn set_winner_loose_file<K: VfsKeyInput + ?Sized, P: AsRef<Path>>(
        &mut self,
        key: &K,
        physical_path: P,
    ) -> Option<VfsFile> {
        self.set_winner_file(key, VfsFile::from(physical_path))
    }

    /// Insert a provider at highest priority for `key`.
    ///
    /// Returns `false` and leaves the VFS unchanged when `key` is not a safe relative VFS key.
    /// Each call creates one source identity for reporting. Use [`Self::push_directory`] or
    /// [`Self::from_directories`] when multiple files should be grouped under one source.
    pub fn push_provider<K: VfsKeyInput + ?Sized>(
        &mut self,
        key: &K,
        provider: VfsProvider,
    ) -> bool {
        let Some(key) = key.to_safe_vfs_key() else {
            return false;
        };
        let source_index = self.push_source(provider.source.clone());
        self.providers
            .entry(key.clone())
            .or_default()
            .push(ProviderEntry {
                source_index,
                provider,
            });
        self.refresh_winner(&key);
        self.rebuild_layer_index();
        true
    }

    /// Insert a batch of providers that share one source identity.
    ///
    /// This is the bulk form of [`Self::push_provider`]: all entries are inserted first, winners are
    /// refreshed for touched keys, and [`LayerIndex`](crate::LayerIndex) is rebuilt once. Rebuilding
    /// per file is not a feature, it is just a tiny denial-of-service attack against your own CPU.
    #[must_use]
    pub fn push_provider_batch(
        &mut self,
        source: &SourceMeta,
        entries: impl IntoIterator<Item = (NormalizedPath, VfsFile)>,
    ) -> usize {
        let entries = entries
            .into_iter()
            .filter(|(key, _)| normalized_safe_normalized_bytes(key.as_bytes()))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return 0;
        }

        let source_index = self.push_source(source.clone());
        let mut touched = Vec::new();
        let mut inserted = 0;
        for (key, file) in entries {
            self.providers
                .entry(key.clone())
                .or_default()
                .push(ProviderEntry {
                    source_index,
                    provider: VfsProvider {
                        source: source.clone(),
                        file,
                    },
                });
            touched.push(key);
            inserted += 1;
        }
        for key in &touched {
            self.refresh_winner(key);
        }
        if inserted > 0 {
            self.rebuild_layer_index();
        }
        inserted
    }

    /// Insert every loose file under `root` as higher-priority providers.
    ///
    /// # Errors
    ///
    /// Returns an error if directory traversal fails.
    pub fn push_directory<P: AsRef<Path>>(&mut self, root: P) -> std::io::Result<()> {
        let entries = Self::directory_providers(root)?;
        let Some(source) = entries.first().map(|(_, provider)| provider.source.clone()) else {
            return Ok(());
        };
        let _ = self.push_provider_batch(
            &source,
            entries
                .into_iter()
                .map(|(key, provider)| (key, provider.file)),
        );
        Ok(())
    }

    /// Insert every file from one archive as a higher-priority provider.
    ///
    /// For OpenMW-style construction, prefer [`Self::from_directories`], which inserts archives
    /// below all loose providers. This method is manual stack mutation and therefore pushes the
    /// archive on top.
    #[cfg(any(feature = "beth-archives", feature = "zip"))]
    pub fn push_archive<P: AsRef<Path>>(&mut self, archive_path: P) -> bool {
        let Some(archive) = archives::open_archive(archive_path.as_ref()) else {
            return false;
        };

        let source = SourceMeta {
            path: archive.path().to_path_buf(),
            kind: SourceKind::Archive,
        };
        let _ = self.push_provider_batch(&source, archives::file_entries(&vec![archive]));
        true
    }

    pub(crate) fn directory_providers<P: AsRef<Path>>(
        root: P,
    ) -> std::io::Result<Vec<(NormalizedPath, VfsProvider)>> {
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

    /// Return providers for `key`, ordered low-to-high priority.
    #[must_use]
    pub fn providers_for<K: VfsKeyInput + ?Sized>(
        &self,
        key: &K,
    ) -> Option<impl ExactSizeIterator + DoubleEndedIterator<Item = &VfsProvider>> {
        let key = key.to_vfs_key();
        self.providers
            .get(&key)
            .map(|providers| providers.iter().map(|entry| &entry.provider))
    }

    /// Remove the current winner for `key`, revealing the next lower-priority provider if present.
    pub fn remove_winner<K: VfsKeyInput + ?Sized>(&mut self, key: &K) -> Option<VfsProvider> {
        let key = key.to_vfs_key();
        let providers = self.providers.get_mut(&key)?;
        let removed = providers.pop().map(|entry| entry.provider);
        self.refresh_winner(&key);
        self.rebuild_layer_index();
        removed
    }

    /// Remove the resolved key entirely, discarding all lower-priority providers for it.
    pub fn remove_resolved_file<K: VfsKeyInput + ?Sized>(&mut self, key: &K) -> Option<VfsFile> {
        let key = key.to_vfs_key();
        self.providers.remove(&key);
        let removed = self.file_map.remove(&key);
        if removed.is_some() {
            self.rebuild_layer_index();
        }
        removed
    }

    /// Remove all providers for `key` whose source path matches `source` lexically.
    pub fn remove_provider<K: VfsKeyInput + ?Sized>(
        &mut self,
        key: &K,
        source: &Path,
    ) -> Vec<VfsProvider> {
        let key = key.to_vfs_key();
        let Some(providers) = self.providers.get_mut(&key) else {
            return Vec::new();
        };

        let mut removed = Vec::new();
        let mut i = 0;
        while i < providers.len() {
            if providers[i].provider.source.path == source {
                removed.push(providers.remove(i).provider);
            } else {
                i += 1;
            }
        }
        self.refresh_winner(&key);
        self.rebuild_layer_index();
        removed
    }

    /// Remove every provider from `source`.
    pub fn remove_source(&mut self, source: &Path) -> Vec<(NormalizedPath, VfsProvider)> {
        self.remove_matching_provider(|_, provider| provider.source.path == source)
    }

    /// Remove every provider under `prefix`, revealing lower-priority providers where available.
    pub fn remove_provider_prefix<K: VfsKeyInput + ?Sized>(
        &mut self,
        prefix: &K,
    ) -> Vec<(NormalizedPath, VfsProvider)> {
        let prefix = prefix.to_vfs_key();
        self.remove_matching_provider(|key, _| key_is_at_or_under_prefix(key, &prefix))
    }

    /// Remove resolved winners under `prefix`, discarding all lower-priority providers too.
    pub fn remove_resolved_prefix<K: VfsKeyInput + ?Sized>(
        &mut self,
        prefix: &K,
    ) -> Vec<(NormalizedPath, VfsFile)> {
        let prefix = prefix.to_vfs_key();
        let keys = self
            .providers
            .keys()
            .filter(|key| key_is_at_or_under_prefix(key, &prefix))
            .cloned()
            .collect::<Vec<_>>();
        self.remove_resolved_keys(keys)
    }

    /// Remove providers accepted by `matcher`, revealing lower-priority providers where available.
    pub fn remove_matching_provider(
        &mut self,
        mut matcher: impl FnMut(&NormalizedPath, &VfsProvider) -> bool,
    ) -> Vec<(NormalizedPath, VfsProvider)> {
        let keys = self.providers.keys().cloned().collect::<Vec<_>>();
        let mut removed = Vec::new();

        for key in keys {
            let Some(providers) = self.providers.get_mut(&key) else {
                continue;
            };
            let mut i = 0;
            while i < providers.len() {
                if matcher(&key, &providers[i].provider) {
                    removed.push((key.clone(), providers.remove(i).provider));
                } else {
                    i += 1;
                }
            }
            self.refresh_winner(&key);
        }

        self.rebuild_layer_index();
        removed
    }

    /// Remove resolved winners whose normalized key matches `glob`.
    pub fn remove_resolved_matching_glob(&mut self, glob: &str) -> Vec<(NormalizedPath, VfsFile)> {
        let keys = self
            .file_map
            .keys()
            .filter(|key| path_glob_matches(glob, &key_to_path_buf_lossy(key)))
            .cloned()
            .collect::<Vec<_>>();

        self.remove_resolved_keys(keys)
    }

    fn remove_resolved_keys(
        &mut self,
        keys: impl IntoIterator<Item = NormalizedPath>,
    ) -> Vec<(NormalizedPath, VfsFile)> {
        let mut removed = Vec::new();
        for key in keys {
            self.providers.remove(&key);
            if let Some(file) = self.file_map.remove(&key) {
                removed.push((key, file));
            }
        }
        if !removed.is_empty() {
            self.rebuild_layer_index();
        }
        removed
    }
}

fn provider_source(file: &VfsFile) -> SourceMeta {
    if file.is_archive() {
        SourceMeta {
            path: PathBuf::from(file.parent_archive_path().unwrap_or_default()),
            kind: SourceKind::Archive,
        }
    } else {
        let source_path = file
            .path()
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf);
        SourceMeta {
            path: source_path,
            kind: SourceKind::LooseDir,
        }
    }
}
