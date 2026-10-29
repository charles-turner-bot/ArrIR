//! Shell 1: array shape and operation effects.
//!
//! An [`ArraySchema`] is the smallest description of an input array that lets a plan be
//! checked without data: an *ordered* list of named dimensions and their extents. A plan's
//! effects are then pure functions of the schema and the operations: which dimensions each
//! operation preserves, resizes or consumes, and what the resulting schema is.
//!
//! Positional conventions (proposed in `docs/shells.md`): positions are 0-based, a negative
//! position counts back from the end of the dimension, and slices are half-open with
//! Python's semantics. Hosts with other conventions (R is 1-based and treats negative
//! indices as exclusions) translate in their adapters.
//!
//! Label-mode selections cannot be sized without coordinates, which arrive in a later
//! shell, so they yield [`Extent::Unknown`] rather than a guess.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{DimId, DimSet, Indexer, Literal, Operation, Plan, SelectionMode};

/// The length of one dimension, when it can be known without data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "size", rename_all = "snake_case")]
pub enum Extent {
    Known(u64),
    /// Depends on information outside the schema, such as coordinate values.
    Unknown,
}

/// One named dimension of an array.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dim {
    pub id: DimId,
    pub extent: Extent,
}

/// The ordered dimensions of an array. Order is significant: it is the storage and
/// iteration order that host adapters need to emit correct code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArraySchema {
    pub dims: Vec<Dim>,
}

impl ArraySchema {
    /// Build a schema from `(name, size)` pairs, in order.
    pub fn new<I, S>(dims: I) -> Result<Self, EffectError>
    where
        I: IntoIterator<Item = (S, u64)>,
        S: Into<String>,
    {
        let dims = dims
            .into_iter()
            .map(|(name, size)| {
                Ok(Dim {
                    id: DimId::new(name).map_err(|_| EffectError::EmptyDimension)?,
                    extent: Extent::Known(size),
                })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        let schema = Self { dims };
        schema.validate()?;
        Ok(schema)
    }

    /// Dimension names must be unique.
    pub fn validate(&self) -> Result<(), EffectError> {
        let mut seen = BTreeSet::new();
        for dim in &self.dims {
            if !seen.insert(&dim.id) {
                return Err(EffectError::DuplicateDimension(dim.id.as_str().to_owned()));
            }
        }
        Ok(())
    }

    fn position(&self, id: &DimId) -> Result<usize, EffectError> {
        self.dims
            .iter()
            .position(|d| &d.id == id)
            .ok_or_else(|| EffectError::UnknownDimension(id.as_str().to_owned()))
    }
}

/// What one operation does to one dimension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DimEffect {
    Preserved,
    Resized { from: Extent, to: Extent },
    Consumed,
}

/// The effect of one operation on every input dimension, in input order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationEffect {
    pub dims: Vec<(DimId, DimEffect)>,
}

/// Failures when a plan does not fit a schema.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum EffectError {
    #[error("dimension identifiers must not be empty")]
    EmptyDimension,
    #[error("dimension `{0}` appears more than once in the schema")]
    DuplicateDimension(String),
    #[error("dimension `{0}` is not in the schema")]
    UnknownDimension(String),
    #[error("position {position} is out of bounds for dimension `{dim}` of size {size}")]
    OutOfBounds {
        dim: String,
        position: i64,
        size: u64,
    },
    #[error("mask length {len} does not match dimension `{dim}` of size {size}")]
    MaskLength { dim: String, len: usize, size: u64 },
    #[error("positional scalar selection on `{0}` needs an integer")]
    NonIntegerPosition(String),
}

impl Plan {
    /// The effect of each operation, applied in order to `input`.
    pub fn effects(&self, input: &ArraySchema) -> Result<Vec<OperationEffect>, EffectError> {
        input.validate()?;
        let mut schema = input.clone();
        let mut out = Vec::with_capacity(self.operations.len());
        for operation in &self.operations {
            let (effect, next) = apply(operation, &schema)?;
            out.push(effect);
            schema = next;
        }
        Ok(out)
    }

    /// The schema of the plan's result.
    pub fn output_schema(&self, input: &ArraySchema) -> Result<ArraySchema, EffectError> {
        input.validate()?;
        let mut schema = input.clone();
        for operation in &self.operations {
            schema = apply(operation, &schema)?.1;
        }
        Ok(schema)
    }
}

