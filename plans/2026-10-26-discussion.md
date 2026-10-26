# Design discussion — 26 October 2026

## What Charles wants to build

ArrIR extracts the IR xrexpr developed out of necessity to make its optimiser
work. It should become a standalone representation that future xrexpr and other
named-array implementations, notably xarray-ts, can consume through adapters.
One semantic foundation supports both proving chain equivalences and translating
operations between implementations. Rewrites may live in a module initially;
a separate crate is an organisational decision, not a different IR.

## Why restart the scaffold

The first review found real wire-validation and empty-shape arithmetic bugs,
but that correctness pass did not establish the architecture. Comparing xrexpr's
lowered IR, interned types, schema, effects and emission exposed a bigger gap:
the scaffold represented a few operation shapes without enough result semantics
or dataset schema to faithfully extract the optimiser's working vocabulary.

Charles suggested translating the xrexpr IR types more literally into Rust
structs. The agreed approach follows its semantic variants closely, while
auditing raw call options rather than blindly copying Python replay payloads.
Adding transpose/reshape/ravel is not the first integration proof.

## Decisions and permissions

- Numerical rewrites preserve evaluation order by default; relaxation requires
  explicit permission. Do not promise cross-engine bitwise identity without
  an execution/numerics contract that the adapters can actually enforce.
- External arrays such as weights and masks use explicit bindings, not Python
  objects embedded in the IR. Binding requirements still need to be designed.
- Rebuilding the repo is authorised. Superseded PRs #7-#13 were closed without
  merging; branches and history remain available. The replacement begins at
  #14 (contract) and #15 (portable symbol IDs).
- Keep code PRs around a couple of hundred changed lines at most where practical.
  Split steps rather than hide missing semantics behind a small diff.
- Charles is learning Rust. Prefer explicit structs/enums and ordinary methods,
  with readable tests and PR explanations; avoid unnecessary macro/framework work.

## Not settled merely by this discussion

The complete reduction option vocabulary, portable label/index types, external
binding schemas, numerical capability checks and wire compatibility policy still
need concrete designs. No catch-all opaque payload makes these portable by itself.
Resolve them in small proposals backed by actual xrexpr usage and adapter tests.

The existing root API is temporarily legacy, not a second architecture to grow.
See [the roadmap](design.md#small-pr-roadmap) for the first useful vertical slice
and the point at which the legacy scaffold can be retired.
