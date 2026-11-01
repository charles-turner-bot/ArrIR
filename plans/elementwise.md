# The Elementwise family

Status: proposed 1 November 2026 for issue #32, reviewed the same day. Everything is
decided except integer division by zero, which is a question for Charles
([Decisions](#decisions)). Nothing here is implemented; #33 implements the first
slice.

Most of what an elementwise operation needs is already decided elsewhere, and this
document links to it rather than restating it:

| Concern | Decided in |
| --- | --- |
| Nodes, ordered inputs, arity from the operation value | [dag.md](dag.md) |
| Broadcasting: positional, named, mixed, and `n` operands | [shapes.md](shapes.md#broadcasting) |
| Promotion, mixed-kind rejection, result dtypes, `Constant` | [dtypes.md](dtypes.md) |
| Strict floating-point evaluation order | [design.md](design.md#decisions) |

This document decides the rest: the operation and its variants, operand domains,
integer overflow and division by zero, non-finite values, the minimum missing-value
rule before #39, the output schema, what rewrites may do, and what adapters must do.

References:

- Array API 2025.12, the
  [elementwise functions](https://data-apis.org/array-api/2025.12/API_specification/elementwise_functions.html)
  and their function pages, retrieved 1 November 2026.
- NumPy 2.5.4, xarray 2026.9.0, dask 2026.8.0, JAX 0.12.0.1 and torch 2.15.0, run 1
  November 2026 on macOS arm64, for every behaviour marked "checked" below.

## One operation, an enum of functions

```rust
pub struct Elementwise {
    pub function: Function,
}

pub enum Function {
    // Slice 1 (#33)
    Add,
    Multiply,
    Greater,
    // Slice 2: arithmetic
    Subtract,
    Negative,
    Abs,
    Square,
    Divide,
    FloorDivide { zero_divisor: ZeroDivisor }, // integers only
    Remainder { zero_divisor: ZeroDivisor },   // integers only
    Pow,         // integers only
    Maximum,
    Minimum,
    // Slice 2: comparison
    Equal,
    NotEqual,
    GreaterEqual,
    Less,
    LessEqual,
    // Slice 2: logical and selection
    LogicalAnd,
    LogicalOr,
    LogicalXor,
    LogicalNot,
    Where,
    // Slice 2: exact classification and rounding
    IsNan,
    IsInf,
    IsFinite,
    SignBit,
    Sign,
    Ceil,
    Floor,
    Trunc,
    Round,
}
```

- **One node per call.** `Elementwise { function: Add }` with inputs `[a, b]` is
  `a + b`. Operands are the node's ordered inputs. Order is meaningful: it fixes the
  output's dimension order (shapes.md) and which operand is the divisor.
- **Arity comes from the function:** `Exactly(1)` for the unary functions,
  `Exactly(2)` for the binary ones, `Exactly(3)` for `Where`. No function takes
  options yet. One that needs them later holds a named struct, as design.md requires.
- **Names follow the Array API.** #32 and #33 call the comparison `GreaterThan`; here it
  is `Greater`, so each variant maps to one Array API function by name (decision 6).
- **Slices are for implementation, not meaning.** This document fixes the meaning of
  every variant above. #33 implements slice 1, and slice 2 follows one variant group at
  a time. Each variant maps to a row of [array-api-coverage.md](array-api-coverage.md),
  which keeps its classification.
- **Not yet:**
  - Transcendental functions (`exp`, `log`, `sin`, …) and float `Pow`. Engines don't
    promise correctly rounded results: Array API calls `pow` "an
    implementation-dependent approximation", and NumPy's float64 `power` differs from
    the correctly rounded result in 27 of 20,000 random cases (checked against
    mpmath). They need an accuracy contract, written as a named permission
    ([dtypes.md](dtypes.md#decisions), decision 5).
  - Float `FloorDivide` and `Remainder`. Array API prefers `floor(divide(x1, x2))`, so
    `1.0 // 0.1` is `10`, but NumPy, JAX and torch give `9.0` (checked). For
    `inf // 2.0` Array API gives `+inf` and NumPy gives NaN (checked). A fixture has to
    pin the definition first (decision 9).
  - Bitwise operations and shifts, which need shift-count rules.
  - These wait for a use case. `clip` stays a composition of `Maximum` and `Minimum`.

**The defining property:** an elementwise operation is pointwise. The output element at
a broadcast index depends only on the operands' elements at that index. Its footprint
(design.md's purity rule 3) is that element; on a stretched axis the operand's index
is 0. Selection pushdown (#37) relies on this.

## Operand domains

Promotion first combines the operands' dtypes ([dtypes.md](dtypes.md#promotion-table));
the result dtype is then [dtypes.md's table](dtypes.md#result-dtypes-by-operation).
This table says which promoted dtypes each function accepts:

| Functions | Accepted promoted dtype | Result |
| --- | --- | --- |
| `Add`, `Subtract`, `Multiply`, `Negative`, `Abs`, `Square`, `Maximum`, `Minimum`, `Sign` | integer or float | promoted |
| `FloorDivide`, `Remainder` | integer | promoted |
| `Pow` | integer, with the exponent condition below | promoted |
| `Divide` | float | promoted |
| `Equal`, `NotEqual` | any | `bool` |
| `Greater`, `GreaterEqual`, `Less`, `LessEqual` | integer or float | `bool` |
| `LogicalAnd`, `LogicalOr`, `LogicalXor`, `LogicalNot` | `bool` | `bool` |
| `IsNan`, `IsInf`, `IsFinite`, `SignBit` | float | `bool` |
| `Ceil`, `Floor`, `Trunc`, `Round` | integer or float (identity on integers) | promoted |
| `Where(condition, x, y)` | `condition` is `bool`; `x` and `y` promote together | promoted `x`, `y` |

- `bool` is rejected wherever "integer or float" is required, as Array API specifies
  (`OperandDomain`). NumPy and xarray compute `bool + bool` as logical OR and
  `bool * bool` as logical AND, and raise `TypeError` for `bool - bool` (checked), so
  lowering writes `LogicalOr` and `LogicalAnd` for them. A cast to an integer would
  change the host's dtype and values.
- Logical functions take `bool` only. NumPy accepts integers there (checked:
  `logical_and(int8 2, int8 0)` is `False`); lowering inserts a comparison with zero.
- **Integer `Pow`** needs an exponent that can't be negative: an unsigned dtype, or a
  `Constant` that is at least 0. Otherwise it's `IntegerPowSignedExponent`. NumPy raises
  at run time for a negative integer exponent (checked), JAX returns 0 (checked), and
  Array API leaves it unspecified. A plan can't promise either, so lowering rejects
  NumPy's `i32 ** i32` unless the exponent is provably non-negative. Casting to float
  first would change the host's dtype and values.
- An unknown dtype gives an unknown result or an unresolved requirement, never a guess
  ([dtypes.md](dtypes.md#unknown-dtypes)). Comparisons still return `bool`.

## Integer overflow wraps

**Integer arithmetic wraps, modulo 2^n, in two's complement.** `int8 127 + 1` is
`-128`. This covers `Add`, `Subtract`, `Multiply`, `Negative`, `Abs`, `Square`, `Pow`
and `FloorDivide`: `abs(int8 -128)` and `int8 -128 // -1` are both `-128`.

- Array API 2025.12 doesn't define integer overflow (checked on the elementwise, `add`,
  data types and array object pages). Its `abs` page says the absolute value of the
  minimum representable integer is "implementation-dependent". NumPy wraps for every
  case above (checked).
- It is the semantics of the variants, not a per-node field. A saturating or trapping
  operation would be a different function (`AddSaturating`), as in Rust, added when a
  case needs it (decision 1).
- Wrapping is exact modular arithmetic at one width. Integer `Add` and `Multiply` are
  therefore associative and commutative when every operand and intermediate has the
  same dtype, and integer `sum` and `prod`, which have one accumulator, don't depend on
  order. That answers [dtypes.md](dtypes.md#rules)'s rule 4. Across widths the order
  matters, because each node wraps at its own width: with `a = i8 127`, `b = i8 1` and
  `c = i16 0`, `(a + b) + c` is `-128` and `a + (b + c)` is `128` (checked).

## Division and other undefined integer cases

| Case | Array API 2025.12 | NumPy 2.5.4 (checked) | ArrIR |
| --- | --- | --- | --- |
| integer `FloorDivide` by 0 | "unspecified and thus implementation-defined" | `0` | `zero_divisor` (decision 2) |
| integer `Remainder` by 0 | implementation-defined | `0` | `zero_divisor` (decision 2) |
| integer `FloorDivide` rounding | toward −∞ | `-7 // 2` is `-4` | toward −∞ |
| integer `Remainder` sign | the divisor's | `-7 % 2` is `1` | the divisor's |
| `INT_MIN // -1` | not stated | wraps to `INT_MIN` | wraps |
| integer `Pow` with a negative exponent | unspecified | `ValueError` | rejected in the plan (above) |

The result of a zero divisor is stated on the node rather than fixed by ArrIR. A plan
can't prove a divisor non-zero, so rejecting would reject every integer division, and
a single fixed definition couldn't change once plans relied on it:

```rust
/// What integer FloorDivide and Remainder give for a zero divisor.
pub enum ZeroDivisor {
    /// x // 0 and x % 0 are 0, as in NumPy and dask (checked).
    Zero,
}
```

There is no default. Lowering writes its host's rule; NumPy and dask lower to `Zero`. A
host with another rule is rejected at lowering until a variant for it is added (for
example `EngineError`, raised at execution like select.md's `PositionOutOfRange`, for
torch). Adding a variant never changes what an existing plan means. Other engines
differ: JAX gives `[-2, -2, -1]` for
`[7, -7, 0] // 0` and torch raises `ZeroDivisionError` (checked), and `INT_MIN // -1`
traps in C and in x86's `idiv`. In an eager engine, `Where(d == 0, 0, x // d)` still
evaluates `x // 0`, so the guard must replace the divisor too:

```text
Where(d == 0, 0, x // Where(d == 0, 1, d))
```

with the same treatment for `INT_MIN // -1` (`INT_MIN % -1` is `0` in NumPy, checked,
and also traps in C). An engine that can't produce `Zero` this way rejects it.

## Floating point

`float32` and `float64` follow IEEE 754 binary32 and binary64:

- **Basic arithmetic is correctly rounded,** to nearest with ties to even: `Add`,
  `Subtract`, `Multiply`, `Divide`, `Square`. Subnormals are kept; an engine that
  flushes them to zero rejects.
- **No fused multiply-add.** `a * b + c` is two nodes, and two roundings. An engine
  that fuses them changes the result and needs explicit permission, under design.md's
  strict-order rule.
- **Division by zero** follows the Array API special cases: a positive dividend over
  `+0` is `+inf`, over `-0` is `-inf`; `±0 / ±0` is NaN.
- **NaN propagates** through arithmetic. Comparisons with NaN are `false`, except
  `NotEqual`, which is `true` (checked). NaN payloads aren't significant
  ([schema-and-values.md](schema-and-values.md#dtypes-and-scalar-values)), so which
  operand's payload survives doesn't matter.
- **`Maximum` and `Minimum` propagate NaN**, as Array API requires ("If either `x1_i`
  or `x2_i` is `NaN`, the result is `NaN`"). Array API leaves the order of signed zeros
  "implementation-defined"; ArrIR orders `-0` below `+0`, as IEEE 754-2019's `maximum`
  and `minimum` do. On arm64, NumPy's `maximum` returns `+0` and `minimum` returns
  `-0` for `±0` in either order, at sizes 1 to 64, and JAX's `maximum` matches
  (checked). x86_64 SIMD paths are unchecked: the #42 fixture must run there before
  the NumPy adapter emits `Maximum` or `Minimum` on floats. NumPy's NaN-ignoring `fmax`
  and `fmin` are not these functions.
- **`Sign(±0)` is `+0`.** Array API says "0" without a sign; NumPy returns `+0` for
  `-0.0` (checked: `signbit` is `False`).
- **`Round`** rounds half to even, as Array API requires. It isn't `Cast`'s `Rounding`.

## Missing values before #39

#39 will define how missing values propagate. Until then, elementwise operations
accept only what has an unambiguous meaning:

- **No sentinels.** An operand whose `missing` is `Known` with any value in `values`
  (a fill value such as `-9999`) is `UnsupportedMissing`. Arithmetic on a sentinel
  treats it as a number, and the plan can't say what the result's sentinel would be.
- **NaN as missing propagates as NaN.** IEEE arithmetic already carries NaN through,
  which is exactly xarray's missing-value behaviour for arithmetic. Float operands that
  may contain NaN must agree on `nan_is_missing`; otherwise it's `MixedNanMissing`. A
  `Constant` that isn't NaN, and integer and `bool` operands, can't contain NaN, so they
  don't take part.
- **A float result** has `missing: Known { values: [], nan_is_missing }` with the
  operands' shared flag (`false` if no operand can contain NaN).
- **A `bool` result** has `missing: Known { values: [], nan_is_missing: false }`. A
  comparison with a missing value follows IEEE comparison, not missingness: `false`,
  except `NotEqual`, which is `true` (checked in xarray: for `[nan, 1]`, `> 0` is
  `[False, True]` and `!= 0` is `[True, True]`). `IsNan` tests for NaN, not for
  missingness.
- **`Unknown` missing values** give an `Unknown` result. The arithmetic is still
  defined; only its missing-value interpretation isn't. This is asymmetric: an
  `Unknown` operand may hide a sentinel that a `Known` one would be rejected for. That
  is accepted, because the result stays `Unknown` rather than claiming anything
  (decision 8).

## The output schema

Each operand must be an array: `ResultKind::Array`, or a `Constant`. An array operand's
payload must have the `Data` role. A coordinate payload (`ds.x + 1`) is
`CoordinatePayload`: xarray gives a payload `x` of `[11, 21, 31]` beside a coordinate
`x` of `[10, 20, 30]` (checked), and the two would share one `VarId`. cast.md rejects
the `Cast` for `ds.x + 1.5` for the same reason. xarray can't put that DataArray in a
Dataset either (`to_dataset` raises). A dataset operand
is `DatasetOperand`: xarray applies dataset arithmetic per variable, which would need a
`Merge` that doesn't exist yet (decision 5). A binding that holds a single NumPy or
Array API array is an `Open` of a one-variable dataset, which is projected to an
`Array` before use ([schema-and-values.md](schema-and-values.md#one-schema-different-result-kinds)),
so it reaches an elementwise node as `Project::Array`.

- **Shape:** [shapes.md](shapes.md#broadcasting), with operands in input order.
- **Dtype:** promotion, then the domain table above.
- **Payload variable name.** If every operand that isn't a `Constant` has the same
  payload `VarId`, the output keeps it. Otherwise it is the fixed `VarId` `result`,
  which lowering reserves: a host variable named `result` is renamed on the way in.
  This is ArrIR's rule, not xarray's: xarray ignores NumPy operands (`t + nd` is named
  `t`), takes `xr.where`'s name from the condition, and drops the name for an unnamed
  0-d operand (checked). The xarray lowering inserts a `Rename` where the host's name
  differs, and the adapter sets `.name` from the plan explicitly (decision 7).
- **Coordinates.** Data-free, so they can differ from xarray's rule, which compares
  values. The rule uses each coordinate's *origin*: the `Open` node and `VarId` it
  comes from, plus the `Select`s applied to its dims on the way, in order. An
  operation that doesn't touch a coordinate's dims (`Project`, `Elementwise`, a
  `Select` on other dims, …) keeps its origin. A `Select` on its dims extends it. Any
  other operation that changes the coordinate (`Warp`, for example) leaves it with no
  origin. Origins are derived, never stored. Two operands' coordinates under one
  `VarId` are *the same* when their origins are equal: the same `Open` and `VarId`,
  and equal selections, compared as literal indexers or, for computed ones, by
  [shapes.md](shapes.md)'s value numbering. Their values are then identical without
  reading them. Selecting both operands the same way keeps them the same, so
  selection pushdown (#37) keeps a valid plan valid.
  - A coordinate that only one operand has is kept, along with its dims. An `Array`'s
    coordinates use only its payload's dims, as xarray requires of a `DataArray`, so
    those dims are in the output. schema-and-values.md doesn't state this yet, so an
    operand that breaks it is `CoordinateOutsidePayload`.
  - A coordinate that several operands share and that is the same in all of them is
    kept. This covers [dag.md](dag.md#worked-examples)'s `mean(a + b)` and
    `Add(shared, shared)`, and `thetao + so` from one NEMO dataset keeping `nav_lon`
    and `nav_lat` (checked: xarray keeps them).
  - An auxiliary coordinate that several operands share but that isn't the same is
    dropped. xarray drops it when the values conflict and keeps it when they're equal
    (checked); the plan can't see values, so it drops, and the xarray adapter emits the
    drop (decision 3).
  - An index coordinate that several operands share but that isn't the same is
    `SharedIndexCoordinate` until #34 establishes label equality (decision 4).
  - Selections are compared after merging adjacent ones into one canonical form, as
    [select.md](select.md)'s pushdown rule 3 allows, so `x[0:20][0:10]` and `x[0:10]`
    give the same origin. Merging the selections of one operand only, as #37 may, then
    can't change the output schema.
- **Grids.** The output's `SpatialState` is `Unresolved` if any operand's is. Placed
  operands must have equal grid definitions ([grids.md](grids.md#grid-equality));
  unequal ones are rejected, as
  [dag.md](dag.md#id-scope-across-opens) requires. Equal definitions at one location give
  `Placed` on the first placed operand's grid. Different locations are
  `GridLocationMismatch` (decision 10): only `Select` drops a placement
  ([schema-and-values.md](schema-and-values.md#spatial-state-is-not-an-optional-placement)).
  A `Placed` operand with an `Ungridded` one gives `Placed`: the operation is
  pointwise, so it keeps the placed operand's geometry. A kept grid whose model names
  a coordinate that the rules above drop ([grids.md](grids.md)'s `Rectilinear` and
  `Curvilinear` models do) is `GridCoordinateDropped`. `Constant`s have no dims and
  don't take part.
- **Name collisions.** If the payload `VarId` equals a kept coordinate's, it's
  `PayloadNameCollision`.

```rust
pub enum ElementwiseError {
    OperandDomain { node: NodeId, input: usize, function: Function, dtype: DType },
    IntegerPowSignedExponent { node: NodeId },
    DatasetOperand { node: NodeId, input: usize },
    CoordinatePayload { node: NodeId, input: usize },
    UnsupportedMissing { node: NodeId, input: usize },
    MixedNanMissing { node: NodeId },
    PayloadNameCollision { node: NodeId, variable: VarId },
    CoordinateOutsidePayload { node: NodeId, input: usize, variable: VarId },
    SharedIndexCoordinate { node: NodeId, dim: DimId },
    GridLocationMismatch { node: NodeId },
    GridCoordinateDropped { node: NodeId, variable: VarId },
}
```

`ShapeError` ([shapes.md](shapes.md#errors)) and `PromotionError`
([dtypes.md](dtypes.md#promotion-table)) report the rest.

## What rewrites may do

- **Selection pushdown** is valid because the operation is pointwise. The shape
  conditions are shapes.md's; the proof is #37's.
- **Commuting** `Add`, `Multiply`, `Equal`, `NotEqual`, `Maximum`, `Minimum` and the
  binary logical functions is exact in IEEE arithmetic, up to NaN payloads, and exact
  for wrapping integers. Swapping named operands changes the output's dimension order
  ([shapes.md](shapes.md#more-than-two-operands)), so a rewrite that swaps them adds a
  transpose (#4), or only swaps when the order is unchanged. `Greater(a, b)` and
  `Less(b, a)` compute the same values, with the same dimension-order caveat.
- **No float reassociation** without explicit permission (design.md). Integer `Add`
  and `Multiply` may be reassociated when every operand and intermediate has the same
  dtype.
- **Constant folding** may evaluate an elementwise node whose inputs are all
  `Constant`s, if it computes exactly as this document says: correctly rounded IEEE
  operations in the result dtype, and wrapping integers. Rust's `/` and `%` truncate
  toward zero and panic on a zero divisor, and `wrapping_div` panics on zero too, so
  floor division, the divisor's-sign remainder and the zero case need explicit code.

## Adapters

An adapter emits each function with these semantics or rejects the node:

- **NumPy and Array API libraries.** One call per node. NumPy matches every checked row
  above on arm64. It warns on integer division by zero and on `INT_MIN // -1`, and a
  user's `np.seterr(all="raise")` turns those warnings into exceptions (checked), so the
  adapter wraps its calls in `np.errstate(divide="ignore", over="ignore",
  invalid="ignore")`. Another Array API library must match the implementation-defined
  rows (integer division by zero) or emit the guard above; JAX and torch need it.
- **xarray.** The same calls through NumPy. To match the output schema it also drops
  the coordinates the plan drops and sets the output's `.name` from the plan, `None`
  for `result`. It never sees mixed kinds or weak scalars, because lowering has already
  cast ([dtypes.md](dtypes.md#adapters)).
- **GDAL.** VRT pixel functions could render some variants as derived bands, but
  nobody has checked their integer overflow or rounding. The GDAL adapter rejects
  elementwise nodes in its first subset ([gdal-adapter.md](gdal-adapter.md)); a fixture
  like the float32 narrowing one in [cast.md](cast.md#what-gdalwarp-can-emit) has to
  come first.
- **Any engine** that fuses multiply-add, flushes subnormals, or computes integers in
  floating point rejects the affected nodes.

## Worked examples

Positional arrays come from a NumPy host. Each array is a binding, opened as a
one-variable dataset and projected to an `Array`; named ones come from xarray.

| Host expression | Lowered nodes | Output |
| --- | --- | --- |
| NumPy `a + b`, `a` `f32 (n, 1)`, `b` `f32 (1, m)` | `Open(a)`, `Project::Array`, `Open(b)`, `Project::Array`, `Add` | positional `f32 (n, m)`; payload `result` when the host gives `a` and `b` different `VarId`s |
| NumPy `i8 + i8` at `127 + 1` | `Add` | `-128`: wraps |
| NumPy `i32 // i32` with a zero divisor | `FloorDivide { zero_divisor: Zero }` | `0` at that element |
| NumPy `i32 ** i32` | rejected at lowering: signed exponent | |
| NumPy `bool + bool` | `LogicalOr` | `bool` |
| NumPy `i32_array + 1.5` | `Cast(i32 → f64)`, `Constant f64 1.5`, `Add` ([dtypes.md](dtypes.md#python-scalars-and-constants)) | `f64` |
| xarray `t2m - 273.15`, `t2m` `f32 (time, lat, lon)` with NaN missing | `Constant f32 273.15`, `Subtract [t2m, c]` | `f32 (time, lat, lon)` named `t2m`, NaN missing, grid kept |
| xarray `obs > thr`, `obs (time, lat)`, `thr (lat)` | `Greater [obs, thr]` | `bool (time, lat)`, no missing values; NaN `obs` gives `false` |
| xarray `xr.where(cond, yx, x)`, `cond` and `x` on `(x: 3)`, `yx` on `(y: 2, x: 3)` | `Where [cond, yx, x]`, then `Rename` to `cond`'s name if it has one | `(x, y)`: shapes.md's combined order |
| xarray `ds.t + ds.u`, one dataset with index `time` | `Add [t, u]` | `time` kept: same origin |
| xarray `a + b`, auxiliary coordinate `aux` from different `Open`s | `Add [a, b]` | `aux` dropped |
| `fill_i16 + 1`, `fill_i16` with sentinel `-9999` | rejected: `UnsupportedMissing` until #39 | |

The NumPy-style and xarray-style rows reach the same `Elementwise` node. They differ
only in their lowering beforehand (weak scalars, casts, label alignment) and in the
operands' axis names, which is what #32's acceptance asks for.

## Decisions

Decided 1 November 2026 under Charles's standing rule of accepting conservative
recommendations, after an independent review:

1. **Integer overflow:** wrapping is the semantics of the variants, with saturating or
   trapping arithmetic as separate functions if needed.
2. **Integer division by zero:** stated per node by `ZeroDivisor`, with one variant,
   `Zero`, for now and no default. Charles chose an explicit enum over a fixed
   definition or leaving integer division out, so other hosts' rules can be added as
   variants later.
3. **Shared auxiliary coordinates:** kept when they have the same origin, otherwise
   dropped. A kept grid that needs a dropped coordinate is rejected.
4. **Shared index coordinates:** kept when they have the same origin, otherwise
   rejected until #34. #33's `mean(a + b)` acceptance needs this. An origin includes the
  selections on the coordinate's dims, so pushdown doesn't break it.
5. **Dataset operands:** rejected until a `Merge` exists. A single positional array
   reaches an elementwise node through `Project::Array`.
6. **Names:** Array API names (`Greater`, not `GreaterThan`).
7. **Unnamed results:** the fixed payload `VarId` `result`. Lowering renames to match
   the host where its rule differs, and the adapter sets names explicitly.
8. **Sentinel missing values:** rejected until #39. `Unknown` missing values give an
   `Unknown` result.
9. **Float `Pow`, `FloorDivide` and `Remainder`:** deferred; integer only for now.
10. **Different grid locations:** rejected, not `Ungridded`.