fn apply(
    operation: &Operation,
    schema: &ArraySchema,
) -> Result<(OperationEffect, ArraySchema), EffectError> {
    let mut effects: Vec<DimEffect> = vec![DimEffect::Preserved; schema.dims.len()];

    match operation {
        Operation::Reduce {
            dimensions,
            keep_dimensions,
            ..
        } => {
            let targets: Vec<usize> = match dimensions {
                DimSet::All => (0..schema.dims.len()).collect(),
                DimSet::Named(names) => names
                    .iter()
                    .map(|n| schema.position(n))
                    .collect::<Result<_, _>>()?,
            };
            for i in targets {
                effects[i] = if *keep_dimensions {
                    DimEffect::Resized {
                        from: schema.dims[i].extent,
                        to: Extent::Known(1),
                    }
                } else {
                    DimEffect::Consumed
                };
            }
        }
        Operation::Select { mode, indexers } => {
            for (id, indexer) in indexers {
                let i = schema.position(id)?;
                effects[i] = select_effect(*mode, &schema.dims[i], indexer)?;
            }
        }
        Operation::Rechunk { chunks } => {
            for id in chunks.keys() {
                schema.position(id)?;
            }
        }
    }

    let mut next = Vec::with_capacity(schema.dims.len());
    for (dim, effect) in schema.dims.iter().zip(&effects) {
        match effect {
            DimEffect::Preserved => next.push(dim.clone()),
            DimEffect::Resized { to, .. } => next.push(Dim {
                id: dim.id.clone(),
                extent: *to,
            }),
            DimEffect::Consumed => {}
        }
    }
    let effect = OperationEffect {
        dims: schema
            .dims
            .iter()
            .map(|d| d.id.clone())
            .zip(effects)
            .collect(),
    };
    Ok((effect, ArraySchema { dims: next }))
}

fn select_effect(
    mode: SelectionMode,
    dim: &Dim,
    indexer: &Indexer,
) -> Result<DimEffect, EffectError> {
    if indexer.drops_dimension() {
        if let (SelectionMode::Position, Indexer::Scalar { value }) = (mode, indexer) {
            let Literal::Integer(p) = value else {
                return Err(EffectError::NonIntegerPosition(dim.id.as_str().to_owned()));
            };
            if let Extent::Known(n) = dim.extent {
                normalise(*p, n, dim)?;
            }
        }
        return Ok(DimEffect::Consumed);
    }

    let to = match (mode, dim.extent) {
        (SelectionMode::Label, _) | (_, Extent::Unknown) => Extent::Unknown,
        (SelectionMode::Position, Extent::Known(n)) => Extent::Known(match indexer {
            Indexer::Slice { start, stop, step } => slice_len(n, *start, *stop, *step),
            Indexer::Positions { values } => {
                for p in values {
                    normalise(*p, n, dim)?;
                }
                values.len() as u64
            }
            Indexer::Mask { values } => {
                if values.len() as u64 != n {
                    return Err(EffectError::MaskLength {
                        dim: dim.id.as_str().to_owned(),
                        len: values.len(),
                        size: n,
                    });
                }
                values.iter().filter(|b| **b).count() as u64
            }
            Indexer::Scalar { .. } => unreachable!("scalar indexers drop their dimension"),
        }),
    };
    Ok(DimEffect::Resized {
        from: dim.extent,
        to,
    })
}

/// Resolve a possibly negative position against a dimension of size `n`.
fn normalise(position: i64, n: u64, dim: &Dim) -> Result<u64, EffectError> {
    let n_i = n as i128;
    let p = position as i128;
    let resolved = if p < 0 { p + n_i } else { p };
    if resolved < 0 || resolved >= n_i {
        return Err(EffectError::OutOfBounds {
            dim: dim.id.as_str().to_owned(),
            position,
            size: n,
        });
    }
    Ok(resolved as u64)
}

