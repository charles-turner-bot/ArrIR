# Chunked layout: a Rust binding of mdim-refs

| | |
|---|---|
| **Status** | Draft, open for review |
| **Authors** | Charles Turner, Michael Sumner |
| **Date** | 2026-10-27 |
| **Builds on** | mdim-refs/0.2 and `raad.virtualize` (Michael Sumner) |
| **Part of** | [ArrIR design overview](README.md) |

## Summary

**What this offers mdim-refs:** one typed definition and validator for the format, shared
by R, Python, JS and Rust, and a fixed schema for its flat Parquet form.

ArrIR needs a storage layout: a generic description of chunked arrays and the byte
ranges that hold them. mdim-refs already is one, built and verified on 22.8M references.
So this document proposes that ArrIR's storage layout **is** mdim-refs, with
`arrir-layout` as its Rust binding. In practice that gives:

- **One validator,** compiled once and used from R (extendr), Python (PyO3), the browser
  (WASM) and Rust, so every producer and reader agrees on what a valid store is.
- **A fixed, typed schema for the flat form,** which the specification currently leaves
  open.
- **A `Schema` for the [logical plan](logical-plan.md)** to bind to, without the plan
  ever seeing a byte reference.

It replaces none of the SQLite store, harvest engines or renderers in `raad.virtualize`.

