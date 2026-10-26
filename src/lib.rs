//! ArrIR is a portable intermediate representation for *unary* array operation plans.
//!
//! A host-specific adapter lowers an xarray, Polars, JavaScript, or other plan into
//! [`Plan`]. Another adapter either emits an equivalent target plan or reports that it
//! cannot support one of the operations. The core never stores host runtime objects and
//! never touches array data.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The wire-format version emitted by [`Plan::new`].
pub const WIRE_VERSION: u16 = 1;

/// A complete linear unary-operation plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Version of this serialisable representation.
    pub version: u16,
    /// Ordered logical dimensions of the array entering this plan.
    ///
    /// ArrIR stores only this input shape. Output shapes are derived by propagating it
    /// through [`Plan::operations`], rather than cached in the wire representation.
    pub input_shape: Shape,
    /// Operations run from input to result.
    pub operations: Vec<Operation>,
}

impl Plan {
    /// Create a plan using the current wire format.
    pub fn new(input_shape: Shape, operations: Vec<Operation>) -> Result<Self, ValidationError> {
        let plan = Self {
            version: WIRE_VERSION,
            input_shape,
            operations,
        };
        plan.validate()?;
        Ok(plan)
    }

    /// Ensure this is a currently supported, internally valid plan.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.version != WIRE_VERSION {
            return Err(ValidationError::UnsupportedWireVersion(self.version));
        }
        self.input_shape.validate()?;
        for operation in &self.operations {
            operation.validate()?;
        }
        Ok(())
    }
}

/// An ordered, named logical array shape.
///
/// A shape may be zero-dimensional and its dimensions may have length zero. Dimension
/// ordering is significant and is preserved in the serialised representation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shape {
    pub dimensions: Vec<Dimension>,
}

impl Shape {
    /// Construct a shape with unique dimension names.
    pub fn new(dimensions: Vec<Dimension>) -> Result<Self, ValidationError> {
        let shape = Self { dimensions };
        shape.validate()?;
        Ok(shape)
    }

    /// Construct a zero-dimensional scalar shape.
    pub fn scalar() -> Self {
        Self {
            dimensions: Vec::new(),
        }
    }

    /// Return the number of logical elements, reporting multiplication overflow.
    pub fn element_count(&self) -> Result<u64, ValidationError> {
        self.dimensions.iter().try_fold(1_u64, |count, dimension| {
            count
                .checked_mul(dimension.length)
                .ok_or(ValidationError::ElementCountOverflow)
        })
    }

    fn validate(&self) -> Result<(), ValidationError> {
        let mut names = BTreeSet::new();
        for dimension in &self.dimensions {
            if !names.insert(dimension.name.clone()) {
                return Err(ValidationError::DuplicateShapeDimension(
                    dimension.name.as_str().to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// One named dimension in an ordered [`Shape`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dimension {
    pub name: DimId,
    pub length: u64,
}

impl Dimension {
    pub fn new(name: DimId, length: u64) -> Self {
        Self { name, length }
    }
}

/// A portable unary array operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Operation {
    /// A dimension-reducing operation such as `mean`, `sum`, or `std`.
    Reduce {
        function: Reduction,
        dimensions: DimSet,
        keep_dimensions: bool,
    },
    /// Positional or label-like selection of one or more dimensions.
    Select {
        mode: SelectionMode,
        indexers: BTreeMap<DimId, Indexer>,
    },
    /// A request to alter chunk topology without changing array values.
    Rechunk { chunks: BTreeMap<DimId, ChunkSpec> },
}

impl Operation {
    /// The operation's stable discriminant.
    pub fn kind(&self) -> OperationKind {
        match self {
            Self::Reduce { .. } => OperationKind::Reduce,
            Self::Select { .. } => OperationKind::Select,
            Self::Rechunk { .. } => OperationKind::Rechunk,
        }
    }

    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Reduce { function, .. } => function.validate(),
            Self::Select { indexers, .. } => {
                if indexers.is_empty() {
                    return Err(ValidationError::EmptySelection);
                }
                indexers.values().try_for_each(Indexer::validate)
            }
            Self::Rechunk { chunks } => {
                if chunks.is_empty() {
                    return Err(ValidationError::EmptyRechunk);
                }
                chunks.values().try_for_each(ChunkSpec::validate)
            }
        }
    }
}

/// Stable operation tags used by adapters to report capability boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Reduce,
    Select,
    Rechunk,
}

/// A host-independent reduction name.
///
/// This remains an open string: host libraries have valid reductions beyond a fixed shared
/// list. An adapter decides whether it can emit a particular function.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Reduction(String);

impl Reduction {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let reduction = Self(value.into());
        reduction.validate()?;
        Ok(reduction)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(&self) -> Result<(), ValidationError> {
        if self.0.trim().is_empty() {
            return Err(ValidationError::EmptyReduction);
        }
        Ok(())
    }
}

/// A dimension identifier in portable plans.
///
/// Host adapters may intern non-string host keys before lowering. Portable wire plans use
/// strings so another language can decode them without carrying host runtime objects.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DimId(String);

impl DimId {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let dimension = Self(value.into());
        if dimension.0.is_empty() {
            return Err(ValidationError::EmptyDimension);
        }
        Ok(dimension)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The dimensions named by an operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "dimensions", rename_all = "snake_case")]
