# 1 November 2026 discussion

The 31 October round (DAG, shapes, dtypes, Array API coverage) was decided and merged.
This round proposes the operations that build on it. Each section states whether it
is decided.

## Schema and values (#26)

Details in [schema-and-values.md](schema-and-values.md). The review comments on #26 were
applied against the decided docs, and the remaining questions were decided 1 November
under Charles's standing rule of accepting conservative recommendations:

- **One stored axis form.** A schema stores `VariableAxis::Named(DimId) |
  Positional(Extent)`. shapes.md's `Axis` is the view inference derives from it.
- **Named extents live in dataset-level `sizes`,** in canonical first-appearance
  order. A positional axis carries its extent on the variable.
- **Every integer on the wire is a decimal string,** except an enumerated list of `u32`
  structural indices (`NodeId`, positional axis positions).
- **Datasets are named.** Positional axes appear only in `Array` results, and a
  coordinate variable has named axes or none.
- **A source must not drop a declared placement, but Select may remove one,** as
  grids.md's Select rules say.
- `Extent` comes from shapes.md, ID scope from dag.md, and a NaN missing-value sentinel
  is rejected.

## Select and indexing modes (#36)

Details in [select.md](select.md). Decided 1 November: the seven recommendations were
accepted under the standing rule for conservative choices, and the independent review's
fixes were applied.

- **One `Select` with an explicit mode.** Orthogonal (xarray `isel` with lists,
  `np.ix_`) and pointwise (Array API integer arrays, NumPy advanced indexing, xarray
  vectorised) are both measured, on xarray 2026.9.0 and NumPy 2.5.4. Pointwise mode
  stores where its axes go (`insert_at`, from the first output axis), so NumPy's
  adjacency rule, which counts integer scalars, stays in lowering.
- **An orthogonal `IntArray` is one-dimensional,** and on a named shape its output axis
  takes the indexer's dimension name. This reproduces xarray's `(p, b, q)` for distinct
  1-D indexers, which no single `insert_at` expresses.
- **Computed indexers are node inputs.** A computed mask's extent is shapes.md's
  `CountNonZero` term, resolved at execution. A literal mask's count is known.
- **Out-of-range positions are always an error.** Literal positions give a new
  `ShapeRequirement::InRange` in shapes.md. Computed positions are
  `PositionOutOfRange`, raised by the engine or adapter, since the core reads no data.
- **Rejected for now:** multi-dimensional masks (no traversal-order contract),
  Boolean arrays and labels in pointwise mode, slices mixed with integer arrays on
  positional shapes, and indexers coupled to un-indexed axes.
- **Pushdown for #37:** any indexer through elementwise operations whose operands are
  all named, only when each indexed extent is proven equal to the output's. The review
  found that an unresolved `Equal` was otherwise dropped:
  `(a5 + b7).isel(time=[0, 2])` raises, but the pushed form returns `[0, 4]`. Only
  proven scalar and slice indexers pass positional broadcasts. Selections merge only
  when the merged indexer is exact for every length.

## Cast (#38)

Issue #38 asked for `Cast` to be reviewed against Array API `astype`, keeping it
separate from `Warp`. The revision is in [cast.md](cast.md), decided 1 November after
an independent review.

- **`Cast` takes an array or a dataset.** `CastTarget::Array` converts an array's
  payload; `CastTarget::Variables` converts listed data variables of a dataset. The
  earlier `var: VarId` assumed a dataset input, which dtypes.md flagged once `Project`
  could produce arrays. Two forms avoid inventing a dataset-rebuilding operation.
- **A conversion matrix replaces the scattered rules.** Each source and target kind
  says whether the conversion is exact, and which of `rounding` and `out_of_range` it
  needs. Nothing has a default.
- **`OutOfRange` gains `Wrap`,** for integer to integer only, because NumPy narrows
  integers modulo 2^bits (checked: 40000 becomes -25536 as `int16`). GDAL clamps, so
  its adapter rejects `Wrap`.
