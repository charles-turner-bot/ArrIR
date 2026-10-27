# Logical plan: a chain of portable array operations

| | |
|---|---|
| **Status** | Draft, open for review |
| **Authors** | Charles Turner, Michael Sumner |
| **Date** | 2026-10-27 |
| **Builds on** | xrexpr (Charles Turner) |
| **Part of** | [ArrIR design overview](README.md) |

## Summary

ArrIR describes *what to do* to an array (`Select`, `Reduce`, `Rechunk`) but has no way to
say *which* array. This document adds an `Open` operation that starts every plan, so a
plan reads as one chain of operations, each acting on the result of the one before:

```text
Open ──▶ Select ──▶ Reduce
```

The plan names a dataset and never embeds its storage layout. Adapters render the chain
to xarray, R or JS. How the data is stored is the [chunked layout](chunked-layout.md)'s
concern, and how the chain is executed belongs to a future physical layer.

## Scope

In scope:

- `Operation::Open` and the rule that a plan is one chain starting with it.
- Where rewrite rules live, and which ones come first.

Not in scope:

- Physical execution: fetching, chunk pruning, decoding, caching.
- Defining new rewrite rules.
- Frontends, backends and language bindings.

## The chain

A plan is an ordered `Vec<Operation>`. The first operation opens a dataset, and each later
operation acts on the result of the one before it.

```rust
pub struct Plan {
    pub version: u16,
    pub operations: Vec<Operation>,
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum Operation {
    /// Open a chunked dataset. Always the first operation, and only that one.
    Open { uri: String },
    Select { /* unchanged */ },
    Reduce { /* unchanged */ },
    Rechunk { /* unchanged */ },
}
```

