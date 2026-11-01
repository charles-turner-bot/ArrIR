use crate::{
    ArrayResult, CoordinateRole, DType, DimId, Extent, IndexCoordinate, ResultKind, Schema,
    SchemaError, TypeKnowledge, VarId, Variable, VariableAxis, VariableRole,
};

fn named(dims: &[&str]) -> Vec<VariableAxis> {
    dims.iter()
        .map(|dim| VariableAxis::Named(DimId::new(*dim)))
        .collect()
}

fn variable(id: &str, dims: Vec<VariableAxis>, role: VariableRole) -> Variable {
    Variable {
        id: VarId::new(id),
        dims,
        role,
        dtype: TypeKnowledge::Known(DType::Float32),
    }
}

fn index(dim: &str) -> VariableRole {
    VariableRole::Coordinate(CoordinateRole::Index(IndexCoordinate {
        dim: DimId::new(dim),
    }))
}

fn size(dim: &str, length: u64) -> (DimId, Extent) {
    (DimId::new(dim), Extent::Known(length))
}

/// `t(time, lat)` with an index coordinate on `time`.
fn observations() -> Schema {
    Schema {
        result: ResultKind::Dataset,
        variables: vec![
            variable("t", named(&["time", "lat"]), VariableRole::Data),
            variable("time", named(&["time"]), index("time")),
        ],
        sizes: vec![size("time", 365), size("lat", 180)],
    }
}

/// The schema in `plans/dag.md`'s wire example, without its `grids` field, which
/// arrives with grids.md's types.
const DAG_EXAMPLE: &str = r#"{"result":"Dataset","variables":[{"id":"t","dims":[{"Named":"time"},{"Named":"lat"}],"role":"Data","dtype":{"Known":"Float32"}}],"sizes":[["time",{"Known":"365"}],["lat",{"Known":"180"}]]}"#;

#[test]
fn the_dag_wire_example_round_trips_exactly() {
    let schema: Schema = serde_json::from_str(DAG_EXAMPLE).unwrap();
    schema.validate().unwrap();
    assert_eq!(serde_json::to_string(&schema).unwrap(), DAG_EXAMPLE);
}

