// SPDX-License-Identifier: GPL-3.0-only
#[cfg(any(feature = "beth-archives", feature = "zip"))]
mod archives;
mod diff;
mod index;
mod reports;

pub use self::index::{ConflictIndex, SourceConflicts};
#[cfg(test)]
#[path = "conflict/tests.rs"]
mod tests;
