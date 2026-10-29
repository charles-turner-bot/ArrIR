# ArrIR

> **Redesign in progress:** the current root API is an experimental scaffold.
> Follow [the extraction contract and small-PR roadmap](plans/design.md) for the
> replacement being developed from xrexpr's lowered semantic IR. The old feature
> stack is not the agreed design; no compatibility guarantee is made yet.

See [`plans/`](plans/README.md) for the discussion context and design decisions.

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

- `Reduce` with symbolic or named dimension sets;
- `Select` with scalar, slice, positional and mask indexers;
- `Rechunk` with typed chunk specifications;
- a stable serde wire representation and validation; and
- `LowerToArrIr` / `EmitFromArrIr` traits for host-specific adapters.

An adapter must either emit an equivalent target operation or return an error. It must not
silently approximate, erase, or execute an ArrIR operation.

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

1. **Core vocabulary (this repository state):** portable symbols, `DimSet`, selections,
   reductions, rechunking, serde and validation.
2. **Operation effects:** pure metadata such as consumed, preserved and resized dimensions;
   enough to validate and reason about a plan without array data.
3. **Rewrite kernel:** local, semantics-preserving selection/project/rechunk rewrites over
   the portable operations.
4. **Contextual reductions:** explicitly model grouped, windowed and weighted reductions;
   no generic `args` bag or host callback escape hatch.
5. **Adapters:** start with xrexpr/PyO3, then prove a second host implementation (for
   example JavaScript/WASM) against shared round-trip fixtures.
6. **Binary operations and schemas:** only after a concrete `merge`/`where`-style use case
   earns the move from a linear list to a graph or tree.

A complementary staged map, from shapes and effects through regions, byte references,
demand-driven planning and coordinates to host adapters, is proposed in
[docs/shells.md](docs/shells.md).

The sequencing is important: a host adapter should consume a stable operation algebra, not
become the place where that algebra is invented.
