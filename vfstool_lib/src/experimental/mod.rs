// SPDX-License-Identifier: MIT OR Apache-2.0
//! Experimental analyzers and policy/solver helpers.
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

/// Asset-class semantic analyzers and deltas.
pub mod semantic {
    pub use crate::semantic::*;
}

/// Constraint-based load-order solving.
pub mod solve {
    pub use crate::solve::*;
}
