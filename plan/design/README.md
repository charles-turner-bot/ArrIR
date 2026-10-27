# ArrIR design: overview

| | |
|---|---|
| **Status** | Draft, open for review |
| **Authors** | Charles Turner, Michael Sumner |
| **Date** | 2026-10-27 |

The aim is a fully generic description of a load-and-operate pipeline over chunked array
data. It has two halves, each in its own document:

- **[Logical plan](logical-plan.md):** what to compute. A chain of operations,
  `Open → Select → Reduce`, that adapters render to xarray, R or JS. This builds on
  xrexpr.
- **[Chunked layout](chunked-layout.md):** what is on disk, and where. A generic model of
  chunked arrays and their byte references. This builds on Mike's mdim-refs model and
  `raad.virtualize`.

This page covers how the two halves relate and how the code is organised. Each linked
document is meant to be readable on its own.

## Where each piece sits

The two halves are generic at *different levels*, and neither is a physical plan:

| | Answers | Database analogue | Here |
|---|---|---|---|
| **Logical plan** | What to compute | Relational algebra: `Filter`, `Aggregate` | `arrir`: `Open → Select → Reduce` |
| **Storage layout** | What is on disk, and where | Catalog and file metadata: Parquet footers, Iceberg manifests | `arrir-layout`: the mdim-refs model |
| **Physical plan** | How to compute it | `Scan(these files) → Filter → HashAggregate` | Not yet designed: a separate, future layer |

The storage layout is easy to mistake for either of its neighbours:

- **It is not a logical plan, although it holds the data's logical schema.** Its
  dimension names, shape and data type are exactly what the logical plan's dimension
  names refer to. Everything else in it describes only how those elements happen to be
  stored.
- **It is not a physical plan, although it looks like one.** A filtered reference table
  ("these 31 chunks") resembles the scan step of a physical plan. The table itself only
  describes what exists; it says nothing about what to read or in what order. A physical
  planner combines a logical plan with a storage layout to produce a physical plan.

`Rechunk` blurs the same line from the other side. It is in the logical vocabulary, but
it only talks about layout, much like Spark's `repartition`. It stays in the chain as a
request that the physical layer honours, not as a change to the data's values.

### Glossary

The two halves came from different projects, and one word clashes:

- **"Plan" in ArrIR** means the logical chain of operations.
- **"Plan" in mdim-refs** means the placement layer: the `gdal mdim mosaic` VRT that says
  where each source's slab lands in the global array. That is part of the storage layout,
  not a plan in ArrIR's sense.

These documents use *plan* for ArrIR's meaning, and *placements* for the mdim-refs layer.

## Proposed structure

The two halves belong to one project, but they are kept apart and meet only through a
small, explicit interface. This is how mature query engines divide the same work.
DataFusion's `TableProvider` gives the logical planner a `schema()`, the cost model
`statistics()`, and the physical planner a `scan(projection, filters, limit)`. Calcite's
`RelOptTable` and Spark's DataSource V2 have the same shape. Table formats such as Iceberg
and Delta are separate projects from the engines that query them. xrexpr already worked
this way implicitly, with xarray acting as the provider for everything below the logical
plan.

### Why not merge them

- **Their users differ.** VirtualiZarr, GDAL and R tooling want layouts with no plans. An
  adapter rendering `Select → Reduce` to JS wants plans with no byte references.
- **They change at different rates.** Codec support and source formats will keep
  growing. The operation vocabulary should stay stable.
- **Merging has already gone wrong once.** An earlier draft embedded the layout in the
  plan (`Source::Inline`), which mixed the two layers. If the logical crate can name a
  chunk reference, a rewrite will eventually come to depend on it.

### Crates

A Cargo workspace, so that the compiler enforces the boundary. Sibling modules in one
crate could not stop layout types leaking into logical code.

```text
arrir-types     DimId, DataType, Scalar, Schema,     shared vocabulary; no dependencies
                Statistics (trait)
arrir           the logical plan (wire vocabulary)   → arrir-types
arrir-logical   logical rewrite rules                → arrir, arrir-types
arrir-layout    the mdim-refs model                  → arrir-types
arrir-physical  physical planning and                → arrir, arrir-logical, arrir-layout
                cost-based optimisation (future)
```

`arrir` and `arrir-layout` never depend on each other. They meet through `Schema` and
`Statistics` in `arrir-types`, and later in `arrir-physical`. If a plan and its data must
travel together, for example for click-to-code, a `Recipe { plan, layouts }` bundle lives
in `arrir-physical`. It pairs the two layers without merging them.

### The interface

From cheapest to richest:

1. **Schema:** dimensions, shape, data type, and which coordinate variables exist. This
   is all the logical layer ever sees: enough for validation and operation effects. An
   mdim-refs array provides one; so does an in-memory array.
2. **Statistics:** the chunk grid, present and absent chunk counts, and referenced bytes.
   mdim-refs' `arrays.json` already carries this per array as its `summary`. These are
   for cost-based rewrites and the "this click costs N requests, M MB" estimate. The
   coarse part is the `Statistics` trait in `arrir-types`; the detail is read directly by
   `arrir-physical`.
3. **Pushdown:** "which chunks intersect this positional selection?" Resolving labels to
   positions sits exactly on the boundary, because it needs coordinate *values*, so this
   part needs the most careful definition.

Only the schema is needed for the first implementation.

### Optimisation

Optimisation is split along the same line, as DataFusion splits `datafusion-optimizer`
from `datafusion-physical-optimizer`. `arrir-logical` holds rewrite rules over the chain,
starting from xrexpr's (see [Logical plan](logical-plan.md#optimisation)).
`arrir-physical` makes the decisions that need the real layout: which chunks to prune,
and which byte ranges to read together.

## Next steps

1. Agree on the shape of these documents, including the questions for Mike in
   [Chunked layout](chunked-layout.md#open-questions).
2. Convert the repository to a Cargo workspace: move the current crate to `arrir`, and
   extract `DimId` and the shared vocabulary into `arrir-types`.
3. In parallel: add `Open` and port xrexpr's rules
   ([Logical plan](logical-plan.md#next-steps)), and build `arrir-layout` as a binding of
   mdim-refs ([Chunked layout](chunked-layout.md#next-steps)).
4. `arrir-physical`: pushdown, cost-based physical optimisation, and the `Recipe`
   bundle.
