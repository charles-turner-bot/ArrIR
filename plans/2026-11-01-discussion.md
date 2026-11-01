# 1 November: second design round

The 31 October round (DAG, shapes, dtypes, Array API coverage) was decided and merged.
This round proposes the operations that build on it. Each section is pending review.

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
