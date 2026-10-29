//! Shared value types for ArrIR.
//!
//! Every other ArrIR crate depends on this one, and it depends on none of them
//! (`plans/design.md`, dependency graph). For now it holds the portable symbol IDs.

mod ids;

pub use ids::{BindingId, DimId, GridId, VarId};
