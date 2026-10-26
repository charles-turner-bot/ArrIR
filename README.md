# ArrIR

**ArrIR** is a small, portable intermediate representation for unary array-operation
plans. It gives array libraries a common semantic core for operations such as selection,
reduction and rechunking without requiring them to share a runtime, array engine, or API.

```text
host plan ── lower adapter ──> ArrIR Plan ── emit adapter ──> target plan
```

It begins as an extraction from [xrexpr](https://github.com/charles-turner-1/xrexpr),
whose IR already separates operation semantics from xarray call recording.

## Scope

ArrIR represents a **linear plan of unary operations**. It does not evaluate data and does
not depend on Python, JavaScript, xarray, Dask, Polars, Arrow, or an execution engine.

The first release contains:

- an explicit ordered input shape of named `u64` dimensions; output shapes are derived
  from the operation sequence rather than serialised;
- `Reduce` with symbolic or named dimension sets;
- `Select` with scalar, slice, positional and mask indexers;
- `Transpose`, `Reshape`, and `Ravel` as explicit logical shape operations;
- `Rechunk` with typed chunk specifications;
- a serde wire representation plus structural and semantic validation; and
- `LowerToArrIr` / `EmitFromArrIr` traits for host-specific adapters.

An adapter must either emit an equivalent target operation or return an error. It must not
silently approximate, erase, or execute an ArrIR operation.

## Positional selection

ArrIR selection is positional only. A lowering adapter resolves coordinate labels in its
host library before constructing an ArrIR plan. The core supports independent scalar,
slice, positional-array, and boolean-mask indexers, with at most one indexer per named
dimension; it does not model coupled advanced-index broadcasting.

Slice semantics are a cross-language contract: stop is exclusive, negative bounds count
from the end, omitted bounds are clamped as in Python, and non-zero negative steps reverse
the selection. Empty slices are valid. Positional arrays preserve order and may repeat a
position.

## Shapes and validation

`Plan` stores the ordered input shape only. It does not cache intermediate or output shapes:
`infer_shape()` propagates the input through the linear operation sequence. Shapes may be
zero-dimensional, and individual dimensions may have length zero. All element-count
transformations use checked arithmetic.

Structural validation checks the wire form without reference to a particular shape. Semantic
validation then detects operations that are incompatible with the shape available at their
point in the plan. This separation lets adapters inspect a well-formed plan while reporting
meaningful errors for unknown dimensions, invalid indexes, or incompatible reshapes.

`Transpose` names an exact output dimension permutation. `Reshape` names ordered output
dimensions whose element count must match the input. `Ravel` flattens to one explicitly
named dimension.

## Rechunking

`Rechunk` is retained because existing adapters have a useful common vocabulary for chunk
requests. Its exact placement is deliberately unresolved: it lies near the boundary between
logical array transformations and physical planning. ArrIR validates its shape compatibility
but does not prescribe chunk execution or materialisation strategy.

## Portable versus host-specific operations

Not every operation is portable. A weighted xarray reduction that carries a live Python
`DataArray`, for example, cannot be faithfully emitted in JavaScript without a separate
representation for those weights. Such operations remain in an adapter-specific layer
until ArrIR has enough explicit semantics to represent them.

This is deliberate: the representation is a translation layer only for operations whose
meaning is represented by the core.

## Development

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

## Extraction roadmap

The work is intentionally divided into reviewable bites:

1. **Shape-aware core (this repository state):** portable symbols, ordered shapes, pure
   inference, selection, reductions, rechunking, and explicit shape operations.
2. **Rewrite kernel:** local, semantics-preserving selection/project/rechunk rewrites over
   the portable operations.
3. **Contextual reductions:** explicitly model grouped, windowed and weighted reductions;
   no generic `args` bag or host callback escape hatch.
4. **Adapters:** start with xrexpr/PyO3, then prove a second host implementation (for
   example JavaScript/WASM) against shared round-trip fixtures.
5. **Binary operations and schemas:** only after a concrete `merge`/`where`-style use case
   earns the move from a linear list to a graph or tree.

The sequencing is important: a host adapter should consume a stable operation algebra, not
become the place where that algebra is invented.
