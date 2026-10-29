# Design discussion: 28 October 2026

## Inputs

- The contract as merged in #18.
- `gdalxarray`, Mike's xarray backend for GDAL. Its `warp()` returns a warped VRT as
  text, with no pixels read. Its multidimensional path attaches a CRS per array, and
  falls back to the most common one with a warning when arrays disagree.
- xrexpr's `SchemaState`, which records variables, coordinate roles and sizes, but no
  spatial reference.

## Why this comes now

The contract's only named adapters were xrexpr and xarray-ts, both xarray-shaped. If
ArrIR can only lower to those, it serialises xarray calls. Lowering to GDAL is what
lets one plan drive a read, warp and compute pipeline that GDAL and xarray both execute.

GDAL needs a spatial reference, grid arithmetic and warp. The schema (step 3) and
`Select` (step 6) are the next things written. If they're written without grids,
adding warp later changes the schema under every operation, or warp becomes a special
case. So the shapes are fixed now and the code follows the roadmap.

## Decisions

All are proposed, pending Mike's answers. The details are in [grids.md](grids.md).

- **The work is split into stacked PRs.** One document covering grids, `Select`, `Warp`
  and the GDAL adapter grew too large to review. This PR covers the grids. `Select` on
  grids and `Warp` follow, both on top of it, and the GDAL adapter follows `Warp`.
- **The CRS lives on a grid that variables refer to.** On the dataset, mixed CRSs
  can't be represented without approximating, which purity rule 4 forbids. On each
  variable, sharing a grid can't be stated.
- **Only structured 2-D grids for now, behind an enum.** `GridKind` has one variant,
  `Horizontal`, on purpose. Meshes, discrete global grids and N-dimensional grids are
  each a planned variant, so adding them is a new variant rather than a change to an
  existing one. A dedicated 2-D type was kept rather than an N-D one because the 2-D
  case is the one GDAL warps, and it's simpler to read.
- **Three grid models, declared:** affine (GDAL's geotransform), rectilinear and
  curvilinear. Only affine permits arithmetic shortcuts (purity rule 8).
- **The affine transform is corner-anchored, and the pixel interpretation is a separate
  field.** The two get confused easily. GDAL keeps the transform at the corner and
  records PixelIsArea or PixelIsPoint separately. ArrIR does the same.
- **Staggering is per variable, from the start, using SGRID and xgcm's positions.**
  The Arakawa letters were considered and rejected as the stored form. They describe a
  whole model's staggering pattern, not where one variable sits, and they don't answer
  the corner-or-centre question. The letter is derived for display.
- **The other grid kinds are sketched now, as fields only.** Meshes (UGRID), DGGS
  (HEALPix, H3) and N-D structured grids each have a sketched `GridKind` variant. The
  point is to check the door is open before any code is written. Sketching them against
  an early draft of `Warp` also found that its target sizes were a fixed `(x, y)` pair,
  and that a target defined by data (a mesh, a list of DGGS cells) had nowhere to come
  from. Those fixes go in the `Warp` PR.
- **Grid dimensions are `column` and `row`, and coordinates are kept per position.**
  Working through a curvilinear NEMO grid showed three gaps. `x_dim` and `y_dim` read
  as directions, but in a curvilinear grid they're only index axes. Staggered grids
  store separate coordinates for each point type (`glamt`, `glamu`, `glamf`), and one
  pair per grid could only hold the centres. And cell bounds had no home. Bounds turn
  out to be the coordinates at `Outer`, so they reuse staggering rather than adding a
  concept.
- **Each concept is checked against a non-GDAL implementation** before it's agreed:
  xgcm and SGRID for staggering, xESMF's corner arrays for bounds, uxarray for meshes,
  xdggs for DGGS.

## Known issues for the stacked PRs

A review of the full draft, before it was split, found these. None changes the schema
shapes proposed here. Each is fixed in the PR that designs the section it affects, and
that PR's copy of this note records how.

### Select on grids

Fixed in #21 (`design/grids-select`), in the "Select on a grid" section of grids.md:

- **Strides on `Area` grids.** The draft multiplied the affine step by the stride and
  kept the corner, so each selected cell claimed the cells skipped beside it, and the
  samples moved. #21 keeps each sample's centre and widens each cell to `k` source
  cells, which an affine transform describes.
- **Reverse slices.** For a negative stride, the new origin is the far edge of the first
  selected cell, not its original corner. #21's single origin formula covers this.
- **Scalar indexers.** The draft said they keep their dimension. The contract says a
  scalar indexer drops it. #21 drops it and removes the placement.

### Warp

Fixed in #22 (`design/warp`), in warp.md:

- **Cell edges for `Nearest` and `Mode`.** Both use containment or overlap, which need
  cell edges. Grids with centre coordinates only don't have them. #22 adds an explicit
  `EdgeRule` and `EdgeShape`.
- **Each variable needs its own output.** One resampling method and empty-cell value
  for the whole warp can't serve categorical and continuous variables together. #22
  gives each variable its own, and moves type conversion to a separate `Cast`
  operation (cast.md).
- **Validating the target geometry.** A bare binding gives validation nothing to check
  offline. #22 declares the target's schema in a `GeometryBinding`, as `Open` does.

## Questions for Mike

Listed in [grids.md](grids.md#questions-for-mike): the grid reference, WKT2 or
PROJJSON, GCPs and RPCs, byterefs `affine` anchoring, and whether the axis positions
cover the ocean models he uses.

## Not settled

- Vertical staggering.
- CF bounds whose neighbouring cells don't share edges, which the `Outer` form can't
  hold.
