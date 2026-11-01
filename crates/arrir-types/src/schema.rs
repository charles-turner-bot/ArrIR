use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DimId, Extent, TypeKnowledge, VarId};

/// The logical schema of one node's result (`plans/schema-and-values.md`).
///
/// This first slice holds structure and unknown facts. Missing values, spatial state
/// and grids follow in later PRs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    pub result: ResultKind,
    /// In the order written. Order is part of the schema and is never sorted.
    pub variables: Vec<Variable>,
    /// One extent per named dimension, in order of first appearance (see [`Schema::validate`]).
    pub sizes: Vec<(DimId, Extent)>,
}

/// Whether the node produces a dataset or a single array.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResultKind {
    /// Several data variables, or none.
    Dataset,
    /// One payload variable; every other variable is a coordinate.
    Array(ArrayResult),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArrayResult {
    pub variable: VarId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variable {
    pub id: VarId,
    /// The variable's axes in order. An empty list is a zero-dimensional variable.
    pub dims: Vec<VariableAxis>,
    pub role: VariableRole,
    pub dtype: TypeKnowledge,
}

/// One axis of a variable.
///
/// A named axis takes its extent from the schema's `sizes`. A positional axis has no
/// `DimId` to key it there, so it carries its own extent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariableAxis {
    Named(DimId),
    Positional(Extent),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariableRole {
    Data,
    Coordinate(CoordinateRole),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinateRole {
    /// Associated with one dimension. This says nothing about sortedness, uniqueness
    /// or regularity.
    Index(IndexCoordinate),
    Auxiliary,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexCoordinate {
    pub dim: DimId,
}

/// A structural contradiction in a schema. Each error names the variable or dimension.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum SchemaError {
    #[error("variable {variable:?} appears more than once")]
    DuplicateVariable { variable: VarId },
    #[error("variable {variable:?} mixes named and positional axes")]
    MixedAxisNames { variable: VarId },
    #[error("array payload {variable:?} is not a variable of the schema")]
    UnknownPayload { variable: VarId },
    #[error("variable {variable:?} is data but is not the array's payload")]
    ExtraDataInArray { variable: VarId },
    #[error("dataset variable {variable:?} has positional axes")]
    PositionalAxisInDataset { variable: VarId },
    #[error("coordinate {variable:?} has positional axes")]
    PositionalCoordinate { variable: VarId },
    #[error("index coordinate {variable:?} must span exactly its dimension {dim:?}")]
    IndexCoordinateAxes { variable: VarId, dim: DimId },
    #[error("dimension {dim:?} has more than one index coordinate")]
    DuplicateIndex { dim: DimId },
    #[error("dimension {dim:?} appears more than once in sizes")]
    DuplicateSize { dim: DimId },
    #[error("dimension {dim:?} is spanned by a variable but has no entry in sizes")]
    MissingSize { dim: DimId },
    #[error("dimension {dim:?} has an entry in sizes but no variable spans it")]
    UnusedSize { dim: DimId },
    #[error("sizes are not in first-appearance order; expected {expected:?}")]
    SizesOutOfOrder { expected: Vec<DimId> },
}

impl Schema {
    /// Check the structural rules of `plans/schema-and-values.md`.
    ///
    /// Unknown dtypes and extents are allowed: they are missing facts, not
    /// contradictions. The first error found is returned: the array payload first, then
    /// each variable in order, then `sizes`.
    pub fn validate(&self) -> Result<(), SchemaError> {
        if let ResultKind::Array(array) = &self.result {
            if !self.variables.iter().any(|v| v.id == array.variable) {
                return Err(SchemaError::UnknownPayload {
                    variable: array.variable.clone(),
                });
            }
        }
        let mut seen_variables = BTreeSet::new();
        let mut indexed_dims = BTreeSet::new();
        for variable in &self.variables {
            if !seen_variables.insert(&variable.id) {
                return Err(SchemaError::DuplicateVariable {
                    variable: variable.id.clone(),
                });
            }
            self.check_variable(variable, &mut indexed_dims)?;
        }
        self.check_sizes()
    }

    fn check_variable<'a>(
        &self,
        variable: &'a Variable,
        indexed_dims: &mut BTreeSet<&'a DimId>,
    ) -> Result<(), SchemaError> {
        let id = || variable.id.clone();
        let named = variable
            .dims
            .iter()
            .filter(|axis| matches!(axis, VariableAxis::Named(_)))
            .count();
        let positional = variable.dims.len() - named;
        if named > 0 && positional > 0 {
            return Err(SchemaError::MixedAxisNames { variable: id() });
        }

        match &self.result {
            ResultKind::Array(array) => {
                if variable.role == VariableRole::Data && variable.id != array.variable {
                    return Err(SchemaError::ExtraDataInArray { variable: id() });
                }
            }
            ResultKind::Dataset => {
                // One exception: an `Open` of a single NumPy or Array API array declares
                // a dataset holding just that data variable.
                let single_array = self.variables.len() == 1 && variable.role == VariableRole::Data;
                if positional > 0 && !single_array {
                    return Err(SchemaError::PositionalAxisInDataset { variable: id() });
                }
            }
        }

        if let VariableRole::Coordinate(role) = &variable.role {
            if positional > 0 {
                return Err(SchemaError::PositionalCoordinate { variable: id() });
            }
            if let CoordinateRole::Index(index) = role {
                if variable.dims != [VariableAxis::Named(index.dim.clone())] {
                    return Err(SchemaError::IndexCoordinateAxes {
                        variable: id(),
                        dim: index.dim.clone(),
                    });
                }
                if !indexed_dims.insert(&index.dim) {
                    return Err(SchemaError::DuplicateIndex {
                        dim: index.dim.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// `sizes` must list exactly the named dimensions the variables span, in order of
    /// first appearance, so two hosts write the same bytes for the same schema.
    fn check_sizes(&self) -> Result<(), SchemaError> {
        let mut expected: Vec<DimId> = Vec::new();
        for variable in &self.variables {
            for axis in &variable.dims {
                if let VariableAxis::Named(dim) = axis {
                    if !expected.contains(dim) {
                        expected.push(dim.clone());
                    }
                }
            }
        }

        let mut listed: Vec<DimId> = Vec::new();
        for (dim, _extent) in &self.sizes {
            if listed.contains(dim) {
                return Err(SchemaError::DuplicateSize { dim: dim.clone() });
            }
            if !expected.contains(dim) {
                return Err(SchemaError::UnusedSize { dim: dim.clone() });
            }
            listed.push(dim.clone());
        }
        for dim in &expected {
            if !listed.contains(dim) {
                return Err(SchemaError::MissingSize { dim: dim.clone() });
            }
        }
        if listed != expected {
            return Err(SchemaError::SizesOutOfOrder { expected });
        }
        Ok(())
    }
}