/// Length of a half-open slice with Python semantics (negative bounds and steps allowed,
/// out-of-range bounds clamped). A zero step is rejected earlier by plan validation.
fn slice_len(n: u64, start: Option<i64>, stop: Option<i64>, step: Option<i64>) -> u64 {
    let n = n as i128;
    let step = step.unwrap_or(1) as i128;
    let (lower, upper) = if step > 0 { (0, n) } else { (-1, n - 1) };
    let clamp = |v: Option<i64>, default: i128| match v {
        None => default,
        Some(v) => {
            let v = v as i128;
            if v < 0 {
                (v + n).max(lower)
            } else {
                v.min(upper)
            }
        }
    };
    let (start, stop) = if step > 0 {
        (clamp(start, lower), clamp(stop, upper))
    } else {
        (clamp(start, upper), clamp(stop, lower))
    };
    let len = if step > 0 {
        if stop > start {
            (stop - start - 1) / step + 1
        } else {
            0
        }
    } else if start > stop {
        (start - stop - 1) / (-step) + 1
    } else {
        0
    };
    len as u64
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::super::Reduction;
    use super::*;

    fn dim(value: &str) -> DimId {
        DimId::new(value).unwrap()
    }

    fn bran() -> ArraySchema {
        ArraySchema::new([
            ("time", 31),
            ("st_ocean", 51),
            ("yt_ocean", 1500),
            ("xt_ocean", 3600),
        ])
        .unwrap()
    }

    fn select(mode: SelectionMode, d: &str, indexer: Indexer) -> Operation {
        Operation::Select {
            mode,
            indexers: BTreeMap::from([(dim(d), indexer)]),
        }
    }

    fn known(schema: &ArraySchema) -> Vec<(&str, Extent)> {
        schema
            .dims
            .iter()
            .map(|d| (d.id.as_str(), d.extent))
            .collect()
    }

    #[test]
    fn slice_lengths_follow_python() {
        // Checked against len(range(10)[slice(start, stop, step)]) in Python.
        let cases = [
            (None, None, None, 10),
            (Some(0), Some(10), None, 10),
            (Some(2), Some(8), Some(3), 2),
            (Some(-3), None, None, 3),
            (None, Some(-3), None, 7),
            (Some(20), None, None, 0),
            (None, None, Some(-1), 10),
            (Some(8), Some(2), Some(-2), 3),
            (Some(-1), Some(-11), Some(-1), 10),
            (Some(5), Some(5), None, 0),
        ];
        for (start, stop, step, expected) in cases {
            assert_eq!(
                slice_len(10, start, stop, step),
                expected,
                "{start:?}:{stop:?}:{step:?}"
            );
        }
    }

    #[test]
    fn point_time_series_keeps_only_time() {
        let plan = Plan::new(vec![
            select(
                SelectionMode::Position,
                "st_ocean",
                Indexer::Scalar {
                    value: Literal::Integer(0),
                },
            ),
            select(
                SelectionMode::Position,
                "yt_ocean",
                Indexer::Scalar {
                    value: Literal::Integer(-1),
                },
            ),
            select(
                SelectionMode::Position,
                "xt_ocean",
                Indexer::Scalar {
                    value: Literal::Integer(1200),
                },
            ),
        ])
        .unwrap();
        let out = plan.output_schema(&bran()).unwrap();
        assert_eq!(known(&out), vec![("time", Extent::Known(31))]);
    }

    #[test]
    fn reduce_consumes_or_keeps_and_preserves_order() {
        let mean_lon = |keep| Operation::Reduce {
            function: Reduction::new("mean").unwrap(),
            dimensions: DimSet::Named(BTreeSet::from([dim("xt_ocean")])),
            keep_dimensions: keep,
        };
        let out = Plan::new(vec![mean_lon(false)])
            .unwrap()
            .output_schema(&bran())
            .unwrap();
        assert_eq!(
            known(&out),
            vec![
                ("time", Extent::Known(31)),
                ("st_ocean", Extent::Known(51)),
                ("yt_ocean", Extent::Known(1500)),
            ]
        );
        let kept = Plan::new(vec![mean_lon(true)])
            .unwrap()
            .output_schema(&bran())
            .unwrap();
        assert_eq!(kept.dims[3].extent, Extent::Known(1));
        assert_eq!(kept.dims[3].id, dim("xt_ocean"));
    }

    #[test]
    fn effects_are_reported_per_input_dimension() {
        let plan = Plan::new(vec![
            select(
                SelectionMode::Position,
                "time",
                Indexer::Slice {
                    start: Some(0),
                    stop: Some(10),
                    step: None,
                },
            ),
            Operation::Reduce {
                function: Reduction::new("mean").unwrap(),
                dimensions: DimSet::All,
                keep_dimensions: false,
            },
        ])
        .unwrap();
        let effects = plan.effects(&bran()).unwrap();
        assert_eq!(
            effects[0].dims[0],
            (
                dim("time"),
                DimEffect::Resized {
                    from: Extent::Known(31),
                    to: Extent::Known(10)
                }
            )
        );
        assert_eq!(effects[0].dims[1], (dim("st_ocean"), DimEffect::Preserved));
        assert!(effects[1]
            .dims
            .iter()
            .all(|(_, e)| *e == DimEffect::Consumed));
        assert!(plan.output_schema(&bran()).unwrap().dims.is_empty());
    }

    #[test]
    fn label_selection_is_unknown_until_coordinates_exist() {
        let plan = Plan::new(vec![select(
            SelectionMode::Label,
            "yt_ocean",
            Indexer::Slice {
                start: Some(-60),
                stop: Some(-45),
                step: None,
            },
        )])
        .unwrap();
        let out = plan.output_schema(&bran()).unwrap();
        assert_eq!(out.dims[2].extent, Extent::Unknown);
    }

    #[test]
    fn rejects_plans_that_do_not_fit_the_schema() {
        let unknown = Plan::new(vec![select(
            SelectionMode::Position,
            "depth",
            Indexer::Positions { values: vec![0] },
        )])
        .unwrap();
        assert_eq!(
            unknown.output_schema(&bran()),
            Err(EffectError::UnknownDimension("depth".into()))
        );

        let oob = Plan::new(vec![select(
            SelectionMode::Position,
            "time",
            Indexer::Positions { values: vec![31] },
        )])
        .unwrap();
        assert!(matches!(
            oob.output_schema(&bran()),
            Err(EffectError::OutOfBounds { .. })
        ));

        let mask = Plan::new(vec![select(
            SelectionMode::Position,
            "time",
            Indexer::Mask {
                values: vec![true; 3],
            },
        )])
        .unwrap();
        assert!(matches!(
            mask.output_schema(&bran()),
            Err(EffectError::MaskLength { .. })
        ));

        assert_eq!(
            ArraySchema::new([("x", 2), ("x", 3)]),
            Err(EffectError::DuplicateDimension("x".into()))
        );
    }

    #[test]
    fn schema_round_trips_through_json() {
        let schema = bran();
        let encoded = serde_json::to_string(&schema).unwrap();
        let decoded: ArraySchema = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, schema);
    }
}
