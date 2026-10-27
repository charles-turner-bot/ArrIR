# arrir-layout: a Rust binding of byterefs

Status: proposal, with the main questions answered by Mike in #17. Part of
[the extraction contract](design.md).

## Summary

`arrir-layout` is the storage-layout component of the
[dependency graph](design.md#dependency-graph). It is a Rust binding of the
[byterefs](https://github.com/byterefs/spec) specification. byterefs 0.1 starts as the
text of mdim-refs/0.2 ([decision 0002](https://github.com/byterefs/spec/blob/main/decisions/0002-start-from-mdim-refs-0.2.md)),
and every change after that is a numbered byterefs decision record.

What the binding adds is one typed definition and validator for the format. R
(extendr), Python (PyO3), the browser (WASM) and Rust then share a single answer to "is
this a valid store?" It could be the first entry in byterefs'
[IMPLEMENTATIONS.md](https://github.com/byterefs/spec/blob/main/IMPLEMENTATIONS.md).

The binding follows the format and does not govern it. It binds 0.1 as it stands.
Proposed changes, including the [candidates below](#proposed-byterefs-decisions), go to
byterefs first and reach the binding only once they are decided.

`arrir-layout` depends only on `arrir-types`. It never depends on `arrir`, and `arrir`
never depends on it. It gives the logical plan a `Schema` and, later, `Statistics`. The
plan never sees a byte reference.

## What is decided

From Mike's answers in #17:

- **Flat form only.** byterefs makes the published Parquet encoding normative. A producer
  may keep any working encoding (SQLite, Lance, in memory). The SQLite store stays inside
  `raad.virtualize`.
- **Today's three-file flat form.** The planned single self-describing Parquet file is
  byterefs question 7. The binding follows when byterefs decides, so ArrIR doesn't settle
  it by shipping first.
- **0.1 exactly.** No additions in the binding itself.
- **A binding of the spec,** not an adapter over another crate.

## The flat form

The structs mirror the three files that `mr_write_flat` in `raad.virtualize`'s
`R/mdimrefs_pipeline.R` writes. They use the same names, the same optionality, and
types that round-trip with that writer.

### `refs_<array>.parquet`

One file per `referenced` array, with one row per chunk in global chunk coordinates.

| Column | Arrow type | Null when |
|---|---|---|
| `dim_0` … `dim_{n-1}` | `int64` | never |
| `present` | `int8` (0 or 1) | never |
| `path` | `utf8` | the chunk is inline, or known empty |
| `offset` | `int64` | the chunk is inline, or known empty |
| `size` | `int64` | the chunk is inline, or known empty |
| `info` | `utf8` | GDAL reported no block info |
| `data` | `binary` | the chunk is referenced, or known empty |

```rust
/// One row of `refs_<array>.parquet`.
pub struct RefRow {
    /// `dim_0 … dim_{n-1}`: the global chunk coordinate, C order.
    pub index: Vec<u64>,
    /// `present`: false for a chunk known to be empty (it reads as the fill value).
    pub present: bool,
    /// `path`: the source URI, verbatim as harvested. Remapped only by a reader.
    pub path: Option<String>,
    /// `offset`, `size`: the encoded chunk's byte range within `path`.
    pub offset: Option<u64>,
    pub size: Option<u64>,
    /// `info`: GDAL block info, `KEY=VALUE` pairs joined by `; `.
    pub info: Option<String>,
    /// `data`: the encoded chunk bytes, for a chunk held inline.
    pub data: Option<Vec<u8>>,
}
```

A chunk is in one of three states. It is *referenced* (`present = 1`, with `path`,
`offset` and `size`), *inline* (`present = 1`, with `data`), or *known empty*
(`present = 0`). A grid cell with **no row at all** is a chunk that was never scanned.
That makes the table invalid to publish, and the binding rejects it. Treating a missing
row as empty once hid an unscanned source (wxylib/design m15 and m17, section 4).

Integers are `int64` on disk and `u64` in Rust, and reading checks they are
non-negative. `path` repeats the URI on every row and relies on Parquet's dictionary
encoding. For the GHRSST MUR series that column is 3.3 GB, past Arrow's 2 GB limit for
one string array, so the writer must build columns in batches or use `large_utf8`.

### `sources.parquet`

```rust
/// One row of `sources.parquet`: a granule as it was at harvest time.
pub struct SourceRow {
    pub source_id: u64,           // int64
    pub source: String,           // utf8, verbatim
    pub size: Option<u64>,        // int64
    pub mtime: Option<i64>,       // int64, unix seconds
    pub etag: Option<String>,     // utf8
}
```

A reference joins to its source by `path == source`. The layout carries `size`, `mtime`
and `etag`. Checking them needs I/O, so it belongs to whatever reads bytes, and what a
reader does on a mismatch is byterefs question 8.

### `arrays.json`

```rust
/// `arrays.json`.
pub struct ArraysDoc {
    pub format_version: String,                     // "mdim-refs/0.2"
    pub meta: BTreeMap<String, String>,             // provenance: dataset, created, revision, ...
    pub dimensions: BTreeMap<String, DimensionRole>,
    pub arrays: Vec<ArrayEntry>,
    pub remap: Vec<RemapRule>,
}

/// From the placements VRT: `type` such as HORIZONTAL_X or TEMPORAL, and `direction` such as EAST.
pub struct DimensionRole {
    #[serde(rename = "type")]
    pub role: Option<String>,
    pub direction: Option<String>,
}

pub struct ArrayEntry {
    pub array: String,
    pub kind: ArrayKind,                  // "data" | "coord"
    pub storage: StorageKind,             // "referenced" | "inline" | "affine"
    pub dims: Vec<String>,                // C order
    pub shape: Vec<u64>,                  // global
    pub chunks: Vec<u64>,                 // chunk shape; the whole shape for inline arrays
    pub chunk_edges: Option<Vec<Vec<u64>>>, // proposed rectilinear grid; see below
    pub dtype: String,                    // numpy, with byte order: "<i2", "<f4", "|u1"
    pub fill: Option<String>,             // text token: "-32768", "NaN", "Infinity"
    pub codec: Option<CodecV2>,           // None: uncompressed, or not referenced
    pub attrs: BTreeMap<String, serde_json::Value>,  // CF attributes, verbatim
    pub affine: Option<Affine>,           // storage = "affine"
    pub values: Option<Vec<f64>>,         // storage = "inline": decoded values, as JSON numbers
    pub summary: Option<Summary>,         // storage = "referenced"
}

/// Zarr v2 codec configuration, as numcodecs writes it: `{"id": "zlib", "level": 1}`.
pub struct CodecV2 {
    pub compressor: Option<NumcodecsConfig>,
    pub filters: Option<Vec<NumcodecsConfig>>,
}

pub struct NumcodecsConfig {
    pub id: String,
    #[serde(flatten)]
    pub configuration: BTreeMap<String, serde_json::Value>,
}

pub struct Affine { pub start: f64, pub step: f64 }   // value i is start + step * i

/// What a reader can know before reading any chunk.
pub struct Summary {
    pub chunks: u64,     // grid size: product of ceil(shape / chunks)
    pub present: u64,
    pub absent: u64,     // known empty: rows with present = 0
    pub bytes: u64,      // sum of `size` over present rows
}

/// Applied by a reader at render time: the longest matching `prefix` wins.
pub struct RemapRule { pub target: String, pub prefix: String, pub replacement: String }
```

`ArrayEntry` is flat because the JSON is flat. An idiomatic layer on top can turn
`storage` and its optional fields into an enum (`Referenced`, `Inline { values }`,
`Affine { start, step }`). Validation then guarantees that exactly the right fields are
set.

`chunk_edges` is included as an `Option` from the start, so that readers refuse unknown
grids in one place. Whether rectilinear grids are part of 0.1 is byterefs question 5.
Until it is decided, the binding rejects an array that sets it.

## Codecs

The binding stores codecs as byterefs does, as Zarr v2 identifiers in
`{compressor, filters}`. Zarr v3's ordered pipeline is a rendering, needed for
Icechunk. The specification fixes how GDAL block info becomes a codec, and the binding
carries this table as data, so every language derives the same codec from the same
`info`:

| GDAL `info` | Zarr v2 | Zarr v3 |
|---|---|---|
| `COMPRESSION=DEFLATE` | `zlib` | `numcodecs.zlib` |
| `COMPRESSION=ZSTD` | `zstd` | `zstd` |
| `FILTER=SHUFFLE` | `shuffle`, with `elementsize` | `numcodecs.shuffle` |
| `PREDICTOR=2` | `imagecodecs_delta` (axis -1, dist 1) | `HorizontalDeltaCodec`, before `bytes` |
| `ENDIANNESS` | must agree with `dtype` | |

Anything else (`PREDICTOR=3`, for example) stops ingest.

`info` stays a string, kept as provenance. A parsed, typed view of it is a read-side
convenience the binding offers on top. Its column names may still move with the GDAL
`mdim` subcommand.

Rendering v2 as v3 is not a plain reordering. VirtualiZarr sorts `[*filters,
*compressor]` into the three Zarr v3 groups (array-to-array, array-to-bytes,
bytes-to-bytes) by codec class, keeping relative order within each group. When no
array-to-bytes codec is present, it inserts a `bytes` codec with the dtype's byte order.
A v2 `shuffle` filter therefore ends up after `bytes`. (VirtualiZarr `4fc1bc5`,
`codecs.py`, `convert_to_codec_pipeline` and `extract_codecs`. Codec classes are from
zarr-python `e28e7dc`.)

Codec support is where translations between forms break. GDAL's Zarr v3 reader has no
`numcodecs.shuffle`, and nothing outside Python decodes `imagecodecs_delta`. A renderer
checks that its target supports every codec in the pipeline before writing. If it
doesn't, the renderer reports the array as not representable in that form instead of
writing something that decodes wrongly.

## Fill and nodata

Mike's starting position, which this binding follows, keeps two concepts apart:

- `fill` is structural. It is the value of elements in chunks that were never written,
  and it belongs to the layout.
- Missing data, or nodata, is meaning. It stays a CF attribute (`_FillValue`,
  `missing_value`), carried verbatim in `attrs`.

The GDAL multidimensional VRT has only `<NoDataValue>`, so that mapping is the one known
lossy point. A renderer for the VRT form says so when the two differ. The layout does not
force them to agree for every form. This is byterefs question 10, and it isn't settled.

## Validation

All of these can be checked without I/O, and each becomes a `ValidationError` variant.

- `format_version` is one the binding supports (`mdim-refs/0.2`).
- Array names are not reserved (`meta`, `sources`, `arrays`, `placements`, `remap`, or
  the `refs_` prefix).
- `dims`, `shape` and `chunks` have the same rank, at least 1, and every `chunks` entry
  is greater than zero.
- `storage` matches the optional fields. A `referenced` array has a reference table and a
  `summary`. An `inline` array has `values` of length `product(shape)`. An `affine` array
  has `affine`.
- Every grid cell of a `referenced` array has exactly one row. A missing row is an error.
- Every `RefRow.index` has the array's rank and lies inside the chunk grid,
  `ceil(shape[d] / chunks[d])`.
- A referenced row has `path`, `offset` and `size`. An inline row has `data`. A known-empty
  row has none of them.
- Every `path` matches a `source` in `sources.parquet`.
- Present rows agree on `info`, compared as parsed pairs.
- `summary` agrees with the rows, and `present + absent == chunks`.
- `chunk_edges` is unset, until byterefs decides question 5.

## Proposed byterefs decisions

Three gaps came out of `raad.virtualize`'s findings. Each would go to byterefs as a
proposed decision record. Mike has said he'd welcome them.

- **Typed attributes.** `attrs` is untyped JSON, so a float32 `scale_factor` comes back
  as float64, and xarray then decodes packed int16 to float64.
- **An SRS per data array.** Nothing records it, so every rendered form is CRS-less and
  terra guesses lon/lat.
- **Integer-exact coordinates.** Inline `values` are JSON numbers, so 64-bit integer
  coordinates above 2⁵³ cannot round-trip.

A fourth needs a decision record too: one cross-language name for filters outside
numcodecs, such as `imagecodecs_delta` and `virtual-cog-pilot`'s `tiff_predictor`
(byterefs question 1).

## Done when

`arrir-layout` round-trips flat forms that `raad.virtualize` has already published (the
synthetic BRAN and OISST stores are small enough for fixtures). It also rejects every
invalid case listed under [Validation](#validation) with the right error.

## Next steps

1. Create `arrir-layout` once the workspace PR (roadmap step 2) has landed. It needs only
   `arrir-types`.
2. Add Arrow/Parquet reading and writing behind a feature flag, with columns written in
   batches.
3. Provide a `Schema`, and later `Statistics` from `summary`.
4. Add bindings, starting with R through extendr and the browser through WASM.
5. Propose the decisions above to byterefs.
