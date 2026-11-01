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

A fixture note, not a #40 issue: on a one-row source, `gdal.Warp` with `-r bilinear`
and a float working type, onto a grid shifted by half a pixel, returned shifted source
values rather than averages. A six-row source averaged as expected, so cast.md's
rounding fixtures use sources at least two rows tall.
