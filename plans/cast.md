# Cast

Status: proposed 28 October 2026; revised by #38 on 1 November 2026, pending review.
The float rules were decided with [dtypes.md](dtypes.md) on 1 November. Part of
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
    /// The input is an array (`ResultKind::Array`). Its payload variable is converted;
    /// its coordinates are unchanged.
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
    /// signed type. Integer to integer only.
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
    /// float type, or toward zero when `value` is the maximum.
    Nudge,
    /// Write it unchanged. It then reads as missing.
    Keep,
}
```

`Cast` has one input (`Arity::Exactly(1)`, [dag.md](dag.md#rules)). `CastTarget`
replaces the earlier `var: VarId`, which assumed a dataset input; `Project`
([dag.md](dag.md#open-projection-and-result-kinds)) produces either kind, and `Cast`
accepts both without a merge operation. Variables that need different dtypes or policies
take one `Cast` each.

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

- Values are clamped, when `out_of_range` is `Clamp`, then rounded, as gdalwarp does.
- Float to integer: every NaN must be missing in the input, and `missing` is required.
  A NaN that isn't missing has no integer value. Infinities are out of range.
- `Wrap` applies only between integer types, where it is NumPy's behaviour. A float has
  no low bits to keep, so float to integer can't wrap.
- Array API leaves float-to-integer conversion of NaN and infinity "implementation-
  dependent", and says nothing about other out-of-range values. ArrIR requires the
  policy instead of inheriting either gap.
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

- **Collisions are explicit.** A conversion that can round needs `missing` whenever the
  input's missing values include a sentinel other than NaN, so its `Collision` is
  stated. A rounded valid value can otherwise land on that sentinel without the plan
  saying it may ([dtypes.md](dtypes.md#casts-that-lowering-inserts)). Nothing rounds
  onto NaN, so NaN alone doesn't need it.
- **No missing values in `bool`.** Converting a variable that may have missing values to
  `bool` is rejected: either sentinel would collide with half of all valid values. #39
  may lift this with a validity mask.

## Effect on the schema

- Each converted variable's data type becomes `dtype`.
- With `missing`, its missing values become `Known { values: [missing.value],
  nan_is_missing }`, where `nan_is_missing` is true when `missing.value` is NaN.
- Without it, they're unchanged.
- The result kind, dimensions, extents, coordinates and grid placement are unchanged.

## Validation

- `CastTarget::Array` needs an array input; `Variables` needs a dataset input and a
  non-empty list of distinct data variables present in it.
- The input's missing values are `Known` whenever `missing` is set, or the conversion
  is float to integer.
- `rounding` and `out_of_range` are present exactly when the matrix requires them, for
  every converted variable, and `out_of_range` is one the matrix allows.
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
spec states is a row of the matrix. Where it leaves a gap (NaN, infinity and
out-of-range values to an integer type), the plan states a policy, so a plan is stricter
than the spec, never looser.

**NumPy's `casting=` levels** gate which dtype pairs `astype` accepts; they change no
value. Lowering applies the gate and either emits a `Cast` or rejects, as NumPy would:

| Level | Accepts | Lowering |
| --- | --- | --- |
| `no`, `equiv` | the same dtype (byte order is storage, not a `DType`) | no node |
| `safe` | exact conversions | `Cast` with no options |
| `same_kind` | also narrowing within a kind, such as `float64` → `float32` | `Cast` with the matrix's options |
| `unsafe` (the default) | anything | `Cast` with the matrix's options |
| `same_value` | anything, failing at run time if a value changes | rejected: a run-time check is not a policy ArrIR has |

**What NumPy computes** (NumPy 2.5.4, checked):

| Conversion | Result | ArrIR |
| --- | --- | --- |
| float → integer | truncates: 2.7 → 2, -2.7 → -2 | `TowardZero` |
| float → integer, out of range or NaN | platform-dependent: `1e6` → 16960 and `inf` → -1 as `int16`; `inf` → `INT64_MAX` as `int64` on this build; NaN → 0 | no faithful policy; see question 2 |
| integer → narrower integer | wraps: 40000 → -25536 as `int16`, 300 → 44 as `uint8` | `Wrap` |
| `int64` → `float32` | 16777217 → 16777216 | ties to even (no option) |
| `float64` → `float32` | overflows to ±inf | `Infinity` |
| to and from `bool` | as Array API | the matrix |

An adapter emitting NumPy uses a bare `astype` only where NumPy's result is defined and
matches the plan. Otherwise it composes the conversion, for example `np.clip` then
`np.rint` (ties to even) or `np.trunc`, then `astype`, with NaN written through
`np.where` from `missing`. Or it rejects the node.

**xarray** (2026.9.0, checked): `DataArray.astype` converts the data and leaves
coordinates alone, which is `CastTarget::Array`. `Dataset.astype` converts the data
variables and leaves every coordinate, index or not, unchanged, which is
`CastTarget::Variables` listing the data variables in schema order. Its numerics are
NumPy's. `keep_attrs` is an attribute policy, which #26 owns.

## What gdalwarp can emit

Checked by experiment on 1 November 2026 with GDAL 3.13.3, except where a row says
otherwise. "Ordinary conversion" is a `Cast` with no warp, rendered as a VRT band's
data type; `gdal_translate` behaves the same. GDAL has no `bool` type.

| GDAL path | `Rounding` | `OutOfRange` | `Collision` |
| --- | --- | --- | --- |
| gdalwarp, integer working type (`-wt` set to the output type), interpolating kernel | `NearestTiesUp` (bilinear: -2.5 → -2, -0.5 → 0) | `Clamp` | `Nudge` (not yet checked) |
| gdalwarp, integer working type, `-r near` | `NearestTiesAway` (-2.5 → -3) on the conversion into the working type | `Clamp` | `Nudge` (not yet checked) |
| gdalwarp, float working type, integer output | `NearestTiesAway` (2.5 → 3, -2.5 → -3, 3.5 → 4) | `Clamp` (`1e6` → 32767, `inf` → 32767) | `Keep` (the nudge is undone by rounding; not yet checked) |
| gdalwarp, float working type (`Float64` unless `Nearest`), float output | none needed | `Infinity` (verified) | `Nudge` (not yet checked) |
| ordinary conversion, integer output from float | `NearestTiesAway` | `Clamp` | `Keep` (not yet checked) |
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

- **NaN to an integer.** Without a nodata value, both paths write NaN as 0. The plan
  requires NaN to be missing and written as `missing.value`, so the adapter must render
  `-dstnodata` (warp) or the band's nodata (ordinary conversion). Whether GDAL then
  writes the nodata value for NaN is not yet checked; until a fixture shows it, the
  adapter rejects a float-to-integer `Cast` whose input has NaN missing.
- **Two rounding rules with an integer working type.** The source is first converted
  into the working type with ties away from zero, then an interpolating kernel's result
  is rounded half up. Plans that ask for one rule on an interpolating warp therefore
  hold only when the source already has the working type, or the source conversion is
  exact; otherwise the adapter uses a float working type or rejects.

The rounding, clamping and NaN cases above, and the `Collision` cells still marked
"not yet checked", need fixtures in the same corpus, so that a GDAL upgrade that changes
any of them is caught.

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

## Questions for review

1. **`CastTarget` with two forms, or array-only `Cast`.** Recommended: two forms. An
   array-only `Cast` would need a dataset-rebuilding operation, which doesn't exist, to
   express `Dataset.astype` or a `Warp` of several variables followed by a `Cast`.
2. **NumPy float to integer out of range.** NumPy's result is platform-dependent, so no
   policy reproduces it. Recommended: lowering from NumPy writes `TowardZero` with
   `Clamp`, recorded as a refinement. The plan is defined where NumPy isn't, and is
   identical wherever NumPy's result is defined. The alternative is rejecting every
   NumPy float-to-integer `astype`, which is common.
3. **Add `Wrap`.** Recommended: yes, for integer to integer only, so NumPy's integer
   narrowing lowers faithfully. GDAL rejects it.
4. **Casting coordinate variables.** `CastTarget::Variables` names data variables only,
   matching xarray, which never converts coordinates through `astype`. Converting an
   index coordinate changes the labels that alignment (#34) reads. Recommended: reject
   until #34 or a real case needs it.
5. **`bool` and missing values.** Recommended: reject, as above, until #39 decides
   validity masks.
6. **`same_value`.** Recommended: reject at lowering. A checked conversion that fails at
   run time is a new kind of policy; add it only for a concrete case.
