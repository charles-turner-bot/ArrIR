# Plans as DAGs

Status: decided 1 November 2026, for issue #27 (proposed 31 October). It replaces the
"start with linear unary chains" decision in [design.md](design.md#decisions). The arena,
structural validation and wire form are implemented in `crates/arrir` (#28, PR #59).

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
  This proposal uses #26's `Schema`, `ResultKind`, `ArrayResult` and `TypeKnowledge`
  ([schema-and-values.md](schema-and-values.md)), and shapes.md's `Extent`, and adds no
  schema types. The wire example below follows #26's revised form.
- **#16** (shells): its purity rules and conventions are already in the contract. Its
  per-operation effects become per-node effects here, with one footprint per input.
  Nothing in this proposal depends on #16's code.

Positional semantics referred to here (broadcasting, via #29 and #46) follow the
[Array API 2025.12](https://data-apis.org/array-api/2025.12/) standard, observed on
31 October 2026.

## The plan

```rust
/// A node's position in its graph. Never reused. The field is public: an ID is only a
/// position, and everything that takes one checks it against a graph.
pub struct NodeId(pub u32);

pub struct Node {
    pub operation: Operation,
    /// Ordered. `inputs[0]` is the first operand; each operation documents the
    /// meaning of each position.
    pub inputs: Vec<NodeId>,
}

/// Append-only. The field is private, so nodes can be added but never changed
/// or removed.
pub struct Graph {
    nodes: Vec<Node>,
}

impl Graph {
    /// Checks the node's inputs and arity, then returns its new ID.
    pub fn append(&mut self, node: Node) -> Result<NodeId, PlanError> { … }
    pub fn node(&self, id: NodeId) -> &Node { … }
}

/// What is exported and executed: a graph and the node whose value is the result.
pub struct Plan {
    pub graph: Graph,
    pub root: NodeId,
}
```

`Operation` is the exhaustive `Operation` enum (proposed; variants land per the
roadmap). None exists in the replacement IR yet. The variants named here are
illustrative: design.md's extraction table lists `Open`, `Project`, `Select`, `Reduce`,
`Scan`, `Rename`, `Drop` and `Rechunk`, and the proposals add `Warp`, `Cast` and #32's
elementwise family. Operations hold their options; nodes hold the edges. A node's ID is
its position, so it isn't stored in the node. Array data is never embedded: inputs are
other nodes, and *external* data enters only through `Open`. `Warp`'s target geometry
is an `Open` input too ([decision 7](#decisions)). A literal scalar, such as the `2` in
`a * 2`, is a zero-input `Constant` node with no binding, defined in
[dtypes.md](dtypes.md).

**Who owns the nodes.** A `Graph` owns them. A rewrite takes `&mut Graph` and a root,
appends to the graph in place, and returns a new root. No existing node is changed or
removed, so every `NodeId` handed out earlier still indexes the same node. Several
roots, the original and each rewrite's result, can live in one graph at once. A `Plan`
pairs the graph with the one root being exported or executed.

### Rules

1. **Append-only.** A node is never changed or removed once added. Its `NodeId` is its
   index in the graph.
2. **Inputs point backwards.** Every input of node `n` is less than `n`. This makes
   cycles unrepresentable: id order is a topological order, and validation is one pass
   with no graph search.
3. **Arity comes from the operation value.** `Operation::arity(&self) -> Arity` looks
   at the operation's fields, not just its variant.

   ```rust
   pub enum Arity {
       Exactly(usize),
       AtLeast(usize),
   }
   ```

   `Open` is `Exactly(0)`, as is a `Constant` ([dtypes.md](dtypes.md)). `Warp` is
   `Exactly(1)`, or `Exactly(2)` when its target geometry is an `Open` input. `Project`,
   `Cast` and
   an unweighted `Reduce` are `Exactly(1)`; a weighted `Reduce` is `Exactly(2)`, its
   weights being the second input. A `Select` takes one input plus one per array-valued
   indexer, such as a computed mask (#36). A binary elementwise function is
   `Exactly(2)`, and a future `Concat` is `AtLeast(1)`. The input count must match.
4. **One root per plan.** A plan computes one result, the value of `root`. Several
   results share work by being several roots in one graph (see
   [decisions](#decisions)).
5. **Sharing is explicit.** `Add` with inputs `[1, 1]` uses one value twice. Operations
   are pure, so an adapter may evaluate a shared node once or twice; how often is
   execution policy, not meaning.
6. **One live `Open` per binding.** Among the nodes reachable from a plan's root, a
   `BindingId` is opened by at most one node. A second use of the same data refers to
   that node. Dead nodes don't count, so a rewrite may append a replacement `Open` for
   the same binding, for example to push a projection into the source. A host binds
   only live `Open`s.
7. **No implicit merging.** Two structurally equal nodes stay two nodes. Merging them
   (common subexpression elimination) is a rewrite, never a side effect of building.

### Structural validation

`Graph::append` checks rules 2 and 3 for each node as it arrives. Validating a `Plan`
(on import, or before export) checks every node in the graph against rules 2 and 3,
then checks the root and rule 6 over the live nodes:

```rust
pub enum PlanError {
    Empty,
    DanglingInput { node: NodeId, input: NodeId },   // input >= graph length
    ForwardInput { node: NodeId, input: NodeId },    // node <= input < graph length
    WrongArity { node: NodeId, expected: Arity, found: usize },
    RootOutOfRange { root: NodeId },
    DuplicateBinding { binding: BindingId, first: NodeId, second: NodeId },
}
```

Validation reports the first failure: nodes in ascending order, and for each node the
errors in the order listed. Root and binding errors come after all node errors. A
self-loop is a `ForwardInput` whose input equals its node. Wire import also rejects a
node whose written `id` isn't its position (see [Wire form](#wire-form)); that's an
import error, not a `PlanError`, because in memory the ID is the position.

These are structural errors only. Whether facts are known enough for an operation
(semantic readiness) and whether an engine can run it (adapter acceptance) are the
separate questions #26 defines.

### Reachability and deletion

The *live* plan is the set of nodes reachable from `root`. Unreachable nodes are
logically deleted: schema propagation, readiness checks, rewrites, adapters and
execution ignore them. They still pass rules 2 and 3, so the graph stays sound.

**Compaction** is optional, explicit, and not part of #28. It keeps the live nodes,
renumbers them in post-order (a depth-first walk from `root`, visiting inputs in order,
numbering a node after its inputs) and returns the old-to-new mapping. It's the one
operation that invalidates node IDs, which is why it returns the mapping and never runs
implicitly. Two plans that describe the same graph compact to identical node lists, so
the compacted form is the canonical one for comparison once compaction exists.

### Rewrites keep old IDs and roots

A rewrite appends replacement nodes and returns a new root. Existing nodes are
untouched, so:

- a host holding an earlier `NodeId` still names the same computation;
- the old root is still a valid root for the unrewritten plan;
- a node shared with another consumer is unaffected when one consumer's path is
  rewritten. This is what makes pushdown safe in the presence of sharing.

**Path copying.** Because nodes can't change, rewriting a node below the root means
re-appending every node on the path from it up to the root, each pointing at its
replaced input. Nodes off that path are reused as they are. See
[Rewriting below the root](#rewriting-below-the-root).

The arena therefore grows with every rewrite, so any termination measure applies to the
live graph, not the arena. xrexpr's measure ("no rule lengthens a plan or moves an
operation rightwards") was defined on chains, and pushing a `Select` below an `Add`
can add a live node, so it needs restating for graphs. That belongs to the first rewrite
proof (#37), not to this contract.

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
one data variable.

`Project` keeps both forms from design.md's extraction table, because array versus
dataset is observable:

```rust
pub enum Project {
    /// One variable. Output is `ResultKind::Array(ArrayResult { variable })`.
    Array(VarId),
    /// A subset of variables, in this order. Output is `ResultKind::Dataset`.
    Dataset(Vec<VarId>),
}
```

One `Open` can feed several projections, which is how two variables from one file meet
in one expression.

Each operation states which result kinds it accepts. Elementwise arithmetic on arrays
comes first; dataset arithmetic (xarray applies it per variable) needs its own decision
in #32, and until then it's rejected rather than guessed.

## ID scope across Opens

#15 and #26 as first proposed didn't say how far a `DimId`, `VarId` or `GridId`
reaches; #26 now defers to this section. With one `Open` it didn't matter. With two,
both datasets can have a `time` dimension and a `temp` variable, and the plan has to
say whether they're the same thing.

**Decision: named dimensions are plan-wide; variables and grids are per schema.**

- **`DimId` is plan-wide, for named axes.** A schema stores an axis as
  `VariableAxis::Named(DimId) | Positional(Extent)` (schema-and-values.md), and
  inference views it as shapes.md's `Axis`. The same `DimId` on named axes in two
  `Open`s names one axis. This is what named broadcasting (#46) needs, and it matches
  xarray, where dimension names are the identity used for alignment. Positional axes, from NumPy and
  Array API hosts, carry no ID and are identified by trailing position (see #46,
  shapes.md); the rules below about shared `DimId`s don't apply to them.
- **A shared `DimId` asserts axis identity, nothing more.** It doesn't say the
  coordinate labels match, or that the extents are equal. `obs` may have 365 `time`
  steps and `model` 360. That's valid at `Open`. An operation that combines inputs
  checks extents where it combines them: a named axis on both sides needs equal known
  extents, and only positional axes stretch (rules in #46, shapes.md). Making
  differently labelled axes agree is coordinate label alignment, which #34 lowers to
  explicit selection nodes before the combining operation.
- **`VarId` is scoped to one node's schema.** `Project` names variables in its single
  input's schema, so two `temp`s in two `Open`s never meet in one name lookup. Where an
  operation's output has to name a variable from several inputs, that operation states
  the rule (#32 for elementwise results; a future `Merge` rejects collisions or
  requires a rename).
- **`GridId` is scoped to one node's schema too.** Two `Open`s can each declare a grid
  `g` with different CRSs, or identical grids with different names. Whether two inputs
  share a grid is checked by comparing grid definitions, never IDs. The operation that
  combines inputs names the grids in its output: #32 for elementwise, #34 where
  alignment is involved.
- **Unequal grids under one dimension are rejected when combined.** If a shared
  `DimId` is placed on grids with unequal definitions in two inputs, an operation that
  combines them rejects the plan, as it would for unequal known extents. A `Warp` onto
  one grid first resolves it: [warp.md](warp.md#validation) lets the target reuse the
  source grid's dimension names once the source grid is gone. Each `Open` on its own
  stays valid.
- **Different axes with one name are renamed at lowering.** If `model`'s `time` is
  really forecast lead time, the host lowers it as `lead` and keeps its own name map,
  as design.md already requires for host symbols. Qualifying IDs by binding
  (`model.time`) was rejected: nodes after a `Project` or an `Add` have no single
  binding, and xarray results carry plain names.
- **`Rename` moves an axis between identities.** Renaming `time` to `t2` on one
  branch makes it a different axis from `time` elsewhere in the plan. Renaming it to a
  name already in use asserts it's the same axis as that one.
- **Rewrites never rename dimensions.** Because a `DimId` is shared, a rewrite that
  moves a `Select { time }` below an `Add` must select on every input that has `time`.
  The index for each input is derived from the broadcast mapping (#46), not copied:
  an input without `time` gets no selection on it, and a stretched positional axis isn't
  sliced like a full one. That legality proof is #37's.
- **New dimensions join the plan's namespace.** A `Warp` target whose dimensions reuse
  a name from elsewhere in the plan (`lat` and `lon` from another dataset, say) asserts
  they're the same axes. Its target `GridId` is still compared by definition.

Worked example, `obs.temp + model.temp`:

| Node | Operation | Inputs | Derived output |
| --- | --- | --- | --- |
| 0 | `Open { binding: obs }` | — | `temp (time: 365, lat: 180)` |
| 1 | `Open { binding: model }` | — | `temp (time: 360, lat: 180)` |
| 2 | `Project::Array(temp)` | 0 | Array `temp (time: 365, lat: 180)` |
| 3 | `Project::Array(temp)` | 1 | Array `temp (time: 360, lat: 180)` |
| 4 | elementwise add | 2, 3 | rejected: known `time` extents 365 and 360 differ |

Both `Open`s are valid, and `time` and `lat` are each one axis. Node 4 fails because
`time` is on both sides with unequal known extents, and named axes never stretch (#46,
shapes.md). Xarray's `obs.temp + model.temp` lowers through #34 instead: label
alignment adds selection nodes on 2 and 3 that bring `time` to the matched labels, then
the add. If `model`'s `time` were lead time, the host would lower it as `lead`, and the
add would give `(time, lat, lead)` under #46's named rules.

[warp.md](warp.md) first gave `Warp`'s target geometry as a `GeometryBinding` field.
Decision 7 makes it an `Open` input instead, so `Open` is the only way external data
enters a plan.

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

**Decision: the Rust arena is the plan.** Hosts keep their own expression
objects and lower them by calling `Graph::append`, which returns a `NodeId` for each
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
| 1 | `Project::Array(t)` | 0 | Array `t (time, lat)` |
| 2 | `Project::Array(u)` | 0 | Array `u (time, lat)` |
| 3 | elementwise add (#32) | 1, 2 | Array `(time, lat)`, dtype by #30 |
| 4 | `Reduce { mean over time }` | 3 | Array `(lat)`, dtype by #30 |

Root is 4. Building it reads no data: every row derives from the declared schema of
node 0. One `Open` feeds both operands.

### `Add(shared, shared)`

| Node | Operation | Inputs |
| --- | --- | --- |
| 0 | `Open { binding: obs, schema }` | — |
| 1 | `Project::Array(t)` | 0 |
| 2 | elementwise add | 1, 1 |

Root is 2. Node 2 adds `t` to itself. Two other spellings are worth comparing:

- A second `Project::Array(t)` as node 2, then add `[1, 2]`. Valid. It denotes the
  same value, but proving that is a rewrite's job (rule 7).
- A second live `Open { binding: obs }`. Rejected with `DuplicateBinding` (rule 6).

### A rewrite that keeps old roots

Start from `Add(shared, shared)` plus a selection, root 3:

| Node | Operation | Inputs |
| --- | --- | --- |
| 0 | `Open { binding: obs }` | — |
| 1 | `Project::Array(t)` | 0 |
| 2 | elementwise add | 1, 1 |
| 3 | `Select { lat: 0..10 }` | 2 |

Pushing the selection below the add appends two nodes and returns root 5:

| Node | Operation | Inputs |
| --- | --- | --- |
| 4 | `Select { lat: 0..10 }` | 1 |
| 5 | elementwise add | 4, 4 |

Nodes 0 to 3 are unchanged, and root 3 still names the original computation. Nodes 2
and 3 are unreachable from root 5, so they're logically deleted. Compacting from root 5
visits `0, 1, 4, 5` and returns `{0→0, 1→1, 4→2, 5→3}`; IDs 2 and 3 have no mapping.
Whether this pushdown is legal is #37's to prove; the example shows only the structure.

### Rewriting below the root

`mean(select(a + b))`, root 5:

| Node | Operation | Inputs |
| --- | --- | --- |
| 0 | `Open { binding: obs }` | — |
| 1 | `Project::Array(a)` | 0 |
| 2 | `Project::Array(b)` | 0 |
| 3 | elementwise add | 1, 2 |
| 4 | `Select { lat: 0..10 }` | 3 |
| 5 | `Reduce { mean over time }` | 4 |

Pushing the selection below the add replaces node 4, which isn't the root. The
`Reduce` above it must be re-appended too, because its input changes:

| Node | Operation | Inputs |
| --- | --- | --- |
| 6 | `Select { lat: 0..10 }` | 1 |
| 7 | `Select { lat: 0..10 }` | 2 |
| 8 | elementwise add | 6, 7 |
| 9 | `Reduce { mean over time }` | 8 |

The new root is 9. Nodes 0 to 2 are reused. The live graph grew from six nodes
(0 to 5) to seven (0, 1, 2, 6 to 9), and the arena from six to ten. Root 5 is still
valid and unchanged.

### Cycles and dangling inputs

```json
{
  "version": "0",
  "nodes": [
    { "id": 0, "op": { "Open": { "binding": "obs", "schema": "…" } }, "inputs": [] },
    { "id": 1, "op": { "Project": { "Array": "t" } }, "inputs": [2] },
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
          { "id": "t", "dims": [{ "Named": "time" }, { "Named": "lat" }],
            "role": "Data", "dtype": { "Known": "Float32" } }
        ],
        "sizes": [["time", { "Known": "365" }], ["lat", { "Known": "180" }]],
        "grids": []
      } } },
      "inputs": []
    },
    { "id": 1, "op": { "Project": { "Array": "t" } }, "inputs": [0] },
    { "id": 2, "op": { "Elementwise": { "function": "Add" } }, "inputs": [1, 1] }
  ],
  "root": 2
}
```

Wire rules:

- A wire plan is a graph and one root. Nodes are serialised in id order, so
  serialising the same graph twice gives the same bytes.
- `id` is written out for readability and must equal the node's position. It exists
  only on the wire.
- `NodeId` is a `u32`, written as a JSON number, which every host reads exactly.
  Every other integer, extents and scalars included, is a decimal string
  ([schema-and-values.md](schema-and-values.md#wire-contract-and-first-proof)).
- Dead nodes may appear on the wire. Adapters and hosts ignore them. Export doesn't
  require compaction.
- An unknown `version` or operation tag fails import.

## Not in this proposal

No scheduler, evaluation order, kernels, memoisation or embedded data. No elementwise
vocabulary (#32), broadcasting (#29, #46), dtype rules (#30) or label alignment
(#34). The implementation is #28; compaction is later still.

## Decisions

Each question was proposed with a recommendation. Charles accepted all of them on
1 November 2026.

1. **Sequencing.** Replace "linear unary chains first" now, so #28 builds the arena
   before the first vertical slice? Decided: yes. The slice's content doesn't
   change, only its container.
2. **Where inputs live.** Decided: an ordered `inputs` list on the node, with arity
   checked per operation value. The alternative is named fields on each operation
   (`Add { left, right }`), which can't be misordered but needs a per-operation match
   for every traversal.
3. **Node IDs.** Decided: a dense `u32` graph index. The alternative is host-chosen
   string IDs, which need uniqueness checks and a lookup table, and lose the
   cycle-free-by-construction property.
4. **One root.** Decided: one root per plan, with several results as several roots
   in one graph. The alternative is a list of named roots in the plan itself. Either
   way, rule 6 holds across every root a host binds together, not just per root.
5. **Duplicate bindings (c).** Decided: check rule 6 over live nodes only, so a
   rewrite can append a replacement `Open`. The stricter alternative counts dead nodes
   too, and bars rewrites from appending an `Open` for a binding already opened.
6. **Dead nodes on the wire.** Decided: allow them, keeping compaction optional and
   out of #28. The alternative compacts at every export, which moves compaction into
   #28.
7. **Warp geometry (b).** Decided: turn the target geometry into an `Open` input
   to `Warp`. It removes the case where one binding is declared by both an `Open` and a
   `GeometryBinding`, which rule 6 doesn't cover, lets geometry be computed, and leaves
   `Open` as the only way external data enters. The alternative keeps the field and
   extends rule 6 to it.
8. **ID scope.** Decided: plan-wide `DimId` for named axes, with `VarId` and
   `GridId` scoped to one node's schema, and renaming at lowering for distinct axes that
   share a name. Positional axes carry no ID (#46). The
   alternative is per-`Open` scope for everything, with an explicit mapping operation
   wherever two datasets meet.
9. **Dimensions across grids (a).** Decided: a shared `DimId` asserts axis
   identity even when its grids' definitions differ, and combining such inputs is
   rejected until a `Warp` makes them agree; warp.md lets the target reuse the source
   grid's dimension names. The alternatives are a `Rename` before the `Warp`, so the
   target needs no reused names, or that each grid owns its dimensions, so dimensions
   on differently defined grids must always have different names.
10. **Integer encoding.** Decided: `NodeId` as a JSON number. #26 settles the rest:
    every integer is a decimal string, except an enumerated list of `u32` structural
    indices that includes `NodeId`
    ([schema-and-values.md](schema-and-values.md#wire-contract-and-first-proof)).
