# Warp

Status: proposal. Part of [the extraction contract](design.md), building on
[grids.md](grids.md). The reasoning is in [the 28 October discussion](2026-10-28-discussion.md).

## Summary

`Warp` is an operation in the `arrir` crate, alongside `Select` and `Reduce`. It covers
reprojection and regridding between any two grids. Its struct lands after `Select` and
`Reduce` (roadmap step 6), but its shape is fixed here, so that adding it changes
neither the schema nor `Select`.

The first code supports `Horizontal` to `Horizontal` only. Meshes and DGGS, sketched in
[grids.md](grids.md#limits-and-extension-plan), fit the same shape.

Every result-affecting option is a field (purity rule 4). Resampling methods are
defined here, on their own terms. No adapter's behaviour defines them, GDAL's included.

## The operation

```rust
pub struct Warp {
    /// Variables are warped from this grid. Each must be placed on it.
    pub source_grid: GridId,
    pub target: WarpTarget,
    /// The variables to warp, each with its own output. Variables on `source_grid`
    /// that aren't listed stay where they are.
    pub variables: Vec<WarpVariable>,
    /// How to find cell edges on a grid that has no bounds.
    pub edges: EdgeRule,
    /// Largest permitted error, in target cells, of an approximate coordinate
    /// transformer. 0 means exact. gdalwarp defaults to 0.125, so it's never implicit.
    pub error_threshold: f64,
}

pub struct WarpVariable {
    pub var: VarId,
    pub resampling: Resampling,
    pub output_dtype: DType,
    /// Required when the method can produce a value the output type can't hold.
    pub rounding: Option<Rounding>,
    pub out_of_range: Option<OutOfRange>,
    /// Written to target cells with no valid source data. Always required: see
    /// "Missing values".
    pub target_nodata: Scalar,
}

pub struct WarpTarget {
    /// The output grid. Its ID, dimensions and coordinate names are new to the schema.
    pub grid: Grid,
    /// The size of each of the target grid's dimensions.
    pub sizes: Vec<(DimId, u64)>,
    /// Where the target's coordinate variables come from, when its model refers to
    /// any: a rectilinear or curvilinear grid, a mesh, or a DGGS cell list. An
    /// affine target needs none.
    pub geometry: Option<GeometryBinding>,
}

/// Declared like `Open`: a local name and the schema it must have.
pub struct GeometryBinding {
    pub binding: BindingId,
    pub schema: Schema,
}
```

### Why each variable has its own output

One data type, nodata value and method for the whole warp can't serve every variable
on a grid. Bilinear or conservative resampling of `int16` data produces fractions. Two
variables can have different types and nodata values. A land-cover class needs `Mode`
or `Nearest`, while a temperature on the same grid needs `Interpolate` or
`Conservative`.

GDAL applies one method to all bands. That is a limit of one adapter, not of the IR.
The GDAL adapter emits one warp per method, or rejects the plan.

### Output types

```rust
pub enum Rounding { NearestTiesEven, NearestTiesAway, TowardZero }

pub enum OutOfRange {
    /// Clamp to the output type's range.
    Clamp,
    /// Write `target_nodata`.
    Nodata,
}
```

- `Nearest` and `Mode` only copy source values. With `output_dtype` equal to the
  source's type, they need neither `rounding` nor `out_of_range`.
- `Interpolate` and `Conservative` can produce fractions, and values outside the
  source's range: cubic kernels overshoot. With an integer `output_dtype`, both
  `rounding` and `out_of_range` are required.
- Any other conversion that can lose a value requires `out_of_range`.

### Why the target geometry has a schema

A mesh target is defined by its node coordinates and connectivity. Those are data, not
parameters, and the plan can't contain data. So the target's geometry is a bound
dataset. Without this, `Warp` could only reach targets that a few numbers can describe,
which in practice means GDAL's.

A bare binding would give validation nothing to check offline. So `GeometryBinding`
declares the schema the binding must have, as `Open` does. The host checks the real
dataset against it when it resolves the binding, exactly as it does for `Open`.

## Resampling

```rust
pub enum Resampling {
    Nearest,
    Interpolate { kernel: Kernel, scaling: KernelScaling },
    Mode { ties: ModeTie },
    Conservative { area: AreaMeasure, renormalise: bool },
}

/// Which value `Mode` returns when several are equally common.
pub enum ModeTie { Smallest, Largest }

pub enum Kernel { Bilinear, Cubic, CubicSpline, Lanczos }

pub enum KernelScaling {
    /// Evaluate the kernel at the target point, as scipy and xESMF's bilinear do.
    Point,
    /// Widen the kernel by the downsampling ratio on each axis, as gdalwarp does.
    WidenWhenDownsampling,
}

pub enum AreaMeasure {
    /// Overlap areas measured in the plane of the source grid's CRS.
    Planar,
    /// Overlap areas measured on the sphere, cell edges as great circles, as ESMF does.
    Spherical,
}

pub enum EdgeRule {
    /// Use the grid's bounds: by arithmetic for `Affine`, or its `Outer` coordinates.
    /// A grid without bounds is invalid for a method that needs edges.
    FromBounds,
    /// Put each edge halfway between neighbouring centres, in index space, and the
    /// outer edges half a cell beyond the outermost centres. This extrapolates, so it
    /// is only ever used when stated.
    MidpointsBetweenCentres,
}
```

Definitions, all proposed:

- **Every method** first transforms target locations into the source grid's CRS. It
  excludes source values that the source variable declares missing (see
  [Missing values](#missing-values)). A target cell with no remaining contributors gets
  that variable's `target_nodata`.
- **Cell edges** come from `edges`. `Nearest`, `Mode` and `Conservative` need them, on
  the source, the target, or both. `Interpolate` needs only centres.
- **`Nearest`:** the value of the source cell that contains the target cell's centre.
  Cells are half-open, so a point on a shared edge belongs to the cell that starts
  there. On a mesh, "contains" means the face containing the point. On a DGGS, it means
  the cell containing it.
- **`Interpolate`:** the kernel's weighted sum of source cell centres around the
  target centre, scaled as `scaling` says. It's defined only between `Horizontal` grids
  for now. On other kinds it's invalid.
- **`Mode`:** the most common value among source cells that overlap the target cell.
  Each cell counts once, whatever its overlap. When several values are equally common,
  `ties` picks the smallest or the largest. NaN never wins, since it's always either
  missing or excluded.
- **`Conservative`:** the overlap-area-weighted mean of the source cells that overlap
  the target cell, with areas measured as `area` says. With `renormalise`, the weights
  are divided by the valid overlap area. Without it, they're divided by the whole target
  cell's area. This is first-order conservative remapping, the method xESMF and ESMF
  call `conservative`. Both grids must be `PixelInterpretation::Area`, or meshes or
  DGGS cells.

`EdgeRule` exists because of the most common GDAL case: a swath with centre coordinates
only. gdalwarp implies edges between centres. Making `Nearest` invalid without bounds
would block that case. Implying edges silently would break purity rule 4. So the plan
says which it wants. Whether gdalwarp's geolocation inversion matches
`MidpointsBetweenCentres` is a question for the GDAL adapter PR.

There's no `Average`. GDAL's `average` weights by fractional overlap computed in source
pixel space. Whether that equals `Conservative { area: Planar, renormalise: true }` is
question 2. New methods are added one at a time, each with a definition.

## Missing values

Which source values count as missing has to be declared. Validation reads no data, so
it can't find out. This proposes a field on each variable in the logical schema, in
`arrir-types`:

```rust
pub enum MissingValues {
    Known {
        /// Values that mean "missing": CF's _FillValue and missing_value, or GDAL's nodata.
        values: Vec<Scalar>,
        /// Whether NaN counts as missing. xarray treats it as missing. GDAL does so only
        /// when nodata is NaN. So it's declared, never assumed.
        nan_is_missing: bool,
    },
    /// Not known. An operation whose result depends on missing values is invalid.
    Unknown,
}
```

- **It lives in the logical schema, not the layout.** `arrir` can't depend on
  `arrir-layout`. The layout's `fill` and the CF attributes become this field when
  `arrir-layout` produces a `Schema` for the logical side. Whether `fill` counts as
  missing there is question 5.
- **`Reduce` needs it too.** `skipna` means "skip the values this field declares
  missing". So the field belongs with the schema work in roadmap step 3, not with
  `Warp` alone.
- **`target_nodata` is always required.** Whether a target cell ends up empty depends
  on the data, and on the geometry: a target cell outside the source's extent has no
  contributors. Neither can be checked offline. So every warped variable says what to
  write in an empty cell.

Valid ranges (CF's `valid_min`, `valid_max` and `valid_range`) aren't covered yet.

## Effect on the schema

- Each listed variable has its placement dimensions replaced by the target's,
  inserted where the first of the old ones was. A `(time, row, column)` variable warped
  to a mesh becomes `(time, face)`. Its data type becomes `output_dtype`. Its missing
  values become `Known { values: [target_nodata], nan_is_missing }`, where
  `nan_is_missing` is true when `target_nodata` is NaN.
- The target grid enters the schema, with its coordinate variables from the model or
  from `target.geometry`. Warped variables are placed on it, at its primary location:
  `Centre` on both axes for `Horizontal`, a face for `Mesh`, a cell for `Dggs`.
- Variables on `source_grid` that aren't listed stay where they are. The source grid and
  its coordinate variables are dropped once no data variable is placed on it.

## Validation

- Every listed variable is placed on `source_grid`, at its primary location. Warping a
  staggered variable would shift it by half a cell without saying so. Moving it to the
  primary location first is a separate operation, not yet designed.
- No variable is listed twice.
- Every listed variable's missing values are `Known`.
- Each `WarpVariable` has the `rounding` and `out_of_range` its method and output type
  require, as above.
- Both grids have a spatial reference: a `crs`, or a DGGS system.
- The target's grid ID, dimension names and coordinate names don't collide with any
  already in the schema.
- `sizes` gives exactly the target grid's dimensions, once each.
- **`target.geometry`:**
  - It is set exactly when the target's model refers to coordinate variables.
  - Its schema contains each one, as a numeric coordinate placed on the target grid at
    the right position, spanning dimensions whose sizes match `sizes`.
- A method that needs edges has them, from bounds or from `edges`.
- When the source grid is the result of an unresolved label selection, the warp can't be
  validated until the label is resolved (see [grids.md](grids.md#select-on-a-grid)).

## Footprint

Warp's footprint, which is the source region needed for a target region, needs a
coordinate transformation. So `arrir-physical` can't plan a warp until that is designed.
An adapter that hands the warp to an engine which plans its own reads, as GDAL does,
doesn't need it.

## Reference fixtures

The definitions above are checked by small fixtures whose expected outputs are worked
out by hand from those definitions, not produced by any engine. They're checked into
the repository. Every adapter's execution tests must match them: GDAL's, xESMF's, and
any other that emits `Warp`.

- **Small grids,** such as 4×4 onto 2×2 or 3×3. Small enough that each expected value
  can be checked by hand.
- **Planar fixtures use one CRS** with a shifted, scaled or rotated affine target, so
  expected values need no projection library.
- **Spherical fixtures** use cells whose areas are known exactly, such as octants of
  the sphere.
- **Each method gets cases for:**
  - missing source values;
  - a target cell outside the source;
  - a tie for `Mode`;
  - integer output with rounding and out-of-range values.
- **Each fixture states its tolerance.** `Nearest` and `Mode` are exact. The contract
  doesn't assume bitwise agreement between engines, so the others state a bound.

Comparing an adapter's output with the same engine run directly only shows that the
adapter renders the plan faithfully. These fixtures are what show the engine computes
what ArrIR means.

## Checked against more than GDAL

| Concept | Non-GDAL reference |
| --- | --- |
| `Warp` between structured grids; `Conservative`; `Interpolate` with `Point` | xESMF (ESMF) |
| Warp to and from meshes | uxarray's remapping |
| Warp to and from DGGS | xdggs; healpy |

## Questions for Mike

1. Does gdalwarp's `mode` break ties by the smallest or the largest value, or by
   something else, such as scan order? If neither `ModeTie` matches, the GDAL adapter
   rejects `Mode`.
2. Does GDAL's `average` equal `Conservative { area: Planar, renormalise: true }`, or
   does it differ, for example because it measures overlap in source pixel space?
3. Can gdalwarp turn off kernel widening when downsampling, so that it can emit
   `KernelScaling::Point`?
4. How does gdalwarp round and clamp when it warps into an integer type? That decides
   which `Rounding` and `OutOfRange` variants the GDAL adapter can emit exactly.
5. When `arrir-layout` produces a `Schema`, should the layout's `fill` count as a missing
   value, or only the CF attributes?

## Not settled

- An operation that moves a staggered variable to its grid's primary location.
- Second-order conservative remapping, and patch recovery.
- Warp's effect on `Structured` grids.
- Warp's footprint, and how `arrir-physical` computes it.
