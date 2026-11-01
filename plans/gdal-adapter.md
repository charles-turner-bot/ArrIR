# GDAL adapter

Status: decided 1 November 2026, after review (see
[the 1 November discussion](2026-11-01-discussion.md#warp-and-gdal-adapter)), except
question 3. Part of [the extraction contract](design.md), building on
[warp.md](warp.md), [cast.md](cast.md) and [select.md](select.md). Its grid rules come
from [grids.md](grids.md), which is still proposed. Every GDAL behaviour cited here was
checked with GDAL 3.13.3. The earlier reasoning is in
[the 28 October discussion](2026-10-28-discussion.md).

## Summary

The GDAL adapter renders a plan as GDAL VRT text. GDAL then executes it: it plans its
own reads, fetches only the windows a consumer asks for, and warps. `gdalxarray`'s
`warp()` already works this way, returning a warped VRT of about 2 KB with no pixels
read.

It's an ordinary adapter. It depends on `arrir` and `arrir-types` only, does no I/O,
and doesn't link GDAL (purity rule 6). Its output is text (see [Output](#output)).

It's one adapter among several. ArrIR's semantics are defined in the plans, not by GDAL.
Where GDAL can't do exactly what a plan says, the adapter rejects the plan (purity
rule 4).

## Inputs

- **The plan.**
- **A mapping from each binding to a GDAL dataset,** such as `/vsis3/...`,
  `ZARR:"..."` or `NETCDF:...:var`, and whether it is a classic raster or a
  multidimensional dataset. The host supplies it when it calls the adapter, so the
  plan itself never names a location, and the logical schema has no source-kind field.

## The first subset

`Open`, `Project`, positional `Select`, `Warp` and `Cast`:

- **Classic sources** render as a classic VRT for a selection, or a warped VRT for a
  warp.
- **Curvilinear sources** render their `(Centre, Centre)` coordinates as GDAL
  geolocation arrays (`X_DATASET`, `Y_DATASET`), which gdalwarp reads itself. gdalwarp's
  geolocation path uses centres only, so such a warp needs
  `EdgeRule::MidpointsBetweenCentres`; `FromBounds` is rejected there.
- **Multidimensional sources** render as classic VRTs, with each slice along the
  non-grid dimensions as a band. Both VRT kinds use the same warper, and the classic
  path is the well-tested one.
- **A warp with several resampling methods** renders as one warped VRT per method,
  each over the variables that use it, combined into `main` (see [Output](#output)).
- **`Select`** renders as a VRT `SrcRect`, for `Scalar` indexers on non-grid
  dimensions and `Slice` indexers with step +1 only ([select.md](select.md)). A stride
  doesn't render directly: with `SrcRect`/`DstRect` decimation GDAL picks samples
  `floor((i + 0.5) k)`, so `x[0::2]` on `0..7` gave `[1, 3, 5, 7]`, not `[0, 2, 4, 6]`.
  A fractional `SrcRect xOff="-0.5"` did give `[0, 2, 4, 6]` for `k = 2`, which a later
  loosening could use. A reversed slice can't render: a negative `SrcRect` size is
  rejected ("Wrong values in SrcRect"). A scalar on a grid dimension leaves variables
  without a grid, which doesn't fit one classic raster.
- **A `Warp` followed by a `Cast`** of the warped variables renders as one warp, with
  the cast's type and nodata ([cast.md](cast.md#fusing-a-warp-and-a-cast)). A `Cast`
  alone renders as a VRT band's data type. Narrowing to `float32` renders after a
  `Warp` (one gdalwarp call or a warped VRT, with `Infinity`), but a standalone `Cast`
  that narrows to `float32` is rejected: GDAL's ordinary conversion clamps infinities
  too, which no `OutOfRange` variant describes
  ([cast.md](cast.md#what-gdalwarp-can-emit)).

## Missing values GDAL can express

GDAL has one nodata value per band, and gdalwarp matches it approximately: with a float
working type, or a 32- or 64-bit integer one, a value within a relative tolerance of
about `2 × FLT_EPSILON` counts as nodata (`ARE_REAL_EQUAL`, gdalwarper.cpp). With
`-srcnodata -9999`, -9999.001 was masked and -9999.01 wasn't; an `Int32` sentinel
2147483647 masked 2147483000. ArrIR compares sentinels exactly
([schema-and-values.md](schema-and-values.md#missing-values-and-decoding)), so the
adapter accepts only these forms, rendered with `-srcnodata` for every warp:

| `MissingValues` | Rendered as |
| --- | --- |
| `Known { values: [], nan_is_missing: false }` | `-srcnodata None`, which overrides any nodata stored in the band |
| `Known { values: [], nan_is_missing: true }` | `-srcnodata nan` |
| one sentinel, on an integer source of 16 bits or fewer | `-srcnodata v`; the tolerance is below 1 there, so the match is exact |

It rejects several sentinels, a sentinel together with NaN, a sentinel on a float or
wider integer source, and a warp whose variables' forms differ: when one band has
nodata and another doesn't, gdalwarp invents -123456.789 for the other
(gdalwarp_lib.cpp). Without `-srcnodata nan`, NaN isn't masked, and propagates under
bilinear.

## Output

```rust
pub struct VrtBundle {
    /// The dataset the plan produces. Open this one.
    pub main: String,
    /// VRTs that `main` refers to, by relative file name.
    pub parts: Vec<(String, String)>,
}
```

A VRT source can't inline another VRT's XML, only refer to it by file name. So a plan
that needs more than one VRT, such as a warp with several methods, renders as a bundle.
Each part is one warped VRT. `main` is a classic VRT whose bands are `SimpleSource`s
reading the parts' bands, in schema order; all parts share the target grid. The host
writes the parts beside `main`, or into GDAL's `/vsimem/`, before opening it. A plan
that needs one VRT has no parts.

`main` is one classic dataset holding the plan's output variables, and only those:

- **One band per variable and slice,** in schema order. Band metadata records the
  variable's name and, for a multidimensional source, the slice's position along each
  non-grid dimension.
- **Each band's nodata** is its variable's missing value, in one of the forms above.
  Classic VRT sets nodata per band.
- **Variables on different grids** can't share a classic raster, whose bands share one
  grid. That happens, for example, when a warp leaves some variables on the source grid.
  The adapter rejects it.

## How it emits a warp

These settings are always written, so no gdalwarp default decides a result:

- **Geotransform:** the effective affine transform, computed from the grid's declared
  `transform` and its per-axis `IndexMap` (offset, step) as
  [grids.md](grids.md#affine-grids) gives it, for sources and for the target.
- **CRS:** WKT2_2019, requested explicitly. GDAL's default `ExportToWkt` writes WKT1
  ([grids.md](grids.md#crs)).

- **Kernel scale:** `XSCALE` and `YSCALE`, set to 1 for `KernelScaling::Point` or to the
  stated factors for `Widen`. gdalwarp's own estimate changes between processing chunks
  ([mdsumner/osgeo-upstream#50](https://github.com/mdsumner/osgeo-upstream/issues/50)).
- **Error threshold:** `-et 0`, which makes gdalwarp skip its approximate transformer
  (gdalwarp_lib.cpp). gdalwarp measures `-et` in source pixels and checks only each
  segment's midpoint, so no other value renders `CellRules.error_threshold`; a value
  above 0 is rejected.
- **Never `GTIFF_POINT_GEO_IGNORE`.** With it set to `YES`, a PixelIsPoint GeoTIFF's
  transform reads back half a pixel off (100 became 105), breaking the corner anchoring
  grids.md relies on ([grids.md](grids.md#pixel-interpretation)).
- **Geolocation:** `GEOREFERENCING_CONVENTION=PIXEL_CENTER`, so the coordinates are
  read as centres, as the schema places them. GDAL's default is the corner. The
  approximate-only inversion (`GDAL_GEOLOC_USE_MAX_ACCURACY=NO`) is never used. With
  both, gdalwarp's inversion is `EdgeRule::MidpointsBetweenCentres`, as Mike's tests in
  hypertidy/rwarp#2 show.
- **Nodata across bands:** `UNIFIED_SRC_NODATA=NO`. gdalwarp otherwise sets it to `YES`
  whenever `-srcnodata` is given for more than one band, and a pixel is then missing
  only when every band is nodata. With one band holding -9999 and another valid,
  `near` wrote -9999 as data and `bilinear` mixed it in (-2490, -7495.5); with `NO` the
  results were -5555 (empty) and 13, 15. It also makes `Collision::Nudge` work, since
  gdalwarp only nudges per band.
- **Types and nodata:** `-ot`, `-wt`, `-dstnodata` and `-srcnodata` (above), from the
  `Cast` that follows the warp, by the table in
  [cast.md](cast.md#what-gdalwarp-can-emit). Without a `Cast`, the output and working
  types are the method's output type, and `-dstnodata` is `empty_value`. For a float
  source with any method other than `Nearest`, `-wt` is always `Float64`, whatever the
  source and output types. Its `Float32` path adds zero-weight neighbours as
  `0 × inf` = NaN, which breaks [warp.md's weight rule](warp.md#resampling), and
  gdalwarp chooses that path by default for a `float32` source (guarded by
  [fixtures/gdal/float32_narrowing.py](../fixtures/gdal/float32_narrowing.py)).

## What it rejects

- `Reduce`, `Elementwise` and `Constant`. VRT pixel functions exist, but none is used
  until one is shown to match an operation's definition.
- Unknown dtypes or extents.
- A grid whose `SpatialState` is `Unresolved` after a label selection. GDAL can't
  resolve labels, so the host resolves them first.
- `Select` other than the subset above.
- A grid that isn't `Horizontal`.
- A rectilinear source grid, until a fixture shows how gdalwarp's geolocation path
  treats one.
- A warp target that isn't `Affine`, so any `Warp` with a geometry input
  ([dag.md](dag.md) decision 7). gdalwarp can warp onto a curvilinear target
  (`DST_METHOD=GEOLOC_ARRAY`), and rwarp matches it. Whether a warped VRT can store that
  transformer is unchecked (question 3).
- A warp of a variable that isn't at its grid's primary location. `Warp`'s own
  validation already rules this out.
- `Conservative`. GDAL's `average` measures overlap against a box built from two
  corners of each target cell, and overweights edge pixels ([warp.md](warp.md)
  question 2). rwarp and ESMF implement `Conservative`.
- `Mode`. GDAL's `mode` weights each source cell by its overlap with a box built from
  two corners of the target cell (`COMPUTE_WEIGHT` in `GWKModeRealType`). On the row
  `[7, 5, 9, 7]` with a target cell covering x in [0.7, 3.2), warp.md's definition
  gives 7; GDAL gave 5 with `MODE_TIES=MIN` and 9 with `MAX`.
- `Interpolate` with `Lanczos`, until a fixture matches it exactly. An impulse shifted
  by a quarter cell matched warp.md's definition at all but one position near the
  source edge (0.1807 against 0.1820). `Bilinear` and `Cubic` matched exactly (Keys
  a = -0.5: 5.2031 = 6 × w(0.25)).
- `Interpolate` on a source grid with an axis of extent 1. gdalwarp then returns
  shifted source values instead of interpolating: a half-cell shift on a one-row source
  gave `[10, 20, 30, 40, 50]` for bilinear, where two or more rows gave
  `[5, 15, 25, 35, 45]`. Cubic and a one-column source behave the same way.
- `Interpolate` with missing values in a single-band warp. There gdalwarp leaves a
  target cell empty when the source cell containing its centre is missing, even when
  other neighbours are valid; warp.md renormalises over them. With two or more bands
  and `UNIFIED_SRC_NODATA=NO`, gdalwarp renormalises, matching warp.md, for NaN and
  sentinel nodata alike.
- An `edge_shape` other than `StraightInIndex`. gdalwarp's cells are straight in the
  source CRS, or bilinear patches between geolocation samples.
- `Widen` with a factor above 0.5 for `Bilinear` or `Cubic`. gdalwarp's four-sample fast
  path is used there, and it doesn't widen.
- `Widen` until a fixture shows the source window padding is enough. gdalwarp ignores
  `XSCALE` and `YSCALE` when it pads the source window
  ([mdsumner/osgeo-upstream#52](https://github.com/mdsumner/osgeo-upstream/issues/52)),
  so a widened kernel may be cut at chunk edges.
- A `Cast` whose rounding, out-of-range and collision rules aren't a verified row of
  [cast.md's table](cast.md#what-gdalwarp-can-emit). A `Cast` with no warp uses GDAL's
  ordinary conversion, which gives the table's "ordinary conversion" rows. A
  standalone `Cast` that narrows to `float32` is rejected, because that conversion
  clamps infinities. So are every `Wrap`, every `bool` conversion, and every
  standalone float-to-integer `Cast` (its input always has NaN missing): the ordinary
  conversion writes NaN as 0 whatever the nodata value. After a `Warp`, that `Cast`
  renders with `-srcnodata nan -dstnodata v`, which writes `v`
  ([cast.md](cast.md#what-gdalwarp-can-emit)).
- A plan whose output variables don't fit one classic dataset (see [Output](#output)).

## Done when

Two checks, which show different things:

1. **Rendering.** For each fixture, the emitted bundle, opened through `gdalxarray`,
   matches `gdal.Warp` or `gdal.Translate` run directly with the same options, value for
   value. This shows the adapter says to GDAL what the plan says. It can't show that
   GDAL computes what ArrIR means.
2. **Meaning.** For each [reference fixture](warp.md#reference-fixtures) the adapter
   accepts, the emitted bundle's output matches the hand-worked expected values, within
   the fixture's tolerance. Every fixture the adapter rejects is listed, with the
   reason. Where GDAL disagrees with a definition, the adapter rejects that case. The
   definition doesn't change to fit GDAL.

The rendering fixtures are real data, which Mike has offered to supply as plain files
in the repository:
- a COG, from GHRSST or OISST;
- a multidimensional NetCDF, a month of BRAN;
- a curvilinear source, from an ocean model's native grid or a swath, including one
  that crosses the antimeridian, to check gdalwarp's geolocation path against
  `MidpointsBetweenCentres`' longitude rule.

Synthetic cases cover a stride and a reversed slice (both rejected), a warp mixing an
integer and a float variable, and the GDAL behaviours cited above: approximate nodata
matching, `UNIFIED_SRC_NODATA`, `Mode` weighting, extent-1 sources and single-band
`Interpolate` next to missing values.

rwarp follows GDAL's source and cites it, so when GDAL disagrees with a reference
fixture, rwarp is where the cause is traced.

## Answers from Mike

1. **Warped or multidimensional VRT.** Classic warped VRTs, with slices as bands. The
   multidimensional VRT is left for later.
2. **Geolocation inversion.** It is `MidpointsBetweenCentres`, with
   `GEOREFERENCING_CONVENTION=PIXEL_CENTER` (see
   [How it emits a warp](#how-it-emits-a-warp)).

Question for Mike:

3. Can a warped VRT store a destination geolocation transformer, or does
   `DST_METHOD=GEOLOC_ARRAY` only work when warping into an existing dataset? GDAL's
   source serialises a destination sub-transformer in a warped VRT
   (gdaltransformer.cpp), so storing one looks possible, but a quick attempt on
   1 November was inconclusive ("Invalid pixel size for backmap", then a VRT without
   geolocation). One small fixture would settle it, and decide whether curvilinear
   targets fit a `VrtBundle`. Until then they are rejected.

## Decisions

Decided 1 November 2026, taking the review's recommendations:

1. **Nodata forms:** only the three in the table above; `UNIFIED_SRC_NODATA=NO` always.
2. **`-et 0` only.**
3. **`Mode` and `Conservative` are rejected.** `Mode` could later accept target cells
   that are exact unions of source cells, where GDAL's weights don't matter.
4. **`Interpolate`:** `Bilinear` and `Cubic` only; rejected on extent-1 axes and, with
   missing values, in single-band warps. Rendering a second band to reach GDAL's
   renormalising path is a possible later loosening.
5. **`Select`:** `Scalar` on non-grid dimensions and `Slice` with step +1.
6. **Reductions** through overviews are rejected; multidimensional VRT output is later;
   curvilinear targets are rejected until question 3 is settled.
