# Symbolic shapes and broadcasting

Status: decided 1 November 2026, for issue #29 (proposed 31 October). It builds on the
schema proposal ([#26](https://github.com/charles-turner-bot/ArrIR/pull/26)) and the
DAG contract ([dag.md](dag.md)). Nothing here is implemented yet; #2 implements it.

## What this decides

How a plan knows the shape of every node's output without reading data, including
when an extent isn't known yet. It covers broadcasting: by position for unnamed axes, as
the Array API defines it, and by name for named axes, as xarray does. Alignment by
coordinate labels (inner and outer joins) is #34's, and lowers to what is defined here.

References:

- Array API 2025.12,
  [Broadcasting](https://data-apis.org/array-api/2025.12/API_specification/broadcasting.html),
  observed 31 October 2026.
- xarray 2026.9.0 with NumPy 2.5.4, run 1 November 2026, for how a named array combines
  with an unnamed one ([below](#named-with-positional-follows-xarray)).

## Shapes are ordered axes with extents

A shape is an ordered list of axes. Each axis has a name, or none, and an extent:

```rust
pub enum AxisName {
    /// A named dimension. The `DimId` is the axis's identity across the plan (dag.md).
    Named(DimId),
    /// An axis from a NumPy or Array API host, with no name. Its identity is its
    /// position counted from the end, derived at each node and never stored.
    Positional,
}

pub struct Axis {
    pub name: AxisName,
    pub extent: Extent,
}
```

Order is part of the shape. A zero-dimensional array has no axes. Rank is always known
(#26); unknown rank is out of scope.

- **A shape is named or positional, not mixed.** All its axes are `Named`, or all are
  `Positional`. A zero-dimensional shape counts as either. A mixed shape is rejected
  (`MixedAxisNames`), for the reason xarray rejects an unnamed axis in a named result.
- **Positional axes have no stored ID.** No `d0, d1, …` names are invented, so
  `x + x.T` on a square `x` and renumbering after `sum(axis=-1)` need nothing extra, and
  two positional arrays in one `Open` don't have to agree on any extent. Operations
  address positional axes by position, as the Array API's `axis` argument does.
- **Grid placements need named axes.** A `GridPlacement` (grids.md) refers to its axes by
  `DimId`, so a positional axis can't be placed on a grid.

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
    /// An `Unknown` named extent declared by the `Open` of this binding.
    DeclaredNamed { binding: BindingId, dim: DimId },
    /// An `Unknown` positional extent declared by the `Open` of this binding: axis
    /// `position` (counted from the start) of `variable`.
    DeclaredPositional { binding: BindingId, variable: VarId, position: u32 },
    /// A positional slice of a symbolic extent.
    Slice { of: SizeSymbol, slice: SliceSpec },
    /// Broadcasting several size symbols together. Sorted, without duplicates and with
    /// nested `Broadcast` terms flattened, so operand order doesn't matter.
    Broadcast { of: Vec<SizeSymbol> },
    /// A label `Select` not yet resolved to positions (design.md conventions), applied
    /// to the value numbered `input`.
    LabelSelect { input: ValueNumber, axis: AxisRef, labels: LabelIndexer },
    /// The number of true values in the mask whose value is numbered `mask` (#36).
    CountNonZero { mask: ValueNumber },
}

/// Value numbering (below): equal operations on equally numbered inputs get one number.
pub struct ValueNumber(u32);

/// How a term, requirement or error refers to one axis of a node's output.
pub enum AxisRef {
    Named(DimId),
    /// 1 is the last axis.
    FromEnd(u32),
}

pub struct SizeTable {
    /// Indexed by `SizeSymbol`. Append-only, like the node arena.
    pub terms: Vec<SizeTerm>,
}
```

`SliceSpec` and `LabelIndexer` are the indexer values of roadmap step 4 (#3 for
positional ones). `SizeSymbol` lives in `arrir-types` next to `Extent`, so schemas
don't depend on `arrir`. `SizeTerm` and `SizeTable` live in `arrir`, beside the
derived-schema cache.
Neither the table nor derived schemas go on the wire, matching dag.md's "derived
schemas are never stored". Inference walks nodes in id order, so the same plan always
numbers its size symbols the same way.

Structural identity is what makes rewrites keep proofs. Pushing `Select { x: 0..10 }`
below `a + a` gives both new `Select` nodes the term `Slice { of: s, slice: 0..10 }`,
which is one size symbol, so the rewritten add is still proven. Keyed by node, the
two slices would be two size symbols and the add would become unresolved.

**Slices are normalised when interned.** A full slice `..` with step 1 is `s` itself,
not a new term. Nested slices compose when the result is exact for every length: both
with non-negative bounds and positive steps, so `Slice { Slice { s, 0..10 }, 2..5 }` is
`Slice { s, 2..5 }`. Negative bounds depend on the length and don't always compose
(`s[-5:][:3]` and `s[-5:-2]` differ when `s < 5`), so those stay nested. Without this,
merging adjacent selections, one of xrexpr's first rewrites, would change the terms.

### Value numbering for data-dependent terms

Two terms depend on data, not only on other extents: `CountNonZero` on a mask's
values, and `LabelSelect` on the coordinates of the value it selects from. They are
keyed by a *value number*, not a node:

- An `Open`'s value number is its binding.
- Any other node's value number interns its operation together with its inputs' value
  numbers. Two nodes with the same operation on equally numbered inputs get one number.

A mask shared by two branches has one value number however it's reached, so both
branches get the same `CountNonZero` size symbol. A path copy (dag.md) has new inputs,
so it would get a new number. When a rewrite is checked (below), its replacement node
takes the value number of the node it replaces, since the rewrite claims they have one
value. Path copies above it then re-intern to the original numbers.

Value numbering is an analysis of size symbols, not a merge of nodes. dag.md's rule
that equal nodes are never merged automatically still holds: the nodes stay distinct,
and only the size symbols they produce coincide. Equal value numbers imply equal
values; different numbers imply nothing, so the analysis can miss an equality but never
invent one. Pushing a label `Select` below an add still changes its input's value
number, so #37's first proof covers positional selection only, and full coordinate
identity stays with #34.

The declared terms use the binding, not the `Open` node. dag.md counts only live `Open`s per
binding, so a rewrite may append a replacement `Open` of the same binding; it keeps the
same size symbols, because it opens the same data.

### Where size symbols come from and where they go

- **One `Open`, one size symbol per unknown named dimension.** Two variables of one
  dataset that share `time` share its size symbol, so `t + u` in dag.md's
  `mean(a + b)` example is proven compatible even when `time` is unknown.
- **Positional axes get one size symbol each,** keyed by binding, variable and position.
  Positional axes of two variables never share one.
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
| `DeclaredNamed`, `DeclaredPositional` | bind |
| `LabelSelect` | label resolution |
| `CountNonZero` | execution of the mask |
| `Slice { of }` | when `of` is resolved |
| `Broadcast { of }` | when every member is resolved |

A `Broadcast` term's value is the common value of its members that aren't 1, or 1 if
all of them are 1. Its requirements (below) guarantee there is at most one such value.

Physical planning needs the extents it plans with. A node whose extents wait on
execution can't be physically planned until the mask it depends on has been evaluated.

## Broadcasting

Operands of an elementwise operation (#32) combine by one of three rules, chosen by
whether their shapes are named or positional:

| Operands | Rule | Output |
| --- | --- | --- |
| positional, positional | Array API: align from the last axis; extents equal, or one is 1 | positional |
| named, named | align by name; shared dims must have equal extents; a dim on one side only is broadcast | named |
| named, positional | the positional operand adopts the named operand's names by trailing position | named |

**Only positional axes stretch.** A length-1 positional axis can take any extent. A named
axis never changes its extent: two named operands must agree on every shared dim, and a
positional operand may not stretch a named length-1 axis. Broadcasting a dim that one
named operand lacks *creates* an axis on that operand, which isn't stretching.

### Positional with positional: the Array API

Align from the last axis. An operand with fewer axes is extended on the left with
length-1 axes, so a zero-dimensional operand broadcasts against anything. Each aligned
pair combines as follows. `k` and `j` are known values other than 1; `s` and `t` are
different size symbols.

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

### Named with named: by name, as xarray

- **Shared dims** must have equal extents, with no length-1 stretching. Unequal known
  extents are `IncompatibleExtents`. With unknown extents the requirement is `s = t`
  (`Equal`), and the output takes the lower-numbered of the two size symbols, so it
  doesn't depend on operand order.
- **A dim on one operand only** is broadcast: the output has it, with the extent and size
  symbol it has on that operand.
- **Output order** is the first operand's dims, then the second operand's new dims in
  their order. xarray 2026.9.0 orders `named(x: 3) + named(y: 2, x: 3)` as `(x, y)`.

This is xarray's rule for arrays without coordinates. With index coordinates, xarray
also aligns labels (inner or outer joins); that is #34's, which lowers it to selections
before the elementwise node. Matching by name is the core's, so #34 needs no
`expand_dims` or `permute_dims` to line names up.

### Named with positional follows xarray

A positional operand takes the names of the named operand it's combined with, aligned
from the last axis, under two conditions:

1. **It has no more axes than the named operand.** Otherwise the result would have an
   unnamed axis among named ones. The error is `PositionalOutranksNamed`.
2. **At each aligned pair, the positional extent equals the named one or is 1.** The
   output keeps the named extent. A positional axis meeting a named axis of length 1 is
   `PositionalStretchesNamed`, with or without coordinates.

With unknown extents, condition 2 becomes the requirement `positional ∈ {1, named}`
(`FitsNamed`, below). The named operand's missing leading axes don't exist: the
positional operand is the one extended, as in the Array API.

Evidence, with `da` a `DataArray` on `(y: 2, x: 3)` and `nd` a NumPy array:

| Expression | xarray 2026.9.0 | Here |
| --- | --- | --- |
| `da + nd(2, 3)`, either order | `(y: 2, x: 3)` | same |
| `da + nd(3,)` | `(y: 2, x: 3)` | same |
| `da + nd(1, 3)`, `da + nd(2, 1)` | `(y: 2, x: 3)` | same |
| `da + nd()` (zero-dimensional) | `(y: 2, x: 3)` | same |
| `da + nd(4, 2, 3)` | `ValueError` | `PositionalOutranksNamed` |
| `da(x: 3) + nd(3, 3)` | `ValueError` | `PositionalOutranksNamed` |
| `da + nd(2,)` | `ValueError` (NumPy mismatch) | `IncompatibleExtents` |
| `named(y: 1) + named(y: 2)`, no coordinates | `AlignmentError` | `IncompatibleExtents` |
| `named(x: 3) + named(y: 2, x: 3)` | `(x: 3, y: 2)` | same |
| `da(y: 1, x: 3) + nd(2, 3)` | accepted: `y` has length 2, its coordinate length 1 | `PositionalStretchesNamed` |

The last row is the one deliberate difference. xarray returns an object whose `y`
coordinate no longer matches its dimension. ArrIR rejects it, for three reasons: one
rule covers every case ("only positional axes stretch"); shape rules don't depend on
whether an axis has a coordinate; and allowing it later loosens a rule, which can't
break a plan that was valid.

### More than two operands

`where` and the rest of #32 take `n` operands. They combine in two stages:

1. **Named operands** combine by name, as a left fold in operand order. Shared dims must
   agree and new dims are appended, so the output order is each dim's first appearance
   across the operands. The fold is stable under regrouping: `(a + b) + c` and
   `a + (b + c)` give the same order. Swapping operands changes the order, as in xarray,
   so a rewrite that swaps them must also insert a `permute_dims`.
2. **Positional operands** broadcast together by the Array API, then adopt names from the
   named result of stage 1, under the two conditions above. If there are no named
   operands, the result is positional.

Adoption is against the combined named shape, not against any single named operand.
This surprises users: `nd(2, 3) + yx(y: 2, x: 3)` is fine on its own, but
`where(cond(x: 3), nd(2, 3), yx(y: 2, x: 3))` fails, because `cond` puts `x` first and the
combined order is `(x: 3, y: 2)`, against which `(2, 3)` doesn't fit. Likewise, with
`x(x: 3)`, `y(y: 3)` and a positional `p(3,)`, `p` aligns with `y`, the last axis of
`(x, y)`, whatever position `p` has among the operands.

Evidence from xarray 2026.9.0 with NumPy 2.5.4, where `cond` and `x` are on `(x: 3)`,
`yx` on `(y: 2, x: 3)` and `nd` is a NumPy array:

| Expression | xarray | Here |
| --- | --- | --- |
| `where(cond, yx, x)` | `(x, y)` | same |
| `where(cond, x, yx)` | `(x, y)` | same |
| `where(cond, nd(2, 3), yx)` | `ValueError`: shapes (3,1) (2,3) (3,2) can't broadcast | `IncompatibleExtents` |
| `where(nd_bool(3,), x, yx)` | `ValueError`: shapes (3,) (3,1) (3,2) | `IncompatibleExtents` |
| `where(cond, x, nd(2, 3))` | `ValueError`: unexpected number of dimensions | `PositionalOutranksNamed` |

The first two rows show named operands combining in first-appearance order. The third
and fourth show positional operands aligned from the right against that combined
order. The last shows a positional operand with more axes than the named result
rejected.

Within each stage, acceptance and requirements don't depend on order. `Broadcast` terms
are flattened, sorted and deduplicated, so the size symbols agree; the requirements
normalise to the same set (see [Rewrites](#rewrites-keep-requirements-exactly)).
Positional extents fit a named extent `n` whether they meet it one by one or after
broadcasting together: `p` and `q` broadcast to a value in `{1, n}` exactly when each
of them is in `{1, n}`. Which error is reported first can depend on the order.

Whether reordering or regrouping the operands is *allowed* is a numerics question
(strict floating-point order, design.md and #47); this only says the shape requirements
then agree.

### Proven, contradicted, unresolved

- **Proven** pairs need nothing more.
- **Contradicted** pairs are an error at inference, reported with the node and axis.
- **Unresolved** pairs aren't errors. Inference records a `ShapeRequirement` and
  carries on.

```rust
pub enum ShapeRequirement {
    /// Every pair of members is equal, or one of the pair is 1. Unordered: the
    /// members are sorted, with `Broadcast` size symbols expanded into theirs.
    Broadcastable { node: NodeId, axis: AxisRef, members: Vec<Extent> },
    /// A positional extent is 1 or equals the named extent whose name it adopts.
    FitsNamed { node: NodeId, dim: DimId, named: Extent, positional: Extent },
    /// The two extents are equal, for example a mask's length and its dimension (#36).
    /// Unordered.
    Equal { node: NodeId, axis: AxisRef, pair: [Extent; 2] },
    /// `staggered_dim`'s length is `centre_dim`'s plus `delta` (grids.md).
    StaggerOffset { node: NodeId, centre_dim: DimId, staggered_dim: DimId, delta: i8 },
}
```

**Each requirement is checked by the core** at the resolution point of its latest term:
at bind for declared extents, at label resolution if a `LabelSelect` is involved, at
execution if a `CountNonZero` is. A host supplies the facts and calls the core's check;
it doesn't reimplement it. For example, `ds.sel(time="2020") + (time: 12)` records
`s = 12` with `s` a `LabelSelect` term. Bind can't decide it, and label resolution
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

A rewrite must leave the meaning of the requirement set unchanged. Fewer requirements
would let a plan that fails at bind run instead; more would make a plan that bound
fine fail. Requirement sets are compared in a normal form, ignoring node IDs:

- each `Broadcastable` becomes the unordered pairs of its members, after expanding
  `Broadcast` size symbols, and pairs that are already proven are dropped;
- each `FitsNamed` whose positional extent is a `Broadcast` size symbol becomes one
  `FitsNamed` per member, and a `Broadcastable` pair whose members both fit the same
  named extent is dropped as implied;
- each `Equal` is an unordered pair;
- the result is a set, so duplicates from different nodes count once.

So `a + b` and `b + a` agree, and so do `(a + b) + c` and `a + (b + c)`: both give the
pairs `{a, b}`, `{a, c}` and `{b, c}`. Structural size symbols, normalised slices and
value numbering are what make "unchanged" achievable for selection pushdown.

A rewrite whose legality depends on which operand stretches needs that pair to be
proven, not unresolved. Pushing `Select { time }` below an `Add` (dag.md) selects on each
named operand that has `time`, whose extent equals the output's, and on nothing else.
A positional operand is selected by position at the axis aligned with `time`, unless that
axis is proven to be 1; with `p ∈ {1, k}` unresolved, the rewrite doesn't apply. #37
checks these conditions.

Semantic readiness (#26) uses the same distinction. An operation that needs a concrete
extent, such as a `ChunkSpec` block partition or a future `Reshape`, is not ready while
that extent is symbolic. That isn't a validation error.

## Errors

```rust
pub enum ShapeError {
    IncompatibleExtents { node: NodeId, axis: AxisRef, left: u64, right: u64 },
    PositionalOutranksNamed { node: NodeId, positional_rank: u32, named_rank: u32 },
    PositionalStretchesNamed { node: NodeId, dim: DimId, positional: u64 },
    MixedAxisNames { node: NodeId },
    SymbolInDeclaredSchema { open: NodeId, variable: VarId, axis: AxisRef },
}

/// Returned by the core's requirement check once the facts are supplied.
pub enum RequirementViolated {
    /// The first pair of members that is neither equal nor contains a 1.
    NotBroadcastable { node: NodeId, axis: AxisRef, left: u64, right: u64 },
    DoesNotFitNamed { node: NodeId, dim: DimId, named: u64, positional: u64 },
    NotEqual { node: NodeId, axis: AxisRef, left: u64, right: u64 },
    StaggerMismatch {
        node: NodeId,
        centre_dim: DimId,
        centre: u64,
        staggered_dim: DimId,
        staggered: u64,
        delta: i8,
    },
}
```

`AxisRef::FromEnd(1)` is the last axis. `ShapeError`s sit beside dag.md's
structural `PlanError`: a plan can be structurally valid and still have a shape error.

## Worked examples

Each operand is an array. Positional shapes are written as tuples of extents, such
as `(2, 3)`; named axes are written `name: extent`. `s`, `t` and `p` are size symbols.

Positional:

1. **`(100, 1) + (1, 50)`.** Aligned pairs are `100 with 1` and `1 with 50`. Output
   `(100, 50)`, proven.
2. **`(2, 3) + (4, 3)`.** The pair `2 with 4` is contradicted:
   `IncompatibleExtents { node, axis: FromEnd(2), left: 2, right: 4 }`.
3. **`(s, 1) + (1, t)`.** Output `(s, t)`, proven, because each pair has a `Known(1)`.
4. **`(s,) + (t,)` from two `Open`s.** Output `(u,)` with `u = Broadcast { of: [s, t] }`,
   and the requirement `s = t` or `s = 1` or `t = 1`. Binding `s = 10, t = 10` gives
   `u = 10`; `s = 1, t = 7` gives `u = 7`; `s = 10, t = 7` is
   `RequirementViolated::NotBroadcastable` at bind, before execution.
5. **Zero size.** `(0,) + (0,)` gives `(0,)`. `(0, 3) + (3,)` gives `(0, 3)`. `(0,) + (1,)`
   gives `(0,)`. `(0,) + (5,)` is `IncompatibleExtents`. `(0,) + (s,)` gives `(0,)` with
   `s ∈ {1, 0}`.
6. **Scalars.** `() + (s, 3)` gives `(s, 3)`. `() + ()` gives `()`. A literal such as the
   `2` in `a * 2` is a zero-input node with shape `()` (#47's `Constant`).

Named:

7. **By name.** `(y: 2, x: 3) + (y: 2, x: 3)` gives `(y: 2, x: 3)`. `(x: 3) +
   (y: 2, x: 3)` gives `(x: 3, y: 2)`: first operand's dims, then the new `y`. `(x: s) +
   (y: t)` gives `(x: s, y: t)`, proven, with no requirement.
8. **No stretching.** `(y: 1, x: 3) + (y: 2, x: 3)` is `IncompatibleExtents`, as xarray's
   `AlignmentError`. `(t: s) + (t: k)` from two `Open`s records `Equal` for `s = k`.
   `(t: s) + (t: s)` from one `Open` is proven.
9. **Names, not positions.** `(lat: 180, lon: 180) + (lat: 180)` gives
   `(lat: 180, lon: 180)`, adding along `lat`. NumPy, with the same shapes unnamed, adds
   along the last axis, which here is longitude.
10. **Label selection.** `ds.sel(time="2020") + (time: 12)`: `s = 12` with `s` a
    `LabelSelect` term, checked at label resolution, not at bind.
11. **Three operands.** `where(c(x: 3), a(y: 2), b(y: 2, x: 3))` gives `(x: 3, y: 2)`;
    regrouping gives the same order.

Named with positional:

12. `(y: 2, x: 3) + (2, 3)` gives `(y: 2, x: 3)`. `(y: 2, x: 3) + (3,)` gives the same.
    `(x: 3) + (3, 3)` is `PositionalOutranksNamed`. `(y: 1, x: 3) + (2, 3)` is
    `PositionalStretchesNamed`. `(y: s, x: 3) + (p, 3)` gives `(y: s, x: 3)` and records
    `FitsNamed` for `p ∈ {1, s}`.
13. **Adoption after combining names.** `where(p(3,), x(x: 3), y(y: 3))` combines `x` and
    `y` into `(x: 3, y: 3)`, then `p` adopts `y`, the last axis.

## No explicit Broadcast node

Inference on each elementwise node gives the output shape, for named and positional
operands alike, so lowering doesn't insert a `Broadcast` node before arithmetic. A node
would add a second spelling of the same plan that every rewrite would have to normalise.
The Array API's `broadcast_to` and `broadcast_arrays` are frontend functions; #41's
coverage matrix records whether they lower to a node or a composition.

`expand_dims` is positional only: named shapes never need a length-1 axis to line up,
because they align by name. Reordering axes (`permute_dims`, #4) remains an operation in
its own right, and a rewrite that swaps named operands needs it to keep their order.

## Implementation through #2

#2 implements this in small PRs:

1. `AxisName`, `Extent::Symbolic`, `SizeSymbol`, `MixedAxisNames` and
   `SymbolInDeclaredSchema`, with serde tests showing that a declared schema with a size
   symbol is rejected.
2. The size table and per-node inference for `Open`, `Project`, positional `Select` and
   `Reduce`, with checked element counts (from closed #1).
3. Broadcasting for a test operation of any arity, named, positional and mixed, with
   each worked example and each row of the xarray table above as a test, including the
   requirements it records and the output axis order. #33 then uses it for `Add`, `Multiply`
   and `GreaterThan`.
4. The requirement check, with `RequirementViolated` tests at bind and at label
   resolution.

None of these reads data, and the tests assert that.

## Decisions

Charles decided questions 2 and 10 directly, and accepted the recommendation on every
other question on 1 November 2026.

1. **One `Extent` enum or two types.** Adding `Symbolic` to #26's `Extent` keeps one
   schema type for declared and derived schemas. The risk is that a single serde enum
   lets a size symbol onto the wire unless something stops it. Marking the variant
   `#[serde(skip)]` makes serialising it an error and makes it unreadable, on top of
   `SymbolInDeclaredSchema`. Two types (declared `Known | Unknown`, derived
   `Known | Symbolic`) make it a compile-time fact but duplicate `Schema`.
   **Decided: one enum with the serde guard.**
2. **(A) Names in broadcasting.** Decided by Charles, 1 November: see
   [Broadcasting](#broadcasting). A positional operand may not stretch a named length-1
   axis, with or without coordinates: one rule ("only positional axes stretch"), shape
   rules that don't depend on coordinate roles, and a restriction that can be loosened
   later without breaking a valid plan.
3. **(B) Size-symbol identity.** By term structure (as written), so rewrites keep
   proofs, or by node, which is simpler but makes pushdown turn proven pairs into
   unresolved ones. **Decided: structure.**
4. **(C) Declared shared unknowns.** Should a host be able to say two `Open`s share an
   unknown extent? **Decided: no.** A host that knows they're equal usually knows the
   value; if not, a requirement at the combining operation catches a mismatch.
5. **Unresolved broadcasts.** Accept them with a requirement checked at its resolution
   point, or reject them until the extents are known? **Decided: accept.**
6. **No solver.** Check each requirement on its own for now? **Decided: yes.**
7. **No Broadcast node,** with `broadcast_to` left to #41? **Decided: yes.**
8. **Staggered offsets.** A `StaggerOffset` requirement (as written), or an offset term
   `s + delta` so an outer dimension's size symbol derives from its centre's?
   **Decided: the requirement,** until a rewrite needs to reason about offsets.
9. **Value numbering for data-dependent terms.** Key `CountNonZero` and `LabelSelect`
   by value number (as written), so shared masks and path copies keep their size
   symbols, or by node, which is simpler but makes any rewrite near a mask or label
   selection turn proven pairs into unresolved ones? **Decided: value numbering.**
10. **Named outer broadcasting.** Decided by Charles, 1 November: named shapes align
    by name, so `a(x) + b(y)` gives `(x, y)` with no broadcast node and no
    `expand_dims`. A dim on one operand only is created on the other, not stretched.
