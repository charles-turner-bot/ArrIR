//! ArrIR: a portable intermediate representation for array operation plans.
//!
//! The replacement IR is being built in small steps (`plans/design.md`, roadmap). For
//! now this crate holds only the [`legacy`] scaffold. The shared value types, such as
//! the symbol IDs, are in the `arrir-types` crate, re-exported here as [`types`].

pub mod legacy;

pub use arrir_types as types;
