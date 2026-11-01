# ArrIR

A portable intermediate representation for array computations. A host library such as
xarray lowers its calls into an ArrIR plan, and an adapter turns that plan into
something another engine runs: xarray again, xarray-ts, or GDAL. Every adapter must
reproduce the plan's meaning exactly or refuse it.

```text
host calls ── lowering ──> ArrIR plan ── optional rewrites ── adapter ──> target engine
```

> **Status: design phase.** The design is largely decided and lives in
> [`plans/`](plans/README.md). The code is a workspace of shared ID types plus the
> original experimental scaffold, a linear chain of `Reduce`, `Select` and `Rechunk`, in
> `arrir::legacy`. The scaffold is being replaced and carries no compatibility guarantee.

## What ArrIR is

- **A plan is a value.** It is a graph of operations over named or positional arrays,
  serialisable and free of host objects. Building or checking it reads no data.
- **Meaning is explicit.** Dtype promotion, rounding, overflow, missing values and
  broadcasting are written on the plan, never left to whichever engine runs it.
- **Adapters don't approximate.** An adapter emits an equivalent operation or returns
  an error.

It grew out of [xrexpr](https://github.com/charles-turner-1/xrexpr), whose IR separates
operation semantics from xarray call recording so that its optimiser can be sound.

<details>
<summary><b>The nine purity rules</b></summary>

From [design.md](plans/design.md#purity-rules). Every crate, operation and PR is held to
them.

1. Planning reads no data chunks.
2. Plans, schemas and demands are serialisable values with no host objects.
3. Every operation declares its effects and its footprint.
4. No silent approximation.
5. Physical planning works in index space; labels are resolved first.
6. The core performs no I/O and has no async runtime.
7. The same plan and source give the same chunk list in the same order.
8. Regularity is declared, never inferred.
9. A chunk key always names the same encoded bytes.

</details>

## Where things stand

| Area | State | Read |
| --- | --- | --- |
| Plans as DAGs | decided | [dag.md](plans/dag.md) |
| Schemas and values | decided | [schema-and-values.md](plans/schema-and-values.md) |
| Symbolic shapes and broadcasting | decided | [shapes.md](plans/shapes.md) |
| Dtypes and promotion | decided | [dtypes.md](plans/dtypes.md) |
| Select and indexing modes | decided | [select.md](plans/select.md) |
| Cast | decided | [cast.md](plans/cast.md) |
| Elementwise operations | decided | [elementwise.md](plans/elementwise.md) |
| Grids, CRS and staggering | proposed | [grids.md](plans/grids.md) |
| Warp (regridding) | proposed | [warp.md](plans/warp.md) |
| GDAL adapter | proposed | [gdal-adapter.md](plans/gdal-adapter.md) |
| Array API 2025.12 coverage | audit | [array-api-coverage.md](plans/array-api-coverage.md) |

Implementation starts next with the DAG arena (#28), shape inference (#2), dtype
inference (#31) and the first elementwise operations (#33). The step-by-step order is
the [small-PR roadmap](plans/design.md#small-pr-roadmap).

<details>
<summary><b>What the current code contains</b></summary>

The workspace has two crates.

`crates/arrir/src/legacy/mod.rs` is the scaffold the redesign replaces:

- `Plan`, a linear list of unary operations;
- `Reduce` over named dimension sets, `Select` with scalar, slice, position and mask
  indexers, and `Rechunk` with typed chunk specifications;
- serde serialisation and basic validation;
- `LowerToArrIr` and `EmitFromArrIr` traits for adapters.

`crates/arrir-types` holds the portable ID types the replacement builds on: `DimId`,
`VarId`, `BindingId` and `GridId` (roadmap step 2).

</details>

<details>
<summary><b>Architecture</b></summary>

Three layers, generic at different levels ([design.md](plans/design.md#architecture)):

| Layer | Answers | Crate |
| --- | --- | --- |
| Logical plan | What to compute | `arrir` |
| Storage layout | What is on disk, and where | `arrir-layout`, binding the [byterefs](https://github.com/byterefs/spec) spec |
| Physical plan | Which chunks and byte ranges to read | `arrir-physical`, later |

</details>

<details>
<summary><b>Evidence and fixtures</b></summary>

Design claims about host behaviour (NumPy, xarray, dask, the Array API, GDAL) are
checked by running the host, and the version is recorded beside the claim. Scripts that
pin a behaviour live in [`fixtures/`](fixtures/gdal/README.md); #42 tracks the rest.

</details>

## Development

```bash
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

Contributors and coding agents should read [AGENTS.md](AGENTS.md) first. Design changes
update `plans/` in the same PR, and the dated discussion notes there record why each
decision was made.
