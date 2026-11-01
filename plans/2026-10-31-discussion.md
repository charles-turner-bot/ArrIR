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

## Dtypes (#30)

Details in [dtypes.md](dtypes.md). All pending review.

- **Implicit promotion only where it is exact.** The Array API 2025.12 table is used for
  the pairs it defines. Mixed kinds and `u64` with a signed integer are rejected, and
  lowering writes its host's rule as an explicit `Cast`.
- **No weak types in the plan.** Python scalars are typed by lowering, as Array API and
  NEP 50 describe, and enter as typed constants.
- **A reduction states its accumulator and its result dtype.** The input must convert
  to the accumulator exactly; otherwise lowering casts first. There is no default
  integer. Charles decided (1 November) on a separate `result` field rather than a
  `Cast` after the reduction. It allows `f64` narrowed to `f32` (by cast.md's rule and
  `OutOfRange`) and an integer-accumulated mean with a float result.
- **A reduction declares its order.** `Sequential` is strict, and `Unspecified` is an
  explicit relaxation. Lowering from NumPy, xarray and dask writes `Unspecified`,
  because none of them promises an order. The first review found that an
  unspecified-only design reversed design.md's strictness rule inside reductions.
- **Float narrowing is writable.** cast.md gains IEEE rounding for conversions to
  float types, `OutOfRange::Infinity`, and an explicit `Collision` for any rounding
  conversion of a variable with missing values.
- **No `Promote` node.**

Open questions are at the end of [dtypes.md](dtypes.md).

## Not settled

The questions at the end of [dag.md](dag.md), and the rewrite termination
measure for graphs, which goes to #37.
