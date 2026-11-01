# Design discussion: 31 October 2026

## Inputs

- Issue #27, from the 31 October backlog review: replace the linear unary chain with a
  multi-input DAG.
- The open PRs it builds on: #15 (workspace and IDs) and #26 (schemas and values).

## Why now

The contract chose "linear unary chains, not a speculative graph". The graph is no
longer speculative. The next proposed operations, elementwise arithmetic (#32),
alignment (#34) and computed masks (#36), all take two inputs, and `mean(a + b)` is the
first milestone in #27's roadmap. Settling the container before #28 builds it avoids
writing a chain type and then replacing it.

## Proposed

Details in [dag.md](dag.md). All pending review.

- **Append-only arena, inputs point backwards.** Node IDs are positions, and every
  input is an earlier node, so cycles can't be written and validation is one pass.
- **Rewrites append and move the root.** Old IDs and roots stay valid, and nodes shared
  with other consumers are never changed underneath them.
- **Unreachable nodes are logically deleted.** Compaction is explicit, returns an ID
  mapping, and isn't needed for export, so it stays out of #28.
- **Derived schemas are never stored.** Since nodes don't change, a cache indexed by
  node ID never goes stale.
- **The Rust arena is the plan.** Hosts lower into it through a builder rather than
  each keeping a validated node table of their own.
- **One live `Open` per binding, and no implicit merging** of equal nodes. Dead nodes
  don't count, so a rewrite can replace an `Open`.
- **A `Graph` owns the nodes; a plan is a graph plus one root.** Rewrites append in
  place and return a new root, copying the path from the rewritten node up to the root.
- **Arity depends on the operation value,** so a weighted `Reduce` or a `Select` with
  a computed mask takes extra inputs. `Project` keeps its array and dataset forms.
- **`DimId` is plan-wide for named axes; `VarId` and `GridId` are per schema.** Neither
  #15 nor #26 set ID scope, and the review of #26 deferred it here. A shared dimension
  name asserts one axis, not matching labels or extents.

## Symbolic shapes (#29)

Details in [shapes.md](shapes.md). All pending review.

- **Unknown extents get identities.** `Extent` gains `Symbolic`. Hosts still declare
  `Unknown`; inference turns each one into a size symbol, so `a + a` is provably
  compatible without knowing `a`'s length.
- **Size symbols are identified by structure,** not by node, so the same slice of the
  same extent is one size symbol and pushdown keeps its proofs.
- **Broadcasting follows the Array API**, aligned from the last axis, with n operands
  as a left fold. Whether the core also requires aligned axes to share a `DimId` is
  left for review, with a recommendation. Named broadcasting stays with #34.
- **Unresolved isn't an error.** A pair that can't be proven compatible records a
  requirement. The core checks it when its facts arrive: at bind, at label resolution
  or at execution. Rewrites must keep the requirement set unchanged.
- **No Broadcast node and no constraint solver** for now.

## Not settled

The questions at the end of [dag.md](dag.md) and [shapes.md](shapes.md), and the
rewrite termination measure for graphs, which goes to #37.