pub enum DimSet {
    /// Every dimension at execution time; used by a bare reduction such as `mean()`.
    All,
    /// A concrete, deterministic set of dimensions.
    Named(BTreeSet<DimId>),
}

/// Whether selections are positional (`isel`) or coordinate/label based (`sel`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    Position,
    Label,
}

/// One selection value. The variants describe values the core can reason about without data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Indexer {
    Scalar {
        value: Literal,
    },
    Slice {
        start: Option<i64>,
        stop: Option<i64>,
        step: Option<i64>,
    },
    Positions {
        values: Vec<i64>,
    },
    Mask {
        values: Vec<bool>,
    },
}

impl Indexer {
    /// Whether applying this indexer removes its addressed dimension.
    pub fn drops_dimension(&self) -> bool {
        matches!(self, Self::Scalar { .. })
    }

    fn validate(&self) -> Result<(), ValidationError> {
        if let Self::Slice { step: Some(0), .. } = self {
            return Err(ValidationError::ZeroSliceStep);
        }
        Ok(())
    }
}

/// Portable scalar values supported by the first wire format.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Literal {
    Integer(i64),
    Boolean(bool),
    String(String),
}

/// One dimension's rechunk request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChunkSpec {
    /// Uniform chunk size, with a possibly shorter final chunk.
    Size { size: u64 },
    /// Let the backend choose a chunk size.
    Auto,
    /// Target an approximate byte size; exact parsing remains backend-specific.
    ByteSize { value: String },
    /// Use one chunk spanning the complete dimension.
    Full,
    /// Preserve the existing chunks along this dimension.
    Preserve,
    /// Explicit chunk lengths.
    Blocks { sizes: Vec<u64> },
}

impl ChunkSpec {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Size { size: 0 } => Err(ValidationError::ZeroChunkSize),
            Self::ByteSize { value } if value.trim().is_empty() => {
                Err(ValidationError::EmptyByteSize)
            }
            Self::Blocks { sizes } if sizes.is_empty() => Err(ValidationError::EmptyChunkBlocks),
            Self::Blocks { sizes } if sizes.contains(&0) => Err(ValidationError::ZeroChunkSize),
            _ => Ok(()),
        }
    }
}

/// Implemented by a host-specific frontend adapter.
pub trait LowerToArrIr {
    type Error;
    fn lower_to_arrir(&self) -> Result<Plan, Self::Error>;
}

/// Implemented by a host-specific backend adapter.
///
/// A backend must return an error for operations it cannot faithfully emit; it may not
/// silently approximate or discard them.
pub trait EmitFromArrIr: Sized {
    type Error;
    fn emit_from_arrir(plan: &Plan) -> Result<Self, Self::Error>;
}

