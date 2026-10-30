# Shells: a staged map for the ArrIR core

Status: proposal, for discussion. Shell 1 is implemented in `crates/arrir/src/legacy/effects.rs`; everything
after it is a light map, not a commitment.

## The core in one sentence

A *demand* (a target grid: a display window, an output tile, or the output of a planned
calculation) pulls back through a *plan* (ArrIR operations) onto a *source* (an array
schema plus a chunk grid whose chunks resolve to bytes). The result of planning is an
ordered list of chunk keys and the byte ranges behind them. No data is read to get there.

Fetching, decoding, assembling and computing happen after planning, in hosts.

## Shells

Each shell adds one capability, depends only on the shells inside it, and keeps every
purity rule below. A shell is done when its tests pass against shared fixtures; later
shells may not change the meaning of earlier ones.

| Shell | Adds | Done when |
| --- | --- | --- |
| 0. Vocabulary | `Plan`, `Select`, `Reduce`, `Rechunk`, wire format, validation | (existing state) |
| 1. Shape and effects | `ArraySchema` (ordered named dims with extents); per-operation effects (preserved, resized, consumed); output schema | a plan can be checked against a schema without data (this PR) |
| 2. Regions and footprints | index-space regions; each operation declares its backward footprint (output region to input region, exact or conservative); chunk grids (regular, rectilinear); region to chunk keys | for a plan and a schema with a chunk grid, the set of needed chunk keys is computed and matches hand-worked cases |
| 3. Chunks and byte refs | a chunk key resolves to a payload: a reference (location, offset, length), inline bytes, a cached copy, or fill. The plan output is an ordered request list | the request list for a fixture matches the byte ranges GDAL actually reads for the same window |
| 4. Demand | target grids; a RasterIO-style index window is the identity case; warps take their coordinate transform as data (a sampled lattice of source coordinates), plus resampling halos and overview level choice | a warp of a fixture to a target tile requests a superset of GDAL's ranges with no extra chunks |
| 5. Coordinates and labels | coordinate models (affine, 1D table, array reference); label selection resolved to positions at plan time; calendars | label-mode `Select` produces known extents and chunk keys |
| 6. Hosts and oracles | lower and emit adapters (Python/xarray, R, JS, GDAL, SQL); cross-language agreement fixtures | the same plan run through three hosts gives identical values |

The existing roadmap fits around these: operation effects is shell 1; the rewrite kernel
needs shell 2 footprints to prove a rewrite preserves the chunks read; contextual
reductions follow shell 2; binary operations and mosaics of several sources follow
shell 4.

## Purity rules

1. Planning reads no data chunks. It uses schemas, chunk grids and byte-ref sets only.
2. Plans, schemas and demands are values: serialisable, versioned, no host objects.
3. Every operation declares its effects (shell 1) and its footprint (shell 2).
4. No silent approximation: an adapter emits an equivalent operation or returns an error.
5. Index space is canonical. Labels and coordinate reference systems are resolved to
   positions during planning.
6. The core performs no I/O and has no async runtime. Hosts fetch and decode.
7. Determinism: the same plan and source give the same chunk list in the same order.
8. Regularity is declared, never inferred. Arithmetic shortcuts are used only when the
   schema says a grid or coordinate is regular.
9. A chunk key always names the same encoded bytes, so any cache of payloads is
   disposable and interchangeable with the references it caches.

## Conventions proposed in shell 1

- Dimensions are ordered. Order is part of the schema and is preserved by every
  operation.
- Positions are 0-based; a negative position counts back from the end; slices are
  half-open with Python semantics. Adapters for hosts with other conventions translate
  (for example, R is 1-based and treats negative indices as exclusions).
- A scalar indexer drops its dimension; slices, position lists and masks never do. A
  host whose indexing drops length-1 results (R's `drop = TRUE`) lowers those to scalar
  indexers.
- A positional mask must match the dimension length exactly; no recycling.
- Label-mode selections have unknown extent until shell 5.

## Byte refs

Byte references exist in several forms (kerchunk JSON and Parquet, Icechunk manifests,
flat tables harvested by GDAL multidim). Shell 3 does not pick one. It states the minimum
it needs from a byte-ref set: for each chunk key, either a location with an offset and a
length, or the encoded bytes inline. Any form that provides that can be adapted.

References and held bytes can share one column. This has been measured with Parquet refs
and Lance blobs, where a value is either held bytes or an external range and one read
returns bytes for both; promoting a reference to held bytes is an ordinary upsert
([wxylib/design m16](https://github.com/wxylib/design/blob/main/docs/m16-versioned-byte-ref-table.md),
sections 7 and 8, with scripts in
[measurements/m16-lance](https://github.com/wxylib/design/tree/main/measurements/m16-lance)).
The same work gives a candidate shell 3 fixture: a year of GHRSST MUR as 935,496 chunk
rows, with GDAL window hashes to check against.

## Open questions

- Is a demand the root of every plan, or is `Warp` an operation like the others?
- May the planner read small coordinate arrays (a metadata class), or must coordinates be
  analytic or pre-resolved into the schema?
- Should coalescing byte ranges into requests be part of the plan or left to hosts?
- How should reductions state missing-value handling (and `ddof` for standard deviation),
  so that emitters cannot silently differ?
- Pointwise (vectorised) selection across several dimensions: a new indexer, or a kind of
  demand?
