# The Elementwise family

Status: proposed 1 November 2026, for issue #32. Nothing here is implemented; #33
implements the first slice.

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
- NumPy 2.5.4 and xarray 2026.9.0, run 1 November 2026, for every behaviour marked
  "checked" below.

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
    FloorDivide,
    Remainder,
    Pow,
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
  is `Greater`, so each variant maps to one Array API function by name (question 6).
- **Slices are for implementation, not meaning.** This document fixes the meaning of
  every variant above. #33 implements slice 1, and slice 2 follows one variant group at
  a time. Each variant maps to a row of [array-api-coverage.md](array-api-coverage.md),
  which keeps its classification.
- **Not yet:** transcendental functions (`exp`, `log`, `sin`, …) need a tolerance
  contract, because engines don't promise correctly rounded results. Bitwise operations
  and shifts need shift-count rules. Both wait for a use case. `clip` stays a
  composition of `Maximum` and `Minimum`.

**The defining property:** an elementwise operation is pointwise. The output element at
a broadcast index depends only on the operands' elements at that index. Selection
pushdown (#37) relies on this.

## Operand domains

Promotion first combines the operands' dtypes ([dtypes.md](dtypes.md#promotion-table));
the result dtype is then [dtypes.md's table](dtypes.md#result-dtypes-by-operation).
This table says which promoted dtypes each function accepts:

| Functions | Accepted promoted dtype | Result |
| --- | --- | --- |
| `Add`, `Subtract`, `Multiply`, `Negative`, `Abs`, `Square`, `Maximum`, `Minimum`, `Sign` | integer or float | promoted |
| `FloorDivide`, `Remainder` | integer or float | promoted |
| `Pow` | integer or float, with the integer condition below | promoted |
| `Divide` | float | promoted |
| `Equal`, `NotEqual` | any | `bool` |
| `Greater`, `GreaterEqual`, `Less`, `LessEqual` | integer or float | `bool` |
| `LogicalAnd`, `LogicalOr`, `LogicalXor`, `LogicalNot` | `bool` | `bool` |
| `IsNan`, `IsInf`, `IsFinite`, `SignBit` | float | `bool` |
| `Ceil`, `Floor`, `Trunc`, `Round` | integer or float (identity on integers) | promoted |
| `Where(condition, x, y)` | `condition` is `bool`; `x` and `y` promote together | promoted `x`, `y` |

- `bool` is rejected wherever "integer or float" is required, as Array API specifies.
  `bool + bool` is `OperandDomain`; lowering casts first if its host allows it.
- Logical functions take `bool` only. NumPy accepts integers there (checked:
  `logical_and(int8 2, int8 0)` is `False`); lowering inserts a comparison with zero.
- **Integer `Pow`** needs an exponent that can't be negative: an unsigned dtype, or a
  `Constant` that is at least 0. Otherwise it's `IntegerPowSignedExponent`. NumPy raises
  at run time for a negative integer exponent (checked), and Array API leaves it
  unspecified. A plan can't promise either, so lowering casts to float instead.
- An unknown dtype gives an unknown result or an unresolved requirement, never a guess
  ([dtypes.md](dtypes.md#unknown-dtypes)). Comparisons still return `bool`.

## Integer overflow wraps

**Integer arithmetic wraps, modulo 2^n, in two's complement.** `int8 127 + 1` is
`-128`. This covers `Add`, `Subtract`, `Multiply`, `Negative`, `Abs`, `Square`, `Pow`
and `FloorDivide`: `abs(int8 -128)` and `int8 -128 // -1` are both `-128`.

- Array API 2025.12 says nothing about integer overflow (checked on the elementwise,
  `add`, data types and array object pages). NumPy wraps for every case above
  (checked).
- It is the semantics of the variants, not a per-node field. A saturating or trapping
  operation would be a different function (`AddSaturating`), as in Rust, added when a
  case needs it (question 1).
- Wrapping is exact modular arithmetic, so integer `Add` and `Multiply` are associative
  and commutative with no change to any value. Integer reassociation is therefore not a
  numerical relaxation, and integer `sum` and `prod` don't depend on order. That
  answers [dtypes.md](dtypes.md#rules)'s rule 4.

## Division and other undefined integer cases

| Case | Array API 2025.12 | NumPy 2.5.4 (checked) | ArrIR |
| --- | --- | --- | --- |
| integer `FloorDivide` by 0 | "unspecified and thus implementation-defined" | `0` | `0` |
| integer `Remainder` by 0 | implementation-defined | `0` | `0` |
| integer `FloorDivide` rounding | toward −∞ | `-7 // 2` is `-4` | toward −∞ |
| integer `Remainder` sign | the divisor's | `-7 % 2` is `1` | the divisor's |
| `INT_MIN // -1` | not stated | wraps to `INT_MIN` | wraps |
| integer `Pow` with a negative exponent | unspecified | `ValueError` | rejected in the plan (above) |

Division by zero is defined as NumPy defines it rather than rejected: every integer
division would otherwise be rejected, because a plan can't prove a divisor non-zero
(question 2). An engine that traps or gives something else must guard it, for example
with `Where(divisor == 0, 0, x // divisor)`, or reject.

## Floating point

`float32` and `float64` follow IEEE 754 binary32 and binary64:

- **Basic arithmetic is correctly rounded,** to nearest with ties to even: `Add`,
  `Subtract`, `Multiply`, `Divide`, `Square`. Subnormals are kept; an engine that
  flushes them to zero rejects.
- **No fused multiply-add.** `a * b + c` is two nodes, and two roundings. An engine
  that fuses them changes the result and needs explicit permission, under design.md's
  strict-order rule.
- **Division by zero** follows the Array API special cases: a positive dividend over
  `+0` is `+inf`, over `-0` is `-inf`; `±0 / ±0` is NaN. `FloorDivide` by zero gives
  `±inf` the same way (NumPy: `7.0 // 0` is `inf`, checked), and `Remainder` by zero is
  NaN (checked).
- **NaN propagates** through arithmetic. Comparisons with NaN are `false`, except
  `NotEqual`, which is `true` (checked). NaN payloads aren't significant
  ([schema-and-values.md](schema-and-values.md#dtypes-and-scalar-values)), so which
  operand's payload survives doesn't matter.
- **`Maximum` and `Minimum` propagate NaN**, as Array API requires ("If either `x1_i`
  or `x2_i` is `NaN`, the result is `NaN`"). Array API leaves the order of signed zeros
  "implementation-defined"; ArrIR orders `-0` below `+0`, as IEEE 754-2019's `maximum`
  and `minimum` do. NumPy's `maximum` returns `+0` for `±0` in either order (checked);
  `Minimum`'s `-0` is for the #42 fixture to confirm. NumPy's NaN-ignoring `fmax` and
  `fmin` are not these functions.
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
  comparison with a missing value is `false`, not missing, in NumPy and in xarray
  (checked: an xarray NaN `> 0` is `False`). `IsNan` tests for NaN, not for missingness.
- **`Unknown` missing values** give an `Unknown` result. The arithmetic is still
  defined; only its missing-value interpretation isn't.

## The output schema

Each operand must be an array: `ResultKind::Array`, or a `Constant`. A dataset operand
is `DatasetOperand`. xarray applies dataset arithmetic per variable, which would need a
`Merge` that doesn't exist yet (question 5).

- **Shape:** [shapes.md](shapes.md#broadcasting), with operands in input order.
- **Dtype:** promotion, then the domain table above.
- **Payload variable name.** If every operand that isn't a `Constant` has the same
  payload `VarId`, the output keeps it. Otherwise it is the fixed `VarId` `result`. This
  is xarray's name rule (checked: `t + 1` and `t + t` are named `t`, `t + u` has no
  name). The xarray adapter maps `result` to an unnamed array (question 7).
- **Coordinates.** Data-free, so they can differ from xarray's rule, which compares
  values:
  - a coordinate that only one operand has is kept, along with its dims, which are all
    in the output by construction;
  - an auxiliary coordinate that several operands have under one `VarId` is dropped.
    xarray drops it when the values conflict and keeps it when they're equal (checked);
    the plan can't see values, so it always drops, and the xarray adapter emits the drop
    (question 3);
  - an index coordinate on a dim that several operands share is kept from the first.
    Its labels must be equal: #34's alignment establishes that for xarray plans, and it
    records a requirement checked at label resolution otherwise. Until #34 lands, two
    operands that both carry an index coordinate on a shared dim are rejected
    (question 4).
- **Grids.** The output's `SpatialState` is `Unresolved` if any operand's is. Placed
  operands must have equal grid definitions; unequal ones are rejected, as
  [dag.md](dag.md#id-scope-across-opens) requires. Equal definitions at one location give
  `Placed` on the first placed operand's grid; different locations give `Ungridded`. A
  `Placed` operand with an `Ungridded` one gives `Placed`: the operation is pointwise, so
  it keeps the placed operand's geometry. `Constant`s have no dims and don't take part.
- **Name collisions.** If the payload `VarId` equals a kept coordinate's, it's
  `PayloadNameCollision`.

```rust
pub enum ElementwiseError {
    OperandDomain { node: NodeId, input: usize, function: Function, dtype: DType },
    IntegerPowSignedExponent { node: NodeId },
    DatasetOperand { node: NodeId, input: usize },
    UnsupportedMissing { node: NodeId, input: usize },
    MixedNanMissing { node: NodeId },
    PayloadNameCollision { node: NodeId, variable: VarId },
    SharedIndexCoordinate { node: NodeId, dim: DimId },
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
  `Less(b, a)` are the same function.
- **No float reassociation** without explicit permission (design.md). Integer
  `Add` and `Multiply` may be reassociated, because wrapping is exact.
- **Constant folding** may evaluate an elementwise node whose inputs are all
  `Constant`s, if it computes exactly as this document says: correctly rounded IEEE
  operations in the result dtype, and wrapping integers.

## Adapters

An adapter emits each function with these semantics or rejects the node:

- **NumPy and Array API libraries.** One call per node. NumPy matches every checked row
  above. Another Array API library must match the implementation-defined rows (integer
  division by zero) or emit a guard.
- **xarray.** The same calls through NumPy. To match the output schema it also drops
  auxiliary coordinates that several operands share, and names a `result` payload
  `None`. It never sees mixed kinds or weak scalars, because lowering has already cast
  ([dtypes.md](dtypes.md#adapters)).
- **GDAL.** VRT pixel functions could render some variants as derived bands, but
  nobody has checked their integer overflow or rounding. The GDAL adapter rejects
  elementwise nodes in its first subset ([gdal-adapter.md](gdal-adapter.md)); a fixture
  like the float32 narrowing one in [cast.md](cast.md#what-gdalwarp-can-emit) has to
  come first.
- **Any engine** that fuses multiply-add, flushes subnormals, or computes integers in
  floating point rejects the affected nodes.

## Worked examples

Positional arrays come from a NumPy host, which gives each array a symbol (`a`, `b`);
named ones come from xarray.

| Host expression | Lowered nodes | Output |
| --- | --- | --- |
| NumPy `a + b`, `a` `f32 (n, 1)`, `b` `f32 (1, m)` | `Add [a, b]` | positional `f32 (n, m)`, payload `result` |
| NumPy `i8 + i8` at `127 + 1` | `Add` | `-128`: wraps |
| NumPy `i32 // i32` with a zero divisor | `FloorDivide` | `0` at that element |
| NumPy `i32 ** i32` | rejected: signed exponent; lowering casts to `f64` first | |
| NumPy `i32_array + 1.5` | `Cast(i32 → f64)`, `Constant f64 1.5`, `Add` ([dtypes.md](dtypes.md#python-scalars-and-constants)) | `f64` |
| xarray `t2m - 273.15`, `t2m` `f32 (time, lat, lon)` with NaN missing | `Constant f32 273.15`, `Subtract [t2m, c]` | `f32 (time, lat, lon)` named `t2m`, NaN missing, grid kept |
| xarray `obs > thr`, `obs (time, lat)`, `thr (lat)` | `Greater [obs, thr]` | `bool (time, lat)`, no missing values; NaN `obs` gives `false` |
| xarray `xr.where(cond, yx, x)`, `cond` and `x` on `(x: 3)`, `yx` on `(y: 2, x: 3)` | `Where [cond, yx, x]` | `(x, y)`: shapes.md's combined order |
| xarray `a + b`, both with auxiliary coordinate `aux` | `Add [a, b]` | `aux` dropped |
| `fill_i16 + 1`, `fill_i16` with sentinel `-9999` | rejected: `UnsupportedMissing` until #39 | |

The NumPy-style and xarray-style rows reach the same `Elementwise` node. They differ
only in their lowering beforehand (weak scalars, casts, label alignment) and in the
operands' axis names, which is what #32's acceptance asks for.

## Questions for review

1. **Integer overflow:** wrapping is the semantics of the variants, with saturating or
   trapping arithmetic as separate functions if needed. *Recommended.*
2. **Integer division by zero:** defined as NumPy's `0` for `FloorDivide` and
   `Remainder`, not rejected. *Recommended.*
3. **Shared auxiliary coordinates:** always dropped, where xarray keeps equal ones.
   *Recommended;* the alternative, keeping them with an equality requirement, fails
   where xarray succeeds.
4. **Shared index coordinates:** kept from the first operand, with label equality
   established by #34, and rejected until #34 lands. *Recommended.*
5. **Dataset operands:** rejected until a `Merge` exists. *Recommended.*
6. **Names:** Array API names (`Greater`, not `GreaterThan`). *Recommended.*
7. **Unnamed results:** the fixed payload `VarId` `result`, which the xarray adapter
   maps to `None`. *Recommended.*
8. **Sentinel missing values:** rejected until #39. *Recommended.*