/// Construction or validation failures in the portable core.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("unsupported ArrIR wire version {0}")]
    UnsupportedWireVersion(u16),
    #[error("dimension identifiers must not be empty")]
    EmptyDimension,
    #[error("shape contains duplicate dimension {0:?}")]
    DuplicateShapeDimension(String),
    #[error("shape element count overflowed u64")]
    ElementCountOverflow,
    #[error("reduction names must not be empty")]
    EmptyReduction,
    #[error("a selection needs at least one indexer")]
    EmptySelection,
    #[error("a rechunk operation needs at least one dimension")]
    EmptyRechunk,
    #[error("slice steps must not be zero")]
    ZeroSliceStep,
    #[error("chunk sizes must be greater than zero")]
    ZeroChunkSize,
    #[error("byte-size chunk specifications must not be empty")]
    EmptyByteSize,
    #[error("explicit chunk specifications need at least one block")]
    EmptyChunkBlocks,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dim(value: &str) -> DimId {
        DimId::new(value).unwrap()
    }

    fn shape(dimensions: &[(&str, u64)]) -> Shape {
        Shape::new(
            dimensions
                .iter()
                .map(|(name, length)| Dimension::new(dim(name), *length))
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn round_trips_a_portable_plan_through_json() {
        let plan = Plan::new(
            shape(&[("time", 10), ("lat", 2)]),
            vec![
                Operation::Select {
                    mode: SelectionMode::Position,
                    indexers: BTreeMap::from([(
                        dim("time"),
                        Indexer::Slice {
                            start: Some(0),
                            stop: Some(10),
                            step: None,
                        },
                    )]),
                },
                Operation::Reduce {
                    function: Reduction::new("mean").unwrap(),
                    dimensions: DimSet::Named(BTreeSet::from([dim("lat")])),
                    keep_dimensions: false,
                },
                Operation::Rechunk {
                    chunks: BTreeMap::from([(dim("time"), ChunkSpec::Size { size: 100 })]),
                },
            ],
        )
        .unwrap();

        let encoded = serde_json::to_string(&plan).unwrap();
        let decoded: Plan = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, plan);
        decoded.validate().unwrap();
    }

    #[test]
    fn rejects_invalid_structural_values() {
        assert_eq!(DimId::new(""), Err(ValidationError::EmptyDimension));
        assert_eq!(Reduction::new("  "), Err(ValidationError::EmptyReduction));
        assert!(Plan::new(
            shape(&[]),
            vec![Operation::Select {
                mode: SelectionMode::Position,
                indexers: BTreeMap::new(),
            }]
        )
        .is_err());
        assert!(Plan::new(
            shape(&[]),
            vec![Operation::Rechunk {
                chunks: BTreeMap::from([(dim("time"), ChunkSpec::Size { size: 0 })]),
            }]
        )
        .is_err());
    }

    #[test]
    fn scalar_selection_drops_a_dimension() {
        assert!(Indexer::Scalar {
            value: Literal::Integer(0)
        }
        .drops_dimension());
        assert!(!Indexer::Positions { values: vec![0] }.drops_dimension());
    }

    #[test]
    fn preserves_shape_order_and_zero_lengths() {
        let shape = shape(&[("time", 0), ("lat", 2)]);
        assert_eq!(shape.dimensions[0].name.as_str(), "time");
        assert_eq!(shape.dimensions[1].name.as_str(), "lat");
        assert_eq!(shape.element_count(), Ok(0));

        let plan = Plan::new(shape.clone(), vec![]).unwrap();
        let encoded = serde_json::to_string(&plan).unwrap();
        let decoded: Plan = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.input_shape, shape);
    }

    #[test]
    fn accepts_zero_dimensional_shapes() {
        let scalar = Shape::scalar();
        assert_eq!(scalar.element_count(), Ok(1));
        Plan::new(scalar, vec![]).unwrap();
    }

    #[test]
    fn rejects_duplicate_shape_dimensions() {
        let duplicate = Shape {
            dimensions: vec![
                Dimension::new(dim("time"), 1),
                Dimension::new(dim("time"), 2),
            ],
        };
        assert_eq!(
            duplicate.validate(),
            Err(ValidationError::DuplicateShapeDimension("time".to_owned()))
        );
    }

    #[test]
    fn detects_element_count_overflow() {
        let shape = shape(&[("x", u64::MAX), ("y", 2)]);
        assert_eq!(
            shape.element_count(),
            Err(ValidationError::ElementCountOverflow)
        );
    }
}
