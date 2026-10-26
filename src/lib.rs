//! ArrIR is a portable intermediate representation for *unary* array operation plans.
//!
//! A host-specific adapter lowers an xarray, Polars, JavaScript, or other plan into
//! [`Plan`]. Another adapter either emits an equivalent target plan or reports that it
//! cannot support one of the operations. The core never stores host runtime objects and
//! never touches array data.
//!
//! Plans retain an ordered input shape only. Shape inference propagates that shape through
//! the operation sequence; intermediate and output shapes are never serialised. Structural
//! validation checks wire-level well-formedness, while semantic validation checks that each
//! operation is compatible with the shape preceding it.

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
        plan.validate_structure()?;
        Ok(plan)
    }

    /// Ensure this is a currently supported, structurally valid plan.
    pub fn validate_structure(&self) -> Result<(), ValidationError> {
        if self.version != WIRE_VERSION {
            return Err(ValidationError::UnsupportedWireVersion(self.version));
        }
        self.input_shape.validate()?;
        for operation in &self.operations {
            operation.validate()?;
        }
        Ok(())
    }

    /// Infer the final logical shape by applying every operation in sequence.
    pub fn infer_shape(&self) -> Result<Shape, SemanticError> {
        self.operations
            .iter()
            .try_fold(self.input_shape.clone(), |shape, operation| {
                operation.infer_shape(&shape)
            })
    }

    /// Verify that every operation is compatible with the preceding shape.
    pub fn validate_semantics(&self) -> Result<(), SemanticError> {
        self.infer_shape().map(|_| ())
    }

    /// Validate both the wire structure and shape-dependent operation semantics.
    pub fn validate(&self) -> Result<(), PlanValidationError> {
        self.validate_structure()?;
        self.validate_semantics()?;
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
    /// Empty dimensions make the count zero regardless of dimension order.
    pub fn element_count(&self) -> Result<u64, ValidationError> {
        if self
            .dimensions
            .iter()
            .any(|dimension| dimension.length == 0)
        {
            return Ok(0);
        }
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

    fn dimension(&self, name: &DimId) -> Result<&Dimension, SemanticError> {
        self.dimensions
            .iter()
            .find(|dimension| &dimension.name == name)
            .ok_or_else(|| SemanticError::UnknownDimension(name.as_str().to_owned()))
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
    /// Positional selection of one or more dimensions.
    ///
    /// Coordinate labels must be resolved by a lowering adapter before constructing
    /// this portable operation.
    Select { indexers: BTreeMap<DimId, Indexer> },
    /// A request to alter chunk topology without changing array values.
    Rechunk { chunks: BTreeMap<DimId, ChunkSpec> },
    /// Explicitly reorder every dimension of an array.
    Transpose { dimensions: Vec<DimId> },
    /// Change logical shape while preserving the total element count.
    Reshape { dimensions: Vec<Dimension> },
    /// Flatten every dimension into one explicitly named dimension.
    Ravel { dimension: DimId },
}

impl Operation {
    /// The operation's stable discriminant.
    pub fn kind(&self) -> OperationKind {
        match self {
            Self::Reduce { .. } => OperationKind::Reduce,
            Self::Select { .. } => OperationKind::Select,
            Self::Rechunk { .. } => OperationKind::Rechunk,
            Self::Transpose { .. } => OperationKind::Transpose,
            Self::Reshape { .. } => OperationKind::Reshape,
            Self::Ravel { .. } => OperationKind::Ravel,
        }
    }

    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Reduce { function, .. } => function.validate(),
            Self::Select { indexers } => {
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
            Self::Transpose { .. } | Self::Ravel { .. } => Ok(()),
            Self::Reshape { dimensions } => Shape::new(dimensions.clone()).map(|_| ()),
        }
    }

    /// Infer the output shape produced by this operation on `input_shape`.
    pub fn infer_shape(&self, input_shape: &Shape) -> Result<Shape, SemanticError> {
        match self {
            Self::Reduce {
                dimensions,
                keep_dimensions,
                ..
            } => {
                let reduced = dimensions.resolve(input_shape)?;
                let dimensions = input_shape
                    .dimensions
                    .iter()
                    .filter_map(|dimension| {
                        if reduced.contains(&dimension.name) {
                            keep_dimensions.then(|| Dimension::new(dimension.name.clone(), 1))
                        } else {
                            Some(dimension.clone())
                        }
                    })
                    .collect();
                Ok(Shape { dimensions })
            }
            Self::Select { indexers } => {
                for dimension in indexers.keys() {
                    input_shape.dimension(dimension)?;
                }
                let mut dimensions = Vec::new();
                for dimension in &input_shape.dimensions {
                    match indexers.get(&dimension.name) {
                        None => dimensions.push(dimension.clone()),
                        Some(indexer) if indexer.drops_dimension() => {
                            indexer.validate_for_dimension(dimension)?;
                        }
                        Some(indexer) => dimensions.push(Dimension::new(
                            dimension.name.clone(),
                            indexer.output_length(dimension)?,
                        )),
                    }
                }
                Ok(Shape { dimensions })
            }
            Self::Rechunk { chunks } => {
                for (name, specification) in chunks {
                    let dimension = input_shape.dimension(name)?;
                    specification.validate_for_dimension(dimension)?;
                }
                Ok(input_shape.clone())
            }
            Self::Transpose { dimensions } => {
                let mut seen = BTreeSet::new();
                let mut output = Vec::with_capacity(dimensions.len());
                for name in dimensions {
                    if !seen.insert(name.clone()) {
                        return Err(SemanticError::DuplicateTransposeDimension(
                            name.as_str().to_owned(),
                        ));
                    }
                    output.push(input_shape.dimension(name)?.clone());
                }
                for dimension in &input_shape.dimensions {
                    if !seen.contains(&dimension.name) {
                        return Err(SemanticError::MissingTransposeDimension(
                            dimension.name.as_str().to_owned(),
                        ));
                    }
                }
                Ok(Shape { dimensions: output })
            }
            Self::Reshape { dimensions } => {
                let output_shape = Shape {
                    dimensions: dimensions.clone(),
                };
                let input_count = shape_element_count(input_shape)?;
                let output_count = shape_element_count(&output_shape)?;
                if input_count != output_count {
                    return Err(SemanticError::ElementCountMismatch {
                        input: input_count,
                        output: output_count,
                    });
                }
                Ok(output_shape)
            }
            Self::Ravel { dimension } => Ok(Shape {
                dimensions: vec![Dimension::new(
                    dimension.clone(),
                    shape_element_count(input_shape)?,
                )],
            }),
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
    Transpose,
    Reshape,
    Ravel,
}

/// A host-independent reduction name.
///
/// This remains an open string: host libraries have valid reductions beyond a fixed shared
/// list. An adapter decides whether it can emit a particular function.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String")]
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

impl TryFrom<String> for Reduction {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// A dimension identifier in portable plans.
///
/// Host adapters may intern non-string host keys before lowering. Portable wire plans use
/// strings so another language can decode them without carrying host runtime objects.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
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

impl TryFrom<String> for DimId {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
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

impl DimSet {
    fn resolve(&self, shape: &Shape) -> Result<BTreeSet<DimId>, SemanticError> {
        match self {
            Self::All => Ok(shape
                .dimensions
                .iter()
                .map(|dimension| dimension.name.clone())
                .collect()),
            Self::Named(dimensions) => {
                for dimension in dimensions {
                    shape.dimension(dimension)?;
                }
                Ok(dimensions.clone())
            }
        }
    }
}

/// One positional selection value.
///
/// Slice semantics follow Python: stop is exclusive; negative bounds count from the end;
/// omitted bounds clamp normally; a negative step reverses; and empty slices are valid.
/// This deliberately models independent one-dimensional indexers only, not NumPy coupled
/// advanced-index broadcasting.
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

    fn validate_for_dimension(&self, dimension: &Dimension) -> Result<(), SemanticError> {
        match self {
            Self::Scalar {
                value: Literal::Integer(index),
            } => {
                normalize_index(*index, dimension)?;
            }
            Self::Scalar { .. } => {
                return Err(SemanticError::NonIntegerPosition {
                    dimension: dimension.name.as_str().to_owned(),
                });
            }
            Self::Positions { values } => {
                for index in values {
                    normalize_index(*index, dimension)?;
                }
            }
            Self::Mask { values } => {
                let actual = u64::try_from(values.len()).expect("usize fits in u64");
                if actual != dimension.length {
                    return Err(SemanticError::MaskLengthMismatch {
                        dimension: dimension.name.as_str().to_owned(),
                        expected: dimension.length,
                        actual,
                    });
                }
            }
            Self::Slice { .. } => {}
        }
        Ok(())
    }

    fn output_length(&self, dimension: &Dimension) -> Result<u64, SemanticError> {
        self.validate_for_dimension(dimension)?;
        match self {
            Self::Scalar { .. } => unreachable!("scalars remove their dimension"),
            Self::Slice { start, stop, step } => slice_length(*start, *stop, *step, dimension),
            Self::Positions { values } => {
                Ok(u64::try_from(values.len()).expect("usize fits in u64"))
            }
            Self::Mask { values } => {
                Ok(u64::try_from(values.iter().filter(|value| **value).count())
                    .expect("usize fits in u64"))
            }
        }
    }
}

fn normalize_index(index: i64, dimension: &Dimension) -> Result<u64, SemanticError> {
    let length = i128::from(dimension.length);
    let normalized = if index < 0 {
        i128::from(index) + length
    } else {
        i128::from(index)
    };
    if !(0..length).contains(&normalized) {
        return Err(SemanticError::IndexOutOfBounds {
            dimension: dimension.name.as_str().to_owned(),
            index,
            length: dimension.length,
        });
    }
    Ok(u64::try_from(normalized).expect("validated non-negative index fits in u64"))
}

fn slice_length(
    start: Option<i64>,
    stop: Option<i64>,
    step: Option<i64>,
    dimension: &Dimension,
) -> Result<u64, SemanticError> {
    let length = i128::from(dimension.length);
    let step = i128::from(step.unwrap_or(1));
    if step == 0 {
        return Err(SemanticError::ZeroSliceStep);
    }

    let output_length = if step > 0 {
        let start = normalize_positive_slice_bound(start, length, 0);
        let stop = normalize_positive_slice_bound(stop, length, length);
        if stop <= start {
            0
        } else {
            1 + (stop - start - 1) / step
        }
    } else {
        let start = normalize_negative_slice_bound(start, length, length - 1);
        let stop = normalize_negative_slice_bound(stop, length, -1);
        if start <= stop {
            0
        } else {
            1 + (start - stop - 1) / -step
        }
    };
    u64::try_from(output_length).map_err(|_| SemanticError::SliceLengthOverflow {
        dimension: dimension.name.as_str().to_owned(),
    })
}

fn normalize_positive_slice_bound(bound: Option<i64>, length: i128, default: i128) -> i128 {
    let bound = bound.map_or(default, i128::from);
    let bound = if bound < 0 { bound + length } else { bound };
    bound.clamp(0, length)
}

fn normalize_negative_slice_bound(bound: Option<i64>, length: i128, default: i128) -> i128 {
    match bound {
        None => default,
        Some(bound) => {
            let bound = i128::from(bound);
            let bound = if bound < 0 { bound + length } else { bound };
            bound.clamp(-1, length - 1)
        }
    }
}

fn shape_element_count(shape: &Shape) -> Result<u64, SemanticError> {
    shape
        .element_count()
        .map_err(|_| SemanticError::ElementCountOverflow)
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
            Self::Blocks { sizes } if sizes.contains(&0) => Err(ValidationError::ZeroChunkSize),
            _ => Ok(()),
        }
    }

    fn validate_for_dimension(&self, dimension: &Dimension) -> Result<(), SemanticError> {
        if let Self::Blocks { sizes } = self {
            let total = sizes.iter().try_fold(0_u64, |total, size| {
                total
                    .checked_add(*size)
                    .ok_or_else(|| SemanticError::ChunkBlockTotalOverflow {
                        dimension: dimension.name.as_str().to_owned(),
                    })
            })?;
            if total != dimension.length {
                return Err(SemanticError::ChunkBlockLengthMismatch {
                    dimension: dimension.name.as_str().to_owned(),
                    expected: dimension.length,
                    actual: total,
                });
            }
        }
        Ok(())
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

/// Shape-independent construction and structural validation failures.
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
}

/// Shape-dependent operation validation failures.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SemanticError {
    #[error("operation references unknown dimension {0:?}")]
    UnknownDimension(String),
    #[error("transpose lists dimension {0:?} more than once")]
    DuplicateTransposeDimension(String),
    #[error("transpose omits dimension {0:?}")]
    MissingTransposeDimension(String),
    #[error("shape element count overflowed u64")]
    ElementCountOverflow,
    #[error("reshape changes element count from {input} to {output}")]
    ElementCountMismatch { input: u64, output: u64 },
    #[error("slice steps must not be zero")]
    ZeroSliceStep,
    #[error("scalar positional index for dimension {dimension:?} must be an integer")]
    NonIntegerPosition { dimension: String },
    #[error("index {index} is out of bounds for dimension {dimension:?} of length {length}")]
    IndexOutOfBounds {
        dimension: String,
        index: i64,
        length: u64,
    },
    #[error("mask for dimension {dimension:?} has length {actual}, expected {expected}")]
    MaskLengthMismatch {
        dimension: String,
        expected: u64,
        actual: u64,
    },
    #[error("slice output length for dimension {dimension:?} cannot fit in u64")]
    SliceLengthOverflow { dimension: String },
    #[error("chunk blocks for dimension {dimension:?} overflowed u64")]
    ChunkBlockTotalOverflow { dimension: String },
    #[error(
        "chunk blocks for dimension {dimension:?} total {actual}, expected exactly {expected}"
    )]
    ChunkBlockLengthMismatch {
        dimension: String,
        expected: u64,
        actual: u64,
    },
}

