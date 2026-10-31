# Plans as DAGs

Status: proposal, 31 October 2026, for issue #27. Pending review. It replaces the
"start with linear unary chains" decision in [design.md](design.md#decisions) only if
approved. Nothing here is implemented.

## Why a chain isn't enough

A linear unary chain can express `Open → Select → Reduce`, but not `mean(a + b)`, a
computed mask, weights, or one operand used twice. Every multi-input operation the
backlog proposes (#32 Elementwise, #34 alignment, #36 computed masks) needs a second
input. Adding them to a chain would mean a side channel for the extra inputs, which is a
graph with worse rules.

A chain is a special case of the graph below, so the first vertical slice
(`Open → Select → Reduce`) keeps its content. It gains node IDs and input lists, nothing
else.

## Dependencies on open PRs

- **#15** (workspace): `BindingId`, `VarId` and `DimId` from `arrir-types`. The new
  `NodeId` lives in `arrir`, not `arrir-types`, because schemas never refer to nodes.
- **#26** (schema and values): each node's output is a `Schema` with a `ResultKind`.
  This proposal uses #26's `Schema`, `ResultKind`, `Extent` and `TypeKnowledge` as
  written there and adds no schema types. If #26 changes, the examples change with it.
- **#16** (shells): its purity rules and conventions are already in the contract. Its
  per-operation effects become per-node effects here, with one footprint per input.
  Nothing in this proposal depends on #16's code.

## The plan

```rust
/// A node's position in its plan's arena. Plan-local and never reused.
pub struct NodeId(u32);

pub struct Node {
    pub id: NodeId,
    pub operation: Operation,
    /// Ordered. `inputs[0]` is the first operand; each operation documents the
    /// meaning of each position.
    pub inputs: Vec<NodeId>,
}

pub struct Plan {
    pub nodes: Vec<Node>,
    pub root: NodeId,
}
```

`Operation` is the existing exhaustive enum (`Open`, `Project`, `Select`, `Reduce`,
`Warp`, `Cast`, and later #32's elementwise family). Operations hold their options;
nodes hold the edges. Data is never embedded: inputs are other nodes, and external
data enters only through `Open`.

### Rules

1. **Append-only.** A node is never changed or removed once added. Its `NodeId` is its
   index in `nodes`.
2. **Inputs point backwards.** Every input of node `n` is less than `n`. This makes
   cycles unrepresentable: id order is a topological order, and validation is one pass
   with no graph search.
3. **Arity is declared by the operation.**

   ```rust
   pub enum Arity {
       Exactly(usize),
       AtLeast(usize),
   }
   ```

   `Open` is `Exactly(0)`, `Project`, `Reduce` and `Cast` are `Exactly(1)`, a binary
   elementwise function is `Exactly(2)`, a `Select` with a computed mask (#36) is
   `Exactly(2)`, and a future `Concat` is `AtLeast(1)`. The input count must match.
4. **One root.** A plan computes one result, the value of `root`. A host wanting
   several results builds several plans over the same nodes (see
   [questions](#questions-for-review)).
5. **Sharing is explicit.** `Add` with inputs `[1, 1]` uses one value twice. Operations
   are pure, so an adapter may evaluate a shared node once or twice; how often is
   execution policy, not meaning.
6. **One `Open` per binding.** A `BindingId` is opened by at most one node. A second
   use of the same data refers to that node. This rules out two `Open`s of one binding
   with different declared schemas.
7. **No implicit merging.** Two structurally equal nodes stay two nodes. Merging them
   (common subexpression elimination) is a rewrite, never a side effect of building.

### Structural validation

Validation checks every node, reachable or not, and reports the first failure with
its node ID:

```rust
pub enum PlanError {
    Empty,
    IdNotPosition { position: u32, id: NodeId },
    DanglingInput { node: NodeId, input: NodeId },   // input >= nodes.len()
    ForwardInput { node: NodeId, input: NodeId },    // node <= input < nodes.len()
    WrongArity { node: NodeId, expected: Arity, found: usize },
    DuplicateBinding { binding: BindingId, first: NodeId, second: NodeId },
    RootOutOfRange { root: NodeId },
}
```

A self-loop is a `ForwardInput` whose input equals its node. These are structural
errors only. Whether facts are known enough for an operation (semantic readiness) and
whether an engine can run it (adapter acceptance) are the separate questions #26
defines.

### Reachability and deletion

The *live* plan is the set of nodes reachable from `root`. Unreachable nodes are
logically deleted: schema propagation, readiness checks, rewrites, adapters and
execution ignore them. They still pass structural validation, so the arena stays sound.

**Compaction** is optional and explicit. It keeps the live nodes, renumbers them in
post-order (a depth-first walk from `root`, visiting inputs in order, numbering a node
after its inputs) and returns the old-to-new mapping. Compaction is the one operation
that invalidates node IDs, which is why it returns the mapping and never runs
implicitly. Two plans that describe the same graph compact to identical arenas, so the
compacted form is the canonical one for comparison and fixtures.

### Rewrites keep old IDs and roots

A rewrite appends replacement nodes and returns a plan with a new root. Existing nodes
are untouched, so:

- a host holding an earlier `NodeId` still names the same computation;
- the old root is still a valid root for the unrewritten plan;
- a node shared with another consumer is unaffected when one consumer's path is
  rewritten. This is what makes pushdown safe in the presence of sharing.

xrexpr's termination measure ("no rule lengthens a plan or moves an operation
rightwards") is defined on chains. Pushing a `Select` below an `Add` adds a node, so the
measure needs restating for graphs. That belongs to the first rewrite proof (#37), not
to this contract.

### Derived schemas and caching

Only `Open` carries a schema, its declared one. Every other node's output schema is
derived from its operation and its inputs' schemas, and is never stored on the wire
(design.md: "do not cache derived effects").

An implementation may cache derived schemas in a vector indexed by `NodeId`. Because
nodes never change, an entry never goes stale: appending nodes extends the cache, a
rewrite reuses every existing entry, and only compaction or import discards it. No
invalidation logic is needed.

Effects and footprints (purity rule 3) become per node. A footprint maps an output
region to one input region per input, in input order.

## Open, projection and result kinds

`Open { binding, schema }` keeps its contract: it binds a dataset, and its declared
schema has `ResultKind::Dataset`. A binding that holds a single array is a dataset with
one data variable. Choosing a variable is a `Project` node, whose output has
`ResultKind::Array`. One `Open` can feed several projections, which is how two variables
from one file meet in one expression.

Each operation states which result kinds it accepts. Elementwise arithmetic on arrays
comes first; dataset arithmetic (xarray applies it per variable) needs its own decision
in #32, and until then it's rejected rather than guessed.

`Warp`'s target geometry stays a `GeometryBinding` field ([warp.md](warp.md)) for now.
It's a declared requirement on external geometry, not a value computed in the plan.
Whether it becomes an `Open` input is a review question below.

## Python table or Rust arena

The issue asks whether the graph should be an immutable Python node table or a Rust
`Vec<Node>` arena.

| | Append-only Python table | Rust `Vec<Node>` arena |
| --- | --- | --- |
| Where validation lives | Python, and again in Rust on import | Rust only |
| Fit with xrexpr | Close to its expression objects | Needs a lowering step from them |
| Other hosts (xarray-ts, R) | Each reimplements it | Each uses one binding |
| Determinism and serde | Per host | One implementation, fixture-tested |
| Learning cost here | None in Rust | An ordinary `Vec` and index type |

**Recommendation: the Rust arena is the plan.** Hosts keep their own expression
objects and lower them by appending to a Rust builder, which returns a `NodeId` for each
node. A Python list of node records that mirrors the wire format is fine as interchange,
but it isn't a second validator. The arena needs no lifetimes or pointers: a node refers
to its inputs by index.

## Worked examples

Schemas are abbreviated. The wire tags are illustrative and are fixed by the first serde
fixtures, as #26 says.

### `mean(a + b)` from one dataset

Dataset `obs` has data variables `t` and `u`, both `(time, lat)`, with an index
coordinate `time`.

| Node | Operation | Inputs | Derived output |
| --- | --- | --- | --- |
| 0 | `Open { binding: obs, schema }` | — | Dataset `{t, u, time}` as declared |
| 1 | `Project { variable: t }` | 0 | Array `t (time, lat)` |
| 2 | `Project { variable: u }` | 0 | Array `u (time, lat)` |
| 3 | elementwise add (#32) | 1, 2 | Array `(time, lat)`, dtype by #30 |
| 4 | `Reduce { mean over time }` | 3 | Array `(lat)`, dtype by #30 |

Root is 4. Building it reads no data: every row derives from the declared schema of
node 0. One `Open` feeds both operands.

### `Add(shared, shared)`

| Node | Operation | Inputs |
| --- | --- | --- |
| 0 | `Open { binding: obs, schema }` | — |
| 1 | `Project { variable: t }` | 0 |
| 2 | elementwise add | 1, 1 |

Root is 2. Node 2 adds `t` to itself. Two other spellings are worth comparing:

- A second `Project { variable: t }` as node 2, then add `[1, 2]`. Valid. It denotes
  the same value, but proving that is a rewrite's job (rule 7).
- A second `Open { binding: obs }`. Rejected with `DuplicateBinding` (rule 6).

### A rewrite that keeps old roots

Start from `Add(shared, shared)` plus a selection, root 3:

| Node | Operation | Inputs |
| --- | --- | --- |
| 0 | `Open { binding: obs }` | — |
| 1 | `Project { variable: t }` | 0 |
| 2 | elementwise add | 1, 1 |
| 3 | `Select { lat: 0..10 }` | 2 |

Pushing the selection below the add appends two nodes and moves the root to 5:

| Node | Operation | Inputs |
| --- | --- | --- |
| 4 | `Select { lat: 0..10 }` | 1 |
| 5 | elementwise add | 4, 4 |

Nodes 0 to 3 are unchanged, and root 3 still names the original computation. Nodes 2
and 3 are now unreachable from root 5, so they're logically deleted. Compacting from
root 5 visits `0, 1, 4, 5` and returns `{0→0, 1→1, 4→2, 5→3}`; IDs 2 and 3 have no
mapping. Whether this pushdown is legal is #37's to prove; the example shows only the
structure.

### Cycles and dangling inputs

```json
{
  "version": "0",
  "nodes": [
    { "id": 0, "op": { "Open": { "binding": "obs", "schema": "…" } }, "inputs": [] },
    { "id": 1, "op": { "Project": { "variable": "t" } }, "inputs": [2] },
    { "id": 2, "op": { "Reduce": "…" }, "inputs": [1] }
  ],
  "root": 2
}
```

Rejected with `ForwardInput { node: 1, input: 2 }`. The cycle 1 → 2 → 1 can't be
written without a forward input. Changing node 1's inputs to `[1]` gives the same error
for a self-loop, `[7]` gives `DanglingInput { node: 1, input: 7 }`, and `[]` gives
`WrongArity { node: 1, expected: Exactly(1), found: 0 }`.

### Wire form

`Add(shared, shared)` on the wire:

```json
{
  "version": "0",
  "nodes": [
    {
      "id": 0,
      "op": { "Open": { "binding": "obs", "schema": {
        "result": "Dataset",
        "variables": [
          { "id": "t", "dims": ["time", "lat"], "role": "Data", "dtype": { "Known": "Float32" } }
        ],
        "sizes": [["time", "365"], ["lat", "180"]],
        "grids": []
      } } },
      "inputs": []
    },
    { "id": 1, "op": { "Project": { "variable": "t" } }, "inputs": [0] },
    { "id": 2, "op": { "Elementwise": { "function": "Add" } }, "inputs": [1, 1] }
  ],
  "root": 2
}
```

Wire rules:

- Nodes are serialised in id order. Serialising the same arena twice gives the same
  bytes.
- `id` is written out for readability and must equal the node's position.
- `NodeId` is a `u32`, so it's an exact JSON number in every host. Extents and other
  64-bit values stay decimal strings, as #26 says.
- Dead nodes may appear on the wire. Export to an adapter or a fixture compacts first.
- An unknown `version` or operation tag fails import.

## Not in this proposal

No scheduler, evaluation order, kernels, memoisation or embedded data. No elementwise
vocabulary (#32), broadcasting (#29), dtype rules (#30) or index alignment (#34). The
implementation is #28, after review.

## Questions for review

1. **Sequencing.** Replace "linear unary chains first" now, so #28 builds the arena
   before the first vertical slice? The slice's content doesn't change, only its
   container.
2. **Where inputs live.** This proposal puts an ordered `inputs` list on the node, with
   arity checked per operation. The alternative is named fields on each operation
   (`Add { left, right }`), which can't be misordered but needs a per-operation match
   for every traversal. Which?
3. **Node IDs.** A dense `u32` arena index, written on the wire and required to match
   its position. The alternative is host-chosen string IDs, which need uniqueness checks
   and a lookup table, and lose the cycle-free-by-construction property.
4. **One root.** Is one result per plan enough for now? The alternative is a list of
   named roots, so one plan can return several arrays that share work.
5. **One `Open` per binding.** Is this rule too strict for any known host lowering?
6. **Dead nodes on the wire.** Allow them, or require compacted plans at every export?
7. **Warp geometry.** Keep `GeometryBinding` as an operation field, or turn the target
   geometry into an `Open` input so it can also be computed?
