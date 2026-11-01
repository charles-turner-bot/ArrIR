"""Cast evidence for plans/cast.md.

Pins NumPy's float-to-integer truncation, integer wrapping, int64 -> float32 ties
to even, float64 -> float32 overflow to infinity, the bool rules, the casting=
levels, and how xarray's astype treats coordinates. The out-of-range float-to-integer
table is undefined behaviour in NumPy and is checked on arm64 only. Verified with
NumPy 2.5.4, xarray 2026.9.0 and array-api-strict 2.6.1 on macOS arm64, 1 November 2026.
"""

import os
import sys
import warnings

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import array_api_strict as xp
import numpy as np
import xarray as xr

from _check import check, finish, header, on_macos_arm64, raises

header(np, xr, xp)
print(f"Array API standard {xp.__array_api_version__}")
check("array-api-strict implements the 2025.12 standard", xp.__array_api_version__, "2025.12")

# An out-of-range float -> integer cast warns (cast.md); later checks silence it.
with warnings.catch_warnings(record=True) as caught:
    warnings.simplefilter("always")
    np.array([np.inf]).astype(np.int16)
check(
    "out-of-range cast warns",
    [str(w.message) for w in caught if w.category is RuntimeWarning],
    ["invalid value encountered in cast"],
)
warnings.simplefilter("ignore", RuntimeWarning)

# Float -> integer truncates (TowardZero), in NumPy and array-api-strict.
check("NumPy truncates", np.array([2.7, -2.7]).astype(np.int16).tolist(), [2, -2])
strict = xp.astype(xp.asarray([2.7, -2.7]), xp.int16)
check("array-api-strict truncates", [int(strict[i]) for i in range(strict.shape[0])], [2, -2])

# Integer narrowing wraps (Wrap).
check("40000 -> int16 wraps", np.array([40000]).astype(np.int16).tolist(), [-25536])
check("300 -> uint8 wraps", np.array([300]).astype(np.uint8).tolist(), [44])

# int64 -> float32 rounds to nearest, ties to even, with no double rounding.
check("16777217 -> float32", float(np.array([16777217]).astype(np.float32)[0]), 16777216.0)
big = 2**60 + 2**36 + 1
check("2^60+2^36+1 -> float32", int(np.array([big]).astype(np.float32)[0]), 2**60 + 2**37)

# float64 -> float32 overflows to infinity (Infinity) and keeps NaN.
narrowed = np.array([1e39, -1e39, np.nan]).astype(np.float32)
check("1e39 -> float32 is +inf", bool(np.isposinf(narrowed[0])), True)
check("-1e39 -> float32 is -inf", bool(np.isneginf(narrowed[1])), True)
check("NaN stays NaN", bool(np.isnan(narrowed[2])), True)

# bool rules (Array API): 0 and -0.0 are False, anything else (NaN too) is True.
check("to bool", np.array([0.0, -0.0, np.nan, 2.0]).astype(bool).tolist(), [False, False, True, True])
check("from bool", np.array([True, False]).astype(np.float64).tolist(), [1.0, 0.0])

# casting= levels gate dtype pairs; they change no value.
can = np.can_cast
check("safe: int64 -> float64 (rounds)", can(np.int64, np.float64, "safe"), True)
check("safe: uint64 -> float64 (rounds)", can(np.uint64, np.float64, "safe"), True)
check("safe: int32 -> float32 rejected", can(np.int32, np.float32, "safe"), False)
check("same_kind: float64 -> float32", can(np.float64, np.float32, "same_kind"), True)
check("same_kind: int64 -> int8", can(np.int64, np.int8, "same_kind"), True)
check("same_kind: uint64 -> int8", can(np.uint64, np.int8, "same_kind"), True)
check("same_kind: int32 -> float32", can(np.int32, np.float32, "same_kind"), True)
check("same_kind: int8 -> uint8 rejected", can(np.int8, np.uint8, "same_kind"), False)
check("same_kind: int64 -> uint64 rejected", can(np.int64, np.uint64, "same_kind"), False)
raises(
    "same_value raises when a value changes",
    lambda: np.array([2.5]).astype(np.int16, casting="same_value"),
    ValueError,
)

# xarray: astype on a coordinate payload converts a copy, not the coordinate.
ds = xr.Dataset(
    {"f": ("x", [1.5, 2.5, 3.5]), "i": ("x", [1, 2, 3])},
    coords={"x": [0.5, 1.5, 2.5], "lon": ("x", [10.5, 11.5, 12.5])},
)
converted = ds.x.astype("int16")
check("ds.x.astype: payload named x", converted.name, "x")
check("ds.x.astype: payload converted", (converted.dtype.name, converted.values.tolist()), ("int16", [0, 1, 2]))
check("ds.x.astype: coordinate unchanged", converted["x"].dtype.name, "float64")
both = ds.astype("int16")
check("Dataset.astype converts every data variable", (both.f.dtype.name, both.i.dtype.name), ("int16", "int16"))
check("Dataset.astype leaves the index coordinate", both.x.dtype.name, "float64")
check("Dataset.astype leaves auxiliary coordinates", both.lon.dtype.name, "float64")

# Out-of-range float -> integer: undefined in NumPy. macOS arm64 values from cast.md.
if on_macos_arm64():
    i32, i64 = np.iinfo(np.int32), np.iinfo(np.int64)
    u32, u64 = np.iinfo(np.uint32), np.iinfo(np.uint64)
    table = {
        "int8": [64, -1, 0, 0, -1],
        "int16": [16960, -1, 0, 0, -1],
        "int32": [1000000, i32.max, i32.min, 0, -1],
        "int64": [1000000, i64.max, i64.min, 0, -1],
        "uint8": [64, 255, 0, 0, 255],
        "uint32": [1000000, u32.max, 0, 0, 0],
        "uint64": [1000000, u64.max, 0, 0, 0],
    }
    inputs = np.array([1e6, np.inf, -np.inf, np.nan, -1.0])
    for name, expected in table.items():
        check(f"arm64 out-of-range -> {name}", inputs.astype(name).tolist(), expected)
else:
    print("skip  out-of-range table: recorded on macOS arm64 only")

finish()
