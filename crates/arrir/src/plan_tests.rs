//! Tests for the plan container, following #28's acceptance and dag.md's examples.
//! The operations are the test placeholders on `Operation`.

use arrir_types::BindingId;

use crate::plan::{Arity, Graph, Node, NodeId, Operation, Plan, PlanError};
use crate::wire::ImportError;

fn open(binding: &str) -> Node {
    Node {
        operation: Operation::TestOpen {
            binding: BindingId::new(binding),
        },
        inputs: vec![],
    }
}

fn unary(label: &str, input: u32) -> Node {
    Node {
        operation: Operation::TestUnary {
            label: label.to_string(),
        },
        inputs: vec![NodeId(input)],
    }
}

fn binary(label: &str, left: u32, right: u32) -> Node {
    Node {
        operation: Operation::TestBinary {
            label: label.to_string(),
        },
        inputs: vec![NodeId(left), NodeId(right)],
    }
}

/// Builds a graph from nodes, panicking on a structural error.
fn graph(nodes: Vec<Node>) -> Graph {
    let mut graph = Graph::new();
    for node in nodes {
        graph.append(node).unwrap();
    }
    graph
}

/// dag.md's `Add(shared, shared)`: open, project, add with one input used twice.
fn shared_input_plan() -> Plan {
    Plan {
        graph: graph(vec![
            open("obs"),
            unary("project t", 0),
            binary("add", 1, 1),
        ]),
        root: NodeId(2),
    }
}

#[test]
fn append_returns_positions_as_ids() {
    let mut graph = Graph::new();
    assert_eq!(graph.append(open("obs")).unwrap(), NodeId(0));
    assert_eq!(graph.append(unary("project", 0)).unwrap(), NodeId(1));
}

#[test]
fn unary_binary_and_shared_graphs_round_trip_deterministically() {
    let unary_plan = Plan {
        graph: graph(vec![open("obs"), unary("project t", 0)]),
        root: NodeId(1),
    };
    let binary_plan = Plan {
        graph: graph(vec![
            open("obs"),
            unary("project a", 0),
            unary("project b", 0),
            binary("add", 1, 2),
        ]),
        root: NodeId(3),
    };
    for plan in [unary_plan, binary_plan, shared_input_plan()] {
        let first = serde_json::to_string(&plan).unwrap();
        let second = serde_json::to_string(&plan).unwrap();
        assert_eq!(first, second);
        let back: Plan = serde_json::from_str(&first).unwrap();
        assert_eq!(back, plan);
    }
}

#[test]
fn wire_form_writes_ids_and_node_ids_as_numbers() {
    let json = serde_json::to_value(shared_input_plan()).unwrap();
    assert_eq!(json["version"], "0");
    assert_eq!(json["root"], 2);
    assert_eq!(json["nodes"][2]["id"], 2);
    assert_eq!(json["nodes"][2]["inputs"], serde_json::json!([1, 1]));
}

#[test]
fn append_rejects_dangling_forward_and_self_inputs() {
    let mut graph = graph(vec![open("obs")]);
    assert_eq!(
        graph.append(unary("select", 7)),
        Err(PlanError::DanglingInput {
            node: NodeId(1),
            input: NodeId(7)
        })
    );
    assert_eq!(
        graph.append(unary("select", 1)),
        Err(PlanError::ForwardInput {
            node: NodeId(1),
            input: NodeId(1)
        })
    );
    assert_eq!(graph.len(), 1, "a rejected node is not added");
}

#[test]
fn append_rejects_wrong_arity() {
    let mut graph = graph(vec![open("obs")]);
    let no_inputs = Node {
        operation: Operation::TestUnary {
            label: "select".to_string(),
        },
        inputs: vec![],
    };
    assert_eq!(
        graph.append(no_inputs),
        Err(PlanError::WrongArity {
            node: NodeId(1),
            expected: Arity::Exactly(1),
            found: 0
        })
    );
}

/// dag.md's "Cycles and dangling inputs": node 1 reads node 2, which reads node 1.
#[test]
fn import_rejects_a_cycle_as_a_forward_input() {
    let json = r#"{
        "version": "0",
        "nodes": [
            { "id": 0, "op": { "TestOpen": { "binding": "obs" } }, "inputs": [] },
            { "id": 1, "op": { "TestUnary": { "label": "project" } }, "inputs": [2] },
            { "id": 2, "op": { "TestUnary": { "label": "reduce" } }, "inputs": [1] }
        ],
        "root": 2
    }"#;
    let error = serde_json::from_str::<Plan>(json).unwrap_err().to_string();
    let expected = ImportError::Plan(PlanError::ForwardInput {
        node: NodeId(1),
        input: NodeId(2),
    });
    assert_eq!(error, expected.to_string());
}

#[test]
fn import_rejects_an_id_that_isnt_its_position() {
    let json = r#"{
        "version": "0",
        "nodes": [{ "id": 3, "op": { "TestOpen": { "binding": "obs" } }, "inputs": [] }],
        "root": 0
    }"#;
    let error = serde_json::from_str::<Plan>(json).unwrap_err().to_string();
    let expected = ImportError::IdNotPosition {
        position: 0,
        written: NodeId(3),
    };
    assert_eq!(error, expected.to_string());
}

#[test]
fn import_rejects_an_unknown_version() {
    let mut json = serde_json::to_value(shared_input_plan()).unwrap();
    json["version"] = "1".into();
    let error = serde_json::from_value::<Plan>(json)
        .unwrap_err()
        .to_string();
    assert_eq!(error, ImportError::UnknownVersion("1".into()).to_string());
}

