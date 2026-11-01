# 1 November: second design round

The 31 October round (DAG, shapes, dtypes, Array API coverage) was decided and merged.
This round proposes the operations that build on it. Each section is pending review.

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
