// SPDX-License-Identifier: GPL-3.0-only
use super::{LayerIndex, LayerProvider, SourceKind};
use crate::{ContentDigest, NormalizedPath, VFS, VfsKeyInput, semantic::ArchiveHashMode};
use ahash::AHashMap;
use std::{
    io::{self, Read},
    path::PathBuf,
    sync::Arc,
};

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

type ProviderCacheKey = (usize, usize, NormalizedPath);
type CachedProviderBytes = Option<Arc<[u8]>>;

pub(super) struct ProviderIoCache {
    fingerprints: AHashMap<ProviderCacheKey, Option<ContentFingerprint>>,
    bytes: AHashMap<ProviderCacheKey, CachedProviderBytes>,
}

impl ProviderIoCache {
    pub(super) fn new() -> Self {
        Self {
            fingerprints: AHashMap::new(),
            bytes: AHashMap::new(),
        }
    }
}

impl LayerIndex {
    pub(super) fn fingerprint_for_provider(
        &self,
        vfs: &VFS,
        provider: &LayerProvider,
        cache: &mut ProviderIoCache,
        archive_hash_mode: ArchiveHashMode,
    ) -> io::Result<Option<ContentFingerprint>> {
        let key = provider.key.to_vfs_key();
        let cache_key = (provider.source_index, provider.provider_index, key.clone());
        if let Some(hit) = cache.fingerprints.get(&cache_key) {
            return Ok(hit.clone());
        }

        let src = &self.sources[provider.source_index];
        let fp = match src.kind {
            SourceKind::LooseDir => {
                if let Some(file) = vfs.provider_file_for_key_index(&key, provider.provider_index) {
                    if file.path().exists() {
                        Some(hash_reader(file.open()?)?)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            SourceKind::Archive => match archive_hash_mode {
                ArchiveHashMode::Disabled => None,
                ArchiveHashMode::WinnerOnly
                    if vfs.winner_provider_index(&key) != Some(provider.provider_index) =>
                {
                    None
                }
                ArchiveHashMode::WinnerOnly | ArchiveHashMode::AllProviders => vfs
                    .provider_file_for_key_index(&key, provider.provider_index)
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
        provider: &LayerProvider,
        cache: &mut ProviderIoCache,
        archive_hash_mode: ArchiveHashMode,
    ) -> io::Result<Option<Arc<[u8]>>> {
        let key = provider.key.to_vfs_key();
        let cache_key = (provider.source_index, provider.provider_index, key.clone());
        if let Some(hit) = cache.bytes.get(&cache_key) {
            return Ok(hit.clone());
        }

        let src = &self.sources[provider.source_index];
        let mut out = Vec::new();

        let bytes = match src.kind {
            SourceKind::LooseDir => {
                if let Some(file) = vfs.provider_file_for_key_index(&key, provider.provider_index) {
                    if file.path().exists() {
                        let mut reader = file.open()?;
                        reader.read_to_end(&mut out)?;
                        Some(Arc::from(out.into_boxed_slice()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            SourceKind::Archive => match archive_hash_mode {
                ArchiveHashMode::Disabled => None,
                ArchiveHashMode::WinnerOnly
                    if vfs.winner_provider_index(&key) != Some(provider.provider_index) =>
                {
                    None
                }
                ArchiveHashMode::WinnerOnly | ArchiveHashMode::AllProviders => {
                    if let Some(file) =
                        vfs.provider_file_for_key_index(&key, provider.provider_index)
                    {
                        let mut reader = file.open()?;
                        reader.read_to_end(&mut out)?;
                        Some(Arc::from(out.into_boxed_slice()))
                    } else {
                        None
                    }
                }
            },
        };

        cache.bytes.insert(cache_key, bytes.clone());
        Ok(bytes)
    }

    pub(super) fn provider_path(&self, provider: &LayerProvider) -> PathBuf {
        self.sources[provider.source_index]
            .path
            .join(&provider.original_path)
    }
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