/// Imports a plan from a list of nodes written as JSON, with root `root`.
fn import(nodes: &str, root: u32) -> Result<Plan, String> {
    let json = format!(r#"{{ "version": "0", "nodes": [{nodes}], "root": {root} }}"#);
    serde_json::from_str::<Plan>(&json).map_err(|error| error.to_string())
}

const OPEN_OBS: &str = r#"{ "id": 0, "op": { "TestOpen": { "binding": "obs" } }, "inputs": [] }"#;

#[test]
fn import_rejects_a_dangling_input() {
    let select = r#"{ "id": 1, "op": { "TestUnary": { "label": "select" } }, "inputs": [5] }"#;
    let expected = ImportError::Plan(PlanError::DanglingInput {
        node: NodeId(1),
        input: NodeId(5),
    });
    assert_eq!(
        import(&format!("{OPEN_OBS}, {select}"), 1).unwrap_err(),
        expected.to_string()
    );
}

#[test]
fn import_rejects_wrong_arity() {
    let add = r#"{ "id": 1, "op": { "TestBinary": { "label": "add" } }, "inputs": [0] }"#;
    let expected = ImportError::Plan(PlanError::WrongArity {
        node: NodeId(1),
        expected: Arity::Exactly(2),
        found: 1,
    });
    assert_eq!(
        import(&format!("{OPEN_OBS}, {add}"), 1).unwrap_err(),
        expected.to_string()
    );
}

#[test]
fn import_accepts_a_dead_duplicate_open() {
    let reopen = r#"{ "id": 1, "op": { "TestOpen": { "binding": "obs" } }, "inputs": [] }"#;
    let plan = import(&format!("{OPEN_OBS}, {reopen}"), 1).unwrap();
    assert_eq!(plan.graph.len(), 2);
}

#[test]
fn import_rejects_two_live_opens_of_one_binding() {
    let reopen = r#"{ "id": 1, "op": { "TestOpen": { "binding": "obs" } }, "inputs": [] }"#;
    let add = r#"{ "id": 2, "op": { "TestBinary": { "label": "add" } }, "inputs": [0, 1] }"#;
    let expected = ImportError::Plan(PlanError::DuplicateBinding {
        binding: BindingId::new("obs"),
        first: NodeId(0),
        second: NodeId(1),
    });
    assert_eq!(
        import(&format!("{OPEN_OBS}, {reopen}, {add}"), 2).unwrap_err(),
        expected.to_string()
    );
}

#[test]
fn import_rejects_a_root_out_of_range() {
    let expected = ImportError::Plan(PlanError::RootOutOfRange { root: NodeId(4) });
    assert_eq!(import(OPEN_OBS, 4).unwrap_err(), expected.to_string());
    assert_eq!(
        expected.to_string(),
        "root 4 isn't a node in the graph",
        "messages print bare positions"
    );
}

#[test]
fn import_rejects_an_empty_node_list() {
    let expected = ImportError::Plan(PlanError::Empty);
    assert_eq!(import("", 0).unwrap_err(), expected.to_string());
}

#[test]
fn import_rejects_an_unknown_field() {
    let json = r#"{ "version": "0", "nodes": [], "root": 0, "comment": "hi" }"#;
    let error = serde_json::from_str::<Plan>(json).unwrap_err().to_string();
    assert!(error.contains("unknown field `comment`"), "{error}");
}

#[test]
fn import_rejects_an_unknown_operation() {
    let mystery = r#"{ "id": 0, "op": { "Mystery": {} }, "inputs": [] }"#;
    let error = import(mystery, 0).unwrap_err();
    assert!(error.contains("unknown variant `Mystery`"), "{error}");
}

/// dag.md checks dangling inputs before forward ones, so `[2, 9]` on node 1 reports 9.
#[test]
fn a_dangling_input_is_reported_before_a_forward_one() {
    let mut graph = graph(vec![open("a"), open("b")]);
    let error = graph.append(binary("add", 2, 9)).unwrap_err();
    assert_eq!(
        error,
        PlanError::DanglingInput {
            node: NodeId(2),
            input: NodeId(9)
        }
    );
}

#[test]
fn plan_new_validates() {
    assert_eq!(Plan::new(Graph::new(), NodeId(3)), Err(PlanError::Empty));
    let plan = shared_input_plan();
    assert_eq!(Plan::new(plan.graph.clone(), plan.root), Ok(plan));
}

#[test]
fn validate_rejects_empty_graphs_and_roots_out_of_range() {
    let empty = Plan {
        graph: Graph::new(),
        root: NodeId(0),
    };
    assert_eq!(empty.validate(), Err(PlanError::Empty));
    let mut plan = shared_input_plan();
    plan.root = NodeId(9);
    assert_eq!(
        plan.validate(),
        Err(PlanError::RootOutOfRange { root: NodeId(9) })
    );
}

#[test]
fn validate_rejects_two_live_opens_of_one_binding() {
    let plan = Plan {
        graph: graph(vec![open("obs"), open("obs"), binary("add", 0, 1)]),
        root: NodeId(2),
    };
    assert_eq!(
        plan.validate(),
        Err(PlanError::DuplicateBinding {
            binding: BindingId::new("obs"),
            first: NodeId(0),
            second: NodeId(1)
        })
    );
}

#[test]
fn dead_opens_dont_count_as_duplicates() {
    // Node 2 re-opens `obs` for a rewrite; node 0's branch is now unreachable.
    let plan = Plan {
        graph: graph(vec![open("obs"), unary("project", 0), open("obs")]),
        root: NodeId(2),
    };
    assert_eq!(plan.validate(), Ok(()));
}

#[test]
fn unreachable_nodes_are_not_live() {
    let graph = graph(vec![
        open("obs"),
        unary("select", 0),
        unary("unused", 0),
        unary("reduce", 1),
    ]);
    assert_eq!(graph.live_nodes(NodeId(3)), vec![true, true, false, true]);
}
