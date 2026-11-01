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
    /// same symbol are equal. Never a stand-in for a known value.
    Symbolic(ExtentSymbol),
    Unknown,
}

/// Plan-local, numbered in the order inference introduces them.
pub struct ExtentSymbol(u32);
```

An `ExtentSymbol` is not one of design.md's "portable symbol IDs" (dimension, variable,
binding and grid names). It is an unknown number with an identity.

The variants have distinct roles:

- **`Unknown` is what a host declares.** An `Open` schema may say an extent is
  `Unknown`. It may not contain `Symbolic`; validation rejects that with
  `SymbolInDeclaredSchema`.
- **`Symbolic` is what inference derives.** Inference gives every non-known extent a
  symbol, so derived schemas contain `Known` and `Symbolic` only.
- **`Known(0)` is empty**, as in #26. It is not unknown.

`ExtentSymbol` lives in `arrir-types` next to `Extent`, so schemas don't depend on
`arrir`. What a symbol means lives in `arrir`, in a table beside the derived-schema
cache:

```rust
pub struct SymbolTable {
    /// Indexed by `ExtentSymbol`. Append-only, like the node arena.
    pub origins: Vec<SymbolOrigin>,
}

pub enum SymbolOrigin {
    /// An `Unknown` extent declared by an `Open`.
    Declared { open: NodeId, dim: DimId },
    /// A label `Select` not yet resolved to positions (design.md conventions).
    LabelSelect { node: NodeId, dim: DimId },
    /// A slice whose length depends on a symbolic input extent.
    Positional { node: NodeId, dim: DimId },
    /// The number of true values in a computed mask (#36's CountNonZero).
    CountTrue { node: NodeId, mask: NodeId },
    /// The result of broadcasting two different symbols (below).
    Broadcast { node: NodeId, dim: DimId, left: ExtentSymbol, right: ExtentSymbol },
}
```

Because the arena is append-only (dag.md), so is the table. Symbols are numbered in
node order and then dimension order, so the same plan always gives the same numbering.
Neither the table nor derived schemas go on the wire, matching dag.md's "derived
schemas are never stored". Compaction discards the table along with the schema cache.

### Where symbols come from and where they go

- **One `Open`, one symbol per unknown dimension.** Two variables of one dataset that
  share `time` share its symbol, so `t + u` in dag.md's `mean(a + b)` example is proven
  compatible even when `time` is unknown.
- **Two `Open`s get different symbols** for a shared `DimId`. dag.md says a shared
  `DimId` asserts axis identity, not equal extents, and this keeps that rule.
- **Operations that keep an extent keep its symbol.** `Project`, elementwise operations
  on that axis and `Reduce` over other dimensions pass it through.
- **Operations that set an extent replace it.** A `Reduce` with `keepdims` gives
  `Known(1)`. A positional slice on a known extent gives a known length; on a symbolic
  extent it gives a new `Positional` symbol.
- **Nothing reads data to resolve a symbol.** Symbols resolve at bind time, when the
  host supplies each `Open`'s real extents, or at execution for `CountTrue`.

## Positional broadcasting

Two operands of an elementwise operation (#32) are aligned from their last axis, as in
the Array API. For each aligned pair of axes, the extents combine as in the table below.
The operand with the higher rank supplies the leading axes unchanged.

Aligned axes must have the same `DimId`. A mismatch is the error
`AxisNameMismatch`. It isn't a positional rule but a named check: dag.md makes a
`DimId` a plan-wide axis identity, so combining two different axes would be a silent
reinterpretation. An Array API host, which has no names, assigns them so that aligned
axes match. Ranks are always known, so it can do this at lowering, for example by naming
axes by their position counted from the end. Xarray lowering (#34) inserts transposes
and length-1 axes first, so that its named rules arrive here already aligned.

### Combining two extents

Write `L` and `R` for the extents of one aligned pair. `k` and `j` are known values
other than 1; `s` and `t` are different symbols.

| `L` | `R` | Output | Compatibility | Requirement recorded |
| --- | --- | --- | --- | --- |
| `Known(k)` | `Known(k)` | `Known(k)` | proven | none |
| `Known(1)` | anything `R` | `R` | proven | none |
| anything `L` | `Known(1)` | `L` | proven | none |
| `Known(k)` | `Known(j)`, `k ≠ j` | — | contradicted | error `IncompatibleExtents` |
| `s` | `s` | `s` | proven | none |
| `s` | `Known(k)` | `Known(k)` | unresolved | `s ∈ {1, k}` |
| `Known(k)` | `s` | `Known(k)` | unresolved | `s ∈ {1, k}` |
| `s` | `t` | new `Broadcast` symbol | unresolved | `s = t` or `s = 1` or `t = 1` |

`Known(0)` is an ordinary known value here: 0 with 1 gives 0, and 0 with 5 is
contradicted, as in the Array API. With a symbol, `Known(0)` gives `Known(0)` and the
requirement `s ∈ {1, 0}`.

A missing leading axis behaves as `Known(1)`, which is the Array API rule. A
zero-dimensional operand therefore broadcasts against anything.

### Proven, contradicted, unresolved

- **Proven** pairs need nothing more.
- **Contradicted** pairs are an error at inference, reported with the node and axis.
- **Unresolved** pairs aren't errors. Inference records a `ShapeRequirement` on the node
  and carries on. The host checks every requirement at bind time, before execution;
  `CountTrue` requirements are checked when the mask is evaluated.

```rust
pub enum ShapeRequirement {
    /// `left = right`, or either is 1.
    Broadcastable { node: NodeId, dim: DimId, left: Extent, right: Extent },
    /// `left = right` exactly, for example a mask's length and its dimension (#36).
    Equal { node: NodeId, dim: DimId, left: Extent, right: Extent },
}
```

Requirements are derived like schemas: recomputed from the plan, never stored on it.

There's no constraint solver. Each requirement is checked on its own. Two
requirements `s ∈ {1, 2}` and `s ∈ {1, 3}` jointly force `s = 1`, but inference doesn't
deduce that, and a jointly unsatisfiable set is found at bind time. A solver can be
added later without changing what a requirement means.

**Rewrites must keep requirements.** The rewritten plan's requirements must imply the
original's. Otherwise a rewrite could turn a plan that fails at bind time into one that
runs, which changes its meaning. #37's pushdown proof checks this.

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
```

