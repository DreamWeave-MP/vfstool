// SPDX-License-Identifier: GPL-3.0-only
use super::{LayerIndex, SourceKind};
use crate::{
    ContentDigest, NormalizedKey, NormalizedPath, VFS, VfsFile, VfsKeyInput, normalize_host_path,
    semantic::ArchiveHashMode,
};
use ahash::AHashMap;
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

#[cfg(any(feature = "beth-archives", feature = "zip"))]
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(super) struct ContentFingerprint {
    pub(super) digest: [u8; 32],
    pub(super) size: u64,
}

impl ContentFingerprint {
    pub(super) fn to_digest(&self) -> ContentDigest {
        ContentDigest::blake3(self.digest, self.size)
    }
}

pub(super) struct ProviderIoCache {
    fingerprints: AHashMap<(usize, NormalizedPath), Option<ContentFingerprint>>,
    bytes: AHashMap<(usize, NormalizedPath), Option<Vec<u8>>>,
    #[cfg(any(feature = "beth-archives", feature = "zip"))]
    archive_files: SharedArchiveFileCache,
}

#[cfg(any(feature = "beth-archives", feature = "zip"))]
pub(super) type SharedArchiveFileCache =
    Arc<Mutex<AHashMap<PathBuf, Option<AHashMap<NormalizedPath, VfsFile>>>>>;

impl ProviderIoCache {
    pub(super) fn new() -> Self {
        Self {
            fingerprints: AHashMap::new(),
            bytes: AHashMap::new(),
            #[cfg(any(feature = "beth-archives", feature = "zip"))]
            archive_files: Self::new_shared_archive_file_cache(),
        }
    }

    #[cfg(any(feature = "beth-archives", feature = "zip"))]
    pub(super) fn with_shared_archive_file_cache(archive_files: SharedArchiveFileCache) -> Self {
        Self {
            fingerprints: AHashMap::new(),
            bytes: AHashMap::new(),
            archive_files,
        }
    }

    #[cfg(any(feature = "beth-archives", feature = "zip"))]
    pub(super) fn new_shared_archive_file_cache() -> SharedArchiveFileCache {
        Arc::new(Mutex::new(AHashMap::new()))
    }
}

impl LayerIndex {
    pub(super) fn fingerprint_for_provider(
        &self,
        vfs: &VFS,
        source_idx: usize,
        key: &(impl VfsKeyInput + ?Sized),
        cache: &mut ProviderIoCache,
        archive_hash_mode: ArchiveHashMode,
    ) -> io::Result<Option<ContentFingerprint>> {
        let key = key.to_vfs_key();
        let cache_key = (source_idx, key.clone());
        if let Some(hit) = cache.fingerprints.get(&cache_key) {
            return Ok(hit.clone());
        }

        let src = &self.sources[source_idx];
        let fp = match src.kind {
            SourceKind::LooseDir => {
                let path = self.provider_path(source_idx, &key);
                if path.exists() {
                    Some(hash_reader(std::fs::File::open(path)?)?)
                } else {
                    None
                }
            }
            SourceKind::Archive => match archive_hash_mode {
                ArchiveHashMode::Disabled => None,
                ArchiveHashMode::WinnerOnly => match vfs.get_file(&key) {
                    Some(current_winner) => match current_winner.parent_archive_path() {
                        Some(parent) if archive_parent_matches(&parent, &src.path) => {
                            Some(hash_reader(current_winner.open()?)?)
                        }
                        _ => None,
                    },
                    None => None,
                },
                ArchiveHashMode::AllProviders => archive_provider_file(&src.path, &key, cache)
                    .map(|file| file.open().and_then(hash_reader))
                    .transpose()?,
            },
        };

        cache.fingerprints.insert(cache_key, fp.clone());
        Ok(fp)
    }

