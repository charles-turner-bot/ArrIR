# Select and indexing modes

Status: decided 1 November 2026, for #36. Builds on #3's positional indexers, on
[dag.md](dag.md) (arity by value), [shapes.md](shapes.md) (size terms and resolution
points) and [grids.md](grids.md#select-on-a-grid) (effects on grids). Nothing here is
implemented.

## What this decides

- One `Select` node carries every indexer of one selection, plus an explicit
  **indexing mode**: orthogonal or pointwise. No mode is ever a host default.
- Indexers are literal values (scalar, slice, position list, mask, label) or
  **computed arrays**, which are extra node inputs. A computed mask's output extent is
  a `CountNonZero` size term.
- Out-of-range positions are always an error, never wrapped or clamped. Slices clamp,
  with Python semantics. Literal positions give an `InRange` shape requirement;
  computed positions are an execution-time error the engine raises.
- An orthogonal `IntArray` is one-dimensional. On a named shape its output axis takes
  the indexer's dimension name.
- Multi-dimensional masks, indexers coupled to un-indexed dimensions, Boolean arrays
  and labels in pointwise mode, and slices mixed with integer arrays on positional
  shapes are rejected for now.
- The first extension to implement after #3 is a computed one-dimensional mask in
  orthogonal mode.

## Evidence

Array API 2025.12, [indexing](https://data-apis.org/array-api/2025.12/API_specification/indexing.html),
retrieved 1 November 2026:

- An integer reduces rank by 1; a slice keeps it. Negative positions count from the end.
- Out-of-bounds integers are unspecified, and slices need not be clipped.
- Integer arrays "must be broadcast against each other", and the result is built "by
  gathering elements from `A` at each coordinate tuple". Combining slices with integer
  arrays is not addressed.
- A Boolean array must be supported only as the **sole** index. It replaces the first
  `M` dimensions with one, in row-major order. Libraries "may choose to omit" it,
  because its output shape depends on data.

xarray 2026.9.0 and NumPy 2.5.4, measured on `da(y:3, x:4)` and `da3(a:2, b:3, c:4)`:

| Expression | Result |
| --- | --- |
| `da.isel(y=[0,2], x=[1,3])` | `(y:2, x:2)`: orthogonal, values 1, 3, 9, 11 |
| `da.isel(y=P([0,2]), x=P([1,3]))`, `P` with dims `p` | `(p:2)`: pointwise, values 1, 11 |
| `nd[[0,2],[1,3]]` | `(2,)`: pointwise, values 1, 11 |
| `nd[np.ix_([0,2],[1,3])]` | `(2, 2)`: orthogonal |
| `da3.isel(b=P, c=P)` / `nd[:, [0,1], [1,3]]` | `(a, p)` / `(2, 2)`: adjacent arrays stay in place |
| `da3.isel(a=P, c=P)` / `nd[[0,1], :, [1,3]]` | `(p, b)` / `(2, 3)`: non-adjacent arrays go first |
| `da3.isel(b=P(p), c=Q(q))` | `(a, p, q)`: indexer dims broadcast by name |
| `da3.isel(a=P(p), c=Q(q))` | `(p, b, q)`: distinct 1-D indexers act orthogonally, each axis renamed in place |
| `da.isel(y=[0,2], x=P(p))` | `(y, p)`: a list keeps the indexed dim's name |
| `nd3[[0,1], :, 0]`, `nd3[0, :, [0,1]]` | `(2, 3)` both: NumPy counts integer scalars as advanced indices |
| `da3.isel(b=P(len 2), c=P(len 3))` | `IndexError`: mismatched indexer dims |
| `da.isel(x=DataArray([3,0,1], dims="y"))` | `(y:3)`, values 3, 4, 9: gathers along `y` |
| `da.isel(x=bool[4])` | `(y:3, x:2)`: orthogonal mask |
| `da3.isel(b=bool DataArray on p)` | `IndexError`: Boolean indexers must be on the indexed dim |
| `da[bool[3,4]]` | `IndexError`; `nd[bool[3,4]]` gives `(6,)` in row-major order |
| `da.isel(x=5)`, `da.isel(x=[0,9])`, `nd[[0,9]]` | `IndexError` |
| `da.isel(x=slice(1,100))` | `(y:3, x:3)`: clipped |
| `da.isel(x=[2,2,0])`, `da.isel(x=[])` | repeats kept; `(y:3, x:0)` |

## The operation

```rust
pub struct Select {
    pub indexers: Vec<AxisIndexer>,
    pub mode: IndexMode,
}

pub struct AxisIndexer {
    /// shapes.md's AxisRef: a DimId on a named shape, a position on a positional one.
    pub axis: AxisRef,
    pub index: Indexer,
}

pub enum Indexer {
    Scalar(i64),
    Slice(SliceSpec),
    /// Literal positions, in order, repeats allowed (#3).
    Positions(Vec<i64>),
    /// Literal mask; its true count is known.
    Mask(Vec<bool>),
    /// A label selection, kept logical until resolved (design.md).
    Label(LabelIndexer),
    /// Positions held by input `slot` of this node (dag.md, arity by value).
    IntArray(InputSlot),
    /// A mask held by input `slot` of this node.
    BoolArray(InputSlot),
}

/// 1 is the first input after the indexed array.
pub struct InputSlot(u32);

pub enum IndexMode {
    /// Each indexer acts on its own axis: xarray `isel` with lists, `np.ix_`.
    Orthogonal,
    /// Array indexers are broadcast together and gather coordinate tuples:
    /// Array API integer-array indexing, NumPy advanced indexing, xarray vectorised.
    Pointwise(PointwiseOptions),
}

pub struct PointwiseOptions {
    /// Output position of the broadcast indexer axes, counted from the first output
    /// axis (0-based). Lowering computes it from its host's rule (NumPy: in place if
    /// the array and integer-scalar indexers are adjacent, else first).
    pub insert_at: u32,
}
```

Rules shared by both modes:

- **One indexer per axis.** Two indexers naming the same axis are rejected
  (`DuplicateIndexer`), as #3 says.
- **Arity.** `Select` takes its array plus one input per `IntArray` or `BoolArray`.
  Every slot is used exactly once (`UnusedInput`, `DuplicateSlot`).
- **Input types.** An `IntArray` input has a signed or unsigned integer dtype, and a
  `BoolArray` input has `Bool` ([dtypes.md](dtypes.md)). An unknown dtype is a
  readiness requirement, not an error.
- **Literal values** follow [schema-and-values.md](schema-and-values.md)'s wire
  encoding: 64-bit integers are decimal strings.
- **Ellipsis and `None`** are lowering conveniences. Ellipsis expands to full slices.
  `None` is positional `expand_dims` ([array-api-coverage.md](array-api-coverage.md)),
  not a `Select`.
- **The dtype is unchanged.** Missing values, attributes of the indexed variable and
  un-indexed axes pass through.

## Shape effects

Each indexer acts on an axis with extent `e`: `Known(n)` or a size symbol `s`.

| Indexer | Output on that axis | Requirement |
| --- | --- | --- |
| `Scalar(i)` | axis removed | `InRange { min: i, max: i }` |
| `Slice(sl)` | `Known(len)` if `e` is known, else `Slice { of: s, slice: sl }` | none; clamps |
| `Positions(p)` | `Known(p.len())` | `InRange { min: min(p), max: max(p) }` |
| `Mask(m)` | `Known(count of true)` | `e == m.len()` |
| `Label(l)` | scalar label: removed; otherwise `LabelSelect { input, axis, labels }` | resolved at label resolution |
| `IntArray(k)` | input `k`'s extent (orthogonal) | `PositionOutOfRange` at execution |
| `BoolArray(k)` | `CountNonZero { mask: vn(k) }` | input `k`'s extent `== e` |

- **Requirements use shapes.md's machinery.** With known extents and literal values, a
  failure is a validation error (`IndexOutOfRange`, `MaskLengthMismatch`). With a size
  symbol it's an `InRange` or `Equal` requirement ([shapes.md](shapes.md)), checked at
  that term's resolution point.
- **Computed positions are checked by the engine.** Their values are only known at
  execution, and the planner reads no data, so shapes.md's core check doesn't cover
  them. An out-of-range computed position is the execution-time error
  `PositionOutOfRange { node, axis }`, which the engine or adapter must raise
  (§What adapters reject).
- **An orthogonal `IntArray` is one-dimensional** (`IntArrayNotOneDimensional`). On a
  named shape, its output axis takes the indexer's dimension name:
  `isel(a=P(p), c=Q(q))` gives `(p, b, q)`, as xarray does. An indexer named after the
  axis it indexes keeps that axis's `DimId`; one named after another axis of the array
  is `CoupledIndexer`. Two orthogonal indexers with the same dimension name are
  `DuplicateOutputDim`: xarray treats `isel(a=P(p), c=P(p))` as pointwise and gives
  `(p, b)`, so lowering emits pointwise mode for it. On a positional shape the output
  axis stays positional, in the same slot. A literal `Positions` keeps the indexed axis.
- **A computed mask is one-dimensional** and lies along the indexed axis. On a named
  shape its axis must be the same `DimId`. On a positional shape it is matched by
  position. Its extent must equal `e`: no stretching, no recycling.
- **A literal mask's count is known statically,** so a literal mask never produces a
  size term. Hosts that hold a mask in memory may lower it either way, as a literal or
  as a bound `Open`.
- **Empty results are valid.** `Positions([])`, an empty slice and an all-false mask
  give extent 0.

### Pointwise mode

In pointwise mode, the array indexers (`IntArray`, and `Positions` treated as a
one-dimensional array) are broadcast together by shapes.md's rules to a shape `B`. The
indexed axes are removed, and `B`'s axes are inserted at `insert_at`. Scalars and
slices on other axes act as in orthogonal mode.

- **On named shapes,** each indexer array carries its own named axes, and they combine
  by name: `P(p) + Q(q)` gives `(p, q)`, and two `p` axes must have equal extents.
  A literal `Positions` is a one-dimensional array named after the axis it indexes, so
  `isel(y=[0,2], x=P(p))` gives `B = (y, p)`. `B`'s axes are ordered as shapes.md's
  named broadcasting orders them, with the indexers taken in the array's axis order.
- **On positional shapes,** the indexer arrays broadcast by the Array API rules, which
  are shapes.md's positional rules. `B`'s axes are positional.
- **Rejected for now:**
  - `BoolArray` in pointwise mode (`BoolInPointwise`). xarray rejects it, and NumPy
    turns it into `nonzero`, which needs a traversal-order contract.
  - An indexer axis with the same name as an un-indexed axis of the array
    (`CoupledIndexer`). xarray then gathers along that axis
    (`take_along_axis`-like), which the Array API doesn't specify.
  - Slices mixed with integer arrays on a positional shape
    (`MixedSliceAndArray`), which the Array API leaves unspecified. An un-indexed
    axis counts as a full slice, as Ellipsis does, so `nd[i]` on a 2-D array is
    rejected in pointwise mode. A one-dimensional gather on one axis is orthogonal
    mode instead. On positional shapes `insert_at` is therefore always 0.
  - `Label` indexers (`LabelInPointwise`). Label resolution runs first.
- **Lowering emits orthogonal mode where it is equivalent.** A vectorised xarray
  `isel` whose array indexers are all one-dimensional with distinct dimension names is
  orthogonal: xarray renames each axis in place (`(p, b, q)` above), which no single
  `insert_at` can express. Pointwise mode is for indexers that share a dimension or are
  multi-dimensional.

### Multi-dimensional masks

The Array API's `A[B]` with an `M`-dimensional `B` flattens `M` axes in row-major
order. ArrIR has no traversal-order contract yet ([array-api-coverage.md](array-api-coverage.md),
item 4), so multi-dimensional masks are rejected (`MultiDimensionalMask`). xarray
rejects them too. Zero-dimensional masks, which add an axis, are rejected with them.

## Labels

A label `Select` stays representable without coordinates (design.md, purity rule 5).

- **A scalar label drops its axis,** so its rank effect is known.
- **Any other label indexer** gives a `LabelSelect` size term, resolved at label
  resolution (shapes.md).
- **Label resolution rewrites it** into a positional `Select` with the same mode.
- **Labels and positions mix** in one orthogonal `Select`, one axis each. Pointwise mode
  takes positions only (`LabelInPointwise`), so a label selection is resolved first.
- **Computed label arrays** (labels held by another array) aren't proposed.

## Axes and grids

- **Named axes keep their `DimId`;** positional axes keep their slot. A scalar removes
  the axis, and pointwise mode adds `B`'s axes.
- **Size symbols pass through** on un-indexed axes (shapes.md).
- **Grids follow [grids.md](grids.md#select-on-a-grid).** Slices transform the grid,
  and scalars, position lists and masks remove placements. Computed `IntArray` and
  `BoolArray` indexers act like position lists and masks: they remove every placement
  on that grid. Pointwise mode removes the placements of every grid with an axis
  indexed by an array; an axis indexed only by a slice transforms its grid as usual.
  A non-scalar label on a grid's dimension leaves that grid's placements `Unresolved`
  until label resolution; a scalar label drops the axis and gives `Ungridded`.

## Pushdown rules for #37

#37 can rely on these, under shapes.md's "rewrites keep requirements exactly":

1. **Through elementwise operations whose operands are all named.** `Select(f(a, b), I)`
   becomes `f(Select(a, I_a), Select(b, I_b))`. `I_a` keeps the indexers for the axes
   `a` has; an input without the axis gets none. The rewrite applies only when every
   operand's extent on an indexed axis is *proven* equal to the output's (the same
   size symbol or the same known value). An unresolved `Equal` would otherwise be lost:
   `(a5 + b7).isel(time=[0, 2])` raises `AlignmentError` in xarray, but the pushed form
   returns `[0, 4]`. With that proof, any indexer kind may be pushed, including
   computed masks. Pushing a mask computed from the operand it selects
   (`Select(f, BoolArray(f > 0))`) is legal but saves nothing, since `f` is still
   computed in full; #37's cost model should say so.
2. **Through elementwise operations with any positional operand,** only `Scalar` and
   `Slice`, and only when shapes.md has *proven* which operand carries the full extent. On an
   input whose axis is a stretched length 1, a slice leaves the axis alone and a scalar
   becomes `Scalar(0)`. Position lists and masks aren't pushed past a positional
   broadcast: if every input were length 1, `Positions([0, 0])` would change the
   result's length.
3. **Merging adjacent selections,** only when the merged indexer is exact for every
   length of the input. Slices compose by shapes.md's normalisation. A scalar or
   position list after a slice maps back through it only when the slice has a
   non-negative start `a`, a positive step `k` and no stop, and the positions are
   non-negative. Then `x[a::k][[p]]` is `x[[a + k·p]]`, and `p < ceil((s - a) / k)`
   holds exactly when `a + k·p < s`, so the two `InRange` requirements agree. Otherwise
   they don't merge:
   `x[-5:][[3]]` raises at length 3 but gives element 6 at length 8, and `x[0:2][[3]]`
   raises where `x[[3]]` may not. Literal masks compose. Computed indexers don't merge.
   Selections on different axes of one grid don't merge, because a staggered
   placement's "in step" status is judged per `Select` node
   ([grids.md](grids.md#staggered-dimensions)).
4. **Past `Cast` and `Project`,** for any indexer. Past a `Reduce`, only on axes the
   `Reduce` keeps.
5. **Not pushed:** label selections (they change the value number, shapes.md),
   pointwise selections, and anything across #34's label alignment until #34 says so.
   Shared consumers are kept by dag.md's path copying.

## What adapters reject

- **Modes their host can't express exactly.** An xarray adapter emits pointwise mode
  as vectorised `isel` with a transpose when `insert_at` differs from xarray's placement.
  GDAL takes positional `Scalar` and `Slice` only (gdal-adapter.md).
- **Computed integer indexers on engines that don't raise on out-of-range positions.**
  The Array API leaves this unspecified, and some libraries clamp or fill. The adapter
  rejects unless it can emit an explicit check that raises `PositionOutOfRange`.
- **Data-dependent extents** (`BoolArray`, `IntArray` of unknown length) on engines
  without them. This maps to the Array API's `capabilities()["data-dependent shapes"]`.
- **Clamped slices** on engines that don't clip out-of-range slice bounds.

## Errors

```rust
/// Validation errors, beside shapes.md's ShapeError and dag.md's PlanError.
pub enum SelectError {
    DuplicateIndexer { node: NodeId, axis: AxisRef },
    UnusedInput { node: NodeId, slot: u32 },
    DuplicateSlot { node: NodeId, slot: u32 },
    IndexOutOfRange { node: NodeId, axis: AxisRef, position: i64, extent: u64 },
    MaskLengthMismatch { node: NodeId, axis: AxisRef, mask: u64, extent: u64 },
    MultiDimensionalMask { node: NodeId, axis: AxisRef },
    /// An orthogonal IntArray input with other than one axis.
    IntArrayNotOneDimensional { node: NodeId, axis: AxisRef },
    BoolInPointwise { node: NodeId, axis: AxisRef },
    LabelInPointwise { node: NodeId, axis: AxisRef },
    MixedSliceAndArray { node: NodeId },
    CoupledIndexer { node: NodeId, dim: DimId },
    /// Two orthogonal indexers whose output axes share a dimension name.
    DuplicateOutputDim { node: NodeId, dim: DimId },
}

/// Raised by the engine or adapter at execution; the core never sees the values.
pub struct PositionOutOfRange { pub node: NodeId, pub axis: AxisRef }
```

A failed `InRange` requirement is shapes.md's `RequirementViolated::OutOfRange`.

## Worked examples

All are metadata-only. `ds` has `t(time: s, lat: 3)`, with `time` an unknown extent
declared by binding `b`, so `s = DeclaredNamed { b, time }`.

1. **Literal slice, symbolic extent.** `Select { time: 0..10 }` gives
   `time: Slice { of: s, slice: 0..10 }`, which resolves at bind.
2. **Scalar on a known axis.** `Select { lat: 5 }` is `IndexOutOfRange` at validation,
   because 5 isn't in `-3 ≤ i < 3`.
3. **Computed mask.** `m = t.lat_mean > 0` has dims `(time)`, value number `v`.
   `Select { time: BoolArray(1) }` with inputs `t, m` gives
   `(time: CountNonZero { v }, lat: 3)`, with the requirement that `m`'s `time` extent
   equals `s`. Both share `s`, so it's proven.
4. **Orthogonal arrays.** `Select { time: Positions([0, 2]), lat: Positions([1, 2]) }`,
   orthogonal, gives `(time: 2, lat: 2)`, with `InRange { extent: s, min: 0, max: 2 }`
   (`2 < s`), checked at bind. `lat`'s range is proven at validation.
5. **Pointwise, named.** Two `IntArray` inputs with dims `(p: 4)` on `time` and `lat`,
   `insert_at: 0`, give `(p: 4)`.
6. **Rejected.** `nd[:, i, j]` on a positional shape mixes a slice with integer arrays:
   `MixedSliceAndArray`. `Select { time: BoolArray(1) }` in pointwise mode is
   `BoolInPointwise`.
   `da.isel(lat=DataArray(dims="time"))` is `CoupledIndexer`.
7. **Label.** `Select { time: Label("2020") }` gives `time: LabelSelect`, resolved at
   label resolution, and leaves any grid on `time` `Unresolved`.

## Decisions

Decided 1 November 2026. The recommendations were accepted under Charles's standing rule
for conservative choices, and the review's three open points were decided with them.

1. **Explicit `insert_at`,** not a named placement rule, so no host rule is embedded in
   the IR. Decided.
2. **Multi-dimensional masks are deferred** until a traversal-order contract exists.
   Decided.
3. **Indexers coupled to un-indexed axes are rejected** (xarray's gather along `y`),
   until a `take_along_axis` design asks for them. Decided.
4. **Out-of-range positions are always an error.** Literal positions give `InRange`
   requirements; computed positions are `PositionOutOfRange`, raised by the engine or
   adapter at execution, not by the core. Decided.
5. **Boolean arrays are rejected in pointwise mode,** as xarray does. Decided.
6. **One `Select` for all axes,** since pointwise mode needs its indexers together and
   xrexpr's `Select` holds a mapping. Decided.
7. **First extension after #3:** a computed one-dimensional mask in orthogonal mode
   (`BoolArray` and `CountNonZero`). Decided.
8. **Slices mixed with integer arrays on positional shapes are rejected**
   (`MixedSliceAndArray`). Decided.
9. **An orthogonal `IntArray` is one-dimensional and its output axis takes the
   indexer's dimension name,** which reproduces xarray's `(p, b, q)`. Decided.
10. **Computed out-of-range positions are an engine duty,** not a core check. Decided.
