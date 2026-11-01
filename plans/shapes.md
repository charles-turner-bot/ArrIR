# Symbolic shapes and positional broadcasting

Status: proposal, 31 October 2026, for issue #29. Pending review. It builds on the
schema proposal ([#26](https://github.com/charles-turner-bot/ArrIR/pull/26)) and the
DAG contract ([dag.md](dag.md), #45). Nothing here is implemented. The implementation
is #2, after review.

## What this decides

How a plan knows the shape of every node's output without reading data, including
when an extent isn't known yet. It covers positional broadcasting as the Array API
defines it. Named broadcasting and label alignment are #34's, and lower to what is
defined here.

Reference: Array API 2025.12,
[Broadcasting](https://data-apis.org/array-api/2025.12/API_specification/broadcasting.html),
observed 31 October 2026.

## Shapes are ordered dimensions with extents

A shape is the variable's ordered `dims` from #26, each with its extent from `sizes`.
Order is part of the shape. A zero-dimensional array has `dims: []`. Rank is always
known (#26); unknown rank is out of scope.

## Three kinds of extent

#26 has `Extent::Known(u64) | Unknown`. Two unknowns there can't be told apart, so
`a + a` with an unknown `time` can't be proven compatible with itself. This proposal
adds a third variant:

```rust
pub enum Extent {
    Known(u64),
    /// An extent not known at planning time, with an identity: two occurrences of the
    /// same size symbol are equal. Never a stand-in for a known value.
    Symbolic(SizeSymbol),
    Unknown,
}

/// Plan-local. Numbered in the order inference first meets each term (below).
pub struct SizeSymbol(u32);
```

A *size symbol* is an unknown number with an identity. It is unrelated to design.md's
"portable symbol IDs", which are the names of dimensions, variables, bindings and
grids. This document always says "size symbol" to keep them apart.

The variants have distinct roles:

- **`Unknown` is what a host declares.** An `Open` schema may say an extent is
  `Unknown`. It may not contain `Symbolic`; validation rejects that with
  `SymbolInDeclaredSchema`.
- **`Symbolic` is what inference derives.** Inference gives every non-known extent a
  size symbol, so derived schemas contain `Known` and `Symbolic` only.
- **`Known(0)` is empty**, as in #26. It is not unknown.

## Size symbols are identified by structure

A size symbol stands for a *term*: how its value is computed from other extents or
from data. Two occurrences of the same term are the same size symbol, wherever in the
plan they arise. The table interns terms:

```rust
pub enum SizeTerm {
    /// An `Unknown` extent declared by the `Open` of this binding.
    Declared { binding: BindingId, dim: DimId },
    /// A positional slice of a symbolic extent.
    Slice { of: SizeSymbol, slice: SliceSpec },
    /// Broadcasting several size symbols together. Sorted, without duplicates and with
    /// nested `Broadcast` terms flattened, so operand order doesn't matter.
    Broadcast { of: Vec<SizeSymbol> },
    /// A label `Select` not yet resolved to positions (design.md conventions).
    LabelSelect { node: NodeId, dim: DimId },
    /// The number of true values in a computed mask (#36's CountNonZero).
    CountNonZero { mask: NodeId },
}

pub struct SizeTable {
    /// Indexed by `SizeSymbol`. Append-only, like the node arena.
    pub terms: Vec<SizeTerm>,
}
```

`SizeSymbol` lives in `arrir-types` next to `Extent`, so schemas don't depend on
`arrir`. `SizeTerm` and `SizeTable` live in `arrir`, beside the derived-schema cache.
Neither the table nor derived schemas go on the wire, matching dag.md's "derived
schemas are never stored". Inference walks nodes in id order, so the same plan always
numbers its size symbols the same way.

Structural identity is what makes rewrites keep proofs. Pushing `Select { x: 0..10 }`
below `a + a` gives both new `Select` nodes the term `Slice { of: s, slice: 0..10 }`,
which is one size symbol, so the rewritten add is still proven. Keyed by node, the
two slices would be two size symbols and the add would become unresolved.

Two terms still use a node, because their value depends on data, not only on other
extents:

- **`CountNonZero`** depends on the mask's values. Rewrites that leave the mask node in
  place keep its size symbol.
- **`LabelSelect`** depends on the coordinate it reads. Its structural key would be the
  indexer, the dimension and the identity of that index coordinate, but coordinate
  identity is #34's to define. Until then it is keyed by node, so pushing a label
  `Select` mints a new size symbol, and #37's first proof covers positional selection
  only.

`Declared` uses the binding, not the `Open` node. dag.md counts only live `Open`s per
binding, so a rewrite may append a replacement `Open` of the same binding; it keeps the
same size symbols, because it opens the same data.

**Path copies keep node-keyed terms.** dag.md's path copying re-appends every node
between a rewritten node and the root. A re-appended node computes the same value as
the node it copies, so when a rewrite compares requirement sets, a `LabelSelect` or
`CountNonZero` term on a copied node stands for the same term on the original. The
rewrite knows which nodes it copied; nothing extra is stored in the graph.

### Where size symbols come from and where they go

- **One `Open`, one size symbol per unknown dimension.** Two variables of one dataset
  that share `time` share its size symbol, so `t + u` in dag.md's `mean(a + b)` example
  is proven compatible even when `time` is unknown.
- **Two `Open`s get different size symbols** for a shared `DimId`. dag.md says a shared
  `DimId` asserts axis identity, not equal extents, and this keeps that rule.
- **Operations that keep an extent keep its size symbol.** `Project`, elementwise
  operations on that axis and `Reduce` over other dimensions pass it through.
- **Operations that set an extent replace it.** A `Reduce` with `keepdims` gives
  `Known(1)`. A positional slice on a known extent gives a known length; on a size
  symbol it gives a `Slice` term.
- **Nothing reads data to find a value.** Each term has a resolution point, below.

## When size symbols get values

Three steps supply facts, in this order. Each is outside the logical core's planning:

1. **Bind.** The host supplies each `Open`'s real extents. No coordinates are read.
2. **Label resolution.** Coordinates are read to turn labels into positions (purity
   rule 5). This is a separate step from bind.
3. **Execution.** Values are computed.

| Term | Resolved at |
| --- | --- |
| `Declared` | bind |
| `LabelSelect` | label resolution |
| `CountNonZero` | execution of the mask |
| `Slice { of }` | when `of` is resolved |
| `Broadcast { of }` | when every member is resolved |

A `Broadcast` term's value is the common value of its members that aren't 1, or 1 if
all of them are 1. Its requirements (below) guarantee there is at most one such value.

Physical planning needs the extents it plans with. A node whose extents wait on
execution can't be physically planned until the mask it depends on has been evaluated.

## Positional broadcasting

Operands of an elementwise operation (#32) are aligned from their last axis, as in the
Array API. The operand with the highest rank supplies the leading axes unchanged. A
missing leading axis behaves as `Known(1)`, so a zero-dimensional operand broadcasts
against anything.

**Name check.** Aligned axes must have the same `DimId`; a mismatch is
`AxisNameMismatch`. Whether the core should make this check is decision A in the
[questions](#questions-for-review). The rest of this section doesn't depend on it.

### Combining two extents

Write `L` and `R` for the extents of one aligned pair. `k` and `j` are known values
other than 1; `s` and `t` are different size symbols.

| `L` | `R` | Output | Compatibility | Requirement recorded |
| --- | --- | --- | --- | --- |
| `Known(k)` | `Known(k)` | `Known(k)` | proven | none |
| `Known(1)` | anything `R` | `R` | proven | none |
| anything `L` | `Known(1)` | `L` | proven | none |
| `Known(k)` | `Known(j)`, `k ≠ j` | — | contradicted | error `IncompatibleExtents` |
| `s` | `s` | `s` | proven | none |
| `s` | `Known(k)` | `Known(k)` | unresolved | `s ∈ {1, k}` |
| `Known(k)` | `s` | `Known(k)` | unresolved | `s ∈ {1, k}` |
| `s` | `t` | `Broadcast { of: [s, t] }` | unresolved | `s = t` or `s = 1` or `t = 1` |

`Known(0)` is an ordinary known value here: 0 with 1 gives 0, and 0 with 5 is
contradicted, as in the Array API. With a size symbol, `Known(0)` gives `Known(0)` and
the requirement `s ∈ {1, 0}`.

### More than two operands

Broadcasting `n` operands, for `where` and the rest of #32, is a left fold over pairs:
`a, b, c` combines `a` with `b`, then the result with `c`. Each step records its own
requirements. `Broadcast` terms are flattened, sorted and deduplicated, so `(a, b, c)`
and `(c, a, b)` give the same size symbol for each axis.

### Proven, contradicted, unresolved

- **Proven** pairs need nothing more.
- **Contradicted** pairs are an error at inference, reported with the node and axis.
- **Unresolved** pairs aren't errors. Inference records a `ShapeRequirement` and
  carries on.

```rust
pub enum ShapeRequirement {
    /// `left = right`, or either is 1.
    Broadcastable { node: NodeId, dim: DimId, left: Extent, right: Extent },
    /// `left = right` exactly, for example a mask's length and its dimension (#36).
    Equal { node: NodeId, dim: DimId, left: Extent, right: Extent },
    /// A staggered dimension's length is its centre dimension's plus `delta` (grids.md).
    StaggerOffset { node: NodeId, centre: Extent, staggered: Extent, delta: i8 },
}
```

**Each requirement is checked by the core** at the resolution point of its latest term:
at bind for declared extents, at label resolution if a `LabelSelect` is involved, at
execution if a `CountNonZero` is. A host supplies the facts and calls the core's check;
it doesn't reimplement it. For example, `ds.sel(time="2020") + (time: 12)` records
`s ∈ {1, 12}` with `s` a `LabelSelect` term. Bind can't decide it, and label resolution
checks it. A failed check is `RequirementViolated` (below).

Requirements are derived like schemas: recomputed from the plan, never stored on it,
and recomputed after compaction, which renumbers the nodes they refer to.

There's no constraint solver. Each requirement is checked on its own. Two
requirements `s ∈ {1, 2}` and `s ∈ {1, 3}` jointly force `s = 1`, but inference doesn't
deduce that, and a jointly unsatisfiable set is found when the requirements are checked.
A solver can be added later without changing what a requirement means.

**Staggered extents.** grids.md checks that a staggered dimension has its centre
length plus or minus one, "whenever both lengths are known". With size symbols, the
check becomes a `StaggerOffset` requirement instead of being skipped. Size symbols
don't express `s + 1`, so an outer dimension's size symbol is not derived from its
centre's (question 8).

### Rewrites keep requirements exactly

A rewrite must leave the requirement set unchanged, compared by content (kind and
extents, ignoring node IDs, with path copies standing for the nodes they copy). Fewer requirements would let a plan that fails at bind
run instead; more would make a plan that bound fine fail. Structural size symbols are
what make "unchanged" achievable for selection pushdown.

A rewrite whose legality depends on which operand stretches needs that pair to be
proven, not unresolved. dag.md's pushdown of `Select { time }` below an `Add` doesn't
slice a size-one `time` axis, so it applies only when each operand's `time` is proven to
be 1 or proven equal to the output's. With `s ∈ {1, k}` unresolved, the rewrite doesn't
apply. #37 checks both conditions.

Semantic readiness (#26) uses the same distinction. An operation that needs a concrete
extent, such as a `ChunkSpec` block partition or a future `Reshape`, is not ready while
that extent is symbolic. That isn't a validation error.

## Errors

```rust
pub enum ShapeError {
    IncompatibleExtents { node: NodeId, dim: DimId, left: u64, right: u64 },
    AxisNameMismatch { node: NodeId, position_from_end: usize, left: DimId, right: DimId },
    SymbolInDeclaredSchema { open: NodeId, dim: DimId },
}

/// Returned by the core's requirement check once the facts are supplied.
pub struct RequirementViolated {
    pub requirement: ShapeRequirement,
    pub resolved_left: u64,
    pub resolved_right: u64,
}
```

`position_from_end` is 1 for the last axis. `ShapeError`s sit beside dag.md's
structural `PlanError`: a plan can be structurally valid and still have a shape error.

## Worked examples

Each operand is an array; the axes are written as `name: extent`.

1. **`(100, 1) + (1, 50)`.** Left `(y: 100, x: 1)`, right `(y: 1, x: 50)`. Aligned pairs
   are `y: 100 with 1` and `x: 1 with 50`. Output `(y: 100, x: 50)`, proven.
2. **`(2, 3) + (4, 3)`.** Left `(y: 2, x: 3)`, right `(y: 4, x: 3)`. The pair `y: 2 with 4`
   is contradicted: `IncompatibleExtents { node, dim: y, left: 2, right: 4 }`.
3. **`(n, 1) + (1, m)`.** Left `(y: s, x: 1)`, right `(y: 1, x: t)`. Output `(y: s, x: t)`,
   proven, because each pair has a `Known(1)`.
4. **`(n,) + (m,)` from two `Open`s.** Left `(x: s)`, right `(x: t)`. Output
   `(x: u)` with `u = Broadcast { of: [s, t] }`, and the requirement `s = t` or `s = 1` or
   `t = 1`. Binding `s = 10, t = 10` gives `u = 10`; `s = 1, t = 7` gives `u = 7`;
   `s = 10, t = 7` is `RequirementViolated` at bind, before execution.
5. **`(n,) + (n,)` from one `Open`.** Both operands share the `Open`'s size symbol `s`
   for `x`. Output `(x: s)`, proven.
6. **Zero size.** `(y: 0) + (y: 0)` gives `(y: 0)`. `(y: 0, x: 3) + (x: 3)` gives
   `(y: 0, x: 3)`. `(y: 0) + (y: 1)` gives `(y: 0)`. `(y: 0) + (y: 5)` is
   `IncompatibleExtents`. `(y: 0) + (y: s)` gives `(y: 0)` with `s ∈ {1, 0}`.
7. **Scalars.** `() + (y: s, x: 3)` gives `(y: s, x: 3)`. `() + ()` gives `()`. A literal
   such as the `2` in `a * 2` is a zero-input node with shape `()` (#47's `Constant`).
8. **Wrong alignment.** `(lat: 180, lon: 180) + (lat: 180)` aligns `lon` with `lat`. With
   the name check: `AxisNameMismatch { position_from_end: 1, left: lon, right: lat }`.
   NumPy accepts the same shapes and adds along longitude.
9. **Label selection.** `ds.sel(time="2020") + (time: 12)`: `s ∈ {1, 12}` with `s` a
   `LabelSelect` term, checked at label resolution, not at bind.

## No explicit Broadcast node, for now

Inference on each elementwise node gives the output shape, so lowering doesn't insert
a `Broadcast` node before arithmetic. A node would add a second spelling of the same
plan that every rewrite would have to normalise. The Array API's `broadcast_to` and
`broadcast_arrays` are frontend functions; #41's coverage matrix records whether they
lower to a node or a composition. Inserting a length-1 axis (`expand_dims`) and
reordering axes (`permute_dims`, #4) are separate operations that #34 needs.

## Implementation through #2

#2 implements this after review, in small PRs:

1. `Extent::Symbolic`, `SizeSymbol` and `SymbolInDeclaredSchema`, with serde tests
   showing that a declared schema with a size symbol is rejected.
2. The size table and per-node inference for `Open`, `Project`, positional `Select` and
   `Reduce`, with checked element counts (from closed #1).
3. Broadcasting for a test operation of any arity, with each worked example above as a
   test, including the requirements it records. #33 then uses it for `Add`, `Multiply`
   and `GreaterThan`.
4. The requirement check, with `RequirementViolated` tests at bind and at label
   resolution.

None of these reads data, and the tests assert that.

## Questions for review

1. **One `Extent` enum or two types.** Adding `Symbolic` to #26's `Extent` keeps one
   schema type for declared and derived schemas. The risk is that a single serde enum
   lets a size symbol onto the wire unless something stops it. Marking the variant
   `#[serde(skip)]` makes serialising it an error and makes it unreadable, on top of
   `SymbolInDeclaredSchema`. Two types (declared `Known | Unknown`, derived
   `Known | Symbolic`) make it a compile-time fact but duplicate `Schema`.
   **Recommended: one enum with the serde guard.**
2. **(A) Where the name check lives.** Two options:
   - *Strict core (as written).* Aligned axes must share a `DimId`. This catches `lon`
     combined with `lat` in any lowering, and keeps #45's rule that a `DimId` names one
     axis. The cost falls on Array API hosts, which have no names: `(x: 3) + (y: 3)` is
     rejected where the Array API gives `(3,)`. A host that names axes by position from
     the end needs a `Rename` (design.md's extraction table already has one) after
     anything that moves axes, such as `sum(axis=-1)` or `x + x.T` on a square `x`. And
     if one `Open` holds several positional arrays, a shared name like `d0` forces them
     to one extent.
   - *Positional core.* The core aligns by position and ignores names; #34's lowering
     checks names for xarray. This matches the Array API exactly and the issue's
     "positional algebra independent of labelled layers". The cost is that the output
     axis must take one operand's `DimId` when names differ, which relabels the other
     operand's axis, against #45's axis identity. A lowering other than #34's gets no
     name check.

   **Recommended: strict**, with Array API lowering inserting `Rename`. A positional
   core still has to choose output names, and choosing silently is the error the check
   exists to prevent.
3. **(B) Size-symbol identity.** By term structure (as written), so rewrites keep
   proofs, or by node, which is simpler but makes pushdown turn proven pairs into
   unresolved ones. **Recommended: structure.**
4. **(C) Declared shared unknowns.** Should a host be able to say two `Open`s share an
   unknown extent? **Recommended: no.** A host that knows they're equal usually knows the
   value; if not, a requirement at the combining operation catches a mismatch.
5. **Unresolved broadcasts.** Accept them with a requirement checked at its resolution
   point, or reject them until the extents are known? **Recommended: accept.**
6. **No solver.** Check each requirement on its own for now? **Recommended: yes.**
7. **No Broadcast node,** with `broadcast_to` left to #41? **Recommended: yes.**
8. **Staggered offsets.** A `StaggerOffset` requirement (as written), or an offset term
   `s + delta` so an outer dimension's size symbol derives from its centre's?
   **Recommended: the requirement,** until a rewrite needs to reason about offsets.