`position_from_end` is 1 for the last axis. These sit beside dag.md's structural
`PlanError`: a plan can be structurally valid and still have a shape error.

## Worked examples

Each operand is an array; the axes are written as `name: extent`.

1. **`(100, 1) + (1, 50)`.** Left `(y: 100, x: 1)`, right `(y: 1, x: 50)`. Aligned pairs
   are `y: 100 with 1` and `x: 1 with 50`. Output `(y: 100, x: 50)`, proven.
2. **`(2, 3) + (4, 3)`.** Left `(y: 2, x: 3)`, right `(y: 4, x: 3)`. The pair `y: 2 with 4`
   is contradicted: `IncompatibleExtents { node, dim: y, left: 2, right: 4 }`.
3. **`(n, 1) + (1, m)`.** Left `(y: s, x: 1)`, right `(y: 1, x: t)`. Output `(y: s, x: t)`,
   proven, because each pair has a `Known(1)`.
4. **`(n,) + (m,)` from two `Open`s.** Left `(x: s)`, right `(x: t)`. Output
   `(x: u)`, where `u` is a new symbol with origin `Broadcast { left: s, right: t }`, plus
   the requirement `s = t` or `s = 1` or `t = 1`. If the host binds `s = 10, t = 10`,
   `u = 10`. If it binds `s = 10, t = 7`, binding fails before execution.
5. **`(n,) + (n,)` from one `Open`.** Both operands share the `Open`'s symbol `s` for `x`.
   Output `(x: s)`, proven.
6. **Zero size.** `(y: 0, x: 3) + (x: 3)` gives `(y: 0, x: 3)`. `(y: 0) + (y: 1)` gives
   `(y: 0)`. `(y: 0) + (y: 5)` is `IncompatibleExtents`. `(y: 0) + (y: s)` gives `(y: 0)`
   with the requirement `s ∈ {1, 0}`.
7. **Scalars.** `() + (y: s, x: 3)` gives `(y: s, x: 3)`. `() + ()` gives `()`.
8. **Wrong alignment.** `(lat: 180, lon: 180) + (lat: 180)` aligns `lon` with `lat`:
   `AxisNameMismatch { position_from_end: 1, left: lon, right: lat }`. NumPy accepts the
   same shapes, `(180, 180) + (180,)`, and silently adds along longitude. #34 lowers the
   xarray version by inserting a length-1 `lon` axis after `lat`.

## No explicit Broadcast node, for now

Inference on each elementwise node gives the output shape, so lowering doesn't insert
a `Broadcast` node before arithmetic. A node would add a second spelling of the same
plan that every rewrite would have to normalise. The Array API's `broadcast_to` and
`broadcast_arrays` are frontend functions; #41's coverage matrix records whether they
lower to a node or a composition. Inserting a length-1 axis (`expand_dims`) and
reordering axes (`permute_dims`, #4) are separate operations that #34 needs.

## Implementation through #2

#2 implements this after review, in small PRs:

1. `Extent::Symbolic`, `ExtentSymbol` and `SymbolInDeclaredSchema`, with serde tests
   showing that a declared schema with a symbol is rejected.
2. The symbol table and per-node inference for `Open`, `Project`, positional `Select` and
   `Reduce`, with checked element counts (from closed #1).
3. Broadcasting for a two-input test operation, with each worked example above as a
   test, including the requirement it records. #33 then uses it for `Add`, `Multiply` and
   `GreaterThan`.

None of these reads data, and the tests assert that.

## Questions for review

1. **A third `Extent` variant.** Add `Symbolic` to #26's `Extent` and reject it in
   declared schemas, or keep `Extent` as #26 has it and give derived schemas a separate
   extent type? One enum is simpler to read; two types make "never on the wire" a
   compile-time fact.
2. **Declared shared unknowns.** Should a host be able to say two `Open`s share an
   unknown extent? This proposal says no: declared `Unknown`s across `Open`s are always
   different symbols.
3. **Strict axis names.** Aligned axes must share a `DimId`. The alternative is a purely
   positional core that ignores names, which matches NumPy but lets an operation combine
   `lon` with `lat`.
4. **Unresolved broadcasts.** Accept them with a requirement checked at bind time
   (proposed), or reject them until the extents are known?
5. **No solver.** Is checking each requirement on its own enough for now?
6. **No Broadcast node.** Agreed, with `broadcast_to` left to #41?
