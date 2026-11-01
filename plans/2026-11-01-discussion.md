# 1 November: reviewing Cast (#38)

Issue #38 asked for `Cast` to be reviewed against Array API `astype`, keeping it
separate from `Warp`. The revision is in [cast.md](cast.md), pending review.

## Cast (#38)

- **`Cast` takes an array or a dataset.** `CastTarget::Array` converts an array's
  payload; `CastTarget::Variables` converts listed data variables of a dataset. The
  earlier `var: VarId` assumed a dataset input, which dtypes.md flagged once `Project`
  could produce arrays. Two forms avoid inventing a dataset-rebuilding operation.
- **A conversion matrix replaces the scattered rules.** Each source and target kind
  says whether the conversion is exact, and which of `rounding` and `out_of_range` it
  needs. Nothing has a default.
- **`OutOfRange` gains `Wrap`,** for integer to integer only, because NumPy narrows
  integers modulo 2^bits (checked: 40000 becomes -25536 as `int16`). GDAL clamps, so
  its adapter rejects `Wrap`.
- **Host behaviour was checked, not assumed.** NumPy 2.5.4, xarray 2026.9.0 and
  GDAL 3.13.3 were run on the same inputs. NumPy truncates floats to integers and its
  out-of-range results depend on the platform; xarray's `astype` never converts
  coordinates; GDAL rounds half away from zero and clamps, writes NaN as 0 without a
  nodata value, and with an integer working type rounds an interpolating kernel's
  result half up. That last one supports Mike's reading of the source in warp.md.
- **Missing values stay minimal.** Only collisions and a ban on missing values in
  `bool` stay in `Cast`; the encoding questions are listed for #39.
- **Array API's `copy` and `device` aren't represented.** They concern aliasing and
  placement, not values.

Open questions are at the end of [cast.md](cast.md#questions-for-review).

Observed but out of scope, for #40: a `gdal.Warp` with `-r bilinear` and a float
working type, onto a grid shifted by half a pixel, returned source values unchanged
rather than averages, while an integer working type averaged them.
