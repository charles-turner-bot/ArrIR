# Plans

This directory preserves the reasoning behind ArrIR, not just a list of features.

- [Design contract and incremental roadmap](design.md): the agreed architecture,
  source mapping, boundaries and implementation sequence.
- [Logical schemas and values](schema-and-values.md): proposed. Result kinds, typed
  values, unknown facts, missingness and the decoding boundary.
- [arrir-layout](chunked-layout.md): the storage-layout component, a Rust binding of
  the byterefs specification.
- [Grids, CRS and staggering](grids.md): proposed. How the schema represents spatial
  grids, the first step towards lowering plans to GDAL.
- [Warp](warp.md): proposed. Reprojection and regridding between grids, with
  resampling defined independently of any engine.
- [Cast](cast.md): proposed. Type conversion with rounding, clamping and missing
  values stated.
- [Dtypes](dtypes.md): decided. Promotion, Python scalars and reduction accumulators,
  with every value-changing conversion an explicit `Cast`.
- [GDAL adapter](gdal-adapter.md): proposed. Rendering plans as GDAL VRT text.
- [Plans as DAGs](dag.md): decided. Nodes with ordered inputs, stable IDs and one
  root, replacing linear unary chains.
- [Symbolic shapes](shapes.md): decided. Named and positional axes, known, symbolic
  and unknown extents, and broadcasting by name or by position, with requirements
  checked at bind, label resolution or execution.
- [Array API coverage](array-api-coverage.md): audit. How each Array API 2025.12
  function maps onto ArrIR, and which concepts are missing.
- [26 October 2026 discussion](2026-10-26-discussion.md): why the old scaffold is
  being replaced and the constraints Charles set for the rebuild.
- [27 October 2026 discussion](2026-10-27-discussion.md): reconciling #16 and #17 with
  this contract: purity rules, the dependency graph, `Open`, and the byterefs layout.
- [28 October 2026 discussion](2026-10-28-discussion.md): GDAL as a target, and grids,
  CRS and staggering in the schema.
- [31 October 2026 discussion](2026-10-31-discussion.md): why plans become DAGs, and
  the 1 November decisions on the graph, shapes and dtypes.

- [30 October 2026 discussion](2026-10-30-discussion.md): schema extension points
  and a bounded first implementation.

Update these notes when a decision changes, stating what changed and why.
Distinguish proposed designs from implemented behaviour. Passing type/unit tests
does not establish cross-host equivalence; adapter execution tests are required.
