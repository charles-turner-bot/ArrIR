# 1 November 2026 discussion

The 31 October round (DAG, shapes, dtypes, Array API coverage) was decided and merged.
This round proposes the operations that build on it. Each section states whether it is decided.

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

Details in [select.md](select.md).

- **One `Select` with an explicit mode.** Orthogonal (xarray `isel` with lists,
  `np.ix_`) and pointwise (Array API integer arrays, NumPy advanced indexing, xarray
  vectorised) are both measured, on xarray 2026.9.0 and NumPy 2.5.4. Pointwise mode
  stores where its axes go (`insert_at`), so NumPy's adjacency rule stays in lowering.
- **Computed indexers are node inputs.** A computed mask's extent is shapes.md's
  `CountNonZero` term, resolved at execution. A literal mask's count is known.
- **Out-of-range positions are always an error.** The Array API leaves it unspecified,
  so an adapter for an engine that clamps or fills rejects computed positions.
- **Rejected for now:** multi-dimensional masks (no traversal-order contract),
  Boolean arrays in pointwise mode, and indexers coupled to un-indexed axes.
- **Pushdown for #37:** any indexer through elementwise operations on named shapes;
  only proven scalar and slice indexers through positional broadcasts.