- **Host behaviour was checked, not assumed.** NumPy 2.5.4, xarray 2026.9.0 and
  GDAL 3.13.3 were run on the same inputs. NumPy truncates floats to integers, and its
  out-of-range results are undefined and vary with the target width; xarray's `astype`
  never converts a coordinate in place; GDAL rounds half away from zero and clamps,
  and with an integer working type rounds an interpolating kernel's result half up.
  That last one supports Mike's reading of the source in warp.md. GDAL's ordinary
  conversion writes NaN as 0 whatever the nodata value; a warp with
  `-srcnodata nan` writes the nodata value.
- **Values round, then are range-checked.** For an integer `Clamp` this matches
  gdalwarp's clamp-then-round, and it keeps `Missing`, float `Clamp` and `Infinity`
  well defined.
- **Collisions cover every inexact conversion.** `Clamp` and `Wrap` can land a valid
  value on a sentinel as rounding can, so they need an explicit `Collision` too
  (mirrored in dtypes.md).
- **Missing values stay minimal.** Only collisions and a ban on missing values in
  `bool` stay in `Cast`; the encoding questions are listed for #39.
- **Array API's `copy` and `device` aren't represented.** They concern aliasing and
  placement, not values.

Decisions, from the review's recommendations ([cast.md](cast.md#decisions)): two
`CastTarget` forms, with an `Array` payload in the `Data` role; a bare NumPy
float-to-integer `astype` rejected at lowering; `Wrap` for integer to integer only;
no coordinate casts; no `bool` with missing values; `same_value` rejected.

On a source with a grid axis of extent 1, `gdal.Warp` with `-r bilinear` or `cubic`,
onto a grid shifted by half a pixel, returns shifted source values rather than
interpolating; two or more rows interpolate as expected. The warp review (below)
confirmed this is GDAL behaviour on any extent-1 axis, not a fixture quirk, so the GDAL
adapter rejects `Interpolate` there and cast.md's rounding fixtures use sources at least
two cells along each axis.

## Elementwise (#32)

Details in [elementwise.md](elementwise.md). Decided after an independent review, under
Charles's standing rule of accepting conservative recommendations, except integer
division by zero, which is a question for Charles.

The graph, shapes and dtypes decisions left #32 a narrow job. Broadcasting, promotion,
`Constant` and strict float order were already settled, so the design only adds what
is specific to elementwise functions:

- **One `Elementwise` node with a `Function` enum,** named as in the Array API.
  `Greater` replaces the issue's `GreaterThan`. Slice 1 is `Add`, `Multiply` and
  `Greater`.
- **Integer arithmetic wraps.** Array API 2025.12 doesn't define integer overflow, and
  NumPy 2.5.4 wraps in every case checked. Reassociation is exact only when every
  operand has the same dtype, because each node wraps at its own width.
- **Float `Pow`, `FloorDivide` and `Remainder` are deferred.** NumPy's `power` isn't
  correctly rounded, and NumPy, JAX and torch disagree with Array API on float floor
  division (`1.0 // 0.1` is `9.0`, not `10`). Integer forms stay.
- **Integer `Pow` needs an exponent that can't be negative,** and lowering rejects
  NumPy's `i32 ** i32` rather than casting to float. `bool + bool` lowers to
  `LogicalOr`, as NumPy computes it.
- **Missing values before #39:** NaN-as-missing propagates as NaN, comparisons follow
  IEEE (`false`, except `NotEqual`), and sentinel fill values are rejected.
- **Coordinates by origin.** A coordinate that several operands share is kept when all
  of them derive it from one `Open` variable through equal selections on its dims.
  That keeps dag.md's `mean(a + b)` and a NEMO grid's `nav_lon`/`nav_lat`. Other
  shared auxiliary coordinates are dropped; other shared index coordinates are
  rejected until #34. Different grid locations are rejected rather than made
  `Ungridded`.
- **The payload name is ArrIR's rule, not xarray's.** Lowering inserts a `Rename`
  where xarray differs, and the adapter sets names explicitly.
