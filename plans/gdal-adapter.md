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
and doesn't link GDAL (purity rule 6). Its output is a string.

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
[grids.md](grids.md#select-on-a-grid)) and `Warp`:

- **Classic sources** render as a classic VRT for a selection, or a warped VRT for a
  warp.
- **Curvilinear sources** render their `(Centre, Centre)` coordinates as GDAL
  geolocation arrays (`X_DATASET`, `Y_DATASET`), which gdalwarp reads itself.
- **Multidimensional sources** render as a multidimensional VRT (question 1).
- **A warp with several resampling methods** renders as one warp per method, each over
  the variables that use it.

## What it rejects

- Reductions, and every other operation with no VRT equivalent.
- Unresolved label selections. GDAL can't resolve them, so the host resolves them first.
- A grid that isn't `Horizontal`.
- A warp target that isn't `Affine`. gdalwarp writes regular grids. A curvilinear
  *source* is fine.
- A warp of a variable that isn't at its grid's primary location. `Warp`'s own
  validation already rules this out.
- `Interpolate` with `KernelScaling::Point`, unless gdalwarp can turn off kernel
  widening ([warp.md](warp.md) question 3).
- `Conservative` with `AreaMeasure::Spherical`. Also `Planar`, unless GDAL's `average`
  matches it ([warp.md](warp.md) question 2).
- `EdgeRule::MidpointsBetweenCentres`, unless gdalwarp's geolocation inversion matches
  it (question 2).
- A `Rounding` or `OutOfRange` variant that gdalwarp doesn't implement exactly
  ([warp.md](warp.md) question 4).
- A `Mode` tie rule that differs from GDAL's ([warp.md](warp.md) question 1).

## Done when

For each fixture, the emitted VRT, opened through `gdalxarray`, matches `gdal.Warp` or
`gdal.Translate` run directly on the same source, value for value.

The fixtures include:
- a COG;
- a multidimensional NetCDF;
- a curvilinear swath;
- a reversed slice;
- a warp mixing an integer and a float variable.

## Questions for Mike

1. Should the adapter emit a warped VRT, a multidimensional VRT, or both? Which does
   GDAL handle more faithfully for multidimensional sources?
2. Does gdalwarp's geolocation-array inversion place cell edges halfway between
   centres, as `EdgeRule::MidpointsBetweenCentres` defines them?

## Not settled

- Whether some reductions, such as averaging overviews, have exact VRT equivalents.
- Rendering through GDAL's multidimensional API (`GetView`, `GetResampled`) instead of
  VRT text.
