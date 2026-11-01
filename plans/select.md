# Select and indexing modes

Status: proposal, 1 November 2026, for #36. Builds on #3's positional indexers, on
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
  with Python semantics.
- Multi-dimensional masks, indexers coupled to un-indexed dimensions, and Boolean
  arrays in pointwise mode are rejected for now.
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
| `da3.isel(b=P(len 2), c=P(len 3))` | `IndexError`: mismatched indexer dims |
| `da.isel(x=DataArray([3,0,1], dims="y"))` | `(y:3)`, values 3, 4, 9: gathers along `y` |
| `da.isel(x=bool[4])` | `(y:3, x:2)`: orthogonal mask |
| `da.isel(b=bool DataArray on p)` | `IndexError`: Boolean indexers must be on the indexed dim |
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
    /// Output position of the broadcast indexer axes. Lowering computes it from its
    /// host's rule (NumPy: in place if the array indexers are adjacent, else first).
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
- **Literal values** follow #26's wire encoding for 64-bit integers.
- **Ellipsis and `None`** are lowering conveniences. Ellipsis expands to full slices.
  `None` is positional `expand_dims` ([array-api-coverage.md](array-api-coverage.md)),
  not a `Select`.
- **The dtype is unchanged.** Missing values, attributes of the indexed variable and
  un-indexed axes pass through.

## Shape effects

Each indexer acts on an axis with extent `e`: `Known(n)` or a size symbol `s`.

| Indexer | Output on that axis | Requirement |
| --- | --- | --- |
| `Scalar(i)` | axis removed | `-e ≤ i < e` |
| `Slice(sl)` | `Known(len)` if `e` is known, else `Slice { of: s, slice: sl }` | none; clamps |
| `Positions(p)` | `Known(p.len())` | every `p_k` in `-e ≤ p_k < e` |
| `Mask(m)` | `Known(count of true)` | `e == m.len()` |
| `Label(l)` | scalar label: removed; otherwise `LabelSelect { input, axis, labels }` | resolved at label resolution |
| `IntArray(k)` | input `k`'s extent (orthogonal) | positions in range, at execution |
| `BoolArray(k)` | `CountNonZero { mask: vn(k) }` | input `k`'s extent `== e` |

- **Requirements use shapes.md's machinery.** With known extents and literal values, a
  failure is a validation error (`IndexOutOfRange`, `MaskLengthMismatch`). With a size
  symbol it's a requirement, checked at that term's resolution point. Positions held by
  a computed array are only known at execution.
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
- **On positional shapes,** the indexer arrays broadcast by the Array API rules, which
  are shapes.md's positional rules.
- **Rejected for now:**
  - `BoolArray` in pointwise mode (`BoolInPointwise`). xarray rejects it, and NumPy
    turns it into `nonzero`, which needs a traversal-order contract.
  - An indexer axis with the same name as an un-indexed axis of the array
    (`CoupledIndexer`). xarray then gathers along that axis
    (`take_along_axis`-like), which the Array API doesn't specify.
  - Slices mixed with integer arrays on a positional shape, which the Array API
    leaves unspecified.
- **A pointwise selection whose only array indexer is one-dimensional and named after
  the axis it indexes** equals orthogonal `IntArray`. Lowering should emit it as
  orthogonal.

### Multi-dimensional masks

The Array API's `A[B]` with an `M`-dimensional `B` flattens `M` axes in row-major
order. ArrIR has no traversal-order contract yet ([array-api-coverage.md](array-api-coverage.md),
item 5), so multi-dimensional masks are rejected (`MultiDimensionalMask`). xarray
rejects them too. Zero-dimensional masks, which add an axis, are rejected with them.

## Labels

A label `Select` stays representable without coordinates (design.md, purity rule 5).

- **A scalar label drops its axis,** so its rank effect is known.
- **Any other label indexer** gives a `LabelSelect` size term, resolved at label
  resolution (shapes.md).
- **Label resolution rewrites it** into a positional `Select` with the same mode.
- **Labels and positions mix** in one orthogonal `Select`, one axis each. Pointwise mode
  takes positions only, so a label selection is resolved first.