- **GDAL rejects elementwise nodes** until a fixture checks VRT pixel functions, as was
  done for float32 narrowing.

**Edits to decided rows:** this PR changes two rows of dtypes.md's decided order table.
Integer `sum` and `prod` may be reordered under `Sequential`, because integers wrap at
the accumulator's width, and `min`/`max` no longer wait for #32.

**Integer division by zero:** Charles chose an explicit `ZeroDivisor` field on integer
`FloorDivide` and `Remainder`, with one variant, `Zero` (NumPy and dask), and no
default. Other hosts' rules become new variants later.

**Coordinate payloads:** `ds.x + 1` is rejected (`CoordinatePayload`). xarray allows a
DataArray named after one of its coordinates but can't put it in a Dataset; making it
representable, or adding a replace-coordinate operation for `ds.assign(x=ds.x + 1)`,
is future work (#54).

Evidence: NumPy 2.5.4, xarray 2026.9.0, dask 2026.8.0, JAX 0.12.0.1 and torch 2.15.0,
run 1 November 2026 on macOS arm64, for integer overflow, division by zero, float floor
division, `pow` accuracy against mpmath, NaN comparisons, `maximum` and signed zeros,
result names and coordinate handling.

## Warp and GDAL adapter

[warp.md](warp.md) and [gdal-adapter.md](gdal-adapter.md) were reviewed against the
decided docs and GDAL 3.13.3 (source and experiments), and are now decided. Their grid
concepts still come from [grids.md](grids.md), reviewed separately.

- **The target geometry is an input,** as dag.md decision 7 said. `WarpTarget` names,
  for each target coordinate, the geometry input's variable that supplies it.
  `GeometryRules` became `CellRules`, so "geometry" means only the input.
- **`MissingValues` lives in schema-and-values.md.** warp.md keeps only its own rules.
- **Warped floats have `nan_is_missing: true`.** Otherwise `Nearest` copies a valid
  NaN that the output then reads as missing.
- **`Interpolate` is defined exactly:** `Σ w·v / Σ w` over contributors inside the
  source, not missing and with non-zero weight; `Cubic` is Keys with a = -0.5 and
  `Lanczos` has radius 3. `CubicSpline` is removed.
- **`error_threshold` is in source cells,** as gdalwarp measures `-et`, and is a
  permission, not a bound. The GDAL adapter renders only `-et 0`.
- **What GDAL computes differently, so the adapter rejects:**
  - `Mode`, which GDAL weights by overlap;
  - non-NaN sentinels except on integer sources of 16 bits or fewer, since gdalwarp
    matches nodata within a relative `2 × FLT_EPSILON`;
  - several sentinels, or a sentinel with NaN;
  - `Interpolate` on an axis of extent 1;
  - `Lanczos`, which differs near the source edge;
  - strided and reversed slices: GDAL's decimation picks `floor((i + 0.5) k)`, and a
    negative `SrcRect` size is an error.
- **`UNIFIED_SRC_NODATA=NO` is always set.** gdalwarp's default for several bands
  treats a pixel as missing only when every band is, writing a sentinel as data.
- **A single-band `Interpolate` next to missing values renders its band twice.** With
  one band GDAL empties a cell whose centre's source cell is missing; with two bands
  and `UNIFIED_SRC_NODATA=NO` it renormalises, matching warp.md exactly (checked).
- **`bool` variables can't be warped,** since they have no legal `empty_value`.
- **The three cast.md cells "not yet checked" are verified:** integer working type
  with bilinear is `Nudge`, and ordinary int→int and int→float conversions are `Keep`.

**Question for Mike:** can a warped VRT store a destination geolocation transformer
([gdal-adapter.md](gdal-adapter.md) question 3)? Curvilinear targets are rejected until
a fixture settles it.

Evidence: GDAL 3.13.3 via `pixi exec --spec gdal`, NumPy 2.5.4, run 1 November 2026 on
macOS arm64.
