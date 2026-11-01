# Dtype inference, promotion and accumulation

Status: decided 1 November 2026, for issue #30 (proposed 31 October). Nothing here is
implemented yet; #31 implements it.

It uses the `DType`, `Scalar` and `TypeKnowledge` types in
[schema-and-values.md](schema-and-values.md) and the node graph in [dag.md](dag.md). It adds
no schema types.

Reference: [Array API 2025.12](https://data-apis.org/array-api/2025.12/), its
[type promotion rules](https://data-apis.org/array-api/2025.12/API_specification/type_promotion.html)
and the function pages for `sum`, `prod`, `mean`, `var`, `divide`, `floor_divide`,
`pow` and `count_nonzero`, retrieved 31 October 2026. NumPy behaviour is NumPy 2 with
[NEP 50](https://numpy.org/neps/nep-0050-scalar-promotion.html). The #42 fixtures check
the NumPy and xarray claims before #31 relies on them.

## Four different things

| Term | What it is | Where it lives |
| --- | --- | --- |
| **Cast** | A conversion the plan asks for, with every value-changing choice stated | A `Cast` node ([cast.md](cast.md)) |
| **Promotion** | The common dtype two operands are combined in | Derived from the inputs' dtypes, never stored |
| **Accumulator** | The dtype an accumulating reduction works in | A required field of that reduction, next to its `result` dtype |
| **Result dtype** | The dtype of a node's output | Derived; the next node sees it |

A result dtype is not always the promoted dtype. A comparison promotes its operands and
returns `bool`, and `Warp` with `Interpolate` returns `float64` whatever its input
([warp.md](warp.md#output-type-and-empty-cells)).

**The rule that ties them together:** ArrIR promotes implicitly only where the
conversion is exact. Anything that can change a value is a `Cast` node, which lowering
inserts with its policies stated. ArrIR has no default that depends on a platform or a
backend.

## Promotion table

Promotion follows Array API 2025.12 for every pair the standard defines, and rejects
every pair it leaves unspecified. Find the row and column dtypes; the cell is the
promoted dtype.

| | bool | i8 | i16 | i32 | i64 | u8 | u16 | u32 | u64 | f32 | f64 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **bool** | bool | K | K | K | K | K | K | K | K | K | K |
| **i8** | K | i8 | i16 | i32 | i64 | i16 | i32 | i64 | U | K | K |
| **i16** | K | i16 | i16 | i32 | i64 | i16 | i32 | i64 | U | K | K |
| **i32** | K | i32 | i32 | i32 | i64 | i32 | i32 | i64 | U | K | K |
| **i64** | K | i64 | i64 | i64 | i64 | i64 | i64 | i64 | U | K | K |
| **u8** | K | i16 | i16 | i32 | i64 | u8 | u16 | u32 | u64 | K | K |
| **u16** | K | i32 | i32 | i32 | i64 | u16 | u16 | u32 | u64 | K | K |
| **u32** | K | i64 | i64 | i64 | i64 | u32 | u32 | u32 | u64 | K | K |
| **u64** | K | U | U | U | U | u64 | u64 | u64 | u64 | K | K |
| **f32** | K | K | K | K | K | K | K | K | K | f32 | f64 |
| **f64** | K | K | K | K | K | K | K | K | K | f64 | f64 |

- **K, mixed kind.** Bool with a number, or an integer with a float. Array API leaves
  these unspecified "because behavior varies between implementations". ArrIR rejects
  them, and lowering inserts a `Cast` on one operand using its host's rule (below).
- **U, no common integer.** `u64` with a signed integer would need 128 bits. Array API
  leaves it unspecified, and NumPy 2 gives `f64`, which is inexact above 2^53. ArrIR
  rejects it.
- Every other cell is an exact widening, so no value changes and no `Cast` is needed.
- Zero-dimensional arrays promote like any other array, as Array API requires.

Promotion only says which dtype operands combine in. Whether an operation accepts that
dtype is the operation's rule: two `bool`s promote to `bool`, and `add` then rejects
them because they aren't numeric.

```rust
pub enum PromotionError {
    /// Bool with a number, or an integer with a float. Lowering casts first.
    MixedKind { left: DType, right: DType },
    /// u64 with a signed integer: no common integer type up to 64 bits.
    NoCommonInteger { signed: IntWidth },
}

/// Derived during type inference. Never stored on a node.
pub fn promote(left: DType, right: DType) -> Result<DType, PromotionError>;
```

### How hosts differ on the rejected cells

Lowering decides these, not ArrIR, and writes each decision as a `Cast`:

| Host expression | NumPy 2 / xarray | Lowered to ArrIR |
| --- | --- | --- |
| `i8_array + f32_array` | `f32` | `Cast(i8 → f32)`, then add |
| `i32_array + f32_array` | `f64` | `Cast(i32 → f64)`, `Cast(f32 → f64)`, then add |
| `i64_array + u64_array` | `f64` | two rounding `Cast`s to `f64`, then add |
| `bool_array + i32_array` | `i32` | `Cast(bool → i32)`, then add |

NumPy's mixed int/float rule: integers up to 16 bits with `f32` give `f32`, and
otherwise `f64`. Xarray computes through NumPy for these dtypes. When an outer join or
`where` introduces missing values, xarray's `maybe_promote` also changes the dtype:
8- and 16-bit integers become `f32`, wider integers become `f64`, and `bool` becomes
`object`. The integer cases are `Cast`s inserted by the alignment lowering (#34) with
#39's missing-value encoding. ArrIR has no `object` dtype, so the `bool` case is
rejected.

GDAL has no implicit promotion. Every band has a declared type, and its engines compute
in a working type and convert at the end. The GDAL adapter emits explicit types from the
plan, by the table in [cast.md](cast.md#what-gdalwarp-can-emit), and rejects anything
else.

## Result dtypes by operation

#30 asks for an operator table. This one gives each family's result dtype; #32 owns
operand domains, overflow, division by zero and special values.

| Operation | Result dtype | Array API 2025.12 | ArrIR |
| --- | --- | --- | --- |
| `add`, `subtract`, `multiply` | promoted | promoted | promoted; `bool` rejected |
| comparisons (`greater`, `equal`, …) | `bool` | `bool` | operands promoted by the table |
| `logical_and`, `logical_or`, `logical_not` | `bool` | `bool` | `bool` operands only |
| `divide` | promoted float | integer inputs "implementation-dependent" | integer operands rejected; lowering casts (NumPy gives `f64`) |
| `floor_divide` | promoted | integer division by zero "implementation-defined" | promoted; division by zero is #32's |
| `pow` | promoted | integer base with negative integer exponent, and integer base with float exponent, "implementation-dependent" | promoted; mixed kinds cast; negative integer exponents are #32's |
| `min`, `max` | input dtype | same as input | input dtype, no accumulator |
| `any`, `all` | `bool` | `bool` | `bool`, no accumulator |
| `count_nonzero` | integer | "the default array index data type" | the counter dtype lowering states (NumPy: `i64`) |
| `sum`, `prod` | `result` | see [Reductions](#reductions) | declared `result` |
| `mean` | `result` | see [Reductions](#reductions) | declared float `result` |
| `var`, `std` | float | float input; integers optional, giving the default float; no `dtype` keyword | not yet supported: no `ReductionTypes` until the formula has a contract |

## Python scalars and constants

`x + 1` has no second array. Array API and NEP 50 agree that a Python scalar is *weak*:
it takes the array's dtype rather than taking part in promotion.

ArrIR has no weak types. Lowering resolves the scalar to a typed `Scalar` (from #26) of
the array's dtype:

| Host expression | NumPy 2 | Lowered to ArrIR |
| --- | --- | --- |
| `i8_array + 1` | `i8` | add `i8` constant `1` |
| `i8_array + 300` | `OverflowError` | rejected at lowering |
| `f32_array + 0.1` | `f32` | add `f32` constant: `0.1` rounded to `f32`, bits recorded |
| `i32_array + 1.5` | `f64` | `Cast(i32 → f64)`, add `f64` constant `1.5` |
| `bool_array + 1` | `i64` | `Cast(bool → i64)`, add `i64` constant `1` |

The last two rows are host rules for cells Array API leaves unspecified, so they appear
as explicit `Cast`s. An out-of-range integer scalar is always rejected: Array API leaves
it unspecified, and NumPy 2 raises.

The constant is a literal node, which [dag.md](dag.md) allows: a zero-input node with
no binding.

```rust
pub struct Constant {
    pub value: Scalar,
    /// Whether this constant is missing. May be true only when `value` is a NaN.
    pub nan_is_missing: bool,
}
```

- **Arity** `Exactly(0)`.
- **Result** `ResultKind::Array(ArrayResult { variable: "constant" })`. Its schema holds
  one data variable with the fixed `VarId` `constant`, no coordinates and no grids. The
  variable has `dims: []`, the dtype of `value`'s variant, and
  `missing: Known { values: [], nan_is_missing }`. VarIds are scoped to one node's
  schema ([dag.md](dag.md)), so the fixed name can't collide with an operand's
  variable. How an elementwise result names its variable is #32's rule
  ([elementwise.md](elementwise.md#the-output-schema)). The constant
  broadcasts against any shape ([shapes.md](shapes.md)).
- **NaN.** A NaN constant states whether it is missing. Xarray treats NaN as missing,
  so its lowering writes `true`. Array API has no missingness, so its lowering writes
  `false`. How missing operands propagate is #39's.
- **Wire.** `value` uses #26's scalar encoding: a tagged variant, integers as decimal
  strings, and floats as fixed-width hexadecimal bits. For example,
  `{ "Constant": { "value": { "Float64": "3ff0000000000000" }, "nan_is_missing": false } }`
  is `1.0`. The exact tags land with the first serde fixtures.

A zero-dimensional array that comes from data, such as `x.sum()`, is not weak. It
promotes by the table, as Array API and NumPy 2 both require.

## Reductions

### Which reductions have an accumulator

- **Accumulating:** `sum`, `prod` and `mean` have an accumulator and a result dtype.
  `var` and `std` stay out until their formula (two-pass, Welford or other) has a
  contract, because the algorithm changes the result as much as the accumulator does.
- **Counting:** `count_nonzero` has an integer counter dtype, written as both
  accumulator and result.
- **Selecting and logical:** `min` and `max` return the input dtype, and `any` and
  `all` return `bool`. They have no accumulator.

```rust
pub struct ReductionTypes {
    /// The dtype values are combined in.
    pub accumulator: DType,
    /// The dtype of the reduction's output.
    pub result: DType,
    /// How a finite value beyond `result`'s range is written, with cast.md's meaning.
    /// Required exactly when `result` is a narrower float than `accumulator`.
    pub out_of_range: Option<OutOfRange>,
    pub order: AccumulationOrder,
}

pub enum AccumulationOrder {
    /// Elements combine one at a time in index order. With several reduced
    /// dimensions, the last in the input's dimension order varies fastest.
    /// A sum is ((x0 + x1) + x2) + …; it starts from x0, not from 0.
    Sequential,
    /// Permission for the engine to use any order and grouping.
    Unspecified,
}
```

`ReductionTypes` is part of the bounded reduction specification in roadmap step 5 of
[design.md](design.md#small-pr-roadmap). Every field is required where it applies, so
nothing has a default. `OutOfRange` is cast.md's enum, not a second one.

### Rules

1. **For `sum`, `prod` and `mean`, the input must convert to the accumulator
   exactly.** This doesn't apply to `count_nonzero`. It never combines input values:
   it tests each element against zero in the input's own dtype and adds 1 to its
   counter, so any input dtype works with any integer counter. Allowed conversions:
   - the same dtype;
   - `bool` to any integer or float (`true` is 1);
   - a signed integer to a signed integer at least as wide;
   - an unsigned integer to an unsigned integer at least as wide, or to a strictly wider
     signed integer;
   - integers up to 16 bits to `f32`, and up to 32 bits to `f64`;
   - `f32` to `f64`.

   Anything else, such as `i64` to `f64` or `f64` to `f32`, needs a `Cast` before the
   reduction, inserted by lowering with its policies stated.
2. **The result may differ from the accumulator in two cases only:**

   | Accumulator | Result | Allowed for | Conversion |
   | --- | --- | --- | --- |
   | any | the same dtype | `sum`, `prod`, `mean`, `count_nonzero` | none |
   | `f64` | `f32` | `sum`, `prod`, `mean` | cast.md's float narrowing: round to nearest, ties to even, then `out_of_range` (`Infinity` or `Clamp`) |
   | integer | `f32` or `f64` | `mean` | the integer sum and the count are each converted to `result` by cast.md's integer-to-float rounding, then divided in `result` |

   Every other pair is rejected. A wanted output dtype outside this table is a `Cast`
   after the reduction. `OutOfRange::Missing` isn't allowed here, because a reduction
   has no missing-value encoding of its own. If the output's missing values include a
   sentinel other than NaN (#39), lowering uses a `Cast` with an explicit `Collision`
   instead.
3. **`mean` with a float accumulator** is the sum divided by the element count, with
   the count converted to the accumulator by rounding to nearest. The division happens
   in the accumulator, and the quotient is then narrowed to `result` if the two differ.
   An empty mean is NaN, as Array API requires. Skipping missing values is #39's.
4. **`sum` and `prod` accept any numeric accumulator.** The empty sum is 0 and the empty
   product is 1. Integer overflow behaves as `add` and `multiply` do. Wrapping
   overflow makes integer results independent of order; trapping or saturating
   overflow doesn't. [elementwise.md](elementwise.md#integer-overflow-wraps) proposes
   wrapping (#32).

### Order

[design.md](design.md#decisions) preserves floating-point evaluation order unless a
relaxation is explicit. A reduction keeps that rule. `Sequential` is the strict order,
and `Unspecified` is a relaxation that lowering writes explicitly.

The rewrites in question split a reduction into partial reductions (dask's tree), merge
`Reduce(lat) ∘ Reduce(time)` into `Reduce(time, lat)`, or reorder the reduced
dimensions.

| Reduction | Under `Sequential` | Under `Unspecified` |
| --- | --- | --- |
| `sum`, `prod` | none allowed with a float accumulator; all allowed with an integer one, because integers wrap ([elementwise.md](elementwise.md#integer-overflow-wraps), proposed) | all allowed. Partial reductions keep the accumulator, and only the final combine converts to `result`. Merging needs both reductions `Unspecified`, with the same accumulator, and the inner one's `result` equal to its accumulator |
| `mean` | none allowed | split through `sum` and a count; merge only when every group has the same count, which needs #39's missing-value facts |
| `count_nonzero` | all allowed: integer counting doesn't depend on order | all allowed |
| `min`, `max`, `any`, `all` | all allowed, once #32 fixes NaN and signed-zero handling | all allowed, on the same condition |

**Adapters expose or reject.** An engine that can't guarantee `Sequential` rejects it.
NumPy, for example, sums pairwise along a contiguous inner axis. An `Unspecified`
reduction may be run in any order.

**Lowering from NumPy, xarray and dask writes `Unspecified`,** because none of them
promises an order: NumPy's depends on memory layout, and dask's on the chunking. A
pairwise variant with a defined tree can be added if a host guarantees one.

### Defaults are a lowering decision

Array API's `sum` and `prod` keep the input dtype, unless the input is an integer type
narrower than the library's default integer. For such an input, a signed input gives
"the default integer data type", and an unsigned input gives "an unsigned integer data
type having the same number of bits as the default integer data type". `bool` input
isn't addressed. `mean` input "should have a floating-point data type". Libraries "may
choose to accept" integers, and then the result is "the default real-valued
floating-point data type". `mean` has no `dtype` keyword.

ArrIR has no default integer or float, so lowering writes the dtype its host uses:

| Input | NumPy 2 / xarray `sum`, `prod` | NumPy 2 / xarray `mean` |
| --- | --- | --- |
| `bool` | `i64` | `f64` |
| `i8` to `i32` | `i64` | `f64` |
| `i64` | `i64` | `Cast(i64 → f64)`, then `f64` |
| `u8` to `u32` | `u64` | `f64` |
| `u64` | `u64` | `Cast(u64 → f64)`, then `f64` |
| `f32` | `f32` | `f32` |
| `f64` | `f64` | `f64` |

For these defaults, lowering writes `result` equal to the accumulator. A host's `dtype=`
argument sets both. If the input doesn't convert exactly, lowering inserts the `Cast`
that Array API's `sum` describes ("cast to the specified data type before computing").

### Precision-sensitive mean

Adding `1.0` to an `f32` running total one element at a time stops growing at 2^24,
because 2^24 + 1 rounds back to 2^24. A mean of 10^8 ones would come out near 0.168.
NumPy only sums pairwise along a contiguous inner axis. A mean over `time` of a
C-ordered `(time, lat)` array adds rows one at a time, so it stalls too. Whether an
engine stalls depends on memory layout and chunking, not on the plan. #42 adds a
fixture for this case.

A plan that needs the precision states it:

| Node | Operation | Inputs | Output |
| --- | --- | --- | --- |
| 0 | `Open { binding: obs }` | — | Dataset with `t: f32 (time, lat)` |
| 1 | `Project::Array(t)` | 0 | Array `t: f32 (time, lat)` |
| 2 | `Reduce { mean over time, accumulator: f64, result: f32, out_of_range: Infinity, order: Unspecified }` | 1 | Array `f32 (lat)` |

One node accumulates in `f64` and returns `f32`. No `Cast` comes before it, because
`f32` to `f64` is exact. The final quotient is narrowed by cast.md's rule: rounding to
nearest, ties to even, and overflowing to infinity as NumPy does
([cast.md](cast.md#definitions)).

**Adapters map the node to one host expression or reject it.** The xarray adapter emits
`x.mean("time", dtype="float64").astype("float32")`, since NumPy's `astype` matches
`Infinity`. With `Clamp`, it rejects, because no single call clamps. The GDAL adapter
rejects every reduction in its first subset ([gdal-adapter.md](gdal-adapter.md)).

## Unknown dtypes

`TypeKnowledge::Unknown` blocks reasoning that depends on the dtype; it doesn't make a
plan invalid. As in #26, inference keeps three answers apart:

- **Known result.** A comparison returns `bool` even when an operand is unknown, and an
  accumulating reduction returns its declared `result`.
- **Unknown result.** Promotion with an unknown operand gives `Unknown`. It is never
  guessed as `f64`.
- **Unresolved requirement.** Whether operands promote, or whether the input converts
  exactly to the accumulator, can't be checked yet. Semantic readiness reports it with
  the node and input position, separately from contradictions. Adapters reject the node
  until it's resolved, and no rewrite that depends on the dtype is certified.

## Casts that lowering inserts

ArrIR never inserts a `Cast` implicitly. Lowering inserts one for:
- mixed-kind operands;
- a weak scalar its host converts across kinds;
- a reduction input that doesn't convert exactly to the accumulator, before the
  reduction;
- a reduction output dtype that `ReductionTypes` can't express, after it;
- missing values introduced into an integer variable (#34, #39).

A conversion that can round needs care with missing values. An integer of 32 bits or
more converted to `f32`, a 64-bit integer converted to `f64`, and `f64` narrowed to
`f32` can all land a valid value on a missing sentinel. When the input's missing values
include a sentinel other than NaN, such a `Cast` must set `missing` with an explicit
`Collision` ([cast.md](cast.md#validation), #39). Nothing rounds onto NaN, so NaN alone
needs neither.

## No Promote node

Implicit promotion is exact, so it has no options to record, and its dtype is derived
from the inputs. Promotion that changes values is a `Cast`, which already exists. A
`Promote` node would duplicate one or the other, and no concrete case needs one.

## Adapters

An adapter realises the exact promoted dtype, accumulator, result conversion and order,
or rejects the node:

- An engine whose reductions accumulate only in `f64` cannot realise an `f32`
  accumulator. Higher precision is still a different result.
- A reduction whose `result` differs from its accumulator is emitted as one host
  expression that realises exactly the stated conversion, such as xarray's
  `mean(…, dtype=…).astype(…)`, or it's rejected.
- An engine whose default integer is `int32` emits the plan's `i64` explicitly or
  rejects.
- An engine that promotes mixed kinds by its own rule never sees a mixed-kind node,
  because lowering has already cast.

## Complex and other families

Array API 2025.12 requires `complex64` and `complex128`. It defines `f32` with
`complex64` as `complex64`, and `f64` with `complex64` as `complex128`. #26 defers
complex dtypes, so complex inputs are rejected at lowering until a variant lands with
its own contract. That extends this table and the exact-conversion list without changing
their structure. Text, temporal and categorical families follow #26's extension
obligations and have no promotion today.

## Not in this proposal

- Operand domains, overflow, division by zero and NaN/signed-zero handling (#32).
- Conversion policies beyond those [cast.md](cast.md) now states (#38).
- Missing-value propagation (#39).
- The `var`/`std` formula.

## Decisions

Charles decided question 3 directly, and accepted the recommendation on every other
question on 1 November 2026.

1. **Mixed kinds.** *Decided:* reject int/float and bool/number promotion in ArrIR,
   and have lowering write its host's rule as `Cast`s. The cost is noisier plans, and a
   plan is no longer a one-to-one echo of the host expression. Adopting NumPy's
   mixed-kind table would make ArrIR NumPy-shaped.
2. **Same-kind widening.** *Decided:* allow exact widening inside operations. The
   alternative requires equal operand dtypes and a `Cast` for every widening.
3. **Accumulator and result.** *Decided by Charles, 1 November:* a separate `result`
   field, so "accumulate in `f64`, return `f32`" is one node. The final conversion reuses
   cast.md's narrowing rule and its `OutOfRange` enum rather than defining its own.
   The two allowed accumulator/result pairs are enough for now (accepted 1 November).
   Another pair needs its own decision.
4. **Strict order inside reductions.** *Decided:* design.md's strictness applies
   inside reductions. `Sequential` is the strict order, and lowering from NumPy, xarray
   and dask writes `Unspecified` explicitly, because those hosts promise nothing. The
   alternative declares reductions exempt from the rule.
5. **One principle for relaxations.** *Decided:* permissions, not tolerances. A plan
   relaxes something only through a named variant, as `Unspecified` does for order.
   Accumulators are exact, and adapters never apply a tolerance. Fixtures compare
   results with a tolerance only between engines inside a permitted set. The
   alternative is a stated numerical tolerance per plan, covering both accumulator
   width and order.
6. **Constants.** *Decided:* a zero-input `Constant { value, nan_is_missing }` node.
   The alternative is a scalar operand field on each elementwise operation.
7. **Bool accumulators.** *Decided:* keep `bool` to integer as an exact
   accumulator conversion, so `sum` of a mask needs no `Cast`.
8. **The constant's variable.** *Decided:* a fixed `VarId` `constant` in the
   constant's own schema. The alternative is a host-chosen `VarId` on `Constant`, which
   would carry a name such as xarray's `DataArray(2, name="k")` but adds a field almost
   nobody uses.
9. **Unverified GDAL cells.** cast.md's GDAL table marked float narrowing's
   `OutOfRange::Infinity` as unverified. The docs first said such cells are rendered
   and settled later by the #42 meaning fixture. The options were:
   - **Render now**. `Warp` + `Cast(f32)` and standalone float casts
     keep working, but a plan could be rendered with semantics nobody has checked,
     which is in tension with purity rule 4.
   - **Reject until the #42 fixture confirms the cell.** This honours purity rule 4, at
     the cost of every `float32` output from GDAL until the fixture lands.

   *Decided:* reject the unverified cells. An experiment on 1 November (GDAL 3.13.3)
   then resolved them: gdalwarp and warped VRTs overflow to infinity, so `Warp` +
   `Cast(f32)` renders with `Infinity`. GDAL's ordinary conversion clamps infinities as
   well, which no `OutOfRange` variant describes, so a standalone `Cast` to `float32` is
   rejected. Details are in [cast.md](cast.md#what-gdalwarp-can-emit).
