//! Plans as DAGs: an append-only node arena, ordered inputs and one root
//! (`plans/dag.md`).

use std::collections::BTreeMap;

use arrir_types::BindingId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A node's position in its graph. Never reused. On the wire it is a JSON number.
///
/// The field is public on purpose: a `NodeId` is only a position, and every place
/// that takes one checks it against a graph (`append`, `validate`, import).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub u32);

impl NodeId {
    /// The node's position in the graph's node list.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// What a node computes. Real operations land one PR at a time (`plans/design.md`,
/// roadmap). Until the first one does, the only variants are test placeholders, so
/// outside tests this enum has no values and no graph can be built.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Operation {
    /// Test stand-in for `Open`: no inputs, opens one binding.
    #[cfg(test)]
    TestOpen { binding: BindingId },
    /// Test stand-in for a one-input operation such as `Select`.
    #[cfg(test)]
    TestUnary { label: String },
    /// Test stand-in for a two-input operation such as elementwise `Add`.
    #[cfg(test)]
    TestBinary { label: String },
}

// The matches below are on `*self` with `ref` bindings, not on `self`. Outside tests
// `Operation` has no variants, and `match self` on a reference to an empty enum doesn't
// compile, while `match *self` does. Keep this form until the first real operation.
impl Operation {
    /// How many inputs this operation value takes (rule 3).
    pub fn arity(&self) -> Arity {
        match *self {
            #[cfg(test)]
            Operation::TestOpen { .. } => Arity::Exactly(0),
            #[cfg(test)]
            Operation::TestUnary { .. } => Arity::Exactly(1),
            #[cfg(test)]
            Operation::TestBinary { .. } => Arity::Exactly(2),
        }
    }

    /// The binding this operation opens, if it is an `Open` (rule 6).
    pub fn opened_binding(&self) -> Option<&BindingId> {
        match *self {
            #[cfg(test)]
            Operation::TestOpen { ref binding } => Some(binding),
            #[cfg(test)]
            Operation::TestUnary { .. } | Operation::TestBinary { .. } => None,
        }
    }
}

/// The number of inputs an operation takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arity {
    Exactly(usize),
    AtLeast(usize),
}

impl Arity {
    fn accepts(self, found: usize) -> bool {
        match self {
            Arity::Exactly(n) => found == n,
            Arity::AtLeast(n) => found >= n,
        }
    }
}

/// One operation and its ordered inputs. Its ID is its position, so it isn't stored.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub operation: Operation,
    /// `inputs[0]` is the first operand; each operation documents each position.
    pub inputs: Vec<NodeId>,
}

/// A structural error in a graph or plan. Validation reports the first one found.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum PlanError {
    #[error("the graph has no nodes")]
    Empty,
    #[error("node {node:?} has input {input:?}, which doesn't exist")]
    DanglingInput { node: NodeId, input: NodeId },
    #[error("node {node:?} has input {input:?}, which isn't an earlier node")]
    ForwardInput { node: NodeId, input: NodeId },
    #[error("node {node:?} expects {expected:?} inputs, found {found}")]
    WrongArity {
        node: NodeId,
        expected: Arity,
        found: usize,
    },
    #[error("root {root:?} isn't a node in the graph")]
    RootOutOfRange { root: NodeId },
    #[error("binding {binding:?} is opened by both {first:?} and {second:?}")]
    DuplicateBinding {
        binding: BindingId,
        first: NodeId,
        second: NodeId,
    },
}

/// Why a rewrite can't be made. Structural errors stay in [`PlanError`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum RewriteError {
    #[error("node {0:?} isn't in the graph")]
    NotInGraph(NodeId),
    #[error("node {0:?} isn't reachable from the root, so replacing it changes nothing")]
    TargetNotLive(NodeId),
}

