# Cast

Status: proposal. Part of [the extraction contract](design.md). `Warp`
([warp.md](warp.md)) is its first user. The reasoning is in
[the 28 October discussion](2026-10-28-discussion.md).

## Summary

`Cast` converts a variable to another data type. It's xarray's `astype`, with every
choice that can change a value stated: how fractions round, what happens out of range,
and how missing values are written in the new type.

It's a separate operation from `Warp` because each is useful without the other. A float
warp needs no conversion, and converting to `float32` before a reduction needs no warp.
gdalwarp does both in one call, but it computes in double and converts at the end, so
`Cast(Warp(x))` describes what it does. The GDAL adapter emits the pair as one call.

## The operation

```rust
pub struct Cast {
    pub var: VarId,
    pub dtype: DType,
    /// Required when a fraction can reach an integer type.
    pub rounding: Option<Rounding>,
    /// Required when a value can fall outside `dtype`'s range.
    pub out_of_range: Option<OutOfRange>,
    /// How missing values are written in `dtype`. Without it, the input's missing
    /// values are kept, and each must be representable in `dtype`.
    pub missing: Option<MissingEncoding>,
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
    /// Clamp to `dtype`'s range.
    Clamp,
    /// Write `missing.value`. Requires `missing`.
    Missing,
    /// Overflow to positive or negative infinity, as IEEE 754 narrowing does.
    /// Float targets only.
    Infinity,
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

## Definitions

- Values are clamped, when `out_of_range` is `Clamp`, then rounded, as gdalwarp does.
- Converting a float to an integer type needs every NaN to be missing in the input,
  and needs `missing`. A NaN that isn't missing has no integer value.
- `Collision` exists because gdalwarp nudges, and because a value that silently becomes
  missing breaks purity rule 4 unless the plan says it may.
- Converting to a float type rounds to nearest, ties to even, as IEEE 754 defines,
  whenever the target can't hold the value exactly. That covers 32-bit or wider integers
  to `float32`, 64-bit integers to `float64`, and `float64` to `float32`. `rounding`
  doesn't apply, because no fraction reaches an integer type.
- Narrowing `float64` to `float32` needs `out_of_range` for finite values beyond the
  `float32` range. `Infinity` matches NumPy's `astype`, while `Clamp` gives the largest
  finite value. NaN stays NaN, and infinities stay infinite.
- These float rules were decided with [dtypes.md](dtypes.md) on 1 November; #38
  reviews `Cast` as a whole. A reduction whose `result` is narrower than its
  accumulator uses the same rules and the same `OutOfRange`
  ([dtypes.md](dtypes.md#rules)).

## Effect on the schema

- The variable's data type becomes `dtype`.
- With `missing`, its missing values become `Known { values: [missing.value],
  nan_is_missing }`, where `nan_is_missing` is true when `missing.value` is NaN.
- Without it, they're unchanged.

## Validation

- The input's missing values are `Known` whenever `missing` is set, or the conversion
  is float to integer.
- `rounding` and `out_of_range` are present exactly when the conversion requires them.
- `missing.value` is representable in `dtype`.
- `Infinity` is used only with a float `dtype`.
- A conversion that can round (the float rules above) needs `missing` whenever the
  input's missing values include a sentinel other than NaN, so its `Collision` is
  explicit. A rounded valid value can otherwise land on that sentinel without the plan
  saying it may ([dtypes.md](dtypes.md#casts-that-lowering-inserts), #39). Nothing
  rounds onto NaN, so NaN alone doesn't need it.

## What gdalwarp can emit

From Mike's reading of the GDAL source, still to be checked by fixtures:

| GDAL path | `Rounding` | `OutOfRange` | `Collision` |
| --- | --- | --- | --- |
| gdalwarp, integer working type (`-wt` set to the output type) | `NearestTiesUp` | `Clamp` | `Nudge` |
| gdalwarp, float working type, integer output | `NearestTiesAway` | `Clamp` | `Keep` (the nudge is undone by rounding) |
| gdalwarp, float working type, float output | none needed | `Infinity`, unverified | `Nudge` |
| ordinary conversion (a `Cast` with no warp), integer output | `NearestTiesAway` | `Clamp` | `Keep` |
| ordinary conversion, float output | none needed | `Infinity`, unverified | `Keep` |

The GDAL adapter rejects any other combination.

"Unverified" means nobody has yet checked which `OutOfRange` GDAL applies when it
narrows to `float32`. Until someone does, the adapter rejects these rows (decided
1 November, [dtypes.md](dtypes.md#decisions) question 9, under purity rule 4). So a
`Cast` that narrows to `float32`, alone or after a `Warp`, is rejected for now. A
`float32` output that needs no narrowing, such as a `Warp` of `float32` data, is
unaffected. The cell is settled by the meaning check in
[gdal-adapter.md](gdal-adapter.md#done-when), using a #42 fixture with values beyond
the `float32` range. That fixture lifts the rejection: if GDAL overflows to infinity,
the cell becomes `Infinity`; if it clamps, the cell becomes `Clamp`, and plans asking
for `Infinity` stay rejected.
