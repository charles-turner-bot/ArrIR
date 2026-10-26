//! The portable semantic core, built incrementally from xrexpr's lowered IR.
//!
//! This first step defines symbol identities only. It cannot yet describe a plan
//! or resolve an external array. The legacy root API is intentionally separate.

mod ids;

pub use ids::{BindingId, DimId, VarId};