- **Computed label arrays** (labels held by another array) aren't proposed.

## Axes and grids

- **Named axes keep their `DimId`;** positional axes keep their slot. A scalar removes
  the axis, and pointwise mode adds `B`'s axes.
- **Size symbols pass through** on un-indexed axes (shapes.md).
- **Grids follow [grids.md](grids.md#select-on-a-grid).** Slices transform the grid,
  and scalars, position lists and masks remove placements. Computed `IntArray` and
  `BoolArray` indexers act like position lists and masks: they remove every placement
  on that grid. Pointwise mode removes the placements of every grid with an indexed
  axis. An unresolved label leaves the grid `Unresolved` (#26).

## Pushdown rules for #37

#37 can rely on these, under shapes.md's "rewrites keep requirements exactly":

1. **Through elementwise operations on named shapes.** `Select(f(a, b), I)` becomes
   `f(Select(a, I_a), Select(b, I_b))`. `I_a` keeps the indexers for the axes `a` has;
   an input without the axis gets none. Named axes never stretch, so the same indexer
   is valid on every input that has the axis. Any indexer kind may be pushed this way,
   including computed masks, because the requirement set is unchanged.
2. **Through elementwise operations on positional shapes,** only `Scalar` and `Slice`,
   and only when shapes.md has *proven* which operand carries the full extent. On an
   input whose axis is a stretched length 1, a slice leaves the axis alone and a scalar
   becomes `Scalar(0)`. Position lists and masks aren't pushed past a positional
   broadcast: if every input were length 1, `Positions([0, 0])` would change the
   result's length.
3. **Merging adjacent selections.** Slices compose by shapes.md's normalisation. A
   scalar or position list after a slice maps back through it. Literal masks compose.
   Computed indexers don't merge.
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
  rejects unless it can emit an explicit check.
- **Data-dependent extents** (`BoolArray`, `IntArray` of unknown length) on engines
  without them. This maps to the Array API's `capabilities()["data-dependent shapes"]`.
- **Clamped slices** on engines that don't clip out-of-range slice bounds.

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
   orthogonal, gives `(time: 2, lat: 2)`, with the requirement `2 < s`, checked at bind.
5. **Pointwise, named.** Two `IntArray` inputs with dims `(p: 4)` on `time` and `lat`,
   `insert_at: 0`, give `(p: 4)`.
6. **Pointwise, positional.** `nd[:, i, j]` on a positional `(5, 6, 7)` with `i` and `j`
   of shape `(2, 1)` and `(3,)` broadcasts to `(2, 3)`. The arrays are adjacent, so
   `insert_at: 1` and the result is `(5, 2, 3)`.
7. **Rejected.** `Select { time: BoolArray(1) }` in pointwise mode is `BoolInPointwise`.
   `da.isel(lat=DataArray(dims="time"))` is `CoupledIndexer`.
8. **Label.** `Select { time: Label("2020") }` gives `time: LabelSelect`, resolved at
   label resolution, and leaves any grid on `time` `Unresolved`.

## Questions for review

1. **Explicit `insert_at`, or a named placement rule** (`InPlace`/`Front`)? Recommended:
   explicit position, so no host rule is embedded in the IR.
2. **Defer multi-dimensional masks** until a traversal-order contract exists?
   Recommended: yes.
3. **Reject indexers coupled to un-indexed axes** (xarray's gather along `y`)?
   Recommended: yes, until a `take_along_axis` design asks for it.
4. **Out-of-range positions are always an error,** and adapters raise or reject?
   Recommended: yes.
5. **Reject Boolean arrays in pointwise mode?** Recommended: yes, as xarray does.
6. **One `Select` for all axes,** rather than one node per axis? Recommended: one node.
   Pointwise mode needs its indexers together, and xrexpr's `Select` holds a mapping.
7. **First extension after #3:** a computed one-dimensional mask in orthogonal mode
   (`BoolArray` and `CountNonZero`). Recommended: yes. It's the filter-by-condition use
   case and needs nothing beyond dag.md and shapes.md.