**Three decisions we need from Mike** before writing code (details in
[Open questions](#open-questions)):

1. Should the binding model only the flat form, or the whole store?
2. Should it target today's three-file flat form, or the planned single Parquet file?
3. Should it mirror 0.2 exactly, or carry proposed 0.3 additions (typed attributes, an
   SRS, integer-exact coordinates)?

### Sources

- The specification: [`wxylib/design`](https://github.com/wxylib/design)
  `specs/mdim-refs-0.2.md`, draft of 2026-10-03.
- `raad.virtualize` as of 2026-10-27: `README.md`, `docs/index.md`, `docs/results.md`
  (cited below as "finding N"), `R/mdimrefs.R`, and `mr_write_flat` in
  `R/mdimrefs_pipeline.R`.

Where this document and the specification disagree, the specification wins. The flat
form's layout is not yet fixed by the specification (its open issue 5), so for that form
this document follows what `mr_write_flat` writes today.

## mdim-refs in brief

The [specification](https://github.com/wxylib/design) is the full reference. In short, a
store has three layers: per-source chunk tables and `sources` (physical), `arrays` and
`placements` from a `gdal mdim mosaic` VRT (placements), and a `refs_<array>` view in
global chunk coordinates. Every published form is a projection of the store. Source URIs
are recorded verbatim and remapped only at render time.

mdim-refs calls its placement layer "the plan". These documents call it *placements*,
because ArrIR uses "plan" for its logical chain (see the [glossary](README.md#glossary)).

The original ArrIR sketch of a chunk reference was:

```rust
struct VirtualArray {
    block: Vec<usize>,
    resource: PathBuf,
    byte_range: (usize, usize),
    bytes: Option<Vec<number>>,
}
```

That is, field for field, a row of the `refs_<array>` view: `block` is `dim_0 …
dim_{n-1}`, `resource` is `path`, `byte_range` is `offset` + `size`, and `bytes` is
`data`. The two designs arrived at the same row independently. Its idea of holding a
reference until the bytes are needed, then the bytes too, matches the Lance render on
`raad.virtualize`'s list of ongoing work.

## The binding

`arrir-layout` models the **flat form**: language-neutral, already published, and read
by `wxy-refs` and any Parquet reader. The structs mirror the three files `mr_write_flat`
writes, with the same names, the same optionality, and types that round-trip with the R
writer.

### `refs_<array>.parquet`: the reference table

One file per `referenced` array, one row per chunk, in global chunk coordinates:

| Column | Arrow type | Null when |
|---|---|---|
| `dim_0` … `dim_{n-1}` | `int64` | never |
| `present` | `int8` (0 or 1) | never |
| `path` | `utf8` | the chunk is inline, or absent |
| `offset` | `int64` | the chunk is inline, or absent |
| `size` | `int64` | the chunk is inline, or absent |
| `info` | `utf8` | GDAL reported no block info |
| `data` | `binary` | the chunk is referenced, or absent |

```rust
/// One row of `refs_<array>.parquet`.
pub struct RefRow {
    /// `dim_0 … dim_{n-1}`: the global chunk coordinate, C order.
    pub index: Vec<u64>,
    /// `present`: false for a chunk that was never written (reads as the fill value).
    pub present: bool,
    /// `path`: the source URI, verbatim as harvested. Remapped only by a reader.
    pub path: Option<String>,
    /// `offset`, `size`: the encoded chunk's byte range within `path`.
    pub offset: Option<u64>,
    pub size: Option<u64>,
    /// `info`: GDAL block info, `KEY=VALUE` pairs separated by `;`.
    pub info: Option<String>,
    /// `data`: the encoded chunk bytes, for a chunk held inline.
    pub data: Option<Vec<u8>>,
}
```

Details that are easy to get wrong:

- **Integers are `int64` on disk and `u64` in Rust.** Reading checks they are
  non-negative.
- **`path` repeats the URI on every row**, relying on Parquet's dictionary encoding. For
  the GHRSST series that column is 3.3 GB, past Arrow's 2 GB limit for one string array,
  so the R writer builds columns in batches of a million rows (finding 11). A Rust writer
  must do the same, or use `large_utf8`.
- **Absent chunks come two ways:** a `present = 0` row, or no row at all. Readers must
  treat them the same; `arrays.json`'s `summary` counts both.

### `sources.parquet`: the granules

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

A reference joins to its source by `path == source`. `size`, `mtime` and `etag` are the
validators a reader needs to notice that a granule changed after harvest, which
otherwise shows up as decode errors (finding 16).

### `arrays.json`: everything else

```rust
/// `arrays.json`.
pub struct ArraysDoc {
    pub format_version: String,                     // "mdim-refs/0.2"
    pub meta: BTreeMap<String, String>,             // provenance: dataset, created, revision, ...
    pub dimensions: BTreeMap<String, DimensionRole>,
    pub arrays: Vec<ArrayEntry>,
    pub remap: Vec<RemapRule>,
}

/// From the placements VRT: `type` such as HORIZONTAL_X or TEMPORAL; `direction` such as EAST.
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
    pub dtype: String,                    // numpy, with byte order: "<i2", "<f4", "|u1"
    pub fill: Option<String>,             // text token: "-32768", "NaN", "Infinity"
    pub codec: Option<CodecV2>,           // None: uncompressed, or not referenced
    pub attrs: BTreeMap<String, serde_json::Value>,  // CF attributes
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
    pub absent: u64,     // chunks - present
    pub bytes: u64,      // sum of `size` over present rows
}

/// Applied by a reader at render time: the longest matching `prefix` wins.
pub struct RemapRule { pub target: String, pub prefix: String, pub replacement: String }
```

`ArrayEntry` is flat because the JSON is flat. An idiomatic layer on top can turn
`storage` and its optional fields into an enum (`Referenced`, `Inline { values }`,
`Affine { start, step }`), with validation guaranteeing that exactly the right fields
are set.

### Gaps in 0.2: candidates for 0.3

Finding 14: when readers had to guess, every guess was about meaning, not layout. 0.2
already records dimension roles and NaN-safe fill tokens. Three gaps remain, and the
binding is a natural place to close them, if Mike agrees:

- **Typed attributes.** `attrs` is untyped JSON, so a float32 `scale_factor` comes back
  as float64, and xarray then decodes packed int16 to float64 (finding 7).
- **Spatial reference.** Nothing records the SRS, so every rendered form is CRS-less and
  terra guesses lon/lat (finding 13).
- **Integer-exact coordinates.** Inline `values` are JSON numbers, so 64-bit integer
  coordinates above 2⁵³ cannot round-trip.

## Codecs

The binding keeps mdim-refs' stored form: Zarr v2 identifiers, `{compressor, filters}`.
Zarr v3's ordered pipeline is a rendering, needed for Icechunk. The specification fixes
how GDAL block info becomes a codec, and the binding should carry this table as data so
every language derives the same codec from the same `info`:

| GDAL `info` | Zarr v2 | Zarr v3 |
|---|---|---|
| `COMPRESSION=DEFLATE` | `zlib` | `numcodecs.zlib` |
| `COMPRESSION=ZSTD` | `zstd` | `zstd` |
| `FILTER=SHUFFLE` | `shuffle`, with `elementsize` | `numcodecs.shuffle` |
| `PREDICTOR=2` | `imagecodecs_delta` (axis -1, dist 1) | `HorizontalDeltaCodec`, before `bytes` |
| `ENDIANNESS` | must agree with `dtype` | |

Anything else (`PREDICTOR=3`, for example) stops ingest.

Rendering v2 as v3 is not a plain reordering. VirtualiZarr sorts `[*filters,
*compressor]` into the three Zarr v3 groups (array-to-array, array-to-bytes,
bytes-to-bytes) by codec class, keeping relative order within each group, and inserts a
`bytes` codec with the dtype's byte order when none is present. So a v2 `shuffle` filter
ends up after `bytes`. (VirtualiZarr `4fc1bc5`, `codecs.py` `convert_to_codec_pipeline`
and `extract_codecs`; codec classes from zarr-python `e28e7dc`.)

Codec support is where translations between forms actually break. GDAL's Zarr v3 reader
has no `numcodecs.shuffle` (finding 7), and nothing outside Python decodes
`imagecodecs_delta` (finding 10). A renderer therefore checks, before writing, that its
target supports every codec in the pipeline. If it doesn't, it reports the array as not
representable in that form, as `virtual-cog-pilot` does for LZW and JPEG tiles, rather
than writing a form that decodes wrongly.

## Validation

All of these can be checked without I/O, and each becomes a `ValidationError` variant.
Most come directly from the specification and `R/mdimrefs.R`:

- `format_version` is one the binding supports (`mdim-refs/0.2`).
- Array names are not reserved (`meta`, `sources`, `arrays`, `placements`, `remap`, or
  the `refs_` prefix).
- `dims`, `shape` and `chunks` have the same rank, at least 1, and every `chunks` entry
  is greater than zero.
- `storage` matches the optional fields: `referenced` has a reference table and a
  `summary`; `inline` has `values` of length `product(shape)`; `affine` has `affine`.
- Every `RefRow.index` has the array's rank and lies inside the chunk grid,
  `ceil(shape[d] / chunks[d])`. No two rows share an index.
- A present row has either `path`, `offset` and `size`, or `data`. An absent row has
  neither.
- Every `path` matches a `source` in `sources.parquet`.
- Present rows agree on `info`, compared as parsed pairs.
- `fill` agrees with `_FillValue` and `missing_value` in `attrs` where those are set.
- `summary` agrees with the rows.

Placement alignment is checked where placements are resolved into the view, before the
flat form exists.

## Mapping to other forms

| mdim-refs (flat form) | kerchunk Parquet | Icechunk | GDAL mdim VRT |
|---|---|---|---|
| `dims` | `_ARRAY_DIMENSIONS` | Zarr v3 `dimension_names` | `<DimensionRef>` |
| `shape`, `chunks`, `dtype` | `.zarray` `shape`, `chunks`, `dtype` | Zarr v3 array metadata | `<Dimension size>`, `<BlockSize>`, `<DataType>` |
| `codec` | `.zarray` `compressor`, `filters` | `codecs` (v3) | none (the source driver decodes) |
| `fill` | `.zarray` `fill_value` | `fill_value` | `<NoDataValue>` (open question 6) |
| `path`, `offset`, `size` | `path`, `offset`, `size` | virtual reference | `<Source>`, from `placements` |
| `data` | `raw` | inline chunk | none |
| `inline` / `affine` arrays | whole-array inline chunk | inline chunks | `<InlineValues>`, `<RegularlySpacedValues>` |

All of `raad.virtualize`'s published forms are *re-addressings* of the store: only the
serialisation or the URIs change, never the bytes. The same table also supports
*repacking* (copying each chunk's bytes into native Zarr objects, or into one blob plus a
new table) without decoding, provided the target supports every codec and keeps the
chunk grid. Anything else, such as rechunking or recompressing, is a transcode and needs
decoding.

## Open questions

For Mike:

1. **Store or flat form?** Should `arrir-layout` model only the flat form, as proposed,
   or all three layers of the store? The flat form is enough for readers; the full model
   would let Rust do collation, too.
2. **Which flat layout?** The specification calls the three-file form provisional, and
   names one self-describing Parquet file as the likely end: `meta`, `arrays` and `remap`
   in key-value metadata, rows sorted by dims, row groups along `dim_0`, and `path`
   dictionary-encoded. Should the binding target that file from the start, and could
   fixing its schema here be what settles it?
3. **0.2 or 0.3?** Mirror 0.2 exactly, or adopt the [0.3 candidates](#gaps-in-02-candidates-for-03)
   in the R writer and the binding together?
4. **Codec names.** Keep v2 identifiers as the stored form? How should filters outside
   numcodecs, such as `imagecodecs_delta` and `virtual-cog-pilot`'s `tiff_predictor`, be
   named so one name works in Python, JS and R?
5. **`info`.** Keep GDAL's `KEY=VALUE` string opaque, or parse it into typed fields? The
   specification also accepts the JSON list from osgeo.gdal's `GetInfo()`, so two
   spellings already exist.
6. **The VRT and fill values.** GDAL nodata ("this value means missing") and Zarr fill
   value ("value of unwritten elements") are different concepts. mdim-refs forces them
   to agree at ingest. Is that the rule for every form?
7. **Merge or separate?** The [overview](README.md#proposed-structure) keeps this layout
   and the logical plan in sibling crates that meet only through `Schema`. Does that
   match how you see it? Should `arrir-layout` be mdim-refs' reference implementation in
   Rust, or a thin adapter over an existing crate (byterefs, `wxy-refs`, blocklist)?
8. **Irregular grids.** Should the binding include the proposed `chunk_edges` column from
   the start, as an `Option`, so that readers refuse unknown grids in one place?
9. **Validators at read time.** Is checking a source's `size` or `etag` before trusting
   its references part of the layout, or of the physical layer that reads bytes?

## Next steps

1. Agree on open questions 1 to 3 with Mike, since they decide what the structs mirror.
2. Create `arrir-layout` with these types and their validation, with round-trip tests
   against flat forms `raad.virtualize` has already published (the synthetic BRAN and
   OISST stores are small enough for fixtures).
3. Arrow/Parquet reading and writing of the flat form, behind a feature flag, with
   columns written in batches so the 2 GB string limit cannot recur.
4. Provide a `Schema` (and later `Statistics`, from `summary`) for the logical layer.
5. Bindings: R through extendr and the browser through WASM first, since those are
   where a shared validator saves the most duplicated checking.