`Open { uri }` is deliberately minimal. The URI can name anything that can be opened as
chunked data: a Zarr store, a netCDF/HDF5 file, kerchunk references, an mdim-refs flat
form, or a GDAL multidimensional VRT. The logical plan does not care which. All it needs
from the opened dataset is a `Schema` (see the [overview](README.md#the-interface)).

`Plan::new(operations)` keeps its signature. The crate is unreleased, so `WIRE_VERSION`
stays at 1.

## Demo

The monthly mean temperature over a latitude band, as a user writes it in xarray:

```python
xr.open_zarr("s3://bucket/ocean.zarr").temp \
  .sel(time=slice("2020-01-01", "2020-01-31"), lat=slice(-40.0, -30.0)) \
  .mean("time")
```

As an ArrIR chain:

```text
[0] Open   { uri: "s3://bucket/ocean.zarr" }
[1] Select { mode: label, time: "2020-01-01" ..= "2020-01-31", lat: -40.0 ..= -30.0 }
[2] Reduce { function: mean, dimensions: [time], keep_dimensions: false }
```

The same chain with positional selection, which the current `Indexer` can express (label
slice bounds are open question 6):

```json
{
  "version": 1,
  "operations": [
    { "type": "open", "uri": "s3://bucket/ocean.zarr" },
    { "type": "select", "mode": "position",
      "indexers": {
        "time": { "kind": "slice", "start": 0, "stop": 31, "step": null },
        "lat": { "kind": "slice", "start": 200, "stop": 241, "step": null } } },
    { "type": "reduce", "function": "mean",
      "dimensions": { "kind": "named", "dimensions": ["time"] }, "keep_dimensions": false }
  ]
}
```

Nothing in the chain says how many chunks are read, in what order, or how the selection
is pushed down to the bytes. That is all left to the physical layer.

## Invariants

- The plan is non-empty, `operations[0]` is `Open`, and no other operation is `Open`.
- `Open { uri }` has a non-empty `uri`.
- The existing per-operation checks are unchanged.

Checking that the dimensions an operation names exist at that point in the chain needs
per-operation effects (README roadmap step 2), checked against the opened dataset's
`Schema`.

## Optimisation

Logical rewrites live in `arrir-logical`. It is a separate crate from `arrir` because
`arrir` is the stable wire vocabulary that every adapter depends on, while rules will
change often; an adapter that only renders plans should not have to compile the
optimiser. Its crate docs should say plainly that the plan types live in `arrir`.

A rule may consult a `Schema`, for example to tell whether a `Select` touches a reduced
dimension. It never sees byte references. A rule that wants a cost estimate (for example,
only reordering when the result gets smaller) uses the `Statistics` trait from
`arrir-types`, which `arrir-layout` implements. Decisions that need the real layout,
such as which chunks to prune, belong to `arrir-physical`.

### Rewrites

Because a plan is a flat chain, rewrite rules can be written as local patterns over
adjacent operations. The starting set is the rules already in xrexpr's optimiser, ported
to the portable `Operation` types. xrexpr (`src/xrexpr/optimize.py`, `_RULES`, at
`76fc580`) has eight. Four act only on operations ArrIR already has, so they port
directly:

| xrexpr rule | What it does |
|---|---|
| `merge_adjacent_selects` | Folds a run of consecutive selects into one, composing indexers where dimensions collide. |
| `merge_adjacent_rechunks` | Collapses consecutive rechunks. |
| `pushdown_selects` | Moves a select left past a preceding operation whose dimensions permit it, such as a `Reduce` over other dimensions. |
| `pushdown_selects_past_rechunks` | Moves a select left past a rechunk, so the rechunk moves less data. |

The other four (`merge_adjacent_projects`, `eliminate_projection_before_coord`,
`push_projection_past_drop`, `pushdown_projections`) act on variable projections and
`drop_vars`. They only port once the chain can run over a dataset of several variables
(open question 3).

Two design properties of xrexpr's optimiser carry over with the rules:

- **Termination.** Rules run to a fixpoint and terminate by a lexicographic measure, so no
  rule may lengthen a plan or move an operation rightwards.
- **One dispatch site for dimension effects.** xrexpr's `dim_effect` is a single exhaustive
  `match`. In Rust this becomes the operation-effects function, and the compiler's
  exhaustiveness check does the work of `assert_never`.

## Open questions

1. **Labels versus positions.** Label selection stays logical, and resolving it to
   positions needs coordinate values, which come from the layout. Is there any case where
   a logical plan should carry already-resolved positions?
2. **CF decoding.** Are scale/offset, masking and time decoding logical operations in the
   chain, or properties of the opened dataset? mdim-refs keeps the CF attributes in the
   layout, which suggests the latter.
3. **Datasets and variables.** An opened dataset usually holds several arrays (data
   variables and their coordinates). Does `Open` name one variable (`Open { uri,
   variable }`), or does the chain run over the whole dataset, as ndr's `Dataset` and
   xarray's do? This decides when xrexpr's projection rules can port.
4. **Cross-language semantics.** These must be stated explicitly rather than inherited from
   a host: NaN skipping in reductions, `ddof` for standard deviation, the output data type
   of a reduction (a `float32` mean is `float32` in NumPy but `double` in R), 0- versus
   1-based indexing, dimension order (terra and gdalraster reverse it), and inclusive
   label-slice ends. The altarr `ArraySubset` hook should agree with positional `Select` on
   indexing base and dimension order.
5. **Element-wise expressions.** For a future `Map(expr)` step: extend Substrait, or define
   our own?
6. **Label slice bounds.** `Indexer::Slice` has `i64` bounds, so it cannot express a label
   slice over float latitudes or timestamps (`sel(lat=slice(-40.0, -30.0))`). Label slices
   need `Scalar`-typed bounds, plus an explicit statement that label-slice ends are
   inclusive.

## Next steps

1. Add `Operation::Open` and the chain invariants to `arrir`.
2. Operation effects (README roadmap step 2), checked against a `Schema`.
3. `arrir-logical`: port xrexpr's four select and rechunk rules onto the portable chain,
   keeping the fixpoint and its termination measure. Add the `Statistics` trait to
   `arrir-types` when the first cost-aware rule needs it.