/// A failure from either structural or shape-dependent plan validation.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PlanValidationError {
    #[error(transparent)]
    Structural(#[from] ValidationError),
    #[error(transparent)]
    Semantic(#[from] SemanticError),
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
            shape(&[("time", 2), ("lat", 3)]),
            vec![
                Operation::Transpose {
                    dimensions: vec![dim("lat"), dim("time")],
                },
                Operation::Select {
                    indexers: BTreeMap::from([(
                        dim("time"),
                        Indexer::Positions { values: vec![1, 1] },
                    )]),
                },
                Operation::Rechunk {
                    chunks: BTreeMap::from([(dim("lat"), ChunkSpec::Blocks { sizes: vec![3] })]),
                },
                Operation::Reduce {
                    function: Reduction::new("mean").unwrap(),
                    dimensions: DimSet::Named(BTreeSet::from([dim("lat")])),
                    keep_dimensions: false,
                },
                Operation::Ravel {
                    dimension: dim("sample"),
                },
                Operation::Reshape {
                    dimensions: vec![
                        Dimension::new(dim("row"), 1),
                        Dimension::new(dim("column"), 2),
                    ],
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

    #[test]
    fn propagates_shapes_through_existing_operations() {
        let plan = Plan::new(
            shape(&[("time", 6), ("lat", 2)]),
            vec![
                Operation::Rechunk {
                    chunks: BTreeMap::from([(
                        dim("time"),
                        ChunkSpec::Blocks { sizes: vec![2, 4] },
                    )]),
                },
                Operation::Reduce {
                    function: Reduction::new("mean").unwrap(),
                    dimensions: DimSet::Named(BTreeSet::from([dim("lat")])),
                    keep_dimensions: false,
                },
            ],
        )
        .unwrap();

        assert_eq!(plan.infer_shape(), Ok(shape(&[("time", 6)])));
        plan.validate_semantics().unwrap();
    }

    #[test]
    fn keeps_structural_and_semantic_validation_separate() {
        let plan = Plan::new(
            shape(&[("time", 6)]),
            vec![Operation::Reduce {
                function: Reduction::new("mean").unwrap(),
                dimensions: DimSet::Named(BTreeSet::from([dim("lat")])),
                keep_dimensions: false,
            }],
        )
        .unwrap();

        assert_eq!(
            plan.validate_semantics(),
            Err(SemanticError::UnknownDimension("lat".to_owned()))
        );
        assert_eq!(
            plan.validate(),
            Err(PlanValidationError::Semantic(
                SemanticError::UnknownDimension("lat".to_owned())
            ))
        );
    }

    #[test]
    fn reduction_can_produce_a_scalar_or_keep_reduced_dimensions() {
        let input = shape(&[("time", 6), ("lat", 2)]);
        let scalar = Operation::Reduce {
            function: Reduction::new("sum").unwrap(),
            dimensions: DimSet::All,
            keep_dimensions: false,
        };
        assert_eq!(scalar.infer_shape(&input), Ok(Shape::scalar()));

        let kept = Operation::Reduce {
            function: Reduction::new("sum").unwrap(),
            dimensions: DimSet::All,
            keep_dimensions: true,
        };
        assert_eq!(
            kept.infer_shape(&input),
            Ok(shape(&[("time", 1), ("lat", 1)]))
        );
    }

    #[test]
    fn validates_rechunk_blocks_against_the_current_shape() {
        let input = shape(&[("time", 6)]);
        let mismatch = Operation::Rechunk {
            chunks: BTreeMap::from([(dim("time"), ChunkSpec::Blocks { sizes: vec![2, 3] })]),
        };
        assert_eq!(
            mismatch.infer_shape(&input),
            Err(SemanticError::ChunkBlockLengthMismatch {
                dimension: "time".to_owned(),
                expected: 6,
                actual: 5,
            })
        );

        let empty_blocks_for_empty_dimension = Operation::Rechunk {
            chunks: BTreeMap::from([(dim("time"), ChunkSpec::Blocks { sizes: vec![] })]),
        };
        assert_eq!(
            empty_blocks_for_empty_dimension.infer_shape(&shape(&[("time", 0)])),
            Ok(shape(&[("time", 0)]))
        );
    }

    #[test]
    fn positional_selection_preserves_duplicate_and_reordered_positions() {
        let selection = Operation::Select {
            indexers: BTreeMap::from([(
                dim("time"),
                Indexer::Positions {
                    values: vec![2, 2, 0, -1],
                },
            )]),
        };
        assert_eq!(
            selection.infer_shape(&shape(&[("time", 3)])),
            Ok(shape(&[("time", 4)]))
        );
    }

    #[test]
    fn scalar_selection_removes_a_dimension_and_checks_bounds() {
        let selection = Operation::Select {
            indexers: BTreeMap::from([(
                dim("time"),
                Indexer::Scalar {
                    value: Literal::Integer(-1),
                },
            )]),
        };
        assert_eq!(
            selection.infer_shape(&shape(&[("time", 3), ("lat", 2)])),
            Ok(shape(&[("lat", 2)]))
        );

        let invalid = Operation::Select {
            indexers: BTreeMap::from([(
                dim("time"),
                Indexer::Scalar {
                    value: Literal::Integer(3),
                },
            )]),
        };
        assert_eq!(
            invalid.infer_shape(&shape(&[("time", 3)])),
            Err(SemanticError::IndexOutOfBounds {
                dimension: "time".to_owned(),
                index: 3,
                length: 3,
            })
        );
    }

    #[test]
    fn slices_follow_python_positional_rules() {
        let stepped = Operation::Select {
            indexers: BTreeMap::from([(
                dim("time"),
                Indexer::Slice {
                    start: Some(-5),
                    stop: Some(-1),
                    step: Some(2),
                },
            )]),
        };
        assert_eq!(
            stepped.infer_shape(&shape(&[("time", 6)])),
            Ok(shape(&[("time", 2)]))
        );

        let reversed = Operation::Select {
            indexers: BTreeMap::from([(
                dim("time"),
                Indexer::Slice {
                    start: None,
                    stop: None,
                    step: Some(-1),
                },
            )]),
        };
        assert_eq!(
            reversed.infer_shape(&shape(&[("time", 6)])),
            Ok(shape(&[("time", 6)]))
        );

        let empty = Operation::Select {
            indexers: BTreeMap::from([(
                dim("time"),
                Indexer::Slice {
                    start: Some(4),
                    stop: Some(2),
                    step: None,
                },
            )]),
        };
        assert_eq!(
            empty.infer_shape(&shape(&[("time", 6)])),
            Ok(shape(&[("time", 0)]))
        );
    }

    #[test]
    fn masks_must_match_the_selected_dimension_length() {
        let valid = Operation::Select {
            indexers: BTreeMap::from([(
                dim("time"),
                Indexer::Mask {
                    values: vec![true, false, true],
                },
            )]),
        };
        assert_eq!(
            valid.infer_shape(&shape(&[("time", 3)])),
            Ok(shape(&[("time", 2)]))
        );

        let invalid = Operation::Select {
            indexers: BTreeMap::from([(
                dim("time"),
                Indexer::Mask {
                    values: vec![true, false],
                },
            )]),
        };
        assert_eq!(
            invalid.infer_shape(&shape(&[("time", 3)])),
            Err(SemanticError::MaskLengthMismatch {
                dimension: "time".to_owned(),
                expected: 3,
                actual: 2,
            })
        );
    }

    #[test]
    fn transpose_reorders_dimensions_without_changing_lengths() {
        let transpose = Operation::Transpose {
            dimensions: vec![dim("lon"), dim("time"), dim("lat")],
        };
        assert_eq!(
            transpose.infer_shape(&shape(&[("time", 0), ("lat", 2), ("lon", 3)])),
            Ok(shape(&[("lon", 3), ("time", 0), ("lat", 2)]))
        );
    }

    #[test]
    fn transpose_requires_an_exact_dimension_permutation() {
        let input = shape(&[("time", 2), ("lat", 3)]);
        let duplicate = Operation::Transpose {
            dimensions: vec![dim("time"), dim("time")],
        };
        assert_eq!(
            duplicate.infer_shape(&input),
            Err(SemanticError::DuplicateTransposeDimension(
                "time".to_owned()
            ))
        );

        let missing = Operation::Transpose {
            dimensions: vec![dim("time")],
        };
        assert_eq!(
            missing.infer_shape(&input),
            Err(SemanticError::MissingTransposeDimension("lat".to_owned()))
        );

        let unknown = Operation::Transpose {
            dimensions: vec![dim("time"), dim("lon")],
        };
        assert_eq!(
            unknown.infer_shape(&input),
            Err(SemanticError::UnknownDimension("lon".to_owned()))
        );
    }

    #[test]
    fn reshape_preserves_checked_element_count() {
        let reshape = Operation::Reshape {
            dimensions: vec![
                Dimension::new(dim("row"), 3),
                Dimension::new(dim("column"), 2),
            ],
        };
        assert_eq!(
            reshape.infer_shape(&shape(&[("time", 2), ("lat", 3)])),
            Ok(shape(&[("row", 3), ("column", 2)]))
        );

        let mismatch = Operation::Reshape {
            dimensions: vec![Dimension::new(dim("row"), 5)],
        };
        assert_eq!(
            mismatch.infer_shape(&shape(&[("time", 2), ("lat", 3)])),
            Err(SemanticError::ElementCountMismatch {
                input: 6,
                output: 5,
            })
        );
    }

    #[test]
    fn reshape_allows_zero_element_shapes_but_rejects_duplicate_output_names() {
        let zero_reshape = Operation::Reshape {
            dimensions: vec![
                Dimension::new(dim("row"), 0),
                Dimension::new(dim("column"), 99),
            ],
        };
        assert_eq!(
            zero_reshape.infer_shape(&shape(&[("time", 0), ("lat", 2)])),
            Ok(shape(&[("row", 0), ("column", 99)]))
        );

        assert_eq!(
            Plan::new(
                shape(&[("time", 2)]),
                vec![Operation::Reshape {
                    dimensions: vec![
                        Dimension::new(dim("duplicate"), 1),
                        Dimension::new(dim("duplicate"), 2),
                    ],
                }],
            ),
            Err(ValidationError::DuplicateShapeDimension(
                "duplicate".to_owned()
            ))
        );
    }

    #[test]
    fn reshape_and_ravel_detect_element_count_overflow() {
        let overflowing_input = shape(&[("x", u64::MAX), ("y", 2)]);
        let ravel = Operation::Ravel {
            dimension: dim("flat"),
        };
        assert_eq!(
            ravel.infer_shape(&overflowing_input),
            Err(SemanticError::ElementCountOverflow)
        );

        let reshape = Operation::Reshape {
            dimensions: vec![Dimension::new(dim("flat"), 1)],
        };
        assert_eq!(
            reshape.infer_shape(&overflowing_input),
            Err(SemanticError::ElementCountOverflow)
        );
    }

    #[test]
    fn ravel_creates_one_explicit_dimension() {
        let ravel = Operation::Ravel {
            dimension: dim("sample"),
        };
        assert_eq!(
            ravel.infer_shape(&shape(&[("time", 2), ("lat", 3)])),
            Ok(shape(&[("sample", 6)]))
        );
    }
}
