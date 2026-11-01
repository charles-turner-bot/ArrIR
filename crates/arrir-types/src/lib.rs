//! Shared value types for ArrIR.
//!
//! Every other ArrIR crate depends on this one, and it depends on none of them
//! (`plans/design.md`, dependency graph). It holds the portable symbol IDs and the
//! logical schema types (`plans/schema-and-values.md`).

mod dtype;
mod extent;
mod ids;
mod schema;
#[cfg(test)]
mod schema_tests;

pub use dtype::{DType, TypeKnowledge};
pub use extent::Extent;
pub use ids::{BindingId, DimId, GridId, VarId};
pub use schema::{
    ArrayResult, CoordinateRole, IndexCoordinate, ResultKind, Schema, SchemaError, Variable,
    VariableAxis, VariableRole,
};
