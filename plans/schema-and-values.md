# Logical schemas and values

Status: proposed 30 October 2026, revised and decided 1 November after review (see
[Decisions](#decisions)); decided once merged. Details roadmap step 3 in
[design.md](design.md). This defines extension points, not an instruction to
implement every future variant. It builds on the decided [dag.md](dag.md),
[shapes.md](shapes.md) and [dtypes.md](dtypes.md), and links to them rather than
restating their rules.

Implemented so far, in `arrir-types`: `Schema`, `ResultKind`, `Variable`,
`VariableAxis`, roles, a flat `DType`, `TypeKnowledge`, a declared `Extent` (`Known`
and `Unknown`) and structural validation. Missing values, scalars, spatial state and
grids follow in later PRs.

## Design rule

Use an enum where alternatives have different meaning. Give an option-bearing variant
its own named struct, as for `ConservativeOptions`. Use a struct for facts that belong
together. Adding a dtype or grid kind should add a variant and its validation, not
replace the enclosing schema or turn it into an untyped options map.

Enums do not make extensions automatically compatible. Every new semantic variant
needs schema effects, wire-version rules and adapter fixtures. No `Other(String)` or
opaque JSON payload counts as portable semantics. Unknown *facts* are different from
unrecognised *variants*: the former block dependent reasoning; the latter reject import.

## One schema, different result kinds

Field sketches below describe meaning, not a frozen Rust API or wire encoding.

```rust
pub struct Schema {
    pub result: ResultKind,
    pub variables: Vec<Variable>,
    pub sizes: Vec<(DimId, Extent)>,
    pub grids: Vec<Grid>,
}
pub enum ResultKind { Dataset, Array(ArrayResult) }
pub struct ArrayResult { pub variable: VarId }
pub struct Variable {
    pub id: VarId,
    pub dims: Vec<VariableAxis>,
    pub role: VariableRole,
    pub dtype: TypeKnowledge,
    pub missing: MissingValues,
    pub spatial: SpatialState,
}
/// Named axes take their extent from `sizes`; a positional axis has no `DimId` to key
/// it there, so its extent sits on the variable.
pub enum VariableAxis { Named(DimId), Positional(Extent) }
pub enum VariableRole { Data, Coordinate(CoordinateRole) }
pub enum CoordinateRole { Index(IndexCoordinate), Auxiliary }
pub struct IndexCoordinate { pub dim: DimId }
pub enum TypeKnowledge { Known(DType), Unknown }
pub enum SpatialState { Ungridded, Placed(GridPlacement), Unresolved }
```

`Extent` is defined in [shapes.md](shapes.md#three-kinds-of-extent): `Known`, `Unknown`
and `Symbolic`. A declared (`Open`) schema may use `Known` and `Unknown` only, and
validation rejects `Symbolic` there (`SymbolInDeclaredSchema`). Derived schemas hold
`Known` and `Symbolic`.

- `Array` names one payload variable, which may itself have a coordinate role; every
  other variable is a coordinate. `Dataset` may have several data variables, or none.
  Projection changes `result`, not
  just membership. A zero-dimensional array is an array with `dims: []`, not a new
  scalar-result kind. A future table/tree result needs its own variant and semantics.
  A literal's schema is the one [dtypes.md](dtypes.md#python-scalars-and-constants)
  gives `Constant`: one zero-dimensional data variable with the fixed `VarId`
  `constant`.
- Variable order and each variable's axis order are explicit and deterministic.
  No operation sorts dimension names to make an implementation convenient, beyond the
  output-order rule an operation documents ([shapes.md](shapes.md#broadcasting)).
- A variable's axes are all `Named` or all `Positional` (shapes.md's `MixedAxisNames`).
  Its shape, as shapes.md defines it, is derived: a named axis's extent is looked up in
  `sizes`, and a positional axis carries its own.
- **Positional axes appear only in `Array` results and their inputs.** Every
  variable of a `Dataset` has named axes or none, with one exception: an `Open` of a
  binding that holds a single NumPy or Array API array declares a `Dataset` with
  exactly that one variable and no coordinates, because every `Open` declares a dataset
  ([dag.md](dag.md#open-projection-and-result-kinds)). It is projected to an `Array`
  before use. This is conservative and can be loosened later.
- **A coordinate variable's axes are named, or it has none.** In an `Array` result,
  every coordinate spans only dimensions the payload spans, as xarray requires of a
  `DataArray` (`CoordinateOutsidePayload`). A positional array therefore carries
  zero-dimensional coordinates only.
- **A variable spans each dimension at most once** (`RepeatedDimension`). xarray
  2026.9.0 allows `dims=('x', 'x')` at construction with a `UserWarning`, but `.sum('x')`
  then raises and `.isel(x=0)` drops both axes. One `sizes` entry would also force the
  two extents to be equal. Rejecting it can be loosened later.
- Dimension membership is derived from variables, as in xrexpr's `SchemaState`.
  `sizes` gives one extent for every named dimension some variable spans, and none for
  any other. It cannot create a dimension by itself. Grids cannot refer to missing
  dimensions.
- **`sizes` is in canonical order:** dimensions in order of first appearance, walking
  `variables` in order and each variable's `dims` in order. Validation rejects any other
  order, so two hosts produce the same bytes for the same schema (purity rule 2: plans
  and schemas are serialisable values).
- Coordinate membership is a role, not a dtype. An index coordinate spans exactly
  its named dimension, which must be a `Named` axis; an auxiliary coordinate may span
  several dimensions or none.
  `Index` declares an association, not sortedness, uniqueness, regularity or a loaded
  lookup table. Label resolution remains separate.
- `Known(0)` is empty, not unknown. Rank is known in the first subset; unknown
  rank/ragged axes require a later explicit contract, not a guessed empty dimension
  list.
- **ID scope is [dag.md's](dag.md#id-scope-across-opens):** `DimId` is plan-wide for
  named axes, while `VarId` and `GridId` are unique within one node's schema.
  Index-coordinate associations are unique within the schema. Coordinates and
  placements must refer to entities present in the schema.

#16's `ArraySchema` put an extent on each dimension of one array. This dataset-level
schema replaces it as the starting point for roadmap step 3. #16's `effects.rs`,
once moved into `arrir::legacy`, keeps its own `ArraySchema` unchanged; legacy code
isn't extended (design.md).

## Spatial state is not an optional placement

Reuse `GridKind`, `GridModel`, `Location` and `AxisPosition` from [grids.md](grids.md).
A schema can hold ordinary arrays and several differently gridded variables together.
`Placed` must match its grid's kind, dimensions and staggered extents. Only a variable
with `Named` axes can be `Placed`, because placements refer to axes by `DimId`
([shapes.md](shapes.md#shapes-are-ordered-axes-with-extents)).

`Ungridded` positively says that grid arithmetic does not apply. `Unresolved` says it
cannot yet be justified, for example after a label selection whose positions are not
known. Neither is an invitation to infer a grid from dimension names or coordinates.
A warp needing unresolved geometry is deferred/rejected, not treated as ungridded.

This clarifies grids.md's placement rule: a source lowering must not silently lose a
source's declared grid. However, Select may explicitly remove placement (a position
list, mask or scalar) and leave ordinary variables, as #21 specifies. A positional
list can retain both named dimensions without retaining a structured grid. Such an
output is valid `Ungridded`; an unresolved label selection is `Unresolved` instead.

New grid kinds add matching `GridKind` and `Location` variants, with the effects of
Select and Warp specified alongside them. Mesh, DGGS and N-D structured grids retain
the extension plan in grids.md; they are not implemented by this proposal.

## Dtypes and scalar values

```rust
/// Flat, so the wire tag is the dtype's name (`"Float32"`), as in dag.md's example.
pub enum DType {
    Bool,
    Int8, Int16, Int32, Int64,
    UInt8, UInt16, UInt32, UInt64,
    Float32, Float64,
}
/// Used by integer scalars, which carry their width alongside the value.
pub enum IntWidth { W8, W16, W32, W64 }
pub enum Scalar {
    Bool(bool),
    Signed(SignedScalar),
    Unsigned(UnsignedScalar),
    Float32(Float32Scalar),
    Float64(Float64Scalar),
}
pub struct SignedScalar { pub width: IntWidth, pub value: i64 }
pub struct UnsignedScalar { pub width: IntWidth, pub value: u64 }
pub struct Float32Scalar { pub bits: u32 }
pub struct Float64Scalar { pub bits: u64 }
```

Scalar types are derived from variants. Integer values must fit their declared width.
Float bits preserve width, signed zero and non-finite values without relying on JSON's
number model. Scalar storage-bit identity is not numeric equality: missing-value
matching and operation semantics must use declared value comparisons, not derived
Rust equality. NaNs are matched by `nan_is_missing`, not NaN payload equality.

DType describes logical values, never storage byte order, a codec, or GDAL's working
type. Unknown dtype is `TypeKnowledge::Unknown`, not a numeric fallback.

Future variants need their own contracts before acceptance:

| Family | Information its variant must carry |
| --- | --- |
| Complex | component float width and component-wise scalar encoding. Required by Array API 2025.12, but deferred ([dtypes.md](dtypes.md#complex-and-other-families), [coverage](array-api-coverage.md)) |
| Text | logical string semantics, distinct from fixed-width storage bytes |
| Timestamp | unit, calendar and time-zone interpretation; no implicit Gregorian conversion |
| Duration | unit and arithmetic rules, distinct from a calendar timestamp |
| Categorical | value dtype, orderedness and dictionary/binding semantics |
| Struct/list/ragged | field/element schemas and offsets or lengths; not fake dense axes |

These are extension obligations, not placeholder variants that adapters accept today.
Reject unsupported logical families at lowering/export. An adapter may keep a host-owned
barrier, but it must not rename an unsupported type `Unknown` to export it.

## Missing values and decoding

Use `MissingValues::Known { values, nan_is_missing } | Unknown` from
[warp.md](warp.md#missing-values). `Known { values: [], nan_is_missing: false }`
means no values are missing. It is not interchangeable with `Unknown`.
Sentinels must be representable in the variable's logical dtype; their numeric
comparison rules are explicit. **`values` may not contain a NaN:** NaN is missing only
through `nan_is_missing`, so validation rejects a NaN sentinel rather than leave it
meaningless. This matches how [cast.md](cast.md) and [warp.md](warp.md#missing-values)
derive `nan_is_missing`. Null bitmaps are a different missingness mechanism:
they need a new variant and propagation rules, not an invented sentinel.

**Schema always describes the values operations receive.** For an already-decoded
xarray variable, that is its decoded dtype and missingness. For a packed on-disk
variable, storage dtype, byte order, structural fill, codecs, scale/offset and CF
attributes remain source/layout metadata. Structural fill is not automatically missing.

Resolving `Open` binds values satisfying its schema. A binding adapter must declare
and check the interpretation it supplies. It cannot pass packed integers to an Open
that promises decoded floats, or silently apply host-default CF decoding. Missing
sentinels are interpreted in their declared domain, before scaling where CF requires.

First subset: already-logical bindings only. Accepting packed sources requires a
separate decode contract: ordered missing interpretation, scale/offset, output dtype,
calendar conversion and any precision loss. Future decoding modes are enum variants
with named specifications, not a `decode: bool` or arbitrary CF-options bag. This
proposal does not invent a `Decode` operation or implement decoding inside the core.

Attributes are not a generic semantics escape hatch. Inventory source fields: semantic
ones become typed facts; descriptive metadata needs an explicit preservation policy.
An adapter cannot claim full round-trip fidelity after silently dropping observable
metadata. The first adapter states its supported policy and rejects the rest.

## Validation, propagation and bindings

Separate three questions, so unknown facts do not make an invalid plan appear valid:

1. **Structural validation:** references, result kind, roles, ranks and type/range
   invariants. Errors carry the variable/dimension/grid ID and failing field.
2. **Semantic readiness:** are the facts an operation needs known? An unresolved dtype
   or grid may allow a structural plan but block a Cast, Warp or rewrite proof. Report
   an unresolved requirement separately from a contradiction; never certify a rewrite
   because no contradiction was found.
3. **Adapter acceptance:** can this engine realise the exact supported semantics?
   Rejection is not evidence that the logical plan is invalid.

Effects derive a new schema from semantic fields; callers do not author independent
"preserved" flags. Selection, reduction, projection and casting update coordinate roles,
result kind, extents, dtype, missingness and grid state as appropriate. Removing an
entity removes or invalidates references to it; it never leaves a dangling placement.
Facts not preserved by an operation become unknown or explicitly unsupported, not copied.

An Open/geometry binding declaration is a requirement, not evidence about real data.
The host checks IDs, dimensions, extents, roles, types, missingness and spatial metadata
when binding. ArrIR validates declarations offline and never fetches coordinates.
Statistics and regularity guesses cannot promote unknown facts into semantic guarantees.

## Wire contract and first proof

Use tagged enum variants and an explicit schema version inside the eventual plan wire
format. One integer rule, consistent with [dag.md](dag.md#wire-form):

- **Every integer is a canonical decimal string**, whatever its width or where it
  appears: integer `Scalar`s, `Known` extents, warp.md's `WarpTarget.sizes`, grids.md's
  `Range` start and step, slice bounds and positional indexers. JavaScript then never
  rounds one through `Number`. Float bit patterns are fixed-width hexadecimal strings.
- **The only exceptions are these `u32` structural indices**, written as JSON numbers,
  which every host reads exactly:
  - a `NodeId`, including each node's `id`, its `inputs` and the plan's `root`;
  - an axis position addressing a positional axis, such as a reduction's `axis`.
  Adding an exception means adding it to this list.

Exact tags and examples land with the first serde fixtures, before adapters.
An unknown semantic tag/version fails import. Adding a variant requires a supported
version/capability check, not an assurance that every old reader will handle it.

First PRs: schema structure/unknown facts; then numeric types/scalars/missingness;
then grid integration. Shared fixtures cover empty and scalar arrays, array versus
dataset, auxiliary coordinates, mixed grids, unknown extents/types, signed zero, NaN,
integers above 2^53 and invalid references. The next proof is Open → Select → one
bounded Reduce, round-tripped and executed through xrexpr before and after one rewrite.
Future dtype families, packed decoding and physical planning do not block that slice.

## Decisions

Decided 1 November 2026, by Charles's standing rule of accepting conservative
recommendations. Each can be loosened later without breaking existing plans.

1. **Named extents live in dataset-level `sizes`,** in canonical first-appearance
   order, and nowhere else. The alternative, shapes.md's `Axis { name, extent }` stored
   on every variable, states each extent once per variable and adds a class of
   disagreement errors. Each variable's shape is derived instead.
2. **Every integer on the wire is a decimal string,** except the enumerated `u32`
   structural indices ([above](#wire-contract-and-first-proof)).
3. **Datasets are named.** Positional axes appear only in `Array` results, and a
   coordinate variable has named axes or none. This is consistent with shapes.md's
   `MixedAxisNames`, which already forbids mixing within one shape.
4. **A source must not drop a declared placement, but Select may remove one.** This is
   the grids.md placement rule as clarified [above](#spatial-state-is-not-an-optional-placement),
   matching [grids.md's Select rules](grids.md#other-indexers).
