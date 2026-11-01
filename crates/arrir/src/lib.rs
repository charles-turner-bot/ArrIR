//! ArrIR: a portable intermediate representation for array operation plans.
//!
//! The replacement IR is being built in small steps (`plans/design.md`, roadmap). So
//! far it has the plan container: an append-only DAG of nodes with one root
//! ([`Graph`], [`Plan`]). The old scaffold is in [`legacy`]. The shared value types,
//! such as the symbol IDs, are in the `arrir-types` crate.

pub mod legacy;
mod plan;
mod wire;

#[cfg(test)]
mod plan_tests;

pub use plan::{Arity, Graph, Node, NodeId, Operation, Plan, PlanError, RewriteError};
pub use wire::{ImportError, WIRE_VERSION};
