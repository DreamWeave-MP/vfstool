// SPDX-License-Identifier: MIT OR Apache-2.0
use super::{ArchiveHashMode, LayerIndex, SourceKind};
use crate::{ContentDigest, NormalizedKey, VFS, VfsFile, normalize_path};
use ahash::AHashMap;
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
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

impl LayerIndex {
    pub(super) fn fingerprint_for_provider(
        &self,
        vfs: &VFS,
        source_idx: usize,
        key: &Path,
        cache: &mut AHashMap<(usize, PathBuf), Option<ContentFingerprint>>,
        archive_hash_mode: ArchiveHashMode,
    ) -> io::Result<Option<ContentFingerprint>> {
        let cache_key = (source_idx, key.to_path_buf());
        if let Some(hit) = cache.get(&cache_key) {
            return Ok(hit.clone());
        }

        let src = &self.sources[source_idx];
        let fp = match src.kind {
            SourceKind::LooseDir => {
                let path = self.provider_path(source_idx, key);
                if path.exists() {
                    Some(hash_reader(std::fs::File::open(path)?)?)
                } else {
                    None
                }
            }
            SourceKind::Archive => match archive_hash_mode {
                ArchiveHashMode::Disabled => None,
                ArchiveHashMode::WinnerOnly => match vfs.get_file(key) {
                    Some(current_winner) => match current_winner.parent_archive_path() {
                        Some(parent) if archive_parent_matches(&parent, &src.path) => {
                            Some(hash_reader(current_winner.open()?)?)
                        }
                        _ => None,
                    },
                    None => None,
                },
                ArchiveHashMode::AllProviders => archive_provider_file(&src.path, key)
                    .and_then(|file| file.open().ok().and_then(|reader| hash_reader(reader).ok())),
            },
        };

        cache.insert(cache_key, fp.clone());
        Ok(fp)
    }

    pub(super) fn read_provider_bytes(
        &self,
        vfs: &VFS,
        source_idx: usize,
        key: &Path,
    ) -> io::Result<Option<Vec<u8>>> {
        let src = &self.sources[source_idx];
        let mut out = Vec::new();

        match src.kind {
            SourceKind::LooseDir => {
                let path = self.provider_path(source_idx, key);
                if !path.exists() {
                    return Ok(None);
                }
                let mut file = std::fs::File::open(path)?;
                file.read_to_end(&mut out)?;
                Ok(Some(out))
            }
            SourceKind::Archive => {
                if let Some(file) = archive_provider_file(&src.path, key) {
                    let mut reader = file.open()?;
                    reader.read_to_end(&mut out)?;
                    return Ok(Some(out));
                }

                let Some(winner) = vfs.get_file(key) else {
                    return Ok(None);
                };
                let Some(parent) = winner.parent_archive_path() else {
                    return Ok(None);
                };
                if !archive_parent_matches(&parent, &src.path) {
                    return Ok(None);
                }

                let mut reader = winner.open()?;
                reader.read_to_end(&mut out)?;
                Ok(Some(out))
            }
        }
    }

    pub(super) fn provider_path(&self, source_idx: usize, key: &Path) -> PathBuf {
        let normalized = NormalizedKey::new(key);
        self.provider_paths
            .get(&(source_idx, normalized))
            .map_or_else(
                || self.sources[source_idx].path.join(key),
                |rel| self.sources[source_idx].path.join(rel),
            )
    }
}

fn archive_parent_matches(parent: &str, source_path: &Path) -> bool {
    normalize_path(Path::new(parent)).as_ref() == normalize_path(source_path).as_ref()
}

#[cfg(any(feature = "bsa", feature = "zip"))]
fn archive_provider_file(source_path: &Path, key: &Path) -> Option<VfsFile> {
    let archive = crate::archives::open_archive(source_path)?;
    let archive_list = vec![archive];
    let file_map = crate::archives::file_map(&archive_list);
    file_map.get(&normalize_path(key).into_owned()).cloned()
}

#[cfg(not(any(feature = "bsa", feature = "zip")))]
fn archive_provider_file(_source_path: &Path, _key: &Path) -> Option<VfsFile> {
    None
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