/// The node arena. The field is private, so nodes can be added but never changed or
/// removed (rule 1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    nodes: Vec<Node>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The nodes in ID order.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The node with this ID.
    ///
    /// # Panics
    ///
    /// If the ID isn't in the graph, like slice indexing.
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// Checks the node's inputs (rule 2) and arity (rule 3), then returns its new ID.
    pub fn append(&mut self, node: Node) -> Result<NodeId, PlanError> {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("graph exceeds u32::MAX nodes"));
        check_node(id, &node, self.nodes.len() + 1)?;
        self.nodes.push(node);
        Ok(id)
    }

    /// Which nodes are live, that is reachable from `root`, indexed by `NodeId`.
    /// Inputs point backwards, so one pass from the root down finds them all.
    ///
    /// # Panics
    ///
    /// If `root` isn't in the graph. `Plan::validate` checks the root first.
    pub fn live_nodes(&self, root: NodeId) -> Vec<bool> {
        let mut live = vec![false; self.nodes.len()];
        live[root.index()] = true;
        for index in (0..=root.index()).rev() {
            if live[index] {
                for input in &self.nodes[index].inputs {
                    live[input.index()] = true;
                }
            }
        }
        live
    }

    /// Replaces `target` with `replacement` in the plan rooted at `root`, and returns
    /// the new root (`plans/dag.md`, "Rewrites"). The caller appends the replacement
    /// nodes first. Every live node that depends on `target`, directly or through
    /// other nodes, is appended again with its inputs updated (path copying); every
    /// other node is reused. No existing node changes, so `root` still names the
    /// unrewritten plan.
    ///
    /// Every live use of `target` on the copied path is substituted, including uses
    /// inside `replacement`'s own dependants: replacing 1 with 2 in `B(1, U(1))`
    /// gives `B(2, U(2))`. Replacing a node with itself returns `root` unchanged.
    ///
    /// Fails if `root`, `target` or `replacement` isn't in the graph, or if `target`
    /// isn't live from `root`, so a rewrite that would change nothing is an error.
    // Outside tests `Operation` has no values yet, so the compiler sees that the copy
    // below can never be made and warns. The `allow` goes with the first real operation.
    #[cfg_attr(not(test), allow(unreachable_code, unused_variables, unused_mut))]
    pub fn replace(
        &mut self,
        root: NodeId,
        target: NodeId,
        replacement: NodeId,
    ) -> Result<NodeId, RewriteError> {
        for id in [root, target, replacement] {
            if id.index() >= self.nodes.len() {
                return Err(RewriteError::NotInGraph(id));
            }
        }
        let live = self.live_nodes(root);
        if !live[target.index()] {
            return Err(RewriteError::TargetNotLive(target));
        }
        if replacement == target {
            return Ok(root);
        }
        // Maps each replaced or copied node to the node that now stands for it.
        let mut replaced: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        replaced.insert(target, replacement);
        // Only nodes after `target` can depend on it, and only live ones are copied.
        // `live` covers the graph as it was, so the copies appended below are skipped.
        for (index, &is_live) in live.iter().enumerate() {
            if index <= target.index() || !is_live {
                continue;
            }
            let node = &self.nodes[index];
            if !node.inputs.iter().any(|i| replaced.contains_key(i)) {
                continue;
            }
            let mut copy = node.clone();
            for input in copy.inputs.iter_mut() {
                if let Some(&new_input) = replaced.get(input) {
                    *input = new_input;
                }
            }
            let new_id = self
                .append(copy)
                .expect("a copy's inputs are existing nodes, so it is valid");
            replaced.insert(NodeId(index as u32), new_id);
        }
        Ok(*replaced.get(&root).unwrap_or(&root))
    }
}

/// Checks rules 2 and 3 for node `id` in a graph of `graph_len` nodes, in dag.md's
/// order: dangling inputs (past the end of the graph), then forward inputs (at or after
/// `id`, a self-loop included), then arity.
pub(crate) fn check_node(id: NodeId, node: &Node, graph_len: usize) -> Result<(), PlanError> {
    for &input in &node.inputs {
        if input.index() >= graph_len {
            return Err(PlanError::DanglingInput { node: id, input });
        }
    }
    for &input in &node.inputs {
        if input >= id {
            return Err(PlanError::ForwardInput { node: id, input });
        }
    }
    let expected = node.operation.arity();
    if !expected.accepts(node.inputs.len()) {
        return Err(PlanError::WrongArity {
            node: id,
            expected,
            found: node.inputs.len(),
        });
    }
    Ok(())
}

/// What is exported and executed: a graph and the node whose value is the result.
///
/// It serialises through the wire form in `wire.rs`. Deserialising validates it;
/// serialising doesn't, so build plans with [`Plan::new`] or call [`Plan::validate`]
/// before export. The fields are public, as dag.md writes them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "crate::wire::WirePlan", try_from = "crate::wire::WirePlan")]
pub struct Plan {
    pub graph: Graph,
    pub root: NodeId,
}

impl Plan {
    /// A validated plan.
    pub fn new(graph: Graph, root: NodeId) -> Result<Plan, PlanError> {
        let plan = Plan { graph, root };
        plan.validate()?;
        Ok(plan)
    }

    /// Checks the root, and that each binding is opened by at most one live node
    /// (rule 6). Node rules 2 and 3 already hold, because `Graph::append` checks them.
    pub fn validate(&self) -> Result<(), PlanError> {
        if self.graph.is_empty() {
            return Err(PlanError::Empty);
        }
        if self.root.index() >= self.graph.len() {
            return Err(PlanError::RootOutOfRange { root: self.root });
        }
        let live = self.graph.live_nodes(self.root);
        let mut opened: BTreeMap<&BindingId, NodeId> = BTreeMap::new();
        for (index, node) in self.graph.nodes().iter().enumerate() {
            let Some(binding) = node.operation.opened_binding() else {
                continue;
            };
            if !live[index] {
                continue;
            }
            let id = NodeId(index as u32);
            if let Some(&first) = opened.get(binding) {
                return Err(PlanError::DuplicateBinding {
                    binding: binding.clone(),
                    first,
                    second: id,
                });
            }
            opened.insert(binding, id);
        }
        Ok(())
    }
}