#[test]
fn an_array_result_with_coordinates_round_trips() {
    let schema = Schema {
        result: ResultKind::Array(ArrayResult {
            variable: VarId::new("t"),
        }),
        ..observations()
    };
    schema.validate().unwrap();
    let encoded = serde_json::to_string(&schema).unwrap();
    assert!(encoded.starts_with(r#"{"result":{"Array":{"variable":"t"}}"#));
    assert!(encoded.contains(r#""role":{"Coordinate":{"Index":{"dim":"time"}}}"#));
    assert_eq!(serde_json::from_str::<Schema>(&encoded).unwrap(), schema);
}

#[test]
fn unknown_facts_are_valid() {
    let mut schema = observations();
    schema.variables[0].dtype = TypeKnowledge::Unknown;
    schema.sizes[0].1 = Extent::Unknown;
    schema.validate().unwrap();
}

#[test]
fn a_single_positional_array_may_be_a_dataset() {
    let axes = vec![
        VariableAxis::Positional(Extent::Known(3)),
        VariableAxis::Positional(Extent::Unknown),
    ];
    let schema = Schema {
        result: ResultKind::Dataset,
        variables: vec![variable("x", axes, VariableRole::Data)],
        sizes: vec![],
    };
    schema.validate().unwrap();
    let encoded = serde_json::to_string(&schema).unwrap();
    assert!(encoded.contains(r#"[{"Positional":{"Known":"3"}},{"Positional":"Unknown"}]"#));
}

#[test]
fn unknown_fields_and_tags_fail_import() {
    let extra = DAG_EXAMPLE.replace(r#""result""#, r#""grids":[],"result""#);
    assert!(serde_json::from_str::<Schema>(&extra).is_err());
    let tag = DAG_EXAMPLE.replace(r#""Data""#, r#""Weights""#);
    assert!(serde_json::from_str::<Schema>(&tag).is_err());
}

fn rejects(schema: Schema, expected: SchemaError) {
    assert_eq!(schema.validate(), Err(expected));
}

#[test]
fn duplicate_variables_are_rejected() {
    let mut schema = observations();
    schema.variables.push(schema.variables[0].clone());
    rejects(
        schema,
        SchemaError::DuplicateVariable {
            variable: VarId::new("t"),
        },
    );
}

#[test]
fn mixed_axes_are_rejected() {
    let mut schema = observations();
    schema.variables[0]
        .dims
        .push(VariableAxis::Positional(Extent::Known(2)));
    rejects(
        schema,
        SchemaError::MixedAxisNames {
            variable: VarId::new("t"),
        },
    );
}

#[test]
fn array_rules_are_enforced() {
    let mut schema = observations();
    schema.result = ResultKind::Array(ArrayResult {
        variable: VarId::new("missing"),
    });
    rejects(
        schema,
        SchemaError::UnknownPayload {
            variable: VarId::new("missing"),
        },
    );

    let mut schema = observations();
    schema.result = ResultKind::Array(ArrayResult {
        variable: VarId::new("time"),
    });
    rejects(
        schema,
        SchemaError::ExtraDataInArray {
            variable: VarId::new("t"),
        },
    );
}

#[test]
fn positional_axes_are_rejected_in_datasets_and_coordinates() {
    let positional = vec![VariableAxis::Positional(Extent::Known(3))];
    let mut schema = observations();
    schema
        .variables
        .push(variable("x", positional.clone(), VariableRole::Data));
    rejects(
        schema,
        SchemaError::PositionalAxisInDataset {
            variable: VarId::new("x"),
        },
    );

    let schema = Schema {
        result: ResultKind::Array(ArrayResult {
            variable: VarId::new("x"),
        }),
        variables: vec![
            variable("x", positional.clone(), VariableRole::Data),
            variable(
                "c",
                positional,
                VariableRole::Coordinate(CoordinateRole::Auxiliary),
            ),
        ],
        sizes: vec![],
    };
    rejects(
        schema,
        SchemaError::PositionalCoordinate {
            variable: VarId::new("c"),
        },
    );
}

#[test]
fn index_coordinates_span_exactly_their_dimension_once() {
    let mut schema = observations();
    schema.variables[1].dims = named(&["time", "lat"]);
    rejects(
        schema,
        SchemaError::IndexCoordinateAxes {
            variable: VarId::new("time"),
            dim: DimId::new("time"),
        },
    );

    let mut schema = observations();
    schema
        .variables
        .push(variable("time2", named(&["time"]), index("time")));
    rejects(
        schema,
        SchemaError::DuplicateIndex {
            dim: DimId::new("time"),
        },
    );
}

#[test]
fn sizes_list_each_spanned_dimension_once_in_first_appearance_order() {
    let mut schema = observations();
    schema.sizes.push(size("time", 365));
    rejects(
        schema,
        SchemaError::DuplicateSize {
            dim: DimId::new("time"),
        },
    );

    let mut schema = observations();
    schema.sizes.pop();
    rejects(
        schema,
        SchemaError::MissingSize {
            dim: DimId::new("lat"),
        },
    );

    let mut schema = observations();
    schema.sizes.push(size("depth", 10));
    rejects(
        schema,
        SchemaError::UnusedSize {
            dim: DimId::new("depth"),
        },
    );

    let mut schema = observations();
    schema.sizes.reverse();
    rejects(
        schema,
        SchemaError::SizesOutOfOrder {
            expected: vec![DimId::new("time"), DimId::new("lat")],
            found: vec![DimId::new("lat"), DimId::new("time")],
        },
    );
}

#[test]
fn a_variable_may_not_span_a_dimension_twice() {
    let mut schema = observations();
    schema.variables[0].dims = named(&["time", "time"]);
    schema.sizes.pop();
    rejects(
        schema,
        SchemaError::RepeatedDimension {
            variable: VarId::new("t"),
            dim: DimId::new("time"),
        },
    );
}

#[test]
fn array_coordinates_span_only_the_payload_dimensions() {
    // A named payload: the coordinate spans `lat`, which the payload `t(time)` doesn't.
    let schema = Schema {
        result: ResultKind::Array(ArrayResult {
            variable: VarId::new("t"),
        }),
        variables: vec![
            variable("t", named(&["time"]), VariableRole::Data),
            variable(
                "c",
                named(&["lat"]),
                VariableRole::Coordinate(CoordinateRole::Auxiliary),
            ),
        ],
        sizes: vec![size("time", 365), size("lat", 3)],
    };
    rejects(
        schema,
        SchemaError::CoordinateOutsidePayload {
            variable: VarId::new("c"),
            dim: DimId::new("lat"),
        },
    );

    // A positional payload may carry zero-dimensional coordinates only.
    let positional = vec![VariableAxis::Positional(Extent::Known(3))];
    let schema = Schema {
        result: ResultKind::Array(ArrayResult {
            variable: VarId::new("x"),
        }),
        variables: vec![
            variable("x", positional.clone(), VariableRole::Data),
            variable(
                "c",
                named(&["lat"]),
                VariableRole::Coordinate(CoordinateRole::Auxiliary),
            ),
        ],
        sizes: vec![size("lat", 3)],
    };
    rejects(
        schema,
        SchemaError::CoordinateOutsidePayload {
            variable: VarId::new("c"),
            dim: DimId::new("lat"),
        },
    );

    let schema = Schema {
        result: ResultKind::Array(ArrayResult {
            variable: VarId::new("x"),
        }),
        variables: vec![
            variable("x", positional, VariableRole::Data),
            variable(
                "c",
                vec![],
                VariableRole::Coordinate(CoordinateRole::Auxiliary),
            ),
        ],
        sizes: vec![],
    };
    assert_eq!(schema.validate(), Ok(()));
}
