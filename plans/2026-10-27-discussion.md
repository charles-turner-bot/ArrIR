# Design discussion: 27 October 2026

## Inputs

- #16 (Mike): shell 1 (schema and effects) plus a staged map of shells and nine purity
  rules.
- #17 (Charles): design docs for an `Open` operation, a crate split, and a Rust binding
  of mdim-refs.
- Mike's review of #17, which asked for one agreed first step, answered the layout
  questions, and asked how #14 and #15 relate.

#17 was written against `main` without this contract in view, so it contradicted it in
three places: it proposed a five-crate workspace, a plan that names a URI, and a
single-array schema. #16 also diverged from it: it built effects on the legacy API and
resolved labels at plan time.

## Decisions

- **This contract (#14) is the base.** The design parts of #14, #16 and #17 are folded
  into one PR, so they're agreed in one place. Code stays in separate PRs.
- **Mike's nine purity rules lead the contract.** Rule 5 is reworded so that a label
  selection stays representable before it is resolved.
- **Dependency graph, not shells.** Shells impose a total order: each may depend on
  everything inside it, which couples outer components to all inner ones and delays
  independent work. Host adapters, for example, need only operations, yet would come
  last. The components form a graph instead, with one crate per component.
- **Crates from the start.** This reverses "ordinary modules, not a workspace". Crates
  let the compiler enforce that the logical plan never sees byte references. To avoid
  speculative crates, each one is created when its first code lands, but its place in
  the graph is fixed in the contract now.
- **`Open { binding, schema }` starts every plan.** This replaces #17's `Open { uri }`,
  which contradicted the binding rule, and covers Mike's suggestion of a schema snapshot
  so that a plan validates without I/O.
- **`Open` binds a dataset.** The logical schema is dataset-level, as in xrexpr, and
  choosing a variable is a projection.
- **Effects are written with each operation,** not as a separate later step. #16's
  effects were written against the legacy `Plan`, which the new operations replace.
- **The layout follows byterefs.** `arrir-layout` binds byterefs 0.1 (the text of
  mdim-refs/0.2) as it stands: the flat form only, today's three-file layout, no 0.3
  additions. Proposed changes go to byterefs as decision records.

## What happens to the open PRs

| PR | Outcome |
| --- | --- |
| #14 | Its contract is carried over and amended with these decisions. |
| #15 | Rebased onto this contract, and becomes the workspace PR, with the IDs in `arrir-types`. |
| #16 | Its purity rules, conventions and shell map move into this contract. Its schema seeds roadmap step 3, and its effects move into the operation PRs. |
| #17 | Narrowed to [the `arrir-layout` design](chunked-layout.md), updated with Mike's answers. |

## Not settled

- The scope of `arrir-physical`: #16's regions, chunk keys, request lists and demands
  (target grids and warps).
- When footprints are defined relative to the operation PRs.
- Whether coordinate models are their own crate, or part of `arrir-physical`.
