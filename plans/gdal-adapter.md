# GDAL adapter

Status: proposal. Part of [the extraction contract](design.md), building on
[grids.md](grids.md) and [warp.md](warp.md). The reasoning is in
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
- **A mapping from each binding to a GDAL dataset name,** such as `/vsis3/...`,
  `ZARR:"..."` or `NETCDF:...:var`. The host supplies it when it calls the adapter, so
  the plan itself never names a location.

The binding's declared schema records whether the source is a classic raster or a
multidimensional dataset.

## The first subset

`Open`, positional `Select` (with its effect on grids, in
[grids.md](grids.md#select-on-a-grid)), `Warp` and `Cast`:

- **Classic sources** render as a classic VRT for a selection, or a warped VRT for a
  warp.
- **Curvilinear sources** render their `(Centre, Centre)` coordinates as GDAL
  geolocation arrays (`X_DATASET`, `Y_DATASET`), which gdalwarp reads itself.
- **Multidimensional sources** render as classic VRTs, with each slice along the
  non-grid dimensions as a band. Both VRT kinds use the same warper, and the classic
  path is the well-tested one.
- **A warp with several resampling methods** renders as one warped VRT per method,
  each over the variables that use it, combined as below.
- **A `Warp` followed by a `Cast`** of the same variable renders as one warp, with the
  cast's type and nodata. A `Cast` alone renders as a VRT band's data type.

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
The host writes the parts beside `main`, or into GDAL's `/vsimem/`, before opening it.
A plan that needs one VRT has no parts.

`main` is one classic dataset holding the plan's output variables, and only those:

- **One band per variable and slice,** in schema order. Band metadata records the
  variable's name and, for a multidimensional source, the slice's position along each
  non-grid dimension.
- **Each band's nodata** is its variable's declared missing value. Classic VRT sets
  nodata per band, so variables with different missing values are fine.
- **Variables on different grids** can't share a classic raster, whose bands share one
  grid. That happens, for example, when a warp leaves some variables on the source grid.
  The adapter rejects it.

## How it emits a warp

These settings are always written, so no gdalwarp default decides a result:

- **Kernel scale:** `XSCALE` and `YSCALE`, set to 1 for `KernelScaling::Point` or to the
  stated factors for `Widen`. gdalwarp's own estimate changes between processing chunks
  (osgeo-upstream#50).
- **Error threshold:** `-et`, from `GeometryRules.error_threshold`.
- **Geolocation:** `GEOREFERENCING_CONVENTION=PIXEL_CENTER`, so the coordinates are
  read as centres, as the schema places them. GDAL's default is the corner. The
  approximate-only inversion (`GDAL_GEOLOC_USE_MAX_ACCURACY=NO`) is never used. With
  both, gdalwarp's inversion is `EdgeRule::MidpointsBetweenCentres`, as Mike's tests in
  hypertidy/rwarp#2 show.
- **Mode ties:** `MODE_TIES=MIN` or `MAX`, which needs GDAL 3.11 or later.
- **Types and nodata:** `-ot`, `-wt` and `-dstnodata`, from the `Cast` that follows the
  warp, by the table in [cast.md](cast.md#what-gdalwarp-can-emit). Without a `Cast`, the
  output and working types are the method's output type, and `-dstnodata` is
  `empty_value`. A `Collision::Nudge` also sets `UNIFIED_SRC_NODATA=NO`, since gdalwarp
  only nudges per band.

## What it rejects

- Reductions, and every other operation with no VRT equivalent.
- Unresolved label selections. GDAL can't resolve them, so the host resolves them first.
- A grid that isn't `Horizontal`.
- A warp target that isn't `Affine`. gdalwarp can warp onto a curvilinear target
  (`DST_METHOD=GEOLOC_ARRAY`), and rwarp matches it. Whether a warped VRT can store that
  transformer is unchecked (question 3).
- A warp of a variable that isn't at its grid's primary location. `Warp`'s own
  validation already rules this out.
- `Conservative`. GDAL's `average` measures overlap against a box built from two
  corners of each target cell, and overweights edge pixels ([warp.md](warp.md)
  question 2). rwarp and ESMF implement `Conservative`.
- An `edge_shape` other than `StraightInIndex`. gdalwarp's cells are straight in the
  source CRS, or bilinear patches between geolocation samples.
- `Mode` with a working type wider than 16 bits. In that case gdalwarp ignores
  `MODE_TIES` for a value's first occurrence (osgeo-upstream#51).
- `Mode` on a source that spans the antimeridian. gdalwarp's wrap check skips each
  chunk's last column (osgeo-upstream#53).
- `Widen` with a factor above 0.5 for `Bilinear` or `Cubic`. gdalwarp's four-sample fast
  path is used there, and it doesn't widen.
- `Widen` until a fixture shows the source window padding is enough. gdalwarp ignores
  `XSCALE` and `YSCALE` when it pads the source window (osgeo-upstream#52), so a widened
  kernel may be cut at chunk edges.
- A `Cast` whose rounding and collision rule aren't a row of
  [cast.md's table](cast.md#what-gdalwarp-can-emit). A `Cast` with no warp uses GDAL's
  ordinary conversion, so only `NearestTiesAway` with `Collision::Keep`.
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
- a curvilinear source, from an ocean model's native grid or a swath.

Synthetic cases cover a reversed slice and a warp mixing an integer and a float
variable.

rwarp follows GDAL's source and cites it, so when GDAL disagrees with a reference
fixture, rwarp is where the cause is traced.

## Questions for Mike

Questions 1 and 2 are answered:

1. **Warped or multidimensional VRT.** Classic warped VRTs, with slices as bands. The
   multidimensional VRT is left for later.
2. **Geolocation inversion.** It is `MidpointsBetweenCentres`, with
   `GEOREFERENCING_CONVENTION=PIXEL_CENTER` (see
   [How it emits a warp](#how-it-emits-a-warp)).

Question for Mike:

3. Can a warped VRT store a destination geolocation transformer, or does
   `DST_METHOD=GEOLOC_ARRAY` only work when warping into an existing dataset? One small
   fixture would settle it, and decide whether curvilinear targets fit a `VrtBundle`.

## Not settled

- Whether some reductions, such as averaging overviews, have exact VRT equivalents.
- Multidimensional VRT output, and rendering through GDAL's multidimensional API
  (`GetView`, `GetResampled`) instead of VRT text.
- Curvilinear warp targets (question 3).
