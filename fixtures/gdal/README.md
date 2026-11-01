# GDAL behaviour fixtures

Checks that pin down how GDAL behaves where the GDAL adapter depends on it. This is a
seed for the #42 fixture corpus, not its format: #42 designs the shared fixture layout,
and these checks move into it once it exists.

## float32_narrowing.py

Guards the float rows of the gdalwarp table in [plans/cast.md](../../plans/cast.md):

- the warp path (gdalwarp or a warped VRT) narrows `float64` to `Float32` as IEEE 754
  does, overflowing to infinity, so `Warp` followed by `Cast(float32)` renders with
  `Infinity`;
- the translate path (gdal_translate or a plain VRT) clamps, including genuine
  infinities, so the adapter rejects a standalone `Cast` that narrows to `float32`;
- with a `Float32` working type, which is gdalwarp's default for a `float32` source,
  bilinear and cubic turn a finite neighbour of an infinite value into NaN, while
  nearest and a `Float64` working type leave it alone. So the adapter always passes
  `-wt Float64` for a float source with any method other than nearest
  ([plans/gdal-adapter.md](../../plans/gdal-adapter.md#how-it-emits-a-warp)).

Run it from the repository root:

```bash
pixi exec --spec gdal --spec numpy -- python -I fixtures/gdal/float32_narrowing.py
```

It exits non-zero and lists each mismatch if a GDAL version behaves differently. If that
happens, update cast.md and the adapter's rejections first, then the expected values in
the script.
