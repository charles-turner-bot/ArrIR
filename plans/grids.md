# Grids, CRS and staggering

Status: decided 1 November 2026 after an independent review, except the two questions
for Mike at the end. Part of [the extraction contract](design.md). The reasoning is in
[the 28 October discussion](2026-10-28-discussion.md) and
[the 1 November discussion](2026-11-01-discussion.md#grids).

## Summary

ArrIR must lower plans to GDAL, not only to xarray-shaped hosts. Otherwise it describes
xarray calls, not a read-then-compute pipeline that GDAL, xarray and others can all
execute. GDAL needs three things an xarray-only target can skip: a spatial reference,
a grid it can do arithmetic on, and warp as a first-class operation.

Replacing a tie to xarray with a tie to GDAL isn't exactly any better. So the design is
general across grid kinds, not only GIS rasters: structured grids, unstructured meshes
and discrete global grids such as HEALPix. Every concept is checked against at least
one non-GDAL implementation, and resampling methods are defined here, not by GDAL.
Without all this, we won't produce something which is generically useful across the
N-D Array and GIS communities.

The schema ([schema-and-values.md](schema-and-values.md)) and `Select`
([select.md](select.md)) are decided; this document states how grids sit in the schema
and how each operation changes them. The code lands in roadmap order, starting with
2-D structured grids. The tests for this design:
- Adding `Warp` changes neither the schema nor `Select`.
- Adding the mesh, DGGS and N-D grid kinds sketched in
  [Limits and extension plan](#limits-and-extension-plan) changes neither `Grid`,
  `GridPlacement`, `Warp` nor `Select`.

`Warp` is in [warp.md](warp.md) and the GDAL adapter in
[gdal-adapter.md](gdal-adapter.md). The Rust here is a sketch of fields and meaning,
not final code.

## Grids are schema entities

A grid is a named entity in the logical schema, alongside variables and dimensions. It
names its dimensions, the CRS and a coordinate model. A spatial variable refers to one
grid. Several variables can share a grid, and a dataset can hold grids with different
CRSs.

```rust
pub struct Grid {
    pub id: GridId,
    pub crs: Option<Crs>,
    pub kind: GridKind,
}

/// One variant for now, on purpose. See "Limits and extension plan".
pub enum GridKind {
    Horizontal(HorizontalGrid),
}

/// A structured 2-D grid over two horizontal dimensions.
pub struct HorizontalGrid {
    /// The index dimensions of the cell centres: GDAL's pixel and line. Staggered
    /// variables may use other dimensions (see Staggering).
    pub column_dim: DimId,
    pub row_dim: DimId,
    pub pixel: PixelInterpretation,
    pub model: GridModel,
}
```

The grid's dimensions are named `column` and `row`, not `x` and `y`. They are index
axes, and they needn't follow a direction in the CRS. In a curvilinear ocean grid, the
column dimension doesn't run east near the poles. `x` and `y` always mean the CRS's two
axes, in the order `Crs.axis_order` gives.

Code that reads a grid matches on `kind`, even while it has one variant. A new kind of
grid is then a new variant, and the compiler lists every place that must handle it.

**Scope of IDs and dimensions** ([dag.md](dag.md)):
- `GridId` is scoped to one node's schema, like `VarId`. Two inputs share a grid only
  when their grid definitions are equal ([Grid equality](#grid-equality)), never
  because their IDs match.
- Within one schema, a `DimId` is the centre or staggered dimension of at most one grid
  (decision H). Across schemas, one plan-wide `DimId` may be placed on unequal grids;
  an operation that combines them rejects the plan (dag.md).
- A grid's dimensions are `Named` axes ([shapes.md](shapes.md)).

Putting the CRS on the dataset was rejected. `gdalxarray` does this today, and when
arrays disagree it keeps the most common CRS and warns. Purity rule 4 rules that out.
Putting it on each variable was rejected too: the CRS is repeated, and two variables
can't be stated to share a grid, which warp and alignment checks need.

CF's `grid_mapping` attaches the CRS through a shared reference like this one. GDAL's
multidimensional API keeps a copy of the spatial reference on each array instead.

For now every grid is horizontal. Time, band and vertical dimensions stay ordinary
dimensions (see [Limits and extension plan](#limits-and-extension-plan)).

## Grid models

```rust
pub enum GridModel {
    /// The declared transform plus an exact index map per axis (decision A).
    Affine { transform: [f64; 6], columns: IndexMap, rows: IndexMap },
    /// Irregular 1-D coordinate variables. The column axis runs along the CRS's x
    /// and the row axis along its y.
    Rectilinear { columns: Vec<AxisCoord>, rows: Vec<AxisCoord> },
    /// 2-D coordinate variables: swaths and ocean models. GDAL warps these as
    /// geolocation arrays.
    Curvilinear { coords: Vec<CurvilinearCoords> },
}

/// Output index i on this axis is cell offset + step * i of the declared transform.
/// A source or a Warp target declares offset 0 and step 1; only Select changes it.
pub struct IndexMap {
    pub offset: i64,
    pub step: i64,
}

/// A 1-D coordinate variable for the points at one position on one axis.
pub struct AxisCoord {
    pub position: AxisPosition,
    pub var: VarId,
}

/// A pair of 2-D coordinate variables for the points at one location.
pub struct CurvilinearCoords {
    pub column: AxisPosition,
    pub row: AxisPosition,
    /// The CRS x and y of each point.
    pub x: VarId,
    pub y: VarId,
}
```

**The affine transform** is in GDAL geotransform order, `(x0, dx, rx, y0, ry, dy)`.
`(x0, y0)` is the outer corner of the first declared cell. Declared column `c` and row
`r` map to `x = x0 + c*dx + r*rx`, `y = y0 + c*ry + r*dy`. The core never does this
arithmetic in floating point. `Select` changes only the integer index maps, which
compose exactly, so merging two slices (select.md's pushdown rule 3) gives the same
grid as applying them one at a time. Computing the composed origin in `f64` instead
differs in the last bit in 600 of 2500 cases (`x0 = -180`, `dx = 0.1`, checked).
Adapters compute the effective transform in `f64` when they render
([Affine grids](#affine-grids)).

The model holds no coordinate values. Its entries name coordinate variables that are
already in the schema, and those hold the values, like any other data. A NEMO file, for
example:

```text
dims:   y = 1021, x = 1442
coords: nav_lon(y, x), nav_lat(y, x)      T-point centres
        glamf(y, x),   gphif(y, x)        F-point corners
data:   thetao(time, y, x)
```

becomes a `Horizontal` grid with `column_dim: x`, `row_dim: y`, and:

```rust
GridModel::Curvilinear { coords: vec![
    CurvilinearCoords { column: Centre, row: Centre, x: "nav_lon", y: "nav_lat" },
    CurvilinearCoords { column: Right,  row: Right,  x: "glamf",   y: "gphif" },
] }
```

Cell `(j, i)` is centred at `(nav_lon[j, i], nav_lat[j, i])`, in the grid's CRS.

`glamf` isn't the cells' bounds, even though it holds corners. It has `N` points per
axis, the upper corner of each cell, so it's at `Right`. Bounds need `N + 1` (below).
Building the missing first row and column means extrapolating, so it's never done
silently.

**Coordinates per position.** Staggered grids often store separate coordinates for each
point type: NEMO's `glamt`, `glamu`, `glamv` and `glamf`, or MOM6's `geolon`,
`geolon_u`, `geolon_v` and `geolon_c`. One coordinate pair per grid could only describe
the centres, and a `u` variable's own coordinates would lose their link to the grid. So
the model holds one entry per position.

**Bounds are coordinates at `Outer`.** The corners of every cell, `N + 1` points on each
axis, are exactly the `Outer` position. xESMF's `lon_b` and `lat_b` have that shape. So
cell bounds don't need a concept of their own. A rectilinear grid has bounds when it has
an `Outer` entry on both axes. A curvilinear grid has them when it has an
`(Outer, Outer)` entry. An affine grid always has them, by arithmetic.

**CF bounds stay ordinary coordinates** in the first subset (decision E). CF writes
bounds as `(n, 2)` or `(ny, nx, 4)` arrays, one row per cell. Converting them to the
`Outer` form needs their values, to check that neighbouring cells share edges, and
planning reads no values. cf_xarray's `bounds_to_vertices` doesn't check either: bounds
`[[0, 1], [1.5, 2], [2, 3]]` become `[0, 1.5, 2, 3]` without an error (checked). So CF
bounds are lowered as auxiliary coordinates with no grid role, and nothing is dropped.
A `Warp` that needs cell edges on such a grid states `MidpointsBetweenCentres` or is
rejected.

**Coordinate variables are placed too.** Each coordinate variable has its own
`GridPlacement`, like a data variable. Its placement says which dimensions it spans and
at which positions. That covers MOM6, where `geolon_c` spans `(yq, xq)`, and NEMO, where
every coordinate spans `(y, x)`. No coordinate rule needs to know dimension order: each
variable records its own, so a file storing `(x, y)` is fine.

Validation rules for the models:
- `Rectilinear` has a `Centre` entry on each axis, and `Curvilinear` a
  `(Centre, Centre)` entry.
- A position or location appears at most once.
- Each named variable is a numeric coordinate, placed on this grid at the entry's
  position or location. A rectilinear coordinate spans only its axis's dimension.
- An `Affine` transform is finite and non-degenerate: `dx*dy - rx*ry ≠ 0`. Each
  `IndexMap` has a nonzero step.

- **Regularity is declared** (purity rule 8). Among the horizontal models, only `Affine`
  permits arithmetic shortcuts. A rectilinear grid whose coordinates happen to be evenly
  spaced stays rectilinear. The planned DGGS kind is regular by declaration too: a
  cell's geometry follows from its ID and the system's parameters (see
  [Limits and extension plan](#limits-and-extension-plan)).
- **The affine transform is declared, never derived.** It is corner-anchored, as in
  GDAL. A host supplies it from a stored transform, such as rioxarray's `GeoTransform`
  attribute or a GeoTIFF's geotransform. Recovering it from cell-centre coordinates is
  inference, which purity rule 8 forbids, and isn't exact either: in `f64`, a centre
  minus `dx/2` fails to round-trip to the corner in 672 of 3600 cells (checked). When
  a source has both a transform and centre coordinates, checking that they agree is a
  binding check, outside planning.
- **Rotation and shear are allowed** (`rx`, `ry`), because GDAL allows them. An adapter
  for a host that can't represent them rejects the plan.
- **Axis direction is in the signs.** A south-up raster has `dy > 0`.
- **byterefs `affine` coordinates** (`start + step * i`, one per axis) stay ordinary
  coordinates for now (decision Q4). They map to an `Affine` grid only once byterefs
  records, through its own decision process, whether `start` is a centre or an edge
  ([design.md](design.md#decisions)).

## Grid equality

Operations that combine inputs ([elementwise.md](elementwise.md), [dag.md](dag.md))
require equal grid definitions. Two grids are equal when all of these are (decision F):
- the CRS: `definition` text and `axis_order`, or both absent;
- the kind, the centre dimensions and `pixel`;
- the model:
  - `Affine`: the transform bitwise and both index maps exactly;
  - `Rectilinear` and `Curvilinear`: the same entries at the same positions, with each
    pair of coordinates having the same origin by elementwise.md's coordinate rule.
    `VarId`s are per schema, so names alone don't decide it. A coordinate with no
    origin, such as a `Warp` output's, equals nothing, so two such grids are unequal.

`GridId`s are ignored. Unequal definitions that describe the same geometry, such as
the same CRS spelled differently, count as unequal. That can block a plan, never
permit a wrong one.

## Pixel interpretation

```rust
pub enum PixelInterpretation {
    /// A value covers its whole cell. GeoTIFF PixelIsArea, GDAL's default.
    Area,
    /// A value is a sample at a point. GeoTIFF PixelIsPoint.
    Point,
}
```

This records what a value means. It doesn't move the transform, which stays
corner-anchored either way, as GDAL keeps it while `GTIFF_POINT_GEO_IGNORE` is `NO`,
its default. With `YES`, GDAL reads a PixelIsPoint transform back moved by half a cell
(checked), so the GDAL adapter must not set it. Warp needs the interpretation:
resampling an area grid and resampling a point grid are different operations.

## CRS

```rust
pub struct Crs {
    /// WKT2_2019, as PROJ exports it (decision Q2).
    pub definition: String,
    pub axis_order: AxisOrder,
}

pub enum AxisOrder {
    /// x is the CRS's first axis as the authority defines it. EPSG:4326 is
    /// latitude first. GDAL's OAMS_AUTHORITY_COMPLIANT.
    Authority,
    /// x is easting or longitude, whatever the authority says.
    /// GDAL's OAMS_TRADITIONAL_GIS_ORDER; pyproj's always_xy.
    Traditional,
}
```

- **The stored form is WKT2_2019.** GDAL's and pyproj's WKT2_2019 strings for EPSG:4326
  are byte-identical, while their PROJJSON differs in whitespace (checked). Lowering
  converts other forms with PROJ before building the plan. GDAL's default
  `ExportToWkt` is WKT1, so an adapter asks for WKT2_2019 explicitly.
- The core never parses a CRS or links PROJ. Two CRSs are equal only when their text is
  equal, so equivalent CRSs spelled differently count as different. That is
  conservative: it can block a rewrite, never permit a wrong one.
- `axis_order` is explicit because latitude-first versus longitude-first is the most
  common way to get a CRS silently wrong. A GeoTIFF in EPSG:4326 reads with GDAL's axis
  mapping `[2, 1]`, which is `Traditional` (checked).
- **GCPs and RPCs are rejected** for now (decision Q3). They would be a fourth
  `GridModel` variant.

## Staggering

Staggering is in from the start. Adding it later would change the meaning of every
variable placed on a grid. Each spatial variable records its grid and where it sits on
each axis:

```rust
pub struct GridPlacement {
    pub grid: GridId,
    pub location: Location,
}

/// Mirrors GridKind: a placement's variant must match its grid's.
pub enum Location {
    Horizontal { column: AxisPlacement, row: AxisPlacement },
}

pub struct AxisPlacement {
    /// The variable's own dimension on this axis. For Centre, the grid's centre
    /// dimension. Otherwise either that or a separate one: MOM6 writes xq beside xh,
    /// while NEMO reuses x.
    pub dim: DimId,
    pub position: AxisPosition,
}

/// Positions relative to the cell centres, as in xgcm. N is the length of the grid's
/// centre dimension on that axis.
pub enum AxisPosition {
    /// N points, at the centres.
    Centre,
    /// N points, on the lower-index face of each cell.
    Left,
    /// N points, on the upper face.
    Right,
    /// N - 1 points, on the faces between centres.
    Inner,
    /// N + 1 points, on every face, including both outer edges.
    Outer,
}
```

SGRID describes the same positions with `padding`, but relative to nodes rather than
centres; xgcm counts from centres, as here.

On this scheme:
- **A grid:** everything is `Centre`.
- **B grid:** scalars are `Centre`; `u` and `v` are `Left` on both axes.
- **C grid:** `u` is `Left` on the column axis and `Centre` on the row axis; `v` is
  `Centre` on the column axis and `Left` on the row axis.

These assume the column axis is the model's "east" axis and positions count from the
lower index. NEMO puts `u` on the upper face of each T cell, so in NEMO's own indexing
`u` is `Right` on the column axis, not `Left`. MOM6's symmetric `xq` has `N + 1`
points and is `Outer`; its non-symmetric `xq` has `N` and is `Right`. The position
records what the file does. The letter alone wouldn't say which.

The Arakawa letter is derived from these positions for display. It's never stored,
because it names a pattern for a whole model, not where one variable sits.

Validation rules:
- The `Location` variant matches the grid's `GridKind` variant.
- A `Centre` placement uses the grid's own centre dimension on that axis (decision I).
- `dim` has the length its `position` implies. When either length is symbolic, this is
  shapes.md's `StaggerOffset` requirement.
- A source declaring a variable gridded must record its placement, not silently drop it.
  A `Select` can explicitly remove it and leave an ordinary variable, even when both
  dimensions remain (see [Select on a grid](#select-on-a-grid)).
  [schema-and-values.md](schema-and-values.md#spatial-state-is-not-an-optional-placement)
  distinguishes `Ungridded` from `Unresolved`.
- A variable that is placed on a grid must also span the placement's two dimensions.

**A grid with no data variable placed on it is removed** from the schema, by every
operation (decision C). Its coordinates are then `Ungridded`. [warp.md](warp.md) applies
the same rule to the source grid.

## Select on a grid

The rules apply after the selection's indexers are normalised: a slice becomes a start
`s`, a nonzero step `k` and a result length `M`.

### Affine grids

A slice `s, k` on an axis composes with that axis's index map:

```text
offset' = offset + s * step
step'   = step * k
```

Both are integers, so the result is exact and merging selections doesn't change it.
The grid stays `Placed`.

An adapter renders the effective transform from the declared one. On the column axis,
with `o` and `t` for the index map's offset and step:

| Effective | Value |
| --- | --- |
With the column map `(o_c, t_c)`, the row map `(o_r, t_r)`, and
`a_c = o_c + (1 - t_c)/2`, `a_r = o_r + (1 - t_r)/2`, the declared transform
`(x0, dx, rx, y0, ry, dy)` becomes:

| Effective | Value |
| --- | --- |
| `x0'` | `x0 + a_c*dx + a_r*rx` |
| `y0'` | `y0 + a_c*ry + a_r*dy` |
| `dx'`, `rx'` | `t_c*dx`, `t_r*rx` |
| `ry'`, `dy'` | `t_c*ry`, `t_r*dy` |

Both origin terms include both axes, so rotated and sheared grids work (checked: every
selected cell keeps its centre exactly, for `t` in 1, -1, 2, -3 and 4). rioxarray's
`rio.transform(recalc=True)` gives the same transform for `k = 1, 4, -1, -2` (checked).

- **Each selected sample keeps its centre.** Cell `o + i*t` was centred at
  `x0 + (o + i*t + 0.5)*dx`. The effective transform puts new cell `i` there. For
  `t = 1` the origin is `x0 + o*dx`. For `t = -1` it's `x0 + (o+1)*dx`, the far edge of
  the first selected cell, because the transform stays corner-anchored.
- **A stride widens the cells of an `Area` grid** (decision Q7). With `|t| > 1`, each
  selected cell stands for a cell `|t|` times as wide, centred on its sample. This
  reinterpretation is part of `Select`'s stated effect, not an approximation an adapter
  makes. The grid stays affine, so a decimated variable can still be warped. Values
  aren't changed: a stride picks samples, and averaging is `Warp`'s job.
- **With an even stride, the outer cells overhang.** With `s = 0` and `k = 4`, the first
  new cell starts 1.5 source cells before the source's edge. The extent grows by up to
  `(|k| - 1)/2` cells on each side.
- **A `Point` grid follows the same arithmetic.** Its samples keep their positions and
  only the spacing changes.
- **An empty result (`M = 0`) keeps the placement and the index map.** With no cells, the
  transform describes nothing. The stored offset still depends on the normalised start,
  so two empty selections can give unequal grids. Staggered placements are removed
  (below).

### Rectilinear and curvilinear grids

Their coordinate variables are placed on the grid, so they're sliced by the ordinary
`Select` effect, like any variable spanning the selected dimension. No grid-specific
arithmetic is needed.

### Staggered dimensions

A staggered variable or coordinate on its own dimension (MOM6's `xq`, or a bounds
dimension of length `N + 1`) isn't sliced when the centre dimension is. It keeps its
placement only when the plan selects its dimension **in step** with the centre
dimension. Otherwise its placement is removed, including when the staggered dimension
is selected and the centre dimension isn't. For a grid coordinate, the model's entry for
that position is removed with it. "In step" is judged within one `Select` node:
`Select { xh: a..b }` followed by `Select { xq: a..b+1 }` removes the placement at the
first node, so [select.md](select.md)'s pushdown rule 3 doesn't merge selections on
different axes of one grid.

For a slice of `k = ±1` on the centre dimension with `M > 0`, "in step" means selecting
the points at that position that belong to the selected cells, in the same order:

| Position | Length | In step with centres `s, s+k, …` (`M` of them) |
| --- | --- | --- |
| `Centre`, `Left`, `Right` | `N` | the same `M` indices |
| `Inner` | `N - 1` | the `M - 1` faces between the selected centres |
| `Outer` | `N + 1` | the `M + 1` edges of the selected cells |

A variable staggered on the centre dimension itself, as NEMO does, is sliced with the
centres, so for `k = ±1` it is always in step.

For `|k| > 1`, only `Centre` stays in step. `Left` and `Right` points are half a source
cell from their centres, but the widened cells' faces are `|k|/2` source cells away, so
those points no longer sit on a face. `Inner` and `Outer` points between the strided
cells aren't in the source. So every non-`Centre` placement and model entry is removed,
including those staggered on the centre dimension itself (NEMO's `u` and `glamf`). On a
rectilinear or curvilinear grid this removes the bounds, so a later `Warp` needs
`EdgeRule::MidpointsBetweenCentres`. For an evenly spaced rectilinear grid, the
midpoints are the same widened cells as on an affine one.

**An empty selection removes staggered placements** (decision J). With `M = 0`, `Inner`
would need `-1` points and `Outer` one point with no cell, so every non-`Centre`
placement and model entry is removed.

**A negative step swaps `Left` and `Right` on that axis.** Positions are relative to
increasing index. After a reversal, the face that was on a cell's lower-index side is
on its higher-index side. So for every placement and every coordinate entry on the
grid, a negative step on the column axis turns `Left` into `Right` on that axis, and
`Right` into `Left`. The same applies on the row axis. `Centre`, `Inner` and `Outer`
keep their position, with their points in reversed order.

For example, NEMO's `u` is `Right` on the column axis. Reversing the column axis makes
it `Left`. In a curvilinear model, a `(Left, Centre)` entry becomes `(Right, Centre)`.
When both entries exist, they swap.

### Other indexers

- **A scalar indexer drops its dimension,** as the contract's conventions say, and
  removes every placement on that grid. A grid with one axis left is no longer a
  `Horizontal` grid. A scalar label also drops its axis, so it gives `Ungridded` too.
- **A position list or a mask** keeps its dimension, as the conventions say, but removes
  every placement on that grid. The selected cells don't form an affine, rectilinear or
  curvilinear grid in general.
- **A computed position array or mask** (a `Select` input, [select.md](select.md)) acts
  like a position list or mask. A pointwise selection removes the placements of every
  grid with an axis indexed by an array; an axis indexed only by a slice keeps its
  transformed grid.
- **An orthogonal `IntArray`** renames the axis to the indexer's dimension
  (select.md). The grid would then name a dimension that isn't in the schema, so its
  placements are removed.
- **A non-scalar label selection on a grid's dimension** makes every placement on that
  grid `Unresolved`, including its coordinates, and removes the `Grid` (decision D).
  Label resolution turns the selection into one of the indexers above and derives the
  grid again. Until then, operations that need the grid, such as `Warp`, can't be
  validated. A label on a dimension that isn't a grid's leaves the grid unchanged.

When a placement is removed, the variables keep their data and dimensions. They are
`Ungridded` from then on, and a later `Warp` on them is invalid.

### Planned grid kinds

So the rules are known before the variants exist:

- **`Mesh`:** any selection on a mesh dimension removes the placement, because the
  connectivity would refer to nodes or faces that were dropped. Subsetting a mesh
  properly is its own operation, designed with the variant.
- **`Dggs`:** a selection on the cell dimension changes `CellIds`. Any subset of cells
  is still a valid DGGS grid, since each position names its cell.
  - A slice `s, k` of `All` gives `Range { start: s, step: k }`, and a slice of
    `Range { start, step }` gives `Range { start: start + s*step, step: step*k }`.
  - Any selection on `Listed` slices its coordinate variable, by the ordinary effect.
  - A position list or a mask on `All` or `Range` removes the placement (decision Q8).
    The selected IDs aren't stored anywhere, and listing them would mean an operation
    creates a variable that wasn't selected.
  - A scalar indexer drops the cell dimension and removes the placement.
- **`Structured`:** per axis, as for `Horizontal`.

## Other operations

- **`Project`** (decision B). An `Array` keeps only coordinates whose dimensions are in
  its payload's ([elementwise.md](elementwise.md)'s `CoordinateOutsidePayload`), as
  xarray does. If the payload is placed on a grid and spans both of the grid's centre
  dimensions, `Project::Array` keeps the grid and removes the model entries whose
  coordinates it drops (staggered entries on other dimensions). Otherwise the grid
  would refer to dimensions the `Array` lacks, which
  [schema-and-values.md](schema-and-values.md) forbids, so it is
  `ProjectError::GridCoordinateOutsidePayload` ([dag.md](dag.md)). This covers every
  grid kind, `Affine` included. MOM6's `ds["u"]` on `(yh, xq)` is rejected for now:
  xarray keeps only `geolon_u` and drops `geolon` on `(yh, xh)` and `geolon_c` on
  `(yq, xq)` (checked). `Project::Dataset` of `u` keeps the grid. A later loosening
  could instead drop the placement and give `Ungridded`.
- **`Reduce`** over a grid dimension removes every placement on that grid, as a scalar
  does, with or without `keepdims` (decision K).
- **`Rename`** of a grid dimension renames it in the grid and in every placement.
- **`Elementwise`** keeps placements by [elementwise.md](elementwise.md)'s rules:
  `GridLocationMismatch` and `GridCoordinateDropped`, with grids compared by
  [Grid equality](#grid-equality).
- **`Cast`** leaves placements unchanged ([cast.md](cast.md)).
- **`Warp`** replaces the source grid ([warp.md](warp.md)).

## Checked against more than GDAL

So that the design isn't GDAL-shaped, each concept is checked against at least one
non-GDAL implementation before it's agreed. The check is whether that implementation
can represent the concept as defined here, or says exactly why not.

| Concept | Non-GDAL reference |
| --- | --- |
| `Horizontal` grids, staggering, bounds at `Outer` | xgcm; SGRID; xESMF's `lon_b` and `lat_b` |
| `Select` on affine grids | rioxarray's `rio.transform(recalc=True)` |
| `Mesh` | UGRID; uxarray |
| `Dggs` | xdggs; healpy; h3 |

odc-geo's `GeoBox` isn't a reference for `Select`: it rejects strided and reversed
slices (`NotImplementedError: scaling not implemented`, checked). xgcm has no
selection semantics, and builds a grid from sliced, reversed or strided coordinates
without complaint, so it isn't one for staggered selection either.

A variant's code PR adds a conformance fixture for its reference, as well as for GDAL
where GDAL supports it. Checked with xarray 2026.9.0, rioxarray 0.24.0, odc-geo 0.5.3,
xgcm 0.10.1, cf_xarray 0.11.3, xdggs 0.7.0, h3 4.5.0, pyproj 3.8.0 (PROJ 9.9) and GDAL
3.13.3.

## Limits and extension plan

This design covers structured 2-D grids, which is what GDAL warps. Abstract index-space
grids, such as image or detector pixels, already fit: they have `crs: None` and an
`Affine` or `Rectilinear` model.

These don't fit yet. Until they do, their variables are ordinary dimensions and
coordinates with no grid, so they get no grid arithmetic. Each becomes a
new `GridKind` variant with a matching `Location` variant.

The sketches below are fields only, not code. They exist to check that `Grid` and
`GridPlacement` already have room for each kind, and that `Select` and `Warp` will too.
On the wire, integer fields such as `level` and `start_index` are decimal strings, like
every integer in [schema-and-values.md](schema-and-values.md)'s encoding.

```rust
pub enum GridKind {
    Horizontal(HorizontalGrid),

    /// A 2-D unstructured mesh, as UGRID defines it (ICON, MPAS, FESOM).
    Mesh(MeshGrid),

    /// A discrete global grid system, as xdggs represents it (HEALPix, H3).
    Dggs(DggsGrid),

    /// N structured axes: 3-D voxels, a vertical axis in a compound CRS, or a
    /// 1-D track or profile.
    Structured(StructuredGrid),
}

pub struct MeshGrid {
    pub node_dim: DimId,
    pub face_dim: DimId,
    pub edge_dim: Option<DimId>,
    pub node_x: VarId,
    pub node_y: VarId,
    /// UGRID face_node_connectivity: (face_dim, max nodes per face).
    pub face_nodes: VarId,
    /// UGRID start_index: whether connectivity counts nodes from 0 or 1.
    pub start_index: u8,
    /// Marks unused slots in face_nodes, for faces with fewer nodes than the maximum.
    pub face_nodes_fill: Option<Scalar>,
}

pub struct DggsGrid {
    pub cell_dim: DimId,
    /// Which cell each position along cell_dim is.
    pub cell_ids: CellIds,
    pub system: DggsSystem,
}

/// Declared like an affine transform where the IDs follow a rule, and stored only when
/// they don't.
pub enum CellIds {
    /// HEALPix only: every cell at the system's level, in index order: position i is
    /// cell i.
    All,
    /// HEALPix only: evenly spaced IDs: position i is cell start + i * step. Its length
    /// is the cell dimension's size; there's no stop. In nested indexing, a
    /// consecutive range is often a spatial patch.
    Range { start: u64, step: i64 },
    /// IDs stored in a coordinate variable on cell_dim, in any order: a regional or
    /// ocean-only subset. The only form for H3.
    Listed(VarId),
}

pub enum DggsSystem {
    Healpix { level: u8, indexing: HealpixIndexing, ellipsoid: Option<String> },
    H3 { resolution: u8 },
}

pub enum HealpixIndexing { Nested, Ring }

pub struct StructuredGrid {
    pub axes: Vec<StructuredAxis>,
    pub model: StructuredModel,   // an N-D affine, or one coordinate variable per axis
}

pub struct StructuredAxis {
    pub dim: DimId,
    pub role: AxisRole,           // e.g. horizontal x, horizontal y, vertical
}

pub enum Location {
    Horizontal { column: AxisPlacement, row: AxisPlacement },
    Mesh(MeshLocation),
    Dggs,                          // one value per cell
    Structured(Vec<AxisPlacement>),
}

pub enum MeshLocation { Node, Edge, Face }
```

`All` and `Range` are HEALPix-only (decision G). H3 IDs aren't `0..N-1`: its 122
resolution-0 cells start at 576495936675512319 and are 2^45 apart (checked), and at
finer resolutions unused digits and pentagons leave gaps. xdggs 0.7.0 also accepts the
`zuniq` and `nuniq` HEALPix schemes and an ellipsoid object, not only a name; the
sketch covers `Nested` and `Ring` only, and others are rejected until the variant is
designed.

How each fits what's already fixed:

| | `Mesh` | `Dggs` | `Structured` |
| --- | --- | --- | --- |
| Spatial reference | `Grid.crs` for node coordinates | `DggsSystem`; `crs` unset | `Grid.crs`, which may be compound |
| Staggering | `MeshLocation` | none, one value per cell | an `AxisPosition` per axis, including vertical |
| Primary location | `Face` | the cell | `Centre` on every axis |
| Pixel interpretation | faces are areas; nodes are points | cells are areas | per grid, as for `Horizontal` |

Rules for adding a variant:
- Existing variants don't change meaning. A plan valid before stays valid, with the
  same meaning.
- An adapter that doesn't support the new variant rejects it (purity rule 4).
- `Warp`'s and `Select`'s effects on the new kind are stated in the same PR, or both are
  invalid on it.
- The variant is checked against its non-GDAL reference (above).
- The variant's coordinate variables are placed on it, as for `Horizontal`.

`Structured` may later absorb `Horizontal`, as its two-axis case. That would happen only
through a versioned schema change that maps old plans onto it exactly.

## Decisions

Decided 1 November 2026 under Charles's standing rule of accepting conservative
recommendations, after an independent review. Q numbers are the earlier questions.

- **Q1. One grid that each variable refers to.** Kept; dag.md and elementwise.md now
  rely on it.
- **Q2. WKT2_2019** is the stored CRS form. Other forms are converted by lowering.
- **Q3. GCPs and RPCs** are rejected for now.
- **Q4. byterefs `affine`** stays an ordinary coordinate until byterefs decides whether
  `start` is a centre or an edge.
- **Q7. A stride keeps the grid,** widening each cell around its sample. rioxarray
  matches the arithmetic, and removing the placement instead would change output
  schemas when loosened later.
- **Q8. A position list or mask on DGGS `All` or `Range`** removes the placement.
- **A. Affine `Select` changes integer index maps,** and only adapters compute `f64`.
- **B. `Project::Array`** keeps a grid only when the payload spans its centre
  dimensions, dropping model entries outside the payload. Otherwise it rejects
  (`GridCoordinateOutsidePayload`); dropping to `Ungridded` is a possible loosening.
- **C. A grid with no data variable placed on it is removed,** for every operation.
- **D. A label on a grid's dimension** makes the grid's placements `Unresolved` and
  removes the grid until label resolution.
- **E. CF bounds** stay ordinary coordinates in the first subset.
- **F. Grid equality** is as in [Grid equality](#grid-equality).
- **G. `CellIds::All` and `Range`** are HEALPix-only.
- **H. One grid per `DimId`** within a schema.
- **I. A `Centre` placement** uses the grid's own centre dimension.
- **J. `M = 0`** removes staggered placements.
- **K. `Reduce` over a grid dimension** removes the grid's placements.

## Questions for Mike

5. Do `Centre`, `Left`, `Right`, `Inner` and `Outer` cover the ocean models you use?
   MOM6's symmetric `xq` is `Outer` and its non-symmetric `xq` is `Right`.
6. Is the "in step" table for staggered dimensions right for MOM-style files?

## Not settled

- Staggering on vertical axes (`w` at layer interfaces). The same `AxisPosition` would
  fit, but vertical axes aren't part of a grid.
- Converting CF bounds to the `Outer` form, which needs a value check outside planning.
