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

**Question for Charles:** integer `FloorDivide` and `Remainder` by zero give `0`, as in
NumPy and dask, with JAX and torch adapters guarding (recommended), or both stay out of
slice 2 until a use case appears. See [elementwise.md](elementwise.md#decisions).

Evidence: NumPy 2.5.4, xarray 2026.9.0, dask 2026.8.0, JAX 0.12.0.1 and torch 2.15.0,
run 1 November 2026 on macOS arm64, for integer overflow, division by zero, float floor
division, `pow` accuracy against mpmath, NaN comparisons, `maximum` and signed zeros,
result names and coordinate handling.
