"""Check how GDAL narrows float64 to Float32, as recorded in plans/cast.md.

Three behaviours are pinned:

1. Warp (gdalwarp, or a warped VRT) with a float64 working type overflows as IEEE 754
   does: out-of-range finite values become +/-inf, and infinities stay infinite. This
   is cast.md's `Infinity`, so `Warp` followed by `Cast(float32)` renders.
2. Translate (gdal_translate, or a plain VRT) clamps to +/-FLT_MAX, and clamps genuine
   infinities too. No `OutOfRange` variant describes this, so the adapter rejects a
   standalone `Cast` that narrows to float32.
3. Warp with a Float32 working type and an interpolating resampler (bilinear, cubic)
   turns a finite neighbour of an infinite value into NaN: a zero-weight neighbour is
   added as 0 x inf. Float32 is gdalwarp's default working type for a float32 source,
   so this happens with no Cast at all. Nearest, or a Float64 working type, leaves the
   neighbour alone. So the adapter always passes `-wt Float64` for a float source with
   any method other than nearest (plans/gdal-adapter.md, plans/warp.md's weight rule).

Verified with GDAL 3.13.3 and NumPy 2.5.4 on 1 November 2026. The script exits non-zero,
naming each mismatch, if a GDAL version behaves differently.
"""

import sys

import numpy as np
from osgeo import gdal

VERIFIED_GDAL = "3.13.3"

FLT_MAX = float(np.finfo(np.float32).max)
INF = float("inf")
NAN = float("nan")
NEXT_ABOVE_FLT_MAX = float(np.nextafter(FLT_MAX, INF))

# Behaviours 1 and 2. Each case: label, float64 input, warp output, translate output.
NARROWING = [
    ("1e39", 1e39, INF, FLT_MAX),
    ("-1e39", -1e39, -INF, -FLT_MAX),
    ("3.5e38", 3.5e38, INF, FLT_MAX),
    ("inf", INF, INF, FLT_MAX),
    ("-inf", -INF, -INF, -FLT_MAX),
    ("nan", NAN, NAN, NAN),
    ("FLT_MAX", FLT_MAX, FLT_MAX, FLT_MAX),
    ("nextafter(FLT_MAX)", NEXT_ABOVE_FLT_MAX, FLT_MAX, FLT_MAX),
    ("1.5", 1.5, 1.5, 1.5),
]

# Behaviour 3, on an identity warp. With a Float32 working path, bilinear and cubic
# turn the finite left neighbour of the infinity into NaN; otherwise nothing changes.
NEIGHBOURS_IN = [1.0, 2.0, INF, 4.0, 5.0, 6.0]
NEIGHBOURS_POISONED = [1.0, NAN, INF, 4.0, 5.0, 6.0]


def source(path, values, data_type=gdal.GDT_Float64):
    numpy_type = np.float32 if data_type == gdal.GDT_Float32 else np.float64
    row = np.array([values], dtype=numpy_type)
    ds = gdal.GetDriverByName("GTiff").Create(path, row.shape[1], 1, 1, data_type)
    ds.SetGeoTransform([0, 1, 0, 1, 0, -1])
    ds.SetProjection("EPSG:4326")
    ds.GetRasterBand(1).WriteArray(row)
    ds.FlushCache()
    return ds


def same(actual, expected):
    if np.isnan(expected):
        return bool(np.isnan(actual))
    return actual == np.float32(expected)


def check(name, dataset, labels, expected, dtype=np.float32):
    result = dataset.ReadAsArray()
    if result.dtype != dtype:
        return [f"{name}: output dtype is {result.dtype}, expected {np.dtype(dtype)}"]
    failures = []
    for label, actual, want in zip(labels, result.ravel(), expected):
        if not same(actual, want):
            failures.append(f"{name}: {label} -> {actual!r}, expected {want!r}")
    return failures


def warp(path, src, fmt, resample, working):
    return gdal.Warp(
        path,
        src,
        format=fmt,
        outputType=gdal.GDT_Float32,
        workingType=working,
        resampleAlg=resample,
    )


def main():
    gdal.UseExceptions()
    f64, f32 = gdal.GDT_Float64, gdal.GDT_Float32
    failures = []

    src = source("/vsimem/narrowing.tif", [case[1] for case in NARROWING])
    labels = [case[0] for case in NARROWING]
    warp_out = [case[2] for case in NARROWING]
    translate_out = [case[3] for case in NARROWING]
    for fmt, ext in [("GTiff", "tif"), ("VRT", "vrt")]:
        # Behaviour 1: a float64 working type, any resampler; nearest with float32 too.
        for resample, working, wname in [
            ("near", f64, "Float64"),
            ("bilinear", f64, "Float64"),
            ("near", f32, "Float32"),
        ]:
            out = warp(f"/vsimem/w_{resample}_{wname}.{ext}", src, fmt, resample, working)
            name = f"Warp {fmt} -r {resample} -wt {wname}"
            failures += check(name, out, labels, warp_out)

        # Behaviour 2.
        out = gdal.Translate(f"/vsimem/t.{ext}", src, format=fmt, outputType=f32)
        failures += check(f"Translate {fmt}", out, labels, translate_out)

    # Behaviour 3. The output type is the source type, so only the working type varies.
    labels = [repr(v) for v in NEIGHBOURS_IN]
    for src_type, src_name, src_dtype in [
        (f32, "float32", np.float32),
        (f64, "float64", np.float64),
    ]:
        src = source(f"/vsimem/n_{src_name}.tif", NEIGHBOURS_IN, src_type)
        for working, wname in [(None, "default"), (f32, "Float32"), (f64, "Float64")]:
            # gdalwarp's default working type is the source type.
            float32_path = working == f32 or (working is None and src_type == f32)
            for resample in ["near", "bilinear", "cubic"]:
                options = dict(outputType=src_type, resampleAlg=resample)
                if working is not None:
                    options["workingType"] = working
                out = gdal.Warp(f"/vsimem/n_{src_name}_{wname}_{resample}.tif", src, **options)
                poisoned = float32_path and resample != "near"
                expected = NEIGHBOURS_POISONED if poisoned else NEIGHBOURS_IN
                name = f"Neighbours {src_name} source -r {resample} -wt {wname}"
                failures += check(name, out, labels, expected, src_dtype)

    version = gdal.__version__
    if failures:
        print(
            f"GDAL {version} no longer matches the behaviour recorded for GDAL "
            f"{VERIFIED_GDAL}. Update plans/cast.md's gdalwarp table and the GDAL "
            "adapter's rules before changing the expected values here."
        )
        for failure in failures:
            print("  " + failure)
        return 1

    print(
        f"GDAL {version}: float32 narrowing matches plans/cast.md "
        f"(verified against {VERIFIED_GDAL})."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