    pub(super) fn read_provider_bytes(
        &self,
        vfs: &VFS,
        source_idx: usize,
        key: &(impl VfsKeyInput + ?Sized),
        cache: &mut ProviderIoCache,
    ) -> io::Result<Option<Vec<u8>>> {
        let key = key.to_vfs_key();
        let cache_key = (source_idx, key.clone());
        if let Some(hit) = cache.bytes.get(&cache_key) {
            return Ok(hit.clone());
        }

        let src = &self.sources[source_idx];
        let mut out = Vec::new();

        let bytes = match src.kind {
            SourceKind::LooseDir => {
                let path = self.provider_path(source_idx, &key);
                if path.exists() {
                    let mut file = std::fs::File::open(path)?;
                    file.read_to_end(&mut out)?;
                    Some(out)
                } else {
                    None
                }
            }
            SourceKind::Archive => {
                if let Some(file) = archive_provider_file(&src.path, &key, cache) {
                    let mut reader = file.open()?;
                    reader.read_to_end(&mut out)?;
                    Some(out)
                } else {
                    let Some(winner) = vfs.get_file(&key) else {
                        cache.bytes.insert(cache_key, None);
                        return Ok(None);
                    };
                    let Some(parent) = winner.parent_archive_path() else {
                        cache.bytes.insert(cache_key, None);
                        return Ok(None);
                    };
                    if !archive_parent_matches(&parent, &src.path) {
                        cache.bytes.insert(cache_key, None);
                        return Ok(None);
                    }

                    let mut reader = winner.open()?;
                    reader.read_to_end(&mut out)?;
                    Some(out)
                }
            }
        };

        cache.bytes.insert(cache_key, bytes.clone());
        Ok(bytes)
    }

    pub(super) fn provider_path(&self, source_idx: usize, key: &NormalizedPath) -> PathBuf {
        let normalized = NormalizedKey::from(key.clone());
        self.provider_paths
            .get(&(source_idx, normalized))
            .map_or_else(
                || {
                    self.sources[source_idx]
                        .path
                        .join(crate::paths::key_to_path_buf_lossy(key))
                },
                |rel| self.sources[source_idx].path.join(rel),
            )
    }
}

fn archive_parent_matches(parent: &str, source_path: &Path) -> bool {
    normalize_host_path(Path::new(parent)).as_ref() == normalize_host_path(source_path).as_ref()
}

#[cfg(any(feature = "beth-archives", feature = "zip"))]
fn archive_provider_file(
    source_path: &Path,
    key: &NormalizedPath,
    cache: &mut ProviderIoCache,
) -> Option<VfsFile> {
    let normalized_source = normalize_host_path(source_path).into_owned();
    if let Some(hit) = cache
        .archive_files
        .lock()
        .ok()?
        .get(&normalized_source)
        .cloned()
    {
        return hit.as_ref()?.get(key).cloned();
    }

    {
        let files = crate::archives::open_archive(source_path).map(|archive| {
            let archive_list = vec![archive];
            crate::archives::file_map(&archive_list)
        });
        cache
            .archive_files
            .lock()
            .ok()?
            .insert(normalized_source.clone(), files);
    }
    cache
        .archive_files
        .lock()
        .ok()?
        .get(&normalized_source)?
        .clone()
        .as_ref()?
        .get(key)
        .cloned()
}

#[cfg(not(any(feature = "beth-archives", feature = "zip")))]
fn archive_provider_file(
    _source_path: &Path,
    _key: &NormalizedPath,
    _cache: &mut ProviderIoCache,
) -> Option<VfsFile> {
    None
}

pub(super) fn fingerprint_bytes(bytes: &[u8]) -> ContentFingerprint {
    let digest = blake3::hash(bytes);
    ContentFingerprint {
        digest: *digest.as_bytes(),
        size: bytes.len() as u64,
    }
}

pub(super) fn hash_reader(mut reader: impl Read) -> io::Result<ContentFingerprint> {
    let mut hasher = blake3::Hasher::new();
    let mut size = 0u64;
    let mut buf = vec![0u8; 65536];

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }

    Ok(ContentFingerprint {
        digest: *hasher.finalize().as_bytes(),
        size,
    })
}
