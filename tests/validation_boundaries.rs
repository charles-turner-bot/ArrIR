use std::collections::BTreeMap;

use arrir::{DimId, Dimension, Indexer, Operation, Plan, Reduction, Shape};

fn shape(lengths: &[u64]) -> Shape {
    Shape::new(
        lengths
            .iter()
            .enumerate()
            .map(|(index, length)| {
                Dimension::new(DimId::new(format!("d{index}")).unwrap(), *length)
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn json_cannot_bypass_identifier_constructors() {
    assert!(serde_json::from_str::<DimId>(r#""""#).is_err());
    assert!(serde_json::from_str::<Reduction>(r#""  ""#).is_err());
    assert!(serde_json::from_str::<Plan>(
        r#"{
            "version": 1,
            "input_shape": {"dimensions": [{"name": "", "length": 2}]},
            "operations": []
        }"#,
    )
    .is_err());

    // Identifier validation must also apply to JSON object keys.
    assert!(serde_json::from_str::<Operation>(
        r#"{"type": "select", "indexers": {"": {"kind": "positions", "values": []}}}"#,
    )
    .is_err());
}

#[test]
fn validated_identifiers_keep_the_string_wire_format() {
    let name = DimId::new("time").unwrap();
    let reduction = Reduction::new("sum").unwrap();
    assert_eq!(serde_json::to_string(&name).unwrap(), r#""time""#);
    assert_eq!(serde_json::to_string(&reduction).unwrap(), r#""sum""#);
    assert_eq!(serde_json::from_str::<DimId>(r#""time""#).unwrap(), name);
    assert_eq!(
        serde_json::from_str::<Reduction>(r#""sum""#).unwrap(),
        reduction
    );
}

#[test]
fn empty_shapes_have_zero_elements_regardless_of_dimension_order() {
    for lengths in [[0, u64::MAX, 2], [u64::MAX, 0, 2], [u64::MAX, 2, 0]] {
        let input = shape(&lengths);
        assert_eq!(input.element_count(), Ok(0));
        let flat = Dimension::new(DimId::new("flat").unwrap(), 0);
        let output = Shape::new(vec![flat.clone()]).unwrap();
        for operation in [
            Operation::Ravel {
                dimension: flat.name.clone(),
            },
            Operation::Reshape {
                dimensions: vec![flat],
            },
        ] {
            assert_eq!(operation.infer_shape(&input), Ok(output.clone()));
        }
        let from_empty = Operation::Reshape {
            dimensions: input.dimensions.clone(),
        };
        assert_eq!(from_empty.infer_shape(&output), Ok(input));
    }
}

#[test]
fn slices_cover_empty_dimensions_omitted_bounds_and_extreme_steps() {
    let bounds = [
        None,
        Some(i64::MIN),
        Some(-5),
        Some(-1),
        Some(0),
        Some(3),
        Some(i64::MAX),
    ];
    let steps = [i64::MIN, -3, -1, 1, 2, i64::MAX];
    for length in 0..=4 {
        let input = shape(&[length]);
        for start in bounds {
            for stop in bounds {
                for step in steps {
                    // Independent small-domain oracle: count actual selected positions
                    // rather than repeating the core's slice-length division formula.
                    let normalize = |bound: Option<i64>, default: i128| {
                        bound.map_or(default, |value| {
                            let value = i128::from(value);
                            let value = if value < 0 {
                                value + i128::from(length)
                            } else {
                                value
                            };
                            if step > 0 {
                                value.clamp(0, i128::from(length))
                            } else {
                                value.clamp(-1, i128::from(length) - 1)
                            }
                        })
                    };
                    let first = normalize(start, if step > 0 { 0 } else { i128::from(length) - 1 });
                    let end = normalize(stop, if step > 0 { i128::from(length) } else { -1 });
                    let expected = (0..length)
                        .filter(|&position| {
                            let position = i128::from(position);
                            if step > 0 {
                                position >= first
                                    && position < end
                                    && (position - first) % i128::from(step) == 0
                            } else {
                                position <= first
                                    && position > end
                                    && (first - position) % -i128::from(step) == 0
                            }
                        })
                        .count() as u64;
                    let selection = Operation::Select {
                        indexers: BTreeMap::from([(
                            input.dimensions[0].name.clone(),
                            Indexer::Slice {
                                start,
                                stop,
                                step: Some(step),
                            },
                        )]),
                    };
                    assert_eq!(
                        selection.infer_shape(&input),
                        Ok(shape(&[expected])),
                        "length={length}, start={start:?}, stop={stop:?}, step={step}"
                    );
                }
            }
        }
    }
}
