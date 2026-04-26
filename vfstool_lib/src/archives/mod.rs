// SPDX-License-Identifier: GPL-3.0-only
//! Low-level archive loading and enumeration (BSA, BA2, ZIP, PK3).

mod enumerate;
mod keys;
mod open;
mod types;

pub use enumerate::{archive_paths, file_map};
#[cfg(test)]
pub(crate) use keys::normalized_archive_key;
pub use open::from_set;
pub(crate) use open::open_archive;
pub use types::{ArchiveList, StoredArchive, TypedArchive};
