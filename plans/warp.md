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
    /// The variables to warp, each with its own method. Variables on `source_grid`
    /// that aren't listed stay where they are.
    pub variables: Vec<WarpVariable>,
    pub geometry: GeometryRules,
}

pub struct WarpVariable {
    pub var: VarId,
    pub resampling: Resampling,
    /// Written to target cells with no valid source data, in the method's output type.
    /// See "Output type and empty cells".
    pub empty_value: Scalar,
}

/// How cells are shaped, and how exactly coordinates are transformed. These apply to
/// every variable in the warp, because they describe the grids, not the values.
pub struct GeometryRules {
    /// How to find cell edges on a grid that has no bounds.
    pub edges: EdgeRule,
    /// The shape of a cell's sides between its corners.
    pub edge_shape: EdgeShape,
    /// Largest permitted error, in target cells, of an approximate coordinate
    /// transformer. 0 means exact. gdalwarp defaults to 0.125, so it's never implicit.
    pub error_threshold: f64,
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

### Why each variable has its own method

One method for the whole warp can't serve every variable on a grid. A land-cover class
needs `Mode` or `Nearest`, while a temperature on the same grid needs `Interpolate` or
`Conservative`.

GDAL applies one method to all bands. That is a limit of one adapter, not of the IR.
The GDAL adapter emits one warp per method, or rejects the plan.

### Output type and empty cells

`Warp` doesn't convert types. Its output type follows from the method:

- `Nearest` and `Mode` copy source values, so the output has the source's type.
- `Interpolate` and `Conservative` produce fractions, so the output is `float64`.

Converting to another type, with rounding and clamping, is [`Cast`](cast.md), a
separate operation. Both are useful alone: a float warp needs no conversion, and an
`astype` before a reduction needs no warp. An adapter whose engine does both in one
call, as gdalwarp does, emits a `Warp` followed by a `Cast` as one call.

`empty_value` is chosen so that no valid result can equal it:

- for `Nearest` and `Mode`, it is NaN or one of the source variable's declared missing
  values, which no copied value can be;
- for `Interpolate` and `Conservative`, it is NaN.

So `Warp` never writes a valid value that reads as missing. Choosing which value marks
an empty cell in an integer output, and what happens when a converted value collides
with it, is `Cast`'s job.

### Why the target geometry has a schema

A regular target is a few numbers: a CRS, a transform and two sizes. Those fit in the
plan. Other targets are data. A tripolar ocean grid is its `geolon(j, i)` and
`geolat(j, i)` arrays, and a mesh is its node coordinates and connectivity. The plan
can't contain data (purity rule 1), so these targets arrive as a binding that the host
fills, as a source does through `Open`. Without this, `Warp` could only reach the
regular grids gdalwarp writes.

A bare binding is only a name, so validation couldn't check the warp or derive its
output schema offline. `GeometryBinding` therefore declares what the bound dataset
will contain: for example, `geolon` and `geolat` on `(yh, xh)`, 1080 × 1440, placed at
`(Centre, Centre)` on the target grid. Validation checks the plan against that
declaration. When the host resolves the binding, it checks the real dataset against
it and fails if they differ, exactly as for `Open`.

## Resampling

Each variant that takes options holds a named struct, so each set of options has its
own `impl` block for validation.

```rust
pub enum Resampling {
    Nearest,
    Interpolate(InterpolateOptions),
    Mode(ModeOptions),
    Conservative(ConservativeOptions),
}

pub struct InterpolateOptions {
    pub kernel: Kernel,
    pub scaling: KernelScaling,
}

pub enum Kernel { Bilinear, Cubic, CubicSpline, Lanczos }

pub enum KernelScaling {
    /// Evaluate the kernel at the target point, as scipy and xESMF's bilinear do.
    Point,
    /// Stretch the kernel by 1/factor on each axis, to cover a downsampled cell.
    /// Each factor is in (0, 1]; 1 is the same as `Point`.
    Widen { column: f64, row: f64 },
}

pub struct ModeOptions {
    pub ties: ModeTie,
}

/// Which value `Mode` returns when several are equally common.
pub enum ModeTie { Smallest, Largest }

pub struct ConservativeOptions {
    pub area: AreaMeasure,
    /// Divide by the valid overlap area (ESMF's `fracarea`), or by the whole target
    /// cell's area (`destarea`, xESMF's default).
    pub renormalise: bool,
}

pub enum AreaMeasure {
    /// Area in the source grid's index space, where each source cell has area 1. For
    /// an affine source this is planar area in its CRS, up to a constant.
    Planar,
    /// Area on the sphere, of cells whose sides are as `edge_shape` says.
    Spherical,
}

pub enum EdgeRule {
    /// Use the grid's bounds: by arithmetic for `Affine`, or its `Outer` coordinates.
    /// A grid without bounds is invalid for a method that needs edges.
    FromBounds,
    /// Derive corners from centres, in the coordinates' own CRS: a corner is the mean
    /// of the four centres around it, and the outer corners extrapolate linearly. This
    /// extrapolates, so it is only ever used when stated.
    MidpointsBetweenCentres,
}

pub enum EdgeShape {
    /// Straight in each grid's own index space. For an affine grid, straight in its CRS.
    StraightInIndex,
    /// Straight in longitude and latitude, as many model grids define their cells.
    StraightInLonLat,
    /// Great-circle arcs, as ESMF assumes.
    GreatCircle,
}
```

Definitions, all proposed:

- **Every method** first transforms target locations into the source grid's CRS. It
  excludes source values that the source variable declares missing (see
  [Missing values](#missing-values)). A target cell with no remaining contributors gets
  that variable's `empty_value`.
- **Cell edges** come from `geometry.edges`, and the sides between corners have the
  shape `geometry.edge_shape`. `Nearest`, `Mode` and `Conservative` need them, on the
  source, the target, or both. `Interpolate` needs only centres.
- **`Nearest`:** the value of the source cell that contains the target cell's centre.
  Cells are half-open, so a point on a shared edge belongs to the cell that starts
  there. On a mesh, "contains" means the face containing the point. On a DGGS, it means
  the cell containing it.
- **`Interpolate`:** the kernel's weighted sum of source cell centres around the
  target centre. With `Widen`, a source centre `d` cells away on the column axis gets
  the kernel's weight at `d * column`, and the kernel's radius is divided by `column`;
  the row axis is the same. The plan computes the factors from the two grids, with no
  data read, and states them. It's defined only between `Horizontal` grids for now. On
  other kinds it's invalid.
- **`Mode`:** the most common value among source cells that overlap the target cell.
  Each cell counts once, whatever its overlap. When several values are equally common,
  `ties` picks the smallest or the largest. NaN never wins, since it's always either
  missing or excluded.
- **`Conservative`:** the overlap-area-weighted mean of the source cells that overlap
  the target cell, with areas measured as `area` says. This is first-order conservative
  remapping, the method xESMF and ESMF call `conservative`. Both grids must be
  `PixelInterpretation::Area`, or meshes or DGGS cells.

`EdgeRule` exists because of the most common GDAL case: a swath with centre coordinates
only. gdalwarp implies edges between centres. Making `Nearest` invalid without bounds
would block that case. Implying edges silently would break purity rule 4. So the plan
says which it wants. Mike's tests show gdalwarp's geolocation inversion matches
`MidpointsBetweenCentres` when told the coordinates are centres (see
[gdal-adapter.md](gdal-adapter.md)).

**The edge shape is a grid rule, not a `Conservative` option.** Mike suggested splitting
`Spherical` into an area measure and an edge shape, because "spherical" also implied
great-circle sides. The split is kept, but `edge_shape` sits in `GeometryRules`. Which
source cell contains a point, for `Nearest`, or overlaps a cell, for `Mode`, depends on
the cells' sides as much as `Conservative`'s areas do.

**The kernel scale is in the plan.** gdalwarp estimates its own widening factor, and
the estimate changes from one processing chunk to the next, so its output depends on
the warp memory setting (osgeo-upstream#50). A stated factor gives every engine the
same kernel.

There's no `Average`. GDAL's `average` differs from `Conservative` (see question 2), so
it isn't an alias. New methods are added one at a time, each with a definition.

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
  `arrir-layout` produces a `Schema` for the logical side. The layout's `fill` doesn't
  count as missing: it says what unwritten elements read as, not that the value means
  missing (question 5).
- **Missing values are stated in the type they're compared with.** CF's are in the
  packed type, before `scale_factor` and `add_offset`. So they're compared with stored
  values, and a later decoding step has to carry them through. How the schema
  represents packed variables isn't designed yet.
- **`Reduce` needs it too.** `skipna` means "skip the values this field declares
  missing". So the field belongs with the schema work in roadmap step 3, not with
  `Warp` alone.
- **`empty_value` is always required.** Whether a target cell ends up empty depends
  on the data, and on the geometry: a target cell outside the source's extent has no
  contributors. Neither can be checked offline. So every warped variable says what to
  write in an empty cell.

Valid ranges (CF's `valid_min`, `valid_max` and `valid_range`) aren't covered yet.

## Effect on the schema

- Each listed variable has its placement dimensions replaced by the target's,
  inserted where the first of the old ones was. A `(time, row, column)` variable warped
  to a mesh becomes `(time, face)`. Its data type becomes the method's output type. Its
  missing values become `Known { values: [empty_value], nan_is_missing }`, where
  `nan_is_missing` is true when `empty_value` is NaN.
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
- Each `empty_value` has the method's output type, and is NaN or a declared missing
  value as above.
- `Widen` factors are in (0, 1].
- Both grids have a spatial reference: a `crs`, or a DGGS system.
- The target's grid ID, dimension names and coordinate names don't collide with any
  already in the schema.
- `sizes` gives exactly the target grid's dimensions, once each.
- **`target.geometry`:**
  - It is set exactly when the target's model refers to coordinate variables.
  - Its schema contains each one, as a numeric coordinate placed on the target grid at
    the right position, spanning dimensions whose sizes match `sizes`.
- A method that needs edges has them, from bounds or from `geometry.edges`.
- When the source grid is the result of an unresolved label selection, the warp can't be
  validated until the label is resolved (see [grids.md](grids.md#select-on-a-grid)).

## Footprint

Warp's footprint, which is the source region needed for a target region, needs a
coordinate transformation. So `arrir-physical` can't plan a warp until that is designed.
An adapter that hands the warp to an engine which plans its own reads, as GDAL does,
doesn't need it.

Regridding weights belong to the same layer. Given the two grids, a table of (target
cell, source cell, weight) is computed with no data read, then applied to any number of
variables and time steps. It's a physical-planning output, like a chunk list, not part
of the logical plan.

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
- **Conservative fixtures that can't be worked by hand** come from ESMF and
  TempestRemap, generated with the `gdal-r-esmf` image in hypertidy/gdal-r-ci and
  committed, so the tests don't need ESMF. Where an implementation's definition differs
  from ESMF's on purpose, as `StraightInLonLat` does from `GreatCircle`, the fixture
  records the distance between them as a number to watch, not as a failure.

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

Questions 1 to 5 are answered, from GDAL's source and from tests in rwarp
(hypertidy/rwarp#2 and #3). The GDAL adapter's rules that follow from them are in
[gdal-adapter.md](gdal-adapter.md).

1. **Mode ties.** gdalwarp keeps the value that reached the winning count first, in
   scan order, unless `MODE_TIES=MIN` or `MAX` is set (GDAL 3.11 and later). Scan order
   depends on the source's orientation, so it isn't adopted as a `ModeTie`. `MIN` and
   `MAX` are only reliable for 8- and 16-bit working types (osgeo-upstream#51).
2. **`average`.** It renormalises over valid pixels, but it isn't `Conservative`. It
   measures overlap in source pixel space, against a box built from two corners of the
   target cell, and overweights edge pixels when the box overhangs the source. It
   agrees with a renormalised `Planar` `Conservative` with `StraightInIndex` edges
   only for axis-aligned grids in one CRS, away from the edges.
3. **Kernel widening.** `XSCALE=1` and `YSCALE=1` turn it off. Bilinear and cubic only
   widen once the factor is 0.5 or less. The automatic factor varies between chunks
   (osgeo-upstream#50), which is why the plan states it.
4. **Rounding.** With an integer working type, gdalwarp clamps, then rounds half up
   (`Rounding::NearestTiesUp`). With a float working type, it converts at the end,
   rounding half away from zero, with NaN becoming 0. A result equal to the nodata
   value is moved off it, which on the float path can be undone by the rounding. These
   rules are what [`Cast`](cast.md) has to express.
5. **The layout's `fill`.** It doesn't count as missing. See
   [Missing values](#missing-values).

Question for Mike:

6. `edge_shape` sits in `GeometryRules`, applying to every method, rather than in the
   `Conservative` options as suggested. Does that fit rwarp's front ends?

## Not settled

- An operation that moves a staggered variable to its grid's primary location.
- Second-order conservative remapping, and patch recovery.
- Warp's effect on `Structured` grids.
- Warp's footprint, and how `arrir-physical` computes it.
- One `edge_shape` for both grids. Each grid's own shape may need stating when they
  differ in a way `StraightInIndex` doesn't cover.
- Packed variables (`scale_factor`, `add_offset`) and how decoding carries missing
  values.
