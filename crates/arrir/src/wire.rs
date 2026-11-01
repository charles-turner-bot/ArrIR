//! The wire form of a plan (`plans/dag.md`, "Wire form").
//!
//! `Plan` serialises through `WirePlan`: nodes in ID order, each with its `id`
//! written out for readability. Import checks that each `id` is the node's position,
//! then rebuilds the graph with `Graph::append` and validates the plan.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::plan::{check_node, Graph, Node, NodeId, Operation, Plan, PlanError};

/// The only wire version so far.
pub const WIRE_VERSION: &str = "0";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WirePlan {
    pub version: String,
    pub nodes: Vec<WireNode>,
    pub root: NodeId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireNode {
    pub id: NodeId,
    pub op: Operation,
    pub inputs: Vec<NodeId>,
}

/// Why a wire plan can't be imported.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ImportError {
    #[error("unknown wire version {0:?}")]
    UnknownVersion(String),
    #[error("node at position {position} is written with id {written:?}")]
    IdNotPosition { position: usize, written: NodeId },
    #[error(transparent)]
    Plan(#[from] PlanError),
}

impl From<Plan> for WirePlan {
    fn from(plan: Plan) -> Self {
        let nodes = plan
            .graph
            .nodes()
            .iter()
            .enumerate()
            .map(|(index, node)| WireNode {
                id: NodeId(index as u32),
                op: node.operation.clone(),
                inputs: node.inputs.clone(),
            })
            .collect();
        WirePlan {
            version: WIRE_VERSION.to_string(),
            nodes,
            root: plan.root,
        }
    }
}

impl TryFrom<WirePlan> for Plan {
    type Error = ImportError;

    fn try_from(wire: WirePlan) -> Result<Self, Self::Error> {
        if wire.version != WIRE_VERSION {
            return Err(ImportError::UnknownVersion(wire.version));
        }
        let total = wire.nodes.len();
        let mut graph = Graph::new();
        for (position, wire_node) in wire.nodes.into_iter().enumerate() {
            if wire_node.id.index() != position {
                return Err(ImportError::IdNotPosition {
                    position,
                    written: wire_node.id,
                });
            }
            let node = Node {
                operation: wire_node.op,
                inputs: wire_node.inputs,
            };
            // Checked against the whole graph's length, so an input to a later node
            // is a forward input and one past the end is dangling (dag.md).
            check_node(wire_node.id, &node, total)?;
            graph.append(node)?;
        }
        let plan = Plan {
            graph,
            root: wire.root,
        };
        plan.validate()?;
        Ok(plan)
    }
}
