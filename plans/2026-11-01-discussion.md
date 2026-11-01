# 1 November 2026 discussion

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

## Elementwise (#32)

Details in [elementwise.md](elementwise.md). Proposed, pending review.

The graph, shapes and dtypes decisions left #32 a narrow job. Broadcasting, promotion,
`Constant` and strict float order were already settled, so the proposal only adds what
is specific to elementwise functions:

- **One `Elementwise` node with a `Function` enum,** named as in the Array API.
  `Greater` replaces the issue's `GreaterThan`. Slice 1 is `Add`, `Multiply` and
  `Greater`; transcendental functions, bitwise operations and shifts wait.
- **Integer arithmetic wraps.** Array API 2025.12 is silent on integer overflow, and
  NumPy 2.5.4 wraps in every case checked. Wrapping also makes integer reassociation
  exact, so integer `sum` and `prod` don't depend on order.
- **Integer division by zero gives 0,** as NumPy does. Rejecting it would reject every
  integer division, because a plan can't prove a divisor non-zero.
- **Integer `Pow` needs an exponent that can't be negative.** NumPy raises at run time
  otherwise, which a plan can't promise.
- **Missing values before #39:** NaN-as-missing propagates as NaN, comparisons give
  `false` rather than missing (as xarray does), and sentinel fill values are rejected.
- **The output schema is data-free.** Payload names follow xarray's rule. Auxiliary
  coordinates that several operands share are dropped, because the plan can't compare
  their values the way xarray does.
- **GDAL rejects elementwise nodes** until a fixture checks VRT pixel functions, as was
  done for float32 narrowing.

Evidence: NumPy 2.5.4 and xarray 2026.9.0, run 1 November 2026, for integer overflow,
division by zero, NaN comparisons, `maximum` and signed zeros, result names and
coordinate handling.
