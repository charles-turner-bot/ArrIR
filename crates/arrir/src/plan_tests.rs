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
    assert!(serde_json::from_str::<Plan>(json).is_err());
}

#[test]
fn import_rejects_an_unknown_version() {
    let mut json = serde_json::to_value(shared_input_plan()).unwrap();
    json["version"] = "1".into();
    assert!(serde_json::from_value::<Plan>(json).is_err());
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

/// dag.md's "A rewrite that keeps old roots": push a selection below a shared add.
#[test]
fn rewriting_the_root_keeps_the_old_root() {
    let mut plan = shared_input_plan();
    plan.graph.append(unary("select", 2)).unwrap(); // node 3, the old root
    let before = plan.graph.clone();

    let select = plan.graph.append(unary("select", 1)).unwrap(); // node 4
    let add = plan.graph.append(binary("add", 4, 4)).unwrap(); // node 5
    let new_root = plan.graph.replace(NodeId(3), NodeId(3), add).unwrap();

    assert_eq!((select, add, new_root), (NodeId(4), NodeId(5), NodeId(5)));
    assert_eq!(
        &plan.graph.nodes()[..4],
        before.nodes(),
        "old nodes unchanged"
    );
    let live = plan.graph.live_nodes(new_root);
    assert_eq!(live, vec![true, true, false, false, true, true]);
}

/// dag.md's "Rewriting below the root": the `Reduce` above the selection is copied.
#[test]
fn rewriting_below_the_root_copies_the_path_and_reuses_the_rest() {
    let mut graph = graph(vec![
        open("obs"),
        unary("project a", 0),
        unary("project b", 0),
        binary("add", 1, 2),
        unary("select", 3),
        unary("reduce", 4),
    ]);
    let before = graph.clone();
    graph.append(unary("select", 1)).unwrap(); // 6
    graph.append(unary("select", 2)).unwrap(); // 7
    let add = graph.append(binary("add", 6, 7)).unwrap(); // 8

    let new_root = graph.replace(NodeId(5), NodeId(4), add).unwrap();

    assert_eq!(new_root, NodeId(9));
    assert_eq!(graph.node(new_root), &unary("reduce", 8));
    assert_eq!(&graph.nodes()[..6], before.nodes(), "root 5 is unchanged");
    let live: Vec<usize> = graph
        .live_nodes(new_root)
        .iter()
        .enumerate()
        .filter(|(_, live)| **live)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(live, vec![0, 1, 2, 6, 7, 8, 9]);
}

#[test]
fn a_shared_node_off_the_path_is_reused_not_copied() {
    // Both inputs of the add read node 1. Replacing node 2 copies only the add.
    let mut graph = graph(vec![
        open("obs"),
        unary("project", 0),
        unary("select", 1),
        binary("add", 2, 1),
    ]);
    let other = graph.append(unary("other select", 1)).unwrap(); // 4
    let new_root = graph.replace(NodeId(3), NodeId(2), other).unwrap();
    assert_eq!(graph.node(new_root), &binary("add", 4, 1));
    assert_eq!(graph.len(), 6);
}
