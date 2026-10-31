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
- **Unreachable nodes are logically deleted.** Compaction is explicit and returns an
  ID mapping, because it's the only step that invalidates IDs.
- **Derived schemas are never stored.** Since nodes don't change, a cache indexed by
  node ID never goes stale.
- **The Rust arena is the plan.** Hosts lower into it through a builder rather than
  each keeping a validated node table of their own.
- **One `Open` per binding, and no implicit merging** of equal nodes.

## Not settled

The seven questions at the end of [dag.md](dag.md), and the rewrite termination
measure for graphs, which goes to #37.
