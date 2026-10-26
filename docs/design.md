# ArrIR: extraction contract

## Purpose

Extract the semantic IR that xrexpr developed to make its optimiser sound.
The same representation must support both shared rewrites and translation between
named-array implementations, initially xrexpr/xarray and xarray-ts.

```text
host calls -> host lowering -> ArrIR -> optional rewrites -> target adapter
```

ArrIR describes operations, not their execution. It contains no Python objects,
JavaScript callbacks, filesystem access, or array computation. An adapter must
faithfully implement the represented semantics or reject the translation.

## Decisions

- Start with linear unary chains, not a speculative graph.
- Extract xrexpr's **lowered** operations, not its fluent call recorder.
  `ContextOpen` belongs in host lowering; contextual reductions are semantic nodes.
- Keep one semantic algebra. Rewrites consume it, not a competing optimiser IR.
- Preserve floating-point evaluation order by default. Reassociation and other
  numerically relaxed rewrites require explicit permission. Strictness does not
  imply that unrelated execution engines already guarantee bitwise equality;
  adapters must expose or reject unsupported numerical requirements.
- Support external arrays through explicit binding IDs and declared metadata.
  A binding is not an embedded array, a Python pointer, or a download instruction.
  Importing a plan never executes or resolves it automatically.
- Unknown facts prevent unjustified rewrites; they are not zeros or empty sets.
- Preserve history. The existing root API is a legacy scaffold while the new
  `portable` module is built; do not silently bridge the two representations.
  This is a temporary migration boundary, not two permanent semantic IRs.

## What to extract

Source reference: charles-turner-1/xrexpr, main at
`76fc58073a3e91dafdc9dd79d21c4ce636f02f1e`.

| Source | Portable destination | Boundary |
| --- | --- | --- |
| `ir.py: LoweredOp` | structs + exhaustive `Operation` enum | no fluent builders |
| `Reduce.consumes`, `keepdims` | symbolic dimension set + explicit option | do not cache derived effects |
| reduction name + result-affecting options | explicit semantic reduction specification | name alone is insufficient |
| `Select.indexer`, `indexers.py` | typed positional, label and advanced indexers | preserve indexing mode and broadcast dims |
| `Scan.dims` | scan specification | preserve order-sensitive behaviour |
| `Project.variables`, `single` | projection + result kind | array versus dataset is observable |
| `Rename.mapping`, `Drop.variables` | typed relabelling/drop specifications | distinguish variable and dimension roles |
| `Rechunk.chunks`, `uniform` | named versus all-dimension chunk requests | execution policy is not value semantics |
| grouped/windowed/weighted nodes | explicit context + reduction + external inputs | structural metadata alone is insufficient |
| `schema.py: SchemaState` | logical schema | variables, ordered dims, coordinate roles, unknown extents |
| `optimize.py: dim_effect`, rules | derived analyses + rewrite module | no independently authored safety flags |
| `lower.py: _emit_node` | adapter conformance obligations | semantic changes must reach emitted calls |

Audit every raw source argument before porting a variant. Classify it as result
semantics, derived analysis, execution hint, or host-only replay payload. Examples
include `skipna`, `ddof`, `drop`, grouping boundaries, and window alignment.
Do not replace this audit with an untyped `args`/`kwargs` JSON bag.

Portable symbol IDs are strings with distinct Rust types for dimensions,
variables and external bindings. They are not coordinate values. A host with
non-string names assigns collision-free symbols and retains its name mapping.
An empty host name need not be rejected just because it is an empty string.

Label selection remains representable without loading coordinate data merely to
turn labels into positions. External bindings need schema/type requirements and
target capability checks before their operations can be emitted.

Unsupported host calls may remain barriers in a host-owned wrapper, preserving a
same-host replay payload. This does not make that payload portable. Export must
fail if unresolved host-only semantics remain in the translated segment.

## Implementation boundaries

Start with ordinary Rust modules, not a workspace of speculative crates:

- `portable`: IDs, values, schema, operations, validation and adapter contracts.
- `rewrite` (later): proofs/conditions and local transformations over those types.
- host adapters: source-specific call decoding and target-specific emission.

Add no module until it has real content. Split crates only when a consumer or
dependency boundary needs it. Use serde for the boundary, not as the definition
of semantics. Shared fixtures must pin the eventual wire contract, including
cross-language integer precision, scalar types and version compatibility.

## Small-PR roadmap

Aim for roughly 100-200 changed lines per PR. If a step does not fit, split it;
do not omit a semantic field or its tests to satisfy the line count.

1. This contract and roadmap; no replacement implementation claims.
2. Distinct dimension, variable and external-binding IDs; serde tests and CI.
3. Logical schema foundations: variable dimensions, coordinate roles, and
   known/unknown sizes, with small invariant tests.
4. Positional indexer values, then label/advanced-index and binding values in
   separate PRs. Preserve source distinctions used by rewrite proofs.
5. A deliberately bounded reduction specification with explicit result options;
   reject anything outside that supported subset.
6. Reduce + Select operation structs, linear plan and structural validation,
   split into independently understandable PRs.
7. Schema propagation, unknown-fact handling, and derived dimension effects.
8. An xrexpr adapter for this subset, first with import/export round-trip tests.
9. One existing rewrite, tested against xrexpr's rule and execution oracle.
10. xarray-ts adapter/conformance fixtures for the same subset, then broaden
    operation coverage one variant at a time. Retire the legacy scaffold once
    the replacement has a tested vertical slice; do not expand both APIs.

The first integration milestone compares original, round-tripped and rewritten
execution on real arrays. Include reduction options, negative/scalar indexes,
empty dimensions, coordinates and unknown extents. Unit tests of structs alone
do not prove a faithful extraction or cross-engine equivalence.

## Learning and review

Prefer named structs, explicit enums, ordinary `impl` blocks and readable tests.
Avoid macro-generated type families, generic adapter frameworks and clever
lifetimes until a concrete use case warrants them. Each code PR explains its
Rust concepts, one semantic decision, how to run it, and what is still missing.
The goal is a library Charles can understand and extend, not merely generated
code that compiles.
