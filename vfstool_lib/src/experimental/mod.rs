// SPDX-License-Identifier: GPL-3.0-only
//! Experimental policy/solver helpers.
//!
//! These modules are public so the workspace can compose and test them, but
//! they are not promoted as stable 1.0 API.

/// Conflict fingerprint knowledge base types and storage.
pub mod kb {
    pub use crate::kb::*;
}

/// Declarative policy rules and evaluation against VFS/layer state.
pub mod policy {
    pub use crate::policy::*;
}

/// Constraint-based load-order solving.
pub mod solve {
    pub use crate::solve::*;
}
