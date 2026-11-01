# Warp

Status: decided 1 November 2026, after review (see
[the 1 November discussion](2026-11-01-discussion.md#warp-and-gdal-adapter)). Part of
[the extraction contract](design.md). It follows the decided [dag.md](dag.md),
[schema-and-values.md](schema-and-values.md) and [cast.md](cast.md). Its grid concepts
(models, placements, `PixelInterpretation`, Select on a grid) come from
[grids.md](grids.md), which is still proposed; a change there may change this doc. The
earlier reasoning is in [the 28 October discussion](2026-10-28-discussion.md).

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
    pub cells: CellRules,
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
pub struct CellRules {
    /// How to find cell edges on a grid that has no bounds.
    pub edges: EdgeRule,
    /// The shape of a cell's sides between its corners, on both grids.
    pub edge_shape: EdgeShape,
    /// Largest permitted error of an approximate coordinate transformer, measured in
    /// source cells (distance in the source grid's index space). 0 means exact. A
    /// value above 0 is a permission to approximate, not a bound any engine promises.
    pub error_threshold: f64,
}

pub struct WarpTarget {
    /// The output grid. Its ID and coordinate names are new to the output schema. A
    /// dimension name already used elsewhere in the plan names the same axis
    /// ([dag.md](dag.md), "ID scope across Opens").
    pub grid: Grid,
    /// The size of each of the target grid's dimensions.
    pub sizes: Vec<(DimId, u64)>,
    /// For each coordinate variable the target grid's model names, the variable of
    /// the geometry input that supplies it. Empty for an affine target.
    pub coordinates: Vec<TargetCoordinate>,
}

/// One target coordinate, taken from the geometry input (`Warp`'s second input).
pub struct TargetCoordinate {
    /// The name the coordinate has in the output schema, as the target model names it.
    pub output: VarId,
    /// The variable in the geometry input's schema that holds its values.
    pub from_input: VarId,
}
```

`Warp`'s inputs are the source dataset, then, when the target's model names coordinate
variables, the geometry input (dag.md decision 7). Arity is `Exactly(2)` when
`target.coordinates` is non-empty and `Exactly(1)` otherwise. The geometry input is any
node with a `Dataset` result: an `Open`, or a computed dataset.

`Warp` takes a `Dataset` input and gives a `Dataset` output. A single variable is
selected afterwards with `Project`.

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

- for `Nearest` and `Mode`, it is NaN for a float variable, or one of the variable's
  declared sentinels;
- for `Interpolate` and `Conservative`, it is NaN.

Every warped float variable must have `nan_is_missing: true` (decision 3). Without it
NaN would be a valid value: `Nearest` would copy it, and the output, which marks empty
cells with NaN, would then read it as missing. With it, NaN is never a contributor, and
the one NaN a warp can produce from valid data, `+inf` and `-inf` meeting with non-zero
weight, reads as missing. That is stated, not silent.

An integer variable with no sentinel (`Known { values: [], nan_is_missing: false }`)
has no legal `empty_value` for `Nearest` or `Mode`, and a `Cast` can't add one, since a
same-dtype cast has no node. Such a warp is rejected (decision 5).

Choosing which value marks an empty cell after a later conversion to an integer type,
and what happens when a converted value collides with it, is `Cast`'s job.

### Why the target geometry is an input

A regular target is a few numbers: a CRS, a transform and two sizes. Those fit in the
plan. Other targets are data. A tripolar ocean grid is its `geolon(j, i)` and
`geolat(j, i)` arrays, and a mesh is its node coordinates and connectivity. The plan
can't contain data (purity rule 1), so these arrive as an input node, usually an
`Open` that the host binds. The `Open`'s declared schema is what validation checks the
warp against, and the host checks the bound dataset against that schema, as for any
`Open`. A computed geometry, such as a `Select` of another model's grid, is an ordinary
node and needs nothing extra.

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

pub enum Kernel {
    /// Tent: w(x) = 1 - |x| for |x| < 1. Radius 1.
    Bilinear,
    /// Keys' cubic convolution with a = -0.5. Radius 2.
    Cubic,
    /// w(x) = sinc(x) sinc(x / 3), with sinc(x) = sin(πx) / (πx). Radius 3.
    Lanczos,
}

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
    /// CF bounds variables never count; they stay ordinary auxiliary coordinates
    /// ([grids.md](grids.md#decisions)). A grid without bounds is invalid for a
    /// method that needs edges.
    FromBounds,
    /// Derive corners from centres, in the coordinates' own CRS: a corner is the mean
    /// of the four centres around it, and the outer corners extrapolate linearly. In a
    /// geographic CRS, each longitude is first shifted by a multiple of 360° to within
    /// 180° of the first of the four. This extrapolates, so it is only ever used when
    /// stated.
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

Definitions:

- **Every method** first transforms target locations into the source grid's CRS:
  target centres for `Nearest` and `Interpolate`, target cells for `Mode` and
  `Conservative`. It excludes source values that the source variable declares missing
  ([schema-and-values.md](schema-and-values.md#missing-values-and-decoding)). A target
  cell with no remaining contributors gets that variable's `empty_value`.
- **Cell edges** come from `cells.edges`, and the sides between corners have the shape
  `cells.edge_shape`. `Nearest`, `Mode` and `Conservative` need them, on the source,
  the target, or both. `Interpolate` needs only centres.
- **`Nearest`:** the value of the source cell that contains the target cell's centre.
  Cells are half-open in the source grid's index space, so a point on a shared edge
  belongs to the cell that starts there (GDAL's floor agrees: a centre exactly on an
  edge picked the higher cell). On a `PixelInterpretation::Point` grid a cell is the
  box centred on its sample, so `Nearest` picks the nearest sample, ties going to the
  higher index. On a mesh, "contains" means the face containing the point. On a DGGS,
  it means the cell containing it.
- **`Interpolate`:** `Σ w·v / Σ w` over the contributors: source cells inside the
  source grid, not missing, and with `w ≠ 0`, where `w` is the product of the
  kernel's weights on the column and row axes at the distance, in source cells, from
  the target centre. Near missing values and the source's edges this renormalises over
  what remains. With `Widen`, a source centre `d` cells away on the column axis gets
  the kernel's weight at `d * column`, and the kernel's radius is divided by `column`;
  the row axis is the same. The factors are free values that lowering states;
  validation checks only that each is in (0, 1]. It's defined only between
  `Horizontal` grids for now. On other kinds it's invalid.
- **`Mode`:** the most common value among source cells that overlap the target cell.
  Each cell counts once, whatever its overlap. When several values are equally common,
  `ties` picks the smallest or the largest. NaN is never a candidate, since warped
  float variables treat NaN as missing.
- **`Conservative`:** the overlap-area-weighted mean of the source cells that overlap
  the target cell, with areas measured as `area` says. This is first-order conservative
  remapping, the method xESMF and ESMF call `conservative`.
- **`Mode` and `Conservative`** need `PixelInterpretation::Area` on both grids, or
  meshes or DGGS cells. `Nearest` and `Interpolate` accept `Area` and `Point`.

**A source cell with zero weight contributes nothing** to `Interpolate` or
`Conservative`. It is left out of the sum, not added as `0 × value`. So a target centre
that falls exactly on a source centre returns that source value, and an infinite source
value never reaches a cell where its weight is zero: `0 × inf` would be NaN. Infinities
with non-zero weight follow IEEE 754. One infinity makes the result that infinity, and
`+inf` together with `-inf` gives NaN, which reads as missing. gdalwarp breaks this rule
with a `Float32` working type ([gdal-adapter.md](gdal-adapter.md#how-it-emits-a-warp)).

`EdgeRule` exists because of the most common GDAL case: a swath with centre coordinates
only. gdalwarp implies edges between centres. Making `Nearest` invalid without bounds
would block that case. Implying edges silently would break purity rule 4. So the plan
says which it wants. Mike's tests show gdalwarp's geolocation inversion matches
`MidpointsBetweenCentres` when told the coordinates are centres (see
[gdal-adapter.md](gdal-adapter.md)).

**The edge shape is a grid rule, not a `Conservative` option.** Mike suggested splitting
`Spherical` into an area measure and an edge shape, because "spherical" also implied
great-circle sides. The split is kept, and `edge_shape` sits in `CellRules`
(decision 1). Which source cell contains a point, for `Nearest`, or overlaps a cell,
for `Mode`, depends on the cells' sides as much as `Conservative`'s areas do.

**The kernel scale is in the plan.** gdalwarp estimates its own widening factor, and
the estimate changes from one processing chunk to the next, so its output depends on
the warp memory setting
([mdsumner/osgeo-upstream#50](https://github.com/mdsumner/osgeo-upstream/issues/50)). A
stated factor gives every engine the same kernel.

There's no `Average`. GDAL's `average` differs from `Conservative` (see question 2), so
it isn't an alias. There's no `CubicSpline`: GDAL's is an approximating B-spline that
still returns the source value on an exactly aligned target, so it isn't one kernel
(decision 7). New methods are added one at a time, each with a definition.

## Missing values

`MissingValues` is defined in
[schema-and-values.md](schema-and-values.md#missing-values-and-decoding). The schema
describes the values operations receive, and the first subset accepts already-logical
bindings only, so sentinels are compared with logical values. Warp adds:

- **Every listed variable's missing values are `Known`,** and a float variable's have
  `nan_is_missing: true` (see [Output type and empty cells](#output-type-and-empty-cells)).
- **`empty_value` is always required.** Whether a target cell ends up empty depends
  on the data, and on the geometry: a target cell outside the source's extent has no
  contributors. Neither can be checked offline. So every warped variable says what to
  write in an empty cell.
- **The layout's `fill` doesn't count as missing.** It says what unwritten elements
  read as, not that the value means missing (question 5).

Valid ranges (CF's `valid_min`, `valid_max` and `valid_range`) aren't covered yet.

## Effect on the schema

- Each listed variable has its placement dimensions replaced by the target's,
  inserted where the first of the old ones was. A `(time, row, column)` variable warped
  to a mesh becomes `(time, face)`. Its data type becomes the method's output type. Its
  missing values become `Known { values: [], nan_is_missing: true }` when
  `empty_value` is NaN, and `Known { values: [empty_value], nan_is_missing: false }`
  otherwise.
- The target grid enters the schema, with its coordinate variables from the model or
  from the geometry input, as `target.coordinates` says. Only the listed coordinates
  are taken from that input. Warped variables are placed on the target grid at its
  primary location: `Centre` on both axes for `Horizontal`, a face for `Mesh`, a cell
  for `Dggs`.
- Variables on `source_grid` that aren't listed stay where they are. The source grid and
  its coordinate variables are dropped once no data variable is placed on it, by
  grids.md's general rule that a grid with nothing placed on it is removed
  ([grids.md](grids.md#decisions), decision C). A target that reuses the source grid's
  names requires this (see [Validation](#validation)).
- When the source grid is dropped, any other variable that spans one of its dimensions
  (an `area(y, x)` or `mask(y, x)` coordinate outside the grid model) is not dropped
  silently: the warp is rejected, and lowering removes such variables with `Project`
  first (decision 9).

## Validation

- The input is a `Dataset` with known dtypes for every listed variable; an unknown
  dtype is a readiness requirement, not an error.
- Every listed variable is placed on `source_grid`, at its primary location. Warping a
  staggered variable would shift it by half a cell without saying so. Moving it to the
  primary location first is a separate operation, not yet designed, and is rejected
  for now.
- No variable is listed twice.
- Every listed variable's missing values are `Known`, with `nan_is_missing: true` for
  a float variable.
- Each `empty_value` has the method's output type, and is NaN or a declared sentinel
  as above. An integer variable with no sentinel can't be warped with `Nearest` or
  `Mode`.
- A `bool` variable can't use `Interpolate` or `Conservative`.
- `Widen` factors are in (0, 1]. `error_threshold` is finite and ≥ 0.
- `Mode` and `Conservative` need `PixelInterpretation::Area` on both grids.
- Both grids have a spatial reference: a `crs`, or a DGGS system.
- Both grids are `Horizontal` for now. `Structured` grids are rejected.
- The target's grid ID, dimension names and coordinate names don't collide with any
  already in the schema, with one exception. The target may reuse the source grid's
  dimension and coordinate names when the warp empties the source grid: every data
  variable placed on `source_grid` is listed, and no variable off the grid spans its
  dimensions. The source grid and its coordinates are then dropped (see above), so the
  names are free. This is how `obs (lat, lon)` is warped onto `model`'s `(lat, lon)`
  grid; under [dag.md](dag.md)'s plan-wide `DimId`, the reused names then are the same
  axes as `model`'s. If any variable stays on the source grid, the names would name two
  grids' axes in one schema, so the warp is rejected; warp the remaining variables too,
  or `Rename` first.
- `sizes` gives exactly the target grid's dimensions, once each.
- An `Affine` target declares `IndexMap { offset: 0, step: 1 }` on both axes, as any
  newly declared grid does ([grids.md](grids.md#grid-models)).
- **The geometry input:**
  - It is present exactly when `target.coordinates` is non-empty, and
    `target.coordinates` lists exactly the coordinate variables the target model names.
  - It is a `Dataset` that has every `from_input` variable, each numeric with a known
    dtype.
  - Each such variable spans the target grid's dimensions, by `DimId`, with `Known`
    extents equal to `sizes`. `VarId`s are per node, so only the `output` names enter
    the output schema, and they follow the collision rule above.
- A method that needs edges has them, from bounds or from `cells.edges`.
- When the source grid's `SpatialState` is `Unresolved`, after a label selection on one
  of its dimensions, the warp can't be validated until the label is resolved (see
  [grids.md](grids.md#select-on-a-grid)).

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
  - missing source values, including a missing value in the cell that contains the
    target centre;
  - a target cell outside the source;
  - sources one cell tall or one cell wide;
  - NaN and infinite neighbours with zero weight;
  - per-variable missing values in a warp of several variables;
  - a tie for `Mode`.
- **Integer output with rounding and out-of-range values** is a `Warp` followed by a
  `Cast`, since `Warp` never rounds. Those cases belong to #42's Warp→Cast set.
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

## Answers from Mike

Answered from GDAL's source and from tests in rwarp (hypertidy/rwarp#2 and #3). The
GDAL adapter's rules that follow from them are in [gdal-adapter.md](gdal-adapter.md).

1. **Mode ties.** gdalwarp keeps the value that reached the winning count first, in
   scan order, unless `MODE_TIES=MIN` or `MAX` is set (GDAL 3.11 and later). Scan order
   depends on the source's orientation, so it isn't adopted as a `ModeTie`. `MIN` and
   `MAX` are only reliable for 8- and 16-bit working types
   ([mdsumner/osgeo-upstream#51](https://github.com/mdsumner/osgeo-upstream/issues/51)).
2. **`average`.** It renormalises over valid pixels, but it isn't `Conservative`. It
   measures overlap in source pixel space, against a box built from two corners of the
   target cell, and overweights edge pixels when the box overhangs the source. It
   agrees with a renormalised `Planar` `Conservative` with `StraightInIndex` edges
   only for axis-aligned grids in one CRS, away from the edges.
3. **Kernel widening.** `XSCALE=1` and `YSCALE=1` turn it off. Bilinear and cubic only
   widen once the factor is 0.5 or less. The automatic factor varies between chunks
   ([mdsumner/osgeo-upstream#50](https://github.com/mdsumner/osgeo-upstream/issues/50)),
   which is why the plan states it.
4. **Rounding.** With an integer working type, gdalwarp clamps, then rounds an
   interpolating kernel's result half up (`Rounding::NearestTiesUp`), but it converts
   the source into the working type rounding half away from zero. With a float working
   type, it converts at the end, rounding half away from zero, with NaN becoming 0. A
   result equal to the nodata value is moved off it, which on the float path can be
   undone by the rounding. These rules are what [`Cast`](cast.md) has to express; they
   were checked with GDAL 3.13.3 on 1 November
   ([cast.md](cast.md#what-gdalwarp-can-emit)).
5. **The layout's `fill`.** It doesn't count as missing. See
   [Missing values](#missing-values).

## Decisions

Decided 1 November 2026, taking the review's recommendations (Charles's standing rule
of accepting conservative ones):

1. **`edge_shape` stays in `CellRules`,** applying to every method, because `Nearest`
   and `Mode` containment depend on it. One shape applies to both grids; grids whose
   own shapes differ can't be stated and are rejected.
2. **`Mode` counts each cell once,** whatever its overlap. The GDAL adapter rejects
   `Mode`, since GDAL weights by overlap.
3. **Warped float variables have `nan_is_missing: true`,** and a NaN computed from
   infinities reads as missing.
4. **`error_threshold` is in source cells,** and a value above 0 is a permission to
   approximate, not a bound.
5. **Integer variables with no sentinel** can't be warped with `Nearest` or `Mode`.
6. **`Interpolate` renormalises** over contributors that are inside the source, not
   missing, and have non-zero weight.
7. **`CubicSpline` is removed.** `Cubic` is Keys with a = -0.5, `Lanczos` has radius 3.
8. **Result kinds:** `Dataset` in and out; `Project` selects a variable afterwards.
9. **Other variables on a dropped source grid's dimensions** reject the warp rather
   than being dropped silently.
10. **Not yet:** an operation moving a staggered variable to its grid's primary location
    (rejected now), `Structured` grids (rejected), packed variables (rejected, since the
    first subset accepts logical bindings only), second-order conservative remapping
    and patch recovery (deferred), and Warp's footprint (deferred; GDAL doesn't need it).
