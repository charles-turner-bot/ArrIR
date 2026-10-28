# ArrIR: extraction contract

Status: agreed direction, amended 2026-10-27 (see
[the 27 October discussion](2026-10-27-discussion.md)). The 28 October amendments are
proposed (see [the 28 October discussion](2026-10-28-discussion.md)). Implementation
proceeds in the small steps below.

## Purpose

Extract the semantic IR that xrexpr developed to make its optimiser sound.
The same representation must support both shared rewrites and translation between
named-array implementations, initially xrexpr/xarray and xarray-ts, and lowering to
GDAL.

```text
host calls -> host lowering -> ArrIR -> optional rewrites -> target adapter
```

ArrIR describes operations, not their execution. It contains no Python objects,
JavaScript callbacks, filesystem access, or array computation. An adapter must
faithfully implement the represented semantics or reject the translation.

## Purity rules

These nine rules are the shortest statement of what ArrIR is. Every crate, operation
and PR is held to them. They come from Mike's shells proposal (#16). Rule 5 is reworded
so that it agrees with the label rule below.

1. **Planning reads no data chunks.** It uses schemas, chunk grids and byte-ref sets
   only.
2. **Plans, schemas and demands are values:** serialisable, versioned, with no host
   objects.
3. **Every operation declares its effects and its footprint:** what it does to
   dimensions, and which input region each output region needs.
4. **No silent approximation.** An adapter emits an equivalent operation or returns an
   error.
5. **Physical planning works in index space.** Labels and coordinate reference systems
   are resolved to positions before chunks are planned. A label selection is still
   representable without coordinates. Resolving it is a separate step.
6. **The core performs no I/O and has no async runtime.** Hosts fetch and decode.
7. **Determinism.** The same plan and source give the same chunk list in the same order.
8. **Regularity is declared, never inferred.** Arithmetic shortcuts are used only when
   the schema says a grid or coordinate is regular.
9. **A chunk key always names the same encoded bytes,** so any cache of payloads is
   disposable and interchangeable with the references it caches.

## Decisions

- Start with linear unary chains, not a speculative graph.
- Extract xrexpr's **lowered** operations, not its fluent call recorder.
  `ContextOpen` belongs in host lowering; contextual reductions are semantic nodes.
- Keep one semantic algebra. Rewrites consume it, not a competing optimiser IR.
- Preserve floating-point evaluation order by default. Reassociation and other
  numerically relaxed rewrites require explicit permission. Strictness does not
  imply that unrelated execution engines already guarantee bitwise equality;
  adapters must expose or reject unsupported numerical requirements.
- Support external arrays through explicit binding IDs and declared metadata.
  A binding is not an embedded array, a Python pointer, or a download instruction.
  Importing a plan never executes or resolves it automatically.
- Unknown facts prevent unjustified rewrites; they are not zeros or empty sets.
- Preserve history. The existing root API is a legacy scaffold while the replacement
  is built, now in the crates below rather than the `portable` module first proposed.
  Do not silently bridge the two representations. This is a temporary migration
  boundary, not two permanent semantic IRs.

Added on 27 October:

- **A plan names its data by binding, not location.** Every plan starts with
  `Open { binding: BindingId, schema }`: a local name and the declared schema of the
  dataset it binds. Mapping the binding to a URI, a byte-ref table or an in-memory array
  happens outside the plan. A plan therefore validates offline and never fetches
  anything when imported.
- **`Open` binds a dataset, not one variable,** because the logical schema is
  dataset-level, as in xrexpr. Choosing a variable is a projection.
- **Label selection is logical. Resolving it is not.** A label `Select` is representable
  without loading coordinates (as below). Turning labels into positions needs coordinate
  values, so it is a separate step on the way to physical planning (purity rule 5).
