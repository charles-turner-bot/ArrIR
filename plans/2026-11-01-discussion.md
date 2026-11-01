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
