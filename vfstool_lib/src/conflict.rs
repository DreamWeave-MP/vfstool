// SPDX-License-Identifier: MIT OR Apache-2.0
#[cfg(any(feature = "bsa", feature = "zip"))]
mod archives;
mod index;
mod report_methods;

pub use self::index::{ConflictIndex, SourceConflicts};
#[cfg(test)]
#[path = "conflict/tests.rs"]
mod tests;