- **ArrIR binds the byte-ref format but does not govern it.** The storage layout is the
  [byterefs](https://github.com/byterefs/spec) specification, whose 0.1 starts as the text
  of mdim-refs/0.2. `arrir-layout` binds it as it stands. Changes to the format go through
  byterefs decision records, not through ArrIR.
- **Components form a dependency graph, not a sequence of layers** (see
  [Architecture](#architecture)).

Proposed on 28 October, designed in [grids.md](grids.md):

- **GDAL is a target.** A plan must be able to lower to GDAL VRT, not only to
  xarray-shaped hosts. That needs grids in the schema, `Select` effects on grids, a
  `Warp` operation and a GDAL adapter, proposed in that order as stacked PRs.
- **Grids are schema entities.** A grid owns its dimensions, the CRS, the pixel
  interpretation and a declared coordinate model (affine, rectilinear or curvilinear).
  Spatial variables, and their coordinate variables, refer to a grid.
- **Staggering is part of the schema from the start.** Each gridded variable records its
  position on each axis, so A, B and C grids are all representable. Cell bounds are the
  coordinates at the `Outer` position.
- **The design is not GDAL-shaped.** Grid kinds beyond 2-D rasters (meshes, discrete
  global grids, N-D grids) are sketched in grids.md, so adding them is a new variant,
  not a change. Each grid concept is checked against at least one non-GDAL
  implementation (xgcm, SGRID, uxarray, xdggs) before it's agreed.

## Architecture

Three kinds of thing are involved, and they are generic at different levels:

| | Answers | Here |
|---|---|---|
| **Logical plan** | What to compute | `arrir`: `Open → Select → Reduce` |
| **Storage layout** | What is on disk, and where | `arrir-layout`: the byterefs binding |
| **Physical plan** | Which chunks and byte ranges to read | `arrir-physical`, later |

The storage layout holds the data's logical schema (dimensions, shape, data type), but it
is not a logical plan. A filtered reference table looks like the scan step of a physical
plan, but the table only describes what exists. A physical planner combines a logical
plan with a storage layout to produce a physical plan.

mdim-refs calls its placement layer "the plan". In ArrIR, *plan* always means the logical
chain, and that layer is called *placements*.

### Dependency graph

Components depend on each other as a graph, not as nested shells. Each one depends only
on what it uses. Independent work can then proceed in parallel, and no component is
coupled to everything built before it.

```text
                          arrir-types
                IDs, values, schema, Statistics
               /             |               \
          arrir         arrir-layout       coordinates
   operations, plans,   byterefs binding    coordinate models,
   validation, effects,                     label resolution
   footprints
     /         \                                 |
arrir-logical   adapters                         |
rewrites        xrexpr, xarray-ts, ...           |
     \______________________|____________________/
                            |
                      arrir-physical
          regions to chunk keys, request lists, demands
```

Rules for the graph:

- `arrir` and `arrir-layout` never depend on each other. They meet only through types
  in `arrir-types`, and later in `arrir-physical`.
- Adapters depend on `arrir` and `arrir-types` only. Rendering a plan to xarray, R or JS
  never needs byte references.
- Nothing depends on `arrir-physical`.

Each component is its own Cargo crate in one workspace, so the compiler enforces the
graph. A crate is created when its first code lands, but its place in the graph is fixed
here. A new dependency edge is a contract change and is recorded in `plans/`.

Each component has a "done when" test against shared fixtures, and later components may
not change the meaning of earlier ones.

## Conventions

From #16, for positional semantics:

- Dimensions are ordered. Order is part of the schema, and no operation in the current
  vocabulary reorders it.
- Positions are 0-based, and a negative position counts back from the end. Slices are
  half-open, with Python semantics. Adapters for hosts with other conventions translate
  them. R, for example, is 1-based and treats negative indices as exclusions.
- A scalar indexer drops its dimension. Slices, position lists and masks never do. A host
  whose indexing drops length-1 results (R's `drop = TRUE`) lowers those to scalar
  indexers.
- A positional mask must match the dimension length exactly. There is no recycling.
- Label selections have unknown extent until they are resolved.

## What to extract

Source reference: charles-turner-1/xrexpr, main at
`76fc58073a3e91dafdc9dd79d21c4ce636f02f1e`.

| Source | Portable destination | Boundary |
| --- | --- | --- |
| `ir.py: LoweredOp` | structs + exhaustive `Operation` enum | no fluent builders |
| `Reduce.consumes`, `keepdims` | symbolic dimension set + explicit option | do not cache derived effects |
| reduction name + result-affecting options | explicit semantic reduction specification | name alone is insufficient |
| `Select.indexer`, `indexers.py` | typed positional, label and advanced indexers | preserve indexing mode and broadcast dims |
| `Scan.dims` | scan specification | preserve order-sensitive behaviour |
| `Project.variables`, `single` | projection + result kind | array versus dataset is observable |
| `Rename.mapping`, `Drop.variables` | typed relabelling/drop specifications | distinguish variable and dimension roles |
| `Rechunk.chunks`, `uniform` | named versus all-dimension chunk requests | execution policy is not value semantics |
| grouped/windowed/weighted nodes | explicit context + reduction + external inputs | structural metadata alone is insufficient |
| `schema.py: SchemaState` | logical schema | variables, ordered dims, coordinate roles, unknown extents |
| `optimize.py: dim_effect`, rules | derived analyses + rewrite module | no independently authored safety flags |
| `lower.py: _emit_node` | adapter conformance obligations | semantic changes must reach emitted calls |

Audit every raw source argument before porting a variant. Classify it as result
semantics, derived analysis, execution hint, or host-only replay payload. Examples
include `skipna`, `ddof`, `drop`, grouping boundaries, and window alignment.
Do not replace this audit with an untyped `args`/`kwargs` JSON bag.

Portable symbol IDs are strings with distinct Rust types for dimensions,
variables, external bindings and grids. They are not coordinate values. A host with
non-string names assigns collision-free symbols and retains its name mapping.
An empty host name need not be rejected just because it is an empty string.

Label selection remains representable without loading coordinate data merely to
turn labels into positions. External bindings need schema/type requirements and
target capability checks before their operations can be emitted.

Unsupported host calls may remain barriers in a host-owned wrapper, preserving a
same-host replay payload. This does not make that payload portable. Export must
fail if unresolved host-only semantics remain in the translated segment.

The first rewrites to port are xrexpr's four rules that act only on selections and
rechunks: `merge_adjacent_selects`, `merge_adjacent_rechunks`, `pushdown_selects` and
`pushdown_selects_past_rechunks`. Its four projection rules follow once projections
exist. The fixpoint and its termination measure (no rule lengthens a plan or moves an
operation rightwards) carry over with them.

## Implementation boundaries

Prefer named structs, explicit enums and ordinary modules *inside* each crate. Use serde
for the boundary, not as the definition of semantics. Shared fixtures must pin the
eventual wire contract, including cross-language integer precision, scalar types and
version compatibility.

The legacy scaffold moves into the `arrir` crate as `arrir::legacy` and is not extended.
It is retired once the replacement has a tested vertical slice.

## Small-PR roadmap

Aim for roughly 100-200 changed lines per PR. If a step does not fit, split it;
do not omit a semantic field or its tests to satisfy the line count.

1. This contract and roadmap; no replacement implementation claims.
2. A workspace with `arrir-types` and `arrir`. Distinct dimension, variable and
   external-binding IDs go in `arrir-types`, with serde tests and CI. The legacy
   scaffold moves to `arrir::legacy`.
3. Logical schema in `arrir-types`: variable dimensions, coordinate roles, and
   known/unknown sizes, with small invariant tests. #16's `ArraySchema` is the starting
   point, widened to a dataset. Then, in separate PRs: grids, CRS and pixel
   interpretation, and staggered placements, as in [grids.md](grids.md).
4. Positional indexer values, then label/advanced-index and binding values in
   separate PRs. Preserve source distinctions used by rewrite proofs.
5. A deliberately bounded reduction specification with explicit result options;
   reject anything outside that supported subset.
6. Reduce and Select operation structs, each with its effects (purity rule 3), then the
   linear plan starting with `Open`, and structural validation, split into
   independently understandable PRs. Footprints, the other half of rule 3, come later
   but before any physical planning. Until an operation has one, it can't be physically
   planned.
7. Schema propagation along the chain, and unknown-fact handling.
8. An xrexpr adapter for this subset, first with import/export round-trip tests.
9. One existing rewrite in `arrir-logical`, tested against xrexpr's rule and execution
   oracle.
10. xarray-ts adapter/conformance fixtures for the same subset, then broaden
    operation coverage one variant at a time. Retire the legacy scaffold once
    the replacement has a tested vertical slice; do not expand both APIs.

In parallel, once step 2 lands: `arrir-layout`, the byterefs binding, designed in
[chunked-layout.md](chunked-layout.md). It needs only `arrir-types`.

Later, and scoped in their own proposals: footprints for each operation (purity rule 3,
needed before physical planning), coordinate models and label resolution, then
`arrir-physical` (#16's regions, chunk keys, request lists and demands).

The first integration milestone compares original, round-tripped and rewritten
execution on real arrays. Include reduction options, negative/scalar indexes,
empty dimensions, coordinates and unknown extents. Unit tests of structs alone
do not prove a faithful extraction or cross-engine equivalence.

## Learning and review

Prefer named structs, explicit enums, ordinary `impl` blocks and readable tests.
Avoid macro-generated type families, generic adapter frameworks and clever
lifetimes until a concrete use case warrants them. Each code PR explains its
Rust concepts, one semantic decision, how to run it, and what is still missing.
The goal is a library Charles can understand and extend, not merely generated
code that compiles.
