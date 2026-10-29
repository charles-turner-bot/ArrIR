# Grids, CRS and staggering

Status: proposal. Part of [the extraction contract](design.md). The reasoning is in
[the 28 October discussion](2026-10-28-discussion.md).

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

This document fixes the shape of all of this now, before the schema (roadmap step 3) and
`Select` (step 6) are written. The code still lands in roadmap order, starting with
2-D structured grids. The tests for this design:
- Adding `Warp` later changes neither the schema nor `Select`.
- Adding the mesh, DGGS and N-D grid kinds sketched in
  [Limits and extension plan](#limits-and-extension-plan) changes neither `Grid`,
  `GridPlacement`, `Warp` nor `Select`.

This document covers the schema side: grids, their models, the CRS and staggering. The
shapes live in `arrir-types`, because the schema needs them. `Select` on grids, `Warp`
and the GDAL adapter follow in stacked PRs. The Rust is a
sketch of fields and meaning, not final code.

## Grids are schema entities

A grid is a named entity in the logical schema, alongside variables and dimensions. It
owns its dimensions, the CRS and a coordinate model. A spatial variable refers to one
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

`GridId` is a fourth portable symbol, next to dimension, variable and binding IDs.

Putting the CRS on the dataset was rejected. `gdalxarray` does this today, and when
arrays disagree it keeps the most common CRS and warns. Purity rule 4 rules that out.
Putting it on each variable was rejected too: the CRS is repeated, and two variables
can't be stated to share a grid, which warp and alignment checks need.

CF's `grid_mapping` and GDAL's per-array spatial reference both attach the CRS through a
shared reference like this one.

For now every grid is horizontal. Time, band and vertical dimensions stay ordinary
dimensions (see [Limits and extension plan](#limits-and-extension-plan)).

## Grid models

```rust
pub enum GridModel {
    /// GDAL geotransform order: (x0, dx, rx, y0, ry, dy). (x0, y0) is the outer
    /// corner of the first cell. Column i and row j map to
    /// x = x0 + i*dx + j*rx, y = y0 + i*ry + j*dy.
    Affine { transform: [f64; 6] },
    /// Irregular 1-D coordinate variables. The column axis runs along the CRS's x
    /// and the row axis along its y.
    Rectilinear { columns: Vec<AxisCoord>, rows: Vec<AxisCoord> },
    /// 2-D coordinate variables: swaths and ocean models. GDAL warps these as
    /// geolocation arrays.
    Curvilinear { coords: Vec<CurvilinearCoords> },
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
point type: NEMO's `glamt`, `glamu`, `glamv` and `glamf`, or MOM's `geolon_t` and
`geolon_c`. One coordinate pair per grid could only describe the centres, and a `u`
variable's own coordinates would lose their link to the grid. So the model holds one
entry per position.

**Bounds are coordinates at `Outer`.** The corners of every cell, `N + 1` points on each
axis, are exactly the `Outer` position. xESMF's `lon_b` and `lat_b` have that shape. So
cell bounds don't need a concept of their own. A rectilinear grid has bounds when it has
an `Outer` entry on both axes. A curvilinear grid has them when it has an
`(Outer, Outer)` entry. An affine grid always has them, by arithmetic.

CF writes bounds differently, as `(n, 2)` or `(ny, nx, 4)` arrays, one row per cell. When
neighbouring cells share their edges, as they do in a structured grid, host lowering
converts those to the `Outer` form exactly. When they don't, the host leaves bounds out,
because the `Outer` form can't hold them.

**Coordinate variables are placed too.** Each coordinate variable has its own
`GridPlacement`, like a data variable. Its placement says which dimensions it spans and
at which positions. That covers MOM, where `geolon_c` spans `(yq, xq)`, and NEMO, where
every coordinate spans `(y, x)`. No coordinate rule needs to know dimension order: each
variable records its own, so a file storing `(x, y)` is fine.

Validation rules for the models:
- `Rectilinear` has a `Centre` entry on each axis, and `Curvilinear` a
  `(Centre, Centre)` entry.
- A position or location appears at most once.
- Each named variable is a numeric coordinate, placed on this grid at the entry's
  position or location. A rectilinear coordinate spans only its axis's dimension.

- **Regularity is declared** (purity rule 8). Among the horizontal models, only `Affine`
  permits arithmetic shortcuts. A rectilinear grid whose coordinates happen to be evenly
  spaced stays rectilinear. The planned DGGS kind is regular by declaration too: a
  cell's geometry follows from its ID and the system's parameters (see
  [Limits and extension plan](#limits-and-extension-plan)).
- **The affine transform is always corner-anchored,** as in GDAL. A host whose
  coordinates are cell centres, such as xarray, converts with a half-step shift. That
  conversion is exact, so no adapter has to guess.
- **Rotation and shear are allowed** (`rx`, `ry`), because GDAL allows them. An adapter
  for a host that can't represent them rejects the plan.
- **Axis direction is in the signs.** A south-up raster has `dy > 0`.
- **byterefs `affine` coordinates** (`start + step * i`, one per axis) map to an `Affine`
  grid with no rotation. Converting them needs to know whether `start` is a centre or an
  edge (question 4).

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
corner-anchored either way, as GDAL keeps it. Warp needs it: resampling an area grid and
resampling a point grid are different operations.

## CRS

```rust
pub struct Crs {
    /// The CRS as declared by the source, stored verbatim (see question 2).
    pub definition: String,
    pub axis_order: AxisOrder,
}

pub enum AxisOrder {
    /// x is the CRS's first axis as the authority defines it. EPSG:4326 is
    /// latitude first. GDAL's OAMS_AUTHORITY_COMPLIANT.
    Authority,
    /// x is easting or longitude, whatever the authority says.
    /// GDAL's OAMS_TRADITIONAL_GIS_ORDER.
    Traditional,
}
```

- The core never parses a CRS or links PROJ. Two CRSs are equal only when their text is
  equal, so equivalent CRSs spelled differently count as different. That is
  conservative: it can block a rewrite, never permit a wrong one.
- `axis_order` is explicit because latitude-first versus longitude-first is the most
  common way to get a CRS silently wrong.

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
    /// The variable's own dimension on this axis. Either the grid's centre dimension
    /// or a separate one: MOM writes xu_ocean beside xt_ocean, while NEMO reuses x.
    pub dim: DimId,
    pub position: AxisPosition,
}

/// Positions relative to the cell centres, as in SGRID and xgcm. N is the length of the
/// grid's centre dimension on that axis.
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

On this scheme:
- **A grid:** everything is `Centre`.
- **B grid:** scalars are `Centre`; `u` and `v` are `Left` on both axes.
- **C grid:** `u` is `Left` on the column axis and `Centre` on the row axis; `v` is
  `Centre` on the column axis and `Left` on the row axis.

These assume the column axis is the model's "east" axis and positions count from the
lower index. NEMO puts `u` on the upper face of each T cell, so in NEMO's own indexing
`u` is `Right` on the column axis, not `Left`. The position records what the file
does. The letter alone wouldn't say which.

The Arakawa letter is derived from these positions for display. It's never stored,
because it names a pattern for a whole model, not where one variable sits.

Validation rules:
- The `Location` variant matches the grid's `GridKind` variant.
- `dim` has the length its `position` implies, whenever both lengths are known.
- A variable spanning both of a grid's centre dimensions, with no placement, is
  rejected. A variable that is gridded must say so.
- A variable that is placed on a grid must also span the placement's two dimensions.

## Checked against more than GDAL

So that the design isn't GDAL-shaped, each concept is checked against at least one
non-GDAL implementation before it's agreed. The check is whether that implementation
can represent the concept as defined here, or says exactly why not.

| Concept | Non-GDAL reference |
| --- | --- |
| `Horizontal` grids, staggering, bounds at `Outer` | xgcm; SGRID; xESMF's `lon_b` and `lat_b` |
| `Mesh` | UGRID; uxarray |
| `Dggs` | xdggs; healpy; h3 |

A variant's code PR adds a conformance fixture for its reference, as well as for GDAL
where GDAL supports it. The stacked PRs for `Select`, `Warp` and the GDAL adapter add
their own rows.

## Limits and extension plan

This design covers structured 2-D grids, which is what GDAL warps. Abstract index-space
grids, such as image or detector pixels, already fit: they have `crs: None` and an
`Affine` or `Rectilinear` model.

These don't fit yet. Until they do, their variables are ordinary dimensions and
coordinates with no grid, so they get no grid arithmetic. Each becomes a
new `GridKind` variant with a matching `Location` variant.

The sketches below are fields only, not code. They exist to check that `Grid` and
`GridPlacement` already have room for each kind, and that `Select` and `Warp` will too.
The stacked PRs for those two operations state each kind's effects.

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
    /// Every cell at the system's level, in index order: position i is cell i.
    All,
    /// Evenly spaced IDs: position i is cell start + i * step. A slice of `All` or
    /// `Range` gives a `Range`. In nested indexing, a consecutive range is often a
    /// spatial patch.
    Range { start: u64, step: i64 },
    /// IDs stored in a coordinate variable on cell_dim, in any order: a regional or
    /// ocean-only subset.
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

## Questions for Mike

1. A grid that each variable refers to: does that fit the datasets you work with, or
   is something else needed?
2. WKT2 or PROJJSON as the stored form of `Crs.definition`?
3. GCPs and RPCs: out of scope for now (rejected), or a fourth `GridModel` variant?
4. Is byterefs' `affine` `start` a cell centre or a cell edge? GDAL's geolocation
   arrays default to the corner (`GEOREFERENCING_CONVENTION=TOP_LEFT_CORNER`), so
   a source that means centres has to say so.
5. Do `Centre`, `Left`, `Right`, `Inner` and `Outer` cover the ocean models you use?

## Not settled

- Staggering on vertical axes (`w` at layer interfaces). The same `AxisPosition` would
  fit, but vertical axes aren't part of a grid.
- CF bounds whose neighbouring cells don't share edges. The `Outer` form can't hold
  them, so for now host lowering drops them.
