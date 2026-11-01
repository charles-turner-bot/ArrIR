# Cast

Status: proposed 28 October 2026; revised by #38 and decided on 1 November 2026. The
float rules were decided with [dtypes.md](dtypes.md) the same day. Part of
[the extraction contract](design.md). `Warp` ([warp.md](warp.md)) is its first user.
The reasoning is in [the 28 October discussion](2026-10-28-discussion.md) and
[the 1 November discussion](2026-11-01-discussion.md).

References: [Array API 2025.12 `astype`](https://data-apis.org/array-api/2025.12/API_specification/generated/array_api.astype.html)
and [NumPy `ndarray.astype`](https://numpy.org/doc/stable/reference/generated/numpy.ndarray.astype.html),
retrieved 1 November 2026. Host behaviour below was checked by experiment that day with
NumPy 2.5.4, xarray 2026.9.0 and GDAL 3.13.3 (Python bindings, macOS arm64).

## Summary

`Cast` converts values to another data type. It's Array API's `astype`, with every
choice that can change a value stated: how fractions round, what happens out of range,
and how missing values are written in the new type. Nothing has a default.

It's a separate operation from `Warp` because each is useful without the other. A float
warp needs no conversion, and converting to `float32` before a reduction needs no warp.
gdalwarp does both in one call, but it computes in its working type and converts at the
end, so `Cast(Warp(x))` describes what it does. Fusing the pair is an adapter's choice
(below), never a change to either operation.

`Cast` is for conversions the plan asks for. Promotion that changes no value is derived
and needs no node ([dtypes.md](dtypes.md#four-different-things)).

## The operation

```rust
pub struct Cast {
    pub target: CastTarget,
    pub dtype: DType,
    /// Required when a fraction can reach an integer type.
    pub rounding: Option<Rounding>,
    /// Required when a value can fall outside `dtype`'s range.
    pub out_of_range: Option<OutOfRange>,
    /// How missing values are written in `dtype`. Without it, the input's missing
    /// values are kept, and each must be representable in `dtype`.
    pub missing: Option<MissingEncoding>,
}

pub enum CastTarget {
    /// The input is an array (`ResultKind::Array`) whose payload has the `Data` role.
    /// The payload is converted; its coordinates are unchanged.
    Array,
    /// The input is a dataset. These data variables are converted, in this order;
    /// every other variable is unchanged.
    Variables(Vec<VarId>),
}

pub enum Rounding {
    NearestTiesEven,
    /// Ties away from zero: -2.5 becomes -3.
    NearestTiesAway,
    /// Ties toward positive infinity: -2.5 becomes -2.
    NearestTiesUp,
    TowardZero,
}

pub enum OutOfRange {
    /// Clamp to `dtype`'s range. Infinities clamp to the extremes of an integer type;
    /// for a float type, infinities stay infinite.
    Clamp,
    /// Write `missing.value`. Requires `missing`.
    Missing,
    /// Overflow to positive or negative infinity, as IEEE 754 narrowing does.
    /// Float to float only.
    Infinity,
    /// Keep the low bits: the value modulo 2^bits, read as two's complement for a
    /// signed type. Integer to integer only. The same rule as #32's wrapping integer
    /// arithmetic.
    Wrap,
}

pub struct MissingEncoding {
    /// Written for every input element that's missing, NaN included when the input
    /// declares NaN missing.
    pub value: Scalar,
    /// What happens to a valid converted value equal to `value`.
    pub collision: Collision,
}

pub enum Collision {
    /// Move it off `value`: to `value - 1` for an integer type, or `lowest + 1` when
    /// `value` is the lowest; to the next representable value toward the maximum for a
    /// float type, or toward zero when `value` is the largest finite value. A float
    /// `value` must be finite; an infinite sentinel is rejected.
    Nudge,
    /// Write it unchanged. It then reads as missing.
    Keep,
}
```

`Cast` has one input (`Arity::Exactly(1)`, [dag.md](dag.md#rules)). `CastTarget`
replaces the earlier `var: VarId`, which assumed a dataset input; `Project`
([dag.md](dag.md#open-projection-and-result-kinds)) produces either kind, and `Cast`
accepts both without a merge operation. Variables that need different dtypes or
options take one `Cast` each.

## Conversion matrix

`DType` is the one in
[schema-and-values.md](schema-and-values.md#dtypes-and-scalar-values): `Bool`, signed
and unsigned integers of 8–64 bits, `float32` and `float64`. "Exact" means every input
value is represented unchanged, so no option applies.

| From → to | Rule | `rounding` | `out_of_range` |
| --- | --- | --- | --- |
| same dtype | identity; lowering emits no node | — | — |
| bool → integer or float | `true` → 1, `false` → 0 (Array API) | — | — |
| integer or float → bool | 0 and -0.0 → `false`; anything else, NaN included, → `true` (Array API: "a non-zero value must cast to `True`") | — | — |
| integer → wider integer of a kind that holds it | exact | — | — |
| integer → integer that can't hold every value (narrower, or a sign change) | exact in range | — | `Clamp`, `Missing` or `Wrap` |
| integer → float | exact when the target holds every value; otherwise round to nearest, ties to even (32-bit or wider integers to `float32`, 64-bit to `float64`) | — | — |
| float → integer | rounded, then range-checked | required | `Clamp` or `Missing` |
| `float32` → `float64` | exact | — | — |
| `float64` → `float32` | round to nearest, ties to even; NaN stays NaN, infinities stay infinite | — | `Infinity` or `Clamp` |
| anything involving complex, text or temporal | rejected until #26 adds the family | | |

Rules that sit beside the table:

- Values are rounded first, then range-checked. For an integer target with `Clamp`
  this gives the same result as gdalwarp's clamp-then-round. Rounding first matters
  for `Missing` (32767.4 rounds into `int16`'s range and is kept), for a float `Clamp`
  (infinities stay infinite) and for `Infinity` (`nextafter(FLT_MAX)` rounds to
  `FLT_MAX` and isn't out of range).
- Float to integer: the input's missing values must have `nan_is_missing: true`, and
  `missing` is required. A NaN that isn't missing has no integer value. Infinities are
  out of range.
- `Wrap` applies only between integer types, where it is NumPy's behaviour. A float has
  no low bits to keep, so float to integer can't wrap.
- Array API leaves float-to-integer conversion of NaN and infinity "implementation-
  dependent", and says nothing about rounding or other out-of-range values. ArrIR
  requires the policy instead of inheriting any of these gaps.
- A reduction whose `result` is narrower than its accumulator uses the same float rules
  and the same `OutOfRange` ([dtypes.md](dtypes.md#rules)).

## Missing values

`Cast` keeps only what a conversion can't avoid saying: which value marks missing in the
new type, and what happens when a valid value lands on it. Everything else is #39's:

- whether missingness is a sentinel, a validity mask, or a semantic layer;
- whether a separate `EncodeMissing` operation is needed;
- packed and CF-encoded sources, and storage fill versus logical missing;
- how `Reduce`'s skipna and `Warp`'s `empty_value` interact with encodings.

Two rules stay here because a conversion can't be stated without them:

- **Collisions are explicit.** A conversion that isn't exact (it can round, or it has
  `out_of_range`) needs `missing` whenever the input's missing values include a
  sentinel other than NaN, so its `Collision` is stated. Otherwise a valid value can
  land on that sentinel without the plan saying it may: by rounding, or by `Clamp` or
  `Wrap` (int32 to int16 with sentinel 32767: 40000 clamps to it, 98303 wraps to it)
  ([dtypes.md](dtypes.md#casts-that-lowering-inserts)). Nothing rounds, clamps or
  wraps onto NaN, so NaN alone doesn't need it.
- **No missing values in `bool`.** Converting a variable that may have missing values to
  `bool` is rejected: either sentinel would collide with half of all valid values. A
  `bool` source has no missing values either, NaN or sentinel, so converting from
  `bool` never needs `missing`. #39 may lift both with a validity mask.

## Effect on the schema

- Each converted variable's data type becomes `dtype`.
- With `missing`, its missing values become `Known { values: [], nan_is_missing: true }`
  when `missing.value` is NaN, and `Known { values: [missing.value], nan_is_missing:
  false }` otherwise. `values` never holds a NaN
  ([schema-and-values.md](schema-and-values.md#missing-values-and-decoding)).
- Without it, they're unchanged.
- The result kind, dimensions, extents, coordinates and grid placement are unchanged.

## Validation

- `CastTarget::Array` needs an array input whose payload has the `Data` role. A payload
  with a coordinate role is rejected (decision 4). `Variables` needs a dataset input
  and a non-empty list of distinct data variables present in it.
- The input's missing values are `Known` whenever `missing` is set. For float to
  integer they are `Known` with `nan_is_missing: true`.
- `rounding` and `out_of_range` are present exactly when the matrix requires them, for
  every converted variable, and `out_of_range` is one the matrix allows. So every
  variable in one `Cast` needs the same options: a dataset whose variables need
  different ones takes one `Cast` per group.
- `missing.value` is representable in `dtype`, and `Missing` is used only with
  `missing`.
- The collision and `bool` rules above hold.
- An unknown input dtype leaves the requirements unresolved: semantic readiness
  reports it, and adapters reject the node until it's known
  ([dtypes.md](dtypes.md#unknown-dtypes)).

## Array API and NumPy

**Array API `astype(x, dtype, /, *, copy=True, device=None)`** maps to `Cast` with
`CastTarget::Array`. `copy` decides whether an unchanged array may be returned as the
same object. That's aliasing, an execution concern, so ArrIR doesn't represent it.
`device` is placement, also outside the logical plan (design.md). Every value rule the
spec states is a row of the matrix. The spec states the `bool` rules and that NaN and
infinity to an integer type are "implementation-dependent"; it says nothing about how
float to integer rounds, or about other out-of-range values. Lowering takes those from
its host: NumPy and array-api-strict 2.6.1 truncate (`TowardZero`). So a plan is
stricter than the spec, never looser.

**NumPy's `casting=` levels** gate which dtype pairs `astype` accepts; they change no
value. Lowering applies the gate and either emits a `Cast` or rejects, as NumPy would:

| Level | Accepts | Lowering |
| --- | --- | --- |
| `no`, `equiv` | the same dtype (byte order is storage, not a `DType`) | no node |
| `safe` | NumPy's safe set: exact conversions, and also `int64` and `uint64` → `float64`, which round | `Cast` with the matrix's options (none), plus `missing` if the collision rule applies |
| `same_kind` | also narrowing within a kind (`float64` → `float32`, `int64` → `int8`), sign changes (`uint64` → `int8`) and integer → float (`int32` → `float32`) | `Cast` with the matrix's options |
| `unsafe` (the default) | anything | `Cast` with the matrix's options |
| `same_value` | anything, failing at run time if a value changes | rejected: a run-time check is not a policy ArrIR has |

**What NumPy computes** (NumPy 2.5.4, checked):

| Conversion | Result | ArrIR |
| --- | --- | --- |
| float → integer | truncates: 2.7 → 2, -2.7 → -2 | `TowardZero` |
| float → integer, out of range, infinite or NaN | undefined in NumPy, with `RuntimeWarning: invalid value encountered in cast`; on arm64 it depends on the target width (table below) | none; lowering rejects (decision 2) |
| integer → narrower integer | wraps: 40000 → -25536 as `int16`, 300 → 44 as `uint8` | `Wrap` |
| `int64` → `float32` | 16777217 → 16777216 | ties to even (no option) |
| `float64` → `float32` | overflows to ±inf | `Infinity` |
| to and from `bool` | as Array API | the matrix |

NumPy 2.5.4 on macOS arm64, out-of-range float to integer. 32- and 64-bit targets
saturate; narrower ones saturate to `int32` and then wrap, so `-1.0` gives 255 as `uint8`
but 0 as `uint32`. NumPy calls these values undefined, and x86 is reported to give
`INT_MIN` instead; that wasn't tested.

| Input | int8 | int16 | int32 | int64 | uint8 | uint32 | uint64 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `1e6` | 64 | 16960 | 1000000 | 1000000 | 64 | 1000000 | 1000000 |
| `inf` | -1 | -1 | INT32_MAX | INT64_MAX | 255 | UINT32_MAX | UINT64_MAX |
| `-inf` | 0 | 0 | INT32_MIN | INT64_MIN | 0 | 0 | 0 |
| NaN | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `-1.0` | -1 | -1 | -1 | -1 | 255 | 0 | 0 |

An adapter emitting NumPy uses a bare `astype` only where NumPy's result is defined and
matches the plan. Otherwise it composes the conversion, for example `np.clip` then
`np.rint` (ties to even) or `np.trunc`, then `astype`, with NaN written through
`np.where` from `missing`. Or it rejects the node.

**xarray** (2026.9.0, checked): `DataArray.astype` converts the data and leaves
coordinates alone, which is `CastTarget::Array`. Called on a coordinate
(`ds.x.astype("int16")`), it returns a converted payload named `x` while `ds`'s `x`
coordinate stays `float64`. ArrIR can't express that split, since payload and
coordinate would share one `VarId`, so `Array` requires the `Data` role.
`Dataset.astype` converts every data variable, integer and float alike, and leaves
every coordinate, index or not, unchanged. That is one `CastTarget::Variables` per
group of data variables that need the same options. Its numerics are NumPy's.
`keep_attrs` is an attribute policy, which #26 owns.

## What gdalwarp can emit

Checked by experiment on 1 November 2026 with GDAL 3.13.3, except where a row says
otherwise. "Ordinary conversion" is a `Cast` with no warp, rendered as a VRT band's
data type; `gdal_translate` behaves the same. GDAL has no `bool` type.

| GDAL path | `Rounding` | `OutOfRange` | `Collision` |
| --- | --- | --- | --- |
| gdalwarp, integer working type (`-wt` set to the output type), interpolating kernel | `NearestTiesUp` (bilinear: -2.5 → -2, -0.5 → 0) | `Clamp` | `Nudge` (not yet checked) |
| gdalwarp, integer working type, `-r near` | `NearestTiesAway` (-2.5 → -3) on the conversion into the working type | `Clamp` | `Nudge` (-9999 → -10000) |
| gdalwarp, float working type, integer output | `NearestTiesAway` (2.5 → 3, -2.5 → -3, 3.5 → 4) | `Clamp` (`1e6` → 32767, `inf` → 32767) | `Keep` (-9999.0 and -9999.4 → -9999: the nudge is undone by rounding) |
| gdalwarp, float working type (`Float64` unless `Nearest`), float output | none needed | `Infinity` (verified) | `Nudge` (-9999 → -9998.999999999998) |
| ordinary conversion, integer output from float | `NearestTiesAway` (2.5 → 3, -2.5 → -3) | `Clamp` (`1e6` and `inf` → 32767) | `Keep` (-9999.4 → -9999) |
| ordinary conversion, integer output from integer | none needed | `Clamp` (40000 → 32767, -5 → 0 as `Byte`) | `Keep` (not yet checked) |
| ordinary conversion, float output from integer | ties to even (16777217 → 16777216 as `Float32`) | none needed | `Keep` (not yet checked) |
| ordinary conversion, float output from float | none needed | none: clamps infinities too, so rejected | — |

The GDAL adapter rejects any other combination, including every `Wrap`, every `bool`
conversion, `TowardZero` and `NearestTiesEven` to an integer type, and `Missing`.

Notes on the checks:

- **gdalwarp and a warped VRT** (`gdal.Warp`, also with `format="VRT"`), with
  `-wt Float64` and `-r near` or `bilinear`, and with `-wt Float32` and `-r near`:
  `1e39` → `inf`, `-1e39` → `-inf`, `3.5e38` → `inf`, `inf` → `inf`, `-inf` → `-inf`,
  NaN → NaN, `FLT_MAX` and `nextafter(FLT_MAX)` → `FLT_MAX`, `1.5` → `1.5`. This is
  IEEE 754 overflow, the same as NumPy's `astype`, so the row is `Infinity`.
- **gdalwarp with `-wt Float32` and an interpolating resampler** (`bilinear`, `cubic`):
  the narrowing itself is still `Infinity`, but a finite neighbour of a value that is
  infinite in the working type becomes NaN. In `[1, 2, 1e39, 4, 5, inf, 7, 8]`, the `2`
  and the `5` come out as NaN. With `-wt Float64` the neighbours are unchanged. The
  same happens to a `float32` source under gdalwarp's default working type, with no
  `Cast` at all. So the adapter always passes `-wt Float64` for a float source with any
  method other than `Nearest` ([gdal-adapter.md](gdal-adapter.md#how-it-emits-a-warp)).
- **gdal_translate and a plain VRT** (`gdal.Translate`, GTiff or VRT): `1e39` →
  `FLT_MAX`, `-1e39` → `-FLT_MAX`, `3.5e38` → `FLT_MAX`, and also `inf` → `FLT_MAX` and
  `-inf` → `-FLT_MAX`; NaN → NaN. This clamps genuine infinities as well, so it matches
  neither `Infinity` nor `Clamp`, under which infinities stay infinite.

So a `Warp` followed by a `Cast` that narrows to `float32` renders, with `Infinity`.
A standalone `Cast` that narrows to `float32` uses GDAL's ordinary conversion, which no
`OutOfRange` variant describes, so the adapter rejects it (purity rule 4; decided
1 November, [dtypes.md](dtypes.md#decisions) question 9). A `float32` output that needs
no narrowing is unaffected. If a real case needs that conversion, a "clamp including
infinities" variant could be added.
[fixtures/gdal/float32_narrowing.py](../fixtures/gdal/float32_narrowing.py) asserts all
three behaviours and fails if a GDAL upgrade changes any of them. It is a seed for the
#42 corpus that the meaning check in [gdal-adapter.md](gdal-adapter.md#done-when) uses.

Other checks:

- **NaN to an integer.** The plan requires NaN to be missing and written as
  `missing.value`. Into `Int16` with `missing.value` -9999, `[nan, 1]` gives:
  - `gdal.Warp(dstNodata=-9999)` with no source nodata: `[0, 1]`;
  - the same warp with source nodata NaN (`-srcnodata nan -dstnodata -9999`):
    `[-9999, 1]`;
  - `gdal.Translate(noData=-9999)`: `[0, 1]`.

  So a warp renders a float-to-integer `Cast` with NaN missing by passing
  `-srcnodata nan -dstnodata v`. The ordinary conversion writes NaN as 0 whatever the
  nodata value, so a standalone float-to-integer `Cast` whose input has NaN missing is
  rejected.
- **Two rounding rules with an integer working type.** The source is first converted
  into the working type with ties away from zero, then an interpolating kernel's result
  is rounded half up. Plans that ask for one rule on an interpolating warp therefore
  hold only when the source already has the working type, or the source conversion is
  exact; otherwise the adapter uses a float working type or rejects.

The rounding, clamping and NaN cases above, and the `Collision` cells still marked
"not yet checked", need fixtures in the same corpus, so that a GDAL upgrade that changes
any of them is caught. Fixtures for the rounding rows use sources at least two rows
tall: on a one-row source, bilinear with a float working type returned shifted source
values instead of averages, while a six-row source averaged as expected.

## Fusing a `Warp` and a `Cast`

Fusion is an adapter concern. An adapter whose engine does both in one call, as gdalwarp
does, may emit `Warp` followed by `Cast` as one call when both are true:

- the `Cast` converts exactly the warped variables it renders, either as
  `CastTarget::Variables` on the warp's dataset output or as `CastTarget::Array` after
  a `Project::Array`; and
- its `Rounding`, `OutOfRange` and `Collision` are a row of the table above for the
  working type it chooses.

Otherwise the adapter emits two steps or rejects. Neither operation changes because it
was fused, and a rewrite never merges them into one node.

## Decisions

Decided 1 November 2026, taking the review's recommendations.

1. **`CastTarget` has two forms.** An array-only `Cast` would need a dataset-rebuilding
   operation, which doesn't exist, to express `Dataset.astype` or a `Warp` of several
   variables followed by a `Cast`. An `Array` payload must have the `Data` role, and a
   dataset whose variables need different options takes one `Cast` per group.
2. **A bare NumPy float-to-integer `astype` is rejected at lowering.** NumPy calls
   out-of-range, infinite and NaN results undefined and warns; its results vary with
   the target width as well as the platform. A NaN that isn't declared missing can't be
   lowered anyway. A plan may still write an explicit `Cast` with `TowardZero` and
   `Clamp` or `Missing`. Loosening this, for example lowering to `TowardZero` with
   `Clamp` as a recorded refinement, needs a NaN rule first.
3. **`Wrap` is added, integer to integer only.** NumPy's integer narrowing is
   deterministic on every platform checked, matches #32's wrapping integer arithmetic,
   and has no other faithful lowering. GDAL rejects it.
4. **Coordinate variables aren't cast.** `CastTarget::Variables` names data variables
   only, and `CastTarget::Array` requires a `Data` payload. xarray's `astype` never
   converts a coordinate in place, and converting an index coordinate changes the
   labels that alignment (#34) reads. Revisit when #34 or a real case needs it.
5. **`bool` with missing values is rejected** until #39 decides validity masks. GDAL
   has no `bool` type.
6. **`same_value` is rejected at lowering.** It is a run-time check, not a value
   policy; NumPy raises `ValueError` when a value changes (checked). Add it only for a
   concrete case.
